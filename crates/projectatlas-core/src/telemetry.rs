//! Purpose: Track `ProjectAtlas` token savings telemetry.

use crate::outline::estimate_tokens;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap};
use thiserror::Error;

/// Token overview counting mode.
pub const TOKEN_ESTIMATE_KIND: &str = "heuristic";
/// Token overview estimator identifier.
pub const TOKEN_ESTIMATOR: &str = "chars_or_bytes_div_ceil_4";
/// Token overview scope label.
pub const TOKEN_ESTIMATE_SCOPE: &str = "workflow_payload_estimate_not_model_billing_tokens";
/// Default token-count provider label for offline estimates.
pub const TOKEN_PROVIDER_HEURISTIC: &str = "heuristic";
/// Default model label when no model-specific counter is used.
pub const TOKEN_MODEL_UNKNOWN: &str = "unknown";
/// Default token-count backend for offline estimates.
pub const TOKENIZER_BACKEND_HEURISTIC: &str = "chars_div_4";
/// Accuracy label for the default offline estimator.
pub const TOKEN_ACCURACY_HEURISTIC: &str = "heuristic_estimate";
/// Bucket for source compression through summaries, outlines, search, or slices.
pub const TOKEN_BUCKET_FULL_FILE_COMPRESSION: &str = "full_file_compression";
/// Bucket for navigation that avoids broad folder/file exploration.
pub const TOKEN_BUCKET_NAVIGATION_AVOIDANCE: &str = "navigation_avoidance";
/// Baseline kind for a concrete full-file comparison.
pub const TOKEN_BASELINE_FULL_FILE: &str = "full_file";
/// Baseline kind for inferred candidate-set navigation savings.
pub const TOKEN_BASELINE_SELECTED_CANDIDATES: &str = "selected_candidates";
/// Baseline kind for broad directory-walk navigation savings.
pub const TOKEN_BASELINE_DIRECTORY_WALK: &str = "directory_walk";
/// Fixed average-policy share of a modeled directory-walk baseline.
pub const TOKEN_AVERAGE_DIRECTORY_WALK_PERCENT: usize = 50;
/// Evidence label distinguishing the fixed policy from measurement or benchmarking.
pub const TOKEN_AVERAGE_POLICY_EVIDENCE: &str =
    "fixed_policy_estimate_not_benchmark_or_provider_measurement";
/// Evidence label used when predecessor overflow rows lost the folder discriminator.
pub const TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE: &str =
    "fixed_policy_estimate_unclassified_overflow_uses_maximum";
/// Confidence label for observed source-compression comparisons.
pub const TOKEN_CONFIDENCE_OBSERVED: &str = "observed";
/// Confidence label for inferred navigation comparisons.
pub const TOKEN_CONFIDENCE_INFERRED: &str = "inferred";
/// Confidence label for policy-modeled navigation comparisons.
pub const TOKEN_CONFIDENCE_POLICY_ESTIMATE: &str = "policy_estimate";
/// Trace label for the default heuristic calculation.
pub const TOKEN_TRACE_HEURISTIC: &str = "heuristic=ceil(chars_or_bytes/4)";
/// Observed before/after accounting layer.
pub const TOKEN_ACCOUNTING_OBSERVED_DELTA: &str = "observed_delta";
/// Modeled counterfactual accounting layer.
pub const TOKEN_ACCOUNTING_MODELED_AVOIDANCE: &str = "modeled_avoidance";
/// Default method label for heuristic token estimates.
pub const TOKEN_ESTIMATE_METHOD_HEURISTIC: &str = "heuristic_chars_or_bytes_div_ceil_4";
/// Dedupe scope for measured one-off events.
pub const TOKEN_DEDUPE_SCOPE_EVENT: &str = "event";
/// Dedupe scope for repeated modeled workflow baselines in one session.
pub const TOKEN_DEDUPE_SCOPE_SESSION: &str = "session";
/// Read-avoidance confidence for directly observed full-file compression events.
pub const READ_AVOIDANCE_CONFIDENCE_OBSERVED: &str = "observed";
/// Read-avoidance confidence for modeled navigation events.
pub const READ_AVOIDANCE_CONFIDENCE_MODELED: &str = "modeled";
/// Read-avoidance confidence when raw command evidence is unavailable.
pub const READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED: &str = "not_recorded";
/// Human-facing explanation for likely read-avoidance counters.
pub const READ_AVOIDANCE_SCOPE: &str =
    "summary_search_slice_calls_that_likely_replaced_whole_file_reads";
/// CLI command label for file summaries.
pub const TOKEN_COMMAND_SUMMARY: &str = "summary";
/// CLI command label for file outlines.
pub const TOKEN_COMMAND_OUTLINE: &str = "outline";
/// CLI command label for source slices.
pub const TOKEN_COMMAND_SLICE: &str = "slice";
/// CLI command label for symbol slices.
pub const TOKEN_COMMAND_SYMBOL_SLICE: &str = "symbol-slice";
/// CLI command label for indexed search.
pub const TOKEN_COMMAND_SEARCH: &str = "search";
/// MCP event label for file summaries.
pub const TOKEN_COMMAND_MCP_FILE_SUMMARY: &str = "mcp.atlas_file_summary";
/// MCP event label for file outlines.
pub const TOKEN_COMMAND_MCP_OUTLINE: &str = "mcp.atlas_outline";
/// MCP event label for source slices.
pub const TOKEN_COMMAND_MCP_SLICE: &str = "mcp.atlas_slice";
/// MCP event label for indexed search.
pub const TOKEN_COMMAND_MCP_SEARCH: &str = "mcp.atlas_search";

/// Typed telemetry-domain validation failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TelemetryContractError {
    /// The all-zero durable runtime identifier is reserved.
    #[error("the zero usage instance identifier is reserved")]
    ZeroUsageInstanceId,
}

/// One bounded CLI invocation or MCP process inside an authoritative project database.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UsageInstanceId([u8; 16]);

impl UsageInstanceId {
    /// Construct an identity from its durable 16-byte representation.
    ///
    /// # Errors
    ///
    /// Returns an error for the reserved all-zero value.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, TelemetryContractError> {
        if bytes == [0; 16] {
            return Err(TelemetryContractError::ZeroUsageInstanceId);
        }
        Ok(Self(bytes))
    }

    /// Return the durable 16-byte representation.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Runtime owner of one internal telemetry instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageInstanceOwner {
    /// One short-lived command-line invocation.
    CliInvocation,
    /// One long-lived MCP server process.
    McpProcess,
    /// One direct database-library handle retained for API compatibility.
    LibraryHandle,
    /// Historical rows compacted during a supported migration.
    MigratedLegacy,
}

impl UsageInstanceOwner {
    /// Return the stable `SQLite` representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CliInvocation => "cli_invocation",
            Self::McpProcess => "mcp_process",
            Self::LibraryHandle => "library_handle",
            Self::MigratedLegacy => "migrated_legacy",
        }
    }

    /// Parse the stable `SQLite` representation.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cli_invocation" => Some(Self::CliInvocation),
            "mcp_process" => Some(Self::McpProcess),
            "library_handle" => Some(Self::LibraryHandle),
            "migrated_legacy" => Some(Self::MigratedLegacy),
            _ => None,
        }
    }
}

/// Truth state for caller-label and raw telemetry detail.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageDetailAvailability {
    /// Aggregate and retained recent detail are complete for the requested scope.
    Retained,
    /// Numeric aggregates remain available but some detail or dimensions were compacted.
    Partial,
    /// A bounded tombstone proves the requested label existed but its report expired.
    Expired,
    /// No retained aggregate or tombstone can establish the requested scope.
    #[default]
    Unavailable,
}

impl UsageDetailAvailability {
    /// Return the stable serialized label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::Partial => "partial",
            Self::Expired => "expired",
            Self::Unavailable => "unavailable",
        }
    }

    /// Parse the stable `SQLite` representation.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "retained" => Some(Self::Retained),
            "partial" => Some(Self::Partial),
            "expired" => Some(Self::Expired),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

/// Wide separated accounting totals before narrowing to the public report representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenAccountingTotals {
    /// Observed before/after saved tokens.
    pub measured_tokens_saved: i128,
    /// Gross modeled avoided tokens before baseline deduplication.
    pub gross_modeled_tokens_avoided: i128,
    /// Modeled avoided tokens after runtime-instance baseline deduplication.
    pub deduped_modeled_tokens_avoided: i128,
    /// Average-policy modeled avoided tokens after baseline deduplication.
    pub average_modeled_tokens_avoided: i128,
    /// Number of repeated modeled baseline calls collapsed by deduplication.
    pub repeated_baselines_deduped: u128,
    /// Observed calls that replaced a whole-file read.
    pub observed_file_read_replacements: u128,
    /// Modeled navigation calls that likely avoided a whole-file read.
    pub modeled_file_reads_avoided: u128,
}

/// Token savings event for a funnel command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageEvent {
    /// Optional caller-visible compatibility label, distinct from runtime identity.
    pub session_id: String,
    /// Command or tool name.
    pub command: String,
    /// Optional path affected by the command.
    pub path: Option<String>,
    /// Optional query text.
    pub query: Option<String>,
    /// Baseline token estimate without `ProjectAtlas`.
    pub estimated_tokens_without_projectatlas: Option<usize>,
    /// Actual token estimate with `ProjectAtlas`.
    pub estimated_tokens_with_projectatlas: Option<usize>,
    /// Estimated token delta.
    pub estimated_tokens_saved: Option<isize>,
    /// Savings bucket used for reporting hard evidence separately from modeled savings.
    #[serde(default = "default_token_savings_bucket")]
    pub token_savings_bucket: String,
    /// Provider used for token counting.
    #[serde(default = "default_token_provider")]
    pub provider: String,
    /// Model used for token counting.
    #[serde(default = "default_token_model")]
    pub model: String,
    /// Tokenizer or API backend used for token counting.
    #[serde(default = "default_tokenizer_backend")]
    pub tokenizer_backend: String,
    /// Accuracy level for the token count.
    #[serde(default = "default_token_accuracy")]
    pub accuracy: String,
    /// Baseline scenario used for the without-ProjectAtlas estimate.
    #[serde(default = "default_token_baseline_kind")]
    pub baseline_kind: String,
    /// Confidence level for the baseline scenario.
    #[serde(default = "default_token_confidence")]
    pub confidence: String,
    /// Compact calculation trace.
    #[serde(default = "default_token_trace")]
    pub calculation_trace: String,
    /// Accounting layer used to separate measured deltas from modeled avoidance.
    #[serde(default = "default_accounting_layer")]
    pub accounting_layer: String,
    /// Token estimate method used for this event.
    #[serde(default = "default_estimate_method")]
    pub estimate_method: String,
    /// Denominator represented by the baseline estimate.
    #[serde(default = "default_denominator_kind")]
    pub denominator_kind: String,
    /// Stable modeled-baseline identity for deduplication.
    #[serde(default)]
    pub baseline_identity: String,
    /// Stable modeled-baseline fingerprint for deduplication.
    #[serde(default)]
    pub baseline_fingerprint: String,
    /// Scope used when deduplicating modeled avoidance.
    #[serde(default = "default_dedupe_scope")]
    pub dedupe_scope: String,
}

/// Aggregated token savings for one bucket and counting mode.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TokenBucketOverview {
    /// Savings bucket.
    pub token_savings_bucket: String,
    /// Provider used for token counting.
    pub provider: String,
    /// Model used for token counting.
    pub model: String,
    /// Tokenizer or API backend used for token counting.
    pub tokenizer_backend: String,
    /// Accuracy level for the token count.
    pub accuracy: String,
    /// Baseline scenario used for the without-ProjectAtlas estimate.
    pub baseline_kind: String,
    /// Confidence level for the baseline scenario.
    pub confidence: String,
    /// Number of tracked calls in this bucket.
    pub calls: usize,
    /// Total baseline estimate.
    pub estimated_without_projectatlas: usize,
    /// Total `ProjectAtlas` estimate.
    pub estimated_with_projectatlas: usize,
    /// Total saved tokens.
    pub estimated_saved: isize,
    /// Signed savings ratio, or `None` when the baseline estimate is zero.
    pub savings_rate: Option<f64>,
    /// Accounting layer used to separate measured deltas from modeled avoidance.
    pub accounting_layer: String,
    /// Token estimate method used for this bucket.
    pub estimate_method: String,
    /// Denominator represented by the baseline estimate.
    pub denominator_kind: String,
    /// Dedupe scope used by events in this bucket.
    pub dedupe_scope: String,
}

/// Optional local tokenizer calibration for indexed UTF-8 files.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TokenCalibrationOverview {
    /// Tokenizer name.
    pub tokenizer: String,
    /// Provider label.
    pub provider: String,
    /// Model label.
    pub model: String,
    /// Tokenizer backend label.
    pub tokenizer_backend: String,
    /// Accuracy label.
    pub accuracy: String,
    /// Indexed UTF-8 file count.
    pub files: usize,
    /// Indexed UTF-8 byte count.
    pub bytes: usize,
    /// Existing heuristic estimate over indexed UTF-8 files.
    pub heuristic_tokens: usize,
    /// Local tokenizer count over indexed UTF-8 files.
    pub calibrated_tokens: usize,
    /// Heuristic-to-calibrated ratio, or `None` when calibrated count is zero.
    pub heuristic_to_calibrated_ratio: Option<f64>,
}

/// Validation state for optional agent-efficiency benchmark evidence.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEfficiencyEvidenceState {
    /// No benchmark artifact was requested.
    #[default]
    Unavailable,
    /// The requested artifact could not be read or decoded safely.
    Failed,
    /// The artifact decoded but does not match the supported release contract.
    Incompatible,
    /// Some matched evidence is valid while retained failures remain explicit.
    Partial,
    /// All required candidate and baseline trials matched successfully.
    Compatible,
}

impl AgentEfficiencyEvidenceState {
    /// Return the stable serialized label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Incompatible => "incompatible",
            Self::Partial => "partial",
            Self::Compatible => "compatible",
        }
    }
}

/// Baseline arm compared with the `v0.4` candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEfficiencyBaseline {
    /// Frozen `ProjectAtlas` `v0.3.26` runtime and packaged skill.
    FrozenProjectAtlasV0326,
    /// Codex navigation without `ProjectAtlas`.
    PlainCodex,
}

/// Identity retained from one validated benchmark artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentEfficiencyArtifactIdentity {
    /// Supported benchmark schema version.
    pub schema_version: u32,
    /// Digest algorithm used for `artifact_digest`.
    pub artifact_digest_kind: String,
    /// Digest of the exact validated artifact bytes.
    pub artifact_digest: String,
    /// Candidate runtime semantic version.
    pub candidate_version: String,
    /// Candidate runtime SHA-256 identity.
    pub candidate_runtime_sha256: String,
    /// Descriptive source checkout commit recorded by the benchmark.
    #[serde(default)]
    pub candidate_source_head: String,
    /// Compatibility identity key; descriptive only and mirrors `candidate_source_head`.
    #[serde(default)]
    pub candidate_functional_head: String,
    /// Compatibility identity key; descriptive only and mirrors `candidate_source_head`.
    #[serde(default)]
    pub candidate_checklist_head: String,
    /// Frozen `ProjectAtlas` runtime semantic version.
    pub frozen_version: String,
    /// Frozen `ProjectAtlas` runtime `SHA-256` identity.
    pub frozen_runtime_sha256: String,
}

/// Closed navigation metric projected from matched benchmark trials.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEfficiencyMetricKind {
    /// All tool calls made by the agent.
    TotalToolCalls,
    /// Calls made through the `ProjectAtlas` MCP server.
    ProjectAtlasCalls,
    /// Productive folder selections.
    ProductiveFolders,
    /// Productive file selections.
    ProductiveFiles,
    /// Productive relation selections.
    ProductiveRelations,
    /// Wrong folder selections.
    WrongFolders,
    /// Wrong file selections.
    WrongFiles,
    /// Wrong relation selections.
    WrongRelations,
    /// Broad source reads.
    BroadReads,
    /// Full source-file reads.
    FullReads,
    /// Navigation backtracks.
    Backtracks,
    /// Gross navigation-context bytes.
    GrossNavigationBytes,
    /// Net navigation-context bytes including setup material.
    NetNavigationBytes,
    /// Gross navigation-context heuristic tokens.
    GrossNavigationTokens,
    /// Net navigation-context heuristic tokens including setup material.
    NetNavigationTokens,
    /// Candidate setup wall time.
    SetupWallSeconds,
    /// Per-task runtime wall time after setup.
    RuntimeWallSeconds,
    /// Persistent bytes retained after the trial.
    PersistentBytes,
}

/// Candidate and baseline distribution summary for one navigation metric.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentEfficiencyMetricComparison {
    /// Metric represented by this row.
    pub metric: AgentEfficiencyMetricKind,
    /// Median across matched candidate trials.
    pub candidate_median: f64,
    /// Median across matched baseline trials.
    pub baseline_median: f64,
    /// Observed maximum across matched candidate trials.
    pub candidate_maximum: f64,
    /// Observed maximum across matched baseline trials.
    pub baseline_maximum: f64,
    /// Lower-is-better median percentage saving, absent for a zero denominator.
    pub median_percent_saving: Option<f64>,
}

/// Workload-specific setup/runtime break-even truth.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentEfficiencyBreakEven {
    /// Validated benchmark workload name.
    pub workload: String,
    /// Tasks required to repay setup wall time, or `None` when no positive saving exists.
    pub wall_time_tasks: Option<u64>,
}

/// Provider counter represented only as descriptive benchmark context.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEfficiencyProviderMetricKind {
    /// Provider input-token counter.
    InputTokens,
    /// Provider cached-input-token counter.
    CachedInputTokens,
    /// Provider cache-write input-token counter.
    CacheWriteInputTokens,
    /// Provider output-token counter.
    OutputTokens,
    /// Provider reasoning-output-token counter.
    ReasoningOutputTokens,
}

/// Descriptive-only candidate and baseline provider counter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentEfficiencyProviderMetric {
    /// Provider counter represented by this row.
    pub metric: AgentEfficiencyProviderMetricKind,
    /// Candidate median reported by the provider.
    pub candidate_median: f64,
    /// Baseline median reported by the provider.
    pub baseline_median: f64,
    /// Candidate observed maximum reported by the provider.
    pub candidate_maximum: f64,
    /// Baseline observed maximum reported by the provider.
    pub baseline_maximum: f64,
    /// Always false because provider counters do not prove navigation causality.
    pub causal_attribution: bool,
}

/// One matched baseline comparison projected from the benchmark artifact.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentEfficiencyBaselineRow {
    /// Compared baseline arm.
    pub baseline: AgentEfficiencyBaseline,
    /// Evidence state for this baseline.
    pub state: AgentEfficiencyEvidenceState,
    /// Candidate and baseline trials that completed the same workload and repeat.
    pub matched_trials: usize,
    /// Failed candidate trials retained outside matched denominators.
    pub candidate_failed_trials: usize,
    /// Failed baseline trials retained outside matched denominators.
    pub baseline_failed_trials: usize,
    /// Completed trials without a completed counterpart.
    pub unmatched_trials: usize,
    /// Bounded matched navigation distributions.
    pub metrics: Vec<AgentEfficiencyMetricComparison>,
    /// Workload-specific setup/runtime break-even truth.
    pub break_even: Vec<AgentEfficiencyBreakEven>,
    /// Provider counters retained as descriptive-only context.
    pub provider_usage_descriptive_only: Vec<AgentEfficiencyProviderMetric>,
}

/// Durable `ProjectAtlas` navigation capability represented in the benchmark.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEfficiencyCapability {
    /// Initial project, purpose, and connection discovery.
    Discovery,
    /// Summary, outline, and exact-slice compression.
    SummaryAndSlice,
    /// Lexical search narrowing.
    Search,
    /// Symbol and relation navigation.
    SymbolsAndRelations,
    /// Trace-completed `ProjectAtlas` calls outside the supported named groups.
    Other,
}

/// Trace-completed `v0.4` MCP calls grouped by navigation responsibility.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentEfficiencyCapabilityContribution {
    /// Capability responsibility represented by this row.
    pub capability: AgentEfficiencyCapability,
    /// Trace-completed `ProjectAtlas` MCP calls.
    pub calls: usize,
    /// Bytes emitted by those MCP calls.
    pub emitted_bytes: u64,
}

/// Optional controlled benchmark comparison attached to live token telemetry.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentEfficiencyComparison {
    /// Overall evidence state.
    pub state: AgentEfficiencyEvidenceState,
    /// Bounded explanation for unavailable, failed, incompatible, or partial evidence.
    pub reason: Option<String>,
    /// Validated artifact and runtime identity.
    pub artifact: Option<AgentEfficiencyArtifactIdentity>,
    /// Frozen-v0.3.26 and plain-control rows.
    pub baselines: Vec<AgentEfficiencyBaselineRow>,
    /// Trace-completed candidate MCP calls grouped without causal token attribution.
    pub capabilities: Vec<AgentEfficiencyCapabilityContribution>,
    /// Whether provider counters are explicitly non-causal.
    pub provider_counters_descriptive_only: bool,
}

impl Default for AgentEfficiencyComparison {
    fn default() -> Self {
        Self {
            state: AgentEfficiencyEvidenceState::Unavailable,
            reason: Some("benchmark artifact not supplied".to_string()),
            artifact: None,
            baselines: Vec::new(),
            capabilities: Vec::new(),
            provider_counters_descriptive_only: true,
        }
    }
}

/// Fixed policy metadata for the primary average token estimate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TokenAveragePolicy {
    /// Share of each retained folder-scope baseline admitted to the average estimate.
    pub directory_walk_baseline_percent: usize,
    /// Share of the actual `ProjectAtlas` payload charged to the estimate.
    pub atlas_payload_percent: usize,
    /// Evidence classification for the estimate.
    pub evidence: String,
}

impl Default for TokenAveragePolicy {
    fn default() -> Self {
        Self {
            directory_walk_baseline_percent: TOKEN_AVERAGE_DIRECTORY_WALK_PERCENT,
            atlas_payload_percent: 100,
            evidence: TOKEN_AVERAGE_POLICY_EVIDENCE.to_string(),
        }
    }
}

/// Token savings overview.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TokenOverview {
    /// Counting mode for the reported numbers.
    pub estimate_kind: String,
    /// Estimator used to produce the reported numbers.
    pub estimator: String,
    /// Scope and accuracy boundary for the reported numbers.
    pub estimate_scope: String,
    /// Number of tracked calls.
    pub calls: usize,
    /// Total baseline estimate.
    pub estimated_without_projectatlas: usize,
    /// Total `ProjectAtlas` estimate.
    pub estimated_with_projectatlas: usize,
    /// Total saved tokens.
    pub estimated_saved: isize,
    /// Signed savings ratio, or `None` when the baseline estimate is zero.
    pub savings_rate: Option<f64>,
    /// Bucketed token savings grouped by baseline and accuracy semantics.
    pub buckets: Vec<TokenBucketOverview>,
    /// Observed before/after saved tokens.
    pub measured_tokens_saved: isize,
    /// Gross modeled avoided-token estimate before dedupe.
    pub gross_modeled_tokens_avoided: isize,
    /// Deduped modeled avoided-token estimate.
    pub deduped_modeled_tokens_avoided: isize,
    /// Average-policy modeled avoided-token estimate.
    #[serde(default)]
    pub average_modeled_tokens_avoided: isize,
    /// Explicit average-policy tokens avoided estimate.
    #[serde(default)]
    pub average_tokens_avoided: isize,
    /// Explicit all-files maximum tokens avoided estimate.
    #[serde(default)]
    pub maximum_tokens_avoided: isize,
    /// Fixed policy metadata shared by JSON, TOON, CLI, and MCP reports.
    #[serde(default)]
    pub average_policy: TokenAveragePolicy,
    /// Primary compatibility alias for `average_tokens_avoided`.
    pub tokens_avoided: isize,
    /// Legacy all-bucket gross estimate retained for migration diagnostics.
    pub legacy_gross_estimated_saved: isize,
    /// Number of duplicate modeled baseline events collapsed by dedupe.
    pub repeated_baselines_deduped: usize,
    /// Observed `ProjectAtlas` summary/search/slice calls compared with whole-file reads.
    #[serde(default)]
    pub observed_file_read_replacements: usize,
    /// Modeled `ProjectAtlas` navigation calls that likely avoided whole-file reads.
    #[serde(default)]
    pub modeled_file_reads_avoided: usize,
    /// Total likely whole-file reads avoided.
    #[serde(default)]
    pub likely_file_reads_avoided: usize,
    /// Scope label for read-avoidance counters.
    #[serde(default = "default_read_avoidance_scope")]
    pub read_avoidance_scope: String,
    /// Confidence label for read-avoidance counters.
    #[serde(default = "default_read_avoidance_confidence")]
    pub read_avoidance_confidence: String,
    /// Optional local tokenizer calibration for indexed UTF-8 files.
    pub calibration: Option<TokenCalibrationOverview>,
    /// Availability of caller-label and retained raw detail for this report.
    #[serde(default)]
    pub detail_availability: UsageDetailAvailability,
    /// Optional validated controlled benchmark evidence kept separate from live accounting.
    #[serde(default)]
    pub agent_efficiency: AgentEfficiencyComparison,
}

/// Token trend grouping window.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenTrendWindow {
    /// Group token telemetry by day.
    Day,
    /// Group token telemetry by week.
    Week,
    /// Group token telemetry by month.
    Month,
    /// Group token telemetry by year.
    Year,
}

impl TokenTrendWindow {
    /// Parse a stable window label.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }

    /// Return the stable CLI/MCP label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

impl std::fmt::Display for TokenTrendWindow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Token trend aggregate for one period.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TokenTrendPeriod {
    /// Period label such as `2026-06-29`, `2026-W26`, `2026-06`, or `2026`.
    pub period: String,
    /// Number of tracked calls in the period.
    pub calls: usize,
    /// Total baseline estimate.
    pub estimated_without_projectatlas: usize,
    /// Total `ProjectAtlas` estimate.
    pub estimated_with_projectatlas: usize,
    /// Total saved tokens.
    pub estimated_saved: isize,
    /// Signed savings ratio, or `None` when the baseline estimate is zero.
    pub savings_rate: Option<f64>,
    /// Bucketed token savings grouped by baseline and accuracy semantics.
    pub buckets: Vec<TokenBucketOverview>,
}

/// Token savings trend report.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TokenTrendReport {
    /// Counting mode for the reported numbers.
    pub estimate_kind: String,
    /// Estimator used to produce the reported numbers.
    pub estimator: String,
    /// Scope and accuracy boundary for the reported numbers.
    pub estimate_scope: String,
    /// Optional caller-visible compatibility-label filter.
    pub session: Option<String>,
    /// Grouping window.
    pub window: TokenTrendWindow,
    /// Period aggregates ordered oldest to newest.
    pub periods: Vec<TokenTrendPeriod>,
    /// Availability of the requested retained trend scope.
    #[serde(default)]
    pub detail_availability: UsageDetailAvailability,
}

impl UsageEvent {
    /// Return whether this event represents an observed before/after source comparison.
    #[must_use]
    pub fn is_observed(&self) -> bool {
        is_observed_event(self)
    }

    /// Return whether this event represents modeled navigation avoidance.
    #[must_use]
    pub fn is_modeled(&self) -> bool {
        is_modeled_event(self)
    }

    /// Return whether this observed event is strong whole-file replacement evidence.
    #[must_use]
    pub fn is_observed_file_read_replacement(&self, baseline_tokens: usize) -> bool {
        is_observed_read_replacement_event(self, baseline_tokens)
    }

    /// Return whether this modeled event is strong whole-file avoidance evidence.
    #[must_use]
    pub fn is_modeled_file_read_avoidance(&self, baseline_tokens: usize) -> bool {
        is_modeled_read_avoidance_event(self, baseline_tokens)
    }

    /// Return the normalized accounting layer used by bucket reports.
    #[must_use]
    pub fn report_accounting_layer(&self) -> &str {
        if self.is_observed() {
            TOKEN_ACCOUNTING_OBSERVED_DELTA
        } else {
            &self.accounting_layer
        }
    }

    /// Return the normalized denominator used by bucket reports.
    #[must_use]
    pub fn report_denominator_kind(&self) -> &str {
        if self.is_observed() {
            TOKEN_BASELINE_FULL_FILE
        } else {
            &self.denominator_kind
        }
    }

    /// Return the normalized deduplication scope used by bucket reports.
    #[must_use]
    pub fn report_dedupe_scope(&self) -> &str {
        if self.is_observed() {
            TOKEN_DEDUPE_SCOPE_EVENT
        } else {
            &self.dedupe_scope
        }
    }

    /// Return the modeled baseline identity, including the legacy fallback.
    #[must_use]
    pub fn effective_baseline_identity(&self) -> Cow<'_, str> {
        if self.baseline_identity.is_empty() {
            Cow::Owned(default_baseline_identity(
                &self.command,
                self.path.as_deref(),
                self.query.as_deref(),
                &self.baseline_kind,
            ))
        } else {
            Cow::Borrowed(&self.baseline_identity)
        }
    }

    /// Return the modeled baseline fingerprint, including the legacy fallback.
    #[must_use]
    pub fn effective_baseline_fingerprint(&self) -> Cow<'_, str> {
        if self.baseline_fingerprint.is_empty() {
            self.effective_baseline_identity()
        } else {
            Cow::Borrowed(&self.baseline_fingerprint)
        }
    }

    /// Return the fixed collision-resistant key for one modeled baseline witness.
    #[must_use]
    pub fn modeled_baseline_key(&self) -> [u8; 32] {
        let identity = self.effective_baseline_identity();
        let fingerprint = if self.baseline_fingerprint.is_empty() {
            identity.as_ref()
        } else {
            self.baseline_fingerprint.as_str()
        };
        let mut hasher = blake3::Hasher::new();
        for value in [
            identity.as_ref(),
            fingerprint,
            self.denominator_kind.as_str(),
        ] {
            let bytes = value.as_bytes();
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        *hasher.finalize().as_bytes()
    }
}

impl TokenOverview {
    /// Build an overview from usage events.
    #[must_use]
    pub fn from_events(events: &[UsageEvent]) -> Self {
        let mut totals = BTreeMap::<TokenBucketKey, (u128, u128, u128)>::new();
        for event in events {
            let (Some(event_without), Some(event_with)) = (
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
            ) else {
                continue;
            };
            let entry = totals.entry(TokenBucketKey::from(event)).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(event_without as u128);
            entry.2 = entry.2.saturating_add(event_with as u128);
        }
        let buckets = totals
            .into_iter()
            .map(|(key, (calls, without, with))| key.into_overview(calls, without, with))
            .collect();
        let mut overview = Self::from_buckets(buckets);
        overview.apply_accounting_from_events(events);
        overview
    }

    /// Build an overview from aggregate heuristic token totals.
    #[must_use]
    pub fn from_estimated_totals(calls: u128, without: u128, with: u128) -> Self {
        Self::from_buckets(vec![TokenBucketOverview::from_totals(
            default_token_savings_bucket(),
            default_token_provider(),
            default_token_model(),
            default_tokenizer_backend(),
            default_token_accuracy(),
            default_token_baseline_kind(),
            default_token_confidence(),
            default_accounting_layer(),
            default_estimate_method(),
            default_denominator_kind(),
            default_dedupe_scope(),
            calls,
            without,
            with,
        )])
    }

    /// Build an overview from pre-aggregated buckets.
    #[must_use]
    pub fn from_buckets(buckets: Vec<TokenBucketOverview>) -> Self {
        let calls = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.calls as u128)
        });
        let without = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_without_projectatlas as u128)
        });
        let with = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_with_projectatlas as u128)
        });
        let saved = aggregate_token_delta(without, with);
        let savings_rate = if without == 0 {
            None
        } else {
            Some((without as f64 - with as f64) / without as f64)
        };
        let measured_tokens_saved_wide = measured_tokens_saved_from_buckets(&buckets);
        let gross_modeled_tokens_avoided_wide = modeled_tokens_saved_from_buckets(&buckets);
        let average_modeled_tokens_avoided_wide =
            average_modeled_tokens_saved_from_buckets(&buckets);
        let measured_tokens_saved = saturating_i128_to_isize(measured_tokens_saved_wide);
        let gross_modeled_tokens_avoided =
            saturating_i128_to_isize(gross_modeled_tokens_avoided_wide);
        let average_modeled_tokens_avoided =
            saturating_i128_to_isize(average_modeled_tokens_avoided_wide);
        let average_tokens_avoided = saturating_i128_to_isize(
            measured_tokens_saved_wide.saturating_add(average_modeled_tokens_avoided_wide),
        );
        let maximum_tokens_avoided = saturating_i128_to_isize(
            measured_tokens_saved_wide.saturating_add(gross_modeled_tokens_avoided_wide),
        );
        Self {
            estimate_kind: TOKEN_ESTIMATE_KIND.to_string(),
            estimator: TOKEN_ESTIMATOR.to_string(),
            estimate_scope: TOKEN_ESTIMATE_SCOPE.to_string(),
            calls: saturating_u128_to_usize(calls),
            estimated_without_projectatlas: saturating_u128_to_usize(without),
            estimated_with_projectatlas: saturating_u128_to_usize(with),
            estimated_saved: saved,
            savings_rate,
            measured_tokens_saved,
            gross_modeled_tokens_avoided,
            deduped_modeled_tokens_avoided: gross_modeled_tokens_avoided,
            average_modeled_tokens_avoided,
            average_tokens_avoided,
            maximum_tokens_avoided,
            average_policy: TokenAveragePolicy::default(),
            tokens_avoided: average_tokens_avoided,
            legacy_gross_estimated_saved: saved,
            repeated_baselines_deduped: 0,
            observed_file_read_replacements: 0,
            modeled_file_reads_avoided: 0,
            likely_file_reads_avoided: 0,
            read_avoidance_scope: READ_AVOIDANCE_SCOPE.to_string(),
            read_avoidance_confidence: READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string(),
            calibration: None,
            detail_availability: UsageDetailAvailability::Retained,
            agent_efficiency: AgentEfficiencyComparison::default(),
            buckets,
        }
    }

    /// Attach a local tokenizer calibration section.
    pub fn set_calibration(&mut self, calibration: TokenCalibrationOverview) {
        self.calibration = Some(calibration);
    }

    /// Attach one validated controlled benchmark comparison.
    pub fn set_agent_efficiency(&mut self, comparison: AgentEfficiencyComparison) {
        self.agent_efficiency = comparison;
    }

    /// Apply exact separated accounting totals loaded from durable aggregates.
    pub fn apply_accounting_totals(&mut self, totals: TokenAccountingTotals) {
        self.measured_tokens_saved = saturating_i128_to_isize(totals.measured_tokens_saved);
        self.gross_modeled_tokens_avoided =
            saturating_i128_to_isize(totals.gross_modeled_tokens_avoided);
        self.deduped_modeled_tokens_avoided =
            saturating_i128_to_isize(totals.deduped_modeled_tokens_avoided);
        self.average_modeled_tokens_avoided =
            saturating_i128_to_isize(totals.average_modeled_tokens_avoided);
        self.average_tokens_avoided = saturating_i128_to_isize(
            totals
                .measured_tokens_saved
                .saturating_add(totals.average_modeled_tokens_avoided),
        );
        self.maximum_tokens_avoided = saturating_i128_to_isize(
            totals
                .measured_tokens_saved
                .saturating_add(totals.deduped_modeled_tokens_avoided),
        );
        self.tokens_avoided = self.average_tokens_avoided;
        self.repeated_baselines_deduped =
            saturating_u128_to_usize(totals.repeated_baselines_deduped);
        self.observed_file_read_replacements =
            saturating_u128_to_usize(totals.observed_file_read_replacements);
        self.modeled_file_reads_avoided =
            saturating_u128_to_usize(totals.modeled_file_reads_avoided);
        self.likely_file_reads_avoided = self
            .observed_file_read_replacements
            .saturating_add(self.modeled_file_reads_avoided);
        self.read_avoidance_confidence = read_avoidance_confidence_for(
            self.observed_file_read_replacements,
            self.modeled_file_reads_avoided,
        )
        .to_string();
    }

    /// Set the truth state for caller-label and retained raw detail.
    pub const fn set_detail_availability(&mut self, availability: UsageDetailAvailability) {
        self.detail_availability = availability;
    }

    /// Apply separated measured/modeled accounting totals from raw usage events.
    pub fn apply_accounting_from_events(&mut self, events: &[UsageEvent]) {
        let summary = TokenAccountingSummary::from_events(events);
        self.measured_tokens_saved = summary.measured_tokens_saved;
        self.gross_modeled_tokens_avoided = summary.gross_modeled_tokens_avoided;
        self.deduped_modeled_tokens_avoided = summary.deduped_modeled_tokens_avoided;
        self.average_modeled_tokens_avoided = summary.average_modeled_tokens_avoided;
        self.average_tokens_avoided = summary.average_tokens_avoided;
        self.maximum_tokens_avoided = summary.maximum_tokens_avoided;
        self.tokens_avoided = summary.average_tokens_avoided;
        self.repeated_baselines_deduped = summary.repeated_baselines_deduped;
        self.observed_file_read_replacements = summary.observed_file_read_replacements;
        self.modeled_file_reads_avoided = summary.modeled_file_reads_avoided;
        self.likely_file_reads_avoided = summary.likely_file_reads_avoided;
        self.read_avoidance_confidence = read_avoidance_confidence_for(
            self.observed_file_read_replacements,
            self.modeled_file_reads_avoided,
        )
        .to_string();
    }
}

impl TokenBucketOverview {
    /// Build a bucket overview from aggregate heuristic token totals.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_totals(
        token_savings_bucket: String,
        provider: String,
        model: String,
        tokenizer_backend: String,
        accuracy: String,
        baseline_kind: String,
        confidence: String,
        accounting_layer: String,
        estimate_method: String,
        denominator_kind: String,
        dedupe_scope: String,
        calls: u128,
        without: u128,
        with: u128,
    ) -> Self {
        let estimated_saved = aggregate_token_delta(without, with);
        let savings_rate = if without == 0 {
            None
        } else {
            Some((without as f64 - with as f64) / without as f64)
        };
        Self {
            token_savings_bucket,
            provider,
            model,
            tokenizer_backend,
            accuracy,
            baseline_kind,
            confidence,
            calls: saturating_u128_to_usize(calls),
            estimated_without_projectatlas: saturating_u128_to_usize(without),
            estimated_with_projectatlas: saturating_u128_to_usize(with),
            estimated_saved,
            savings_rate,
            accounting_layer,
            estimate_method,
            denominator_kind,
            dedupe_scope,
        }
    }
}

impl TokenTrendPeriod {
    /// Build a period aggregate from token totals.
    #[must_use]
    pub fn from_totals(period: String, calls: u128, without: u128, with: u128) -> Self {
        let bucket = TokenBucketOverview::from_totals(
            default_token_savings_bucket(),
            default_token_provider(),
            default_token_model(),
            default_tokenizer_backend(),
            default_token_accuracy(),
            default_token_baseline_kind(),
            default_token_confidence(),
            default_accounting_layer(),
            default_estimate_method(),
            default_denominator_kind(),
            default_dedupe_scope(),
            calls,
            without,
            with,
        );
        Self::from_buckets(period, vec![bucket])
    }

    /// Build a period aggregate from pre-aggregated buckets.
    #[must_use]
    pub fn from_buckets(period: String, buckets: Vec<TokenBucketOverview>) -> Self {
        let calls = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.calls as u128)
        });
        let without = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_without_projectatlas as u128)
        });
        let with = buckets.iter().fold(0u128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_with_projectatlas as u128)
        });
        let saved = aggregate_token_delta(without, with);
        let savings_rate = if without == 0 {
            None
        } else {
            Some((without as f64 - with as f64) / without as f64)
        };
        Self {
            period,
            calls: saturating_u128_to_usize(calls),
            estimated_without_projectatlas: saturating_u128_to_usize(without),
            estimated_with_projectatlas: saturating_u128_to_usize(with),
            estimated_saved: saved,
            savings_rate,
            buckets,
        }
    }
}

impl TokenTrendReport {
    /// Build a trend report from period aggregates.
    #[must_use]
    pub fn new(
        session: Option<String>,
        window: TokenTrendWindow,
        periods: Vec<TokenTrendPeriod>,
    ) -> Self {
        Self {
            estimate_kind: TOKEN_ESTIMATE_KIND.to_string(),
            estimator: TOKEN_ESTIMATOR.to_string(),
            estimate_scope: TOKEN_ESTIMATE_SCOPE.to_string(),
            session,
            window,
            periods,
            detail_availability: UsageDetailAvailability::Retained,
        }
    }

    /// Set the truth state for the requested retained trend scope.
    pub const fn set_detail_availability(&mut self, availability: UsageDetailAvailability) {
        self.detail_availability = availability;
    }
}

/// Grouping key for token bucket aggregation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TokenBucketKey {
    /// Savings bucket.
    token_savings_bucket: String,
    /// Provider used for token counting.
    provider: String,
    /// Model used for token counting.
    model: String,
    /// Tokenizer or API backend used for token counting.
    tokenizer_backend: String,
    /// Accuracy level for the token count.
    accuracy: String,
    /// Baseline scenario used for the without-ProjectAtlas estimate.
    baseline_kind: String,
    /// Confidence level for the baseline scenario.
    confidence: String,
    /// Accounting layer used to separate measured deltas from modeled avoidance.
    accounting_layer: String,
    /// Token estimate method used for this bucket.
    estimate_method: String,
    /// Denominator represented by the baseline estimate.
    denominator_kind: String,
    /// Dedupe scope used by events in this bucket.
    dedupe_scope: String,
}

/// Stable key used to dedupe repeated modeled baselines within a session.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModeledBaselineKey {
    /// Session that emitted the modeled events.
    session_id: String,
    /// Human-readable baseline identity.
    baseline_identity: String,
    /// Stable fingerprint for the modeled baseline.
    baseline_fingerprint: String,
    /// Denominator kind represented by the baseline.
    denominator_kind: String,
}

/// Accumulators for one modeled baseline dedupe group.
#[derive(Default)]
struct ModeledBaselineTotals {
    /// Number of modeled events in the group.
    calls: usize,
    /// Single baseline token count retained for the group.
    baseline_without_projectatlas: usize,
    /// Sum of all `ProjectAtlas` payload tokens emitted for the group.
    emitted_with_projectatlas: u128,
}

/// Final separated accounting totals derived from raw usage events.
#[derive(Default)]
struct TokenAccountingSummary {
    /// Observed before/after saved tokens.
    measured_tokens_saved: isize,
    /// Gross modeled avoided tokens before dedupe.
    gross_modeled_tokens_avoided: isize,
    /// Modeled avoided tokens after repeated baseline dedupe.
    deduped_modeled_tokens_avoided: isize,
    /// Average-policy modeled avoided tokens after baseline dedupe.
    average_modeled_tokens_avoided: isize,
    /// Average-policy tokens avoided.
    average_tokens_avoided: isize,
    /// All-files maximum tokens avoided.
    maximum_tokens_avoided: isize,
    /// Number of duplicate modeled baseline events collapsed by dedupe.
    repeated_baselines_deduped: usize,
    /// Observed `ProjectAtlas` calls compared with full-file reads.
    observed_file_read_replacements: usize,
    /// Modeled `ProjectAtlas` calls that likely avoided whole-file reads.
    modeled_file_reads_avoided: usize,
    /// Total likely whole-file reads avoided.
    likely_file_reads_avoided: usize,
}

impl TokenAccountingSummary {
    /// Build separated accounting totals from raw usage events.
    fn from_events(events: &[UsageEvent]) -> Self {
        let mut measured_tokens_saved = 0i128;
        let mut gross_modeled_tokens_avoided = 0i128;
        let mut event_scoped_modeled_tokens_avoided = 0i128;
        let mut average_non_directory_tokens_avoided = 0i128;
        let mut average_directory_without = 0u128;
        let mut average_directory_with = 0u128;
        let mut observed_file_read_replacements = 0usize;
        let mut modeled_file_reads_avoided = 0usize;
        let mut modeled_baselines = BTreeMap::<ModeledBaselineKey, ModeledBaselineTotals>::new();

        for event in events {
            let (Some(without), Some(with)) = (
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
            ) else {
                continue;
            };
            let delta = aggregate_token_delta_wide(without as u128, with as u128);
            if is_observed_event(event) {
                measured_tokens_saved = measured_tokens_saved.saturating_add(delta);
                if is_observed_read_replacement_event(event, without) {
                    observed_file_read_replacements =
                        observed_file_read_replacements.saturating_add(1);
                }
                continue;
            }
            if !is_modeled_event(event) {
                continue;
            }
            if is_modeled_read_avoidance_event(event, without) {
                modeled_file_reads_avoided = modeled_file_reads_avoided.saturating_add(1);
            }
            gross_modeled_tokens_avoided = gross_modeled_tokens_avoided.saturating_add(delta);
            if event.dedupe_scope == TOKEN_DEDUPE_SCOPE_EVENT {
                event_scoped_modeled_tokens_avoided =
                    event_scoped_modeled_tokens_avoided.saturating_add(delta);
                if event.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
                    average_directory_without =
                        average_directory_without.saturating_add(without as u128);
                    average_directory_with = average_directory_with.saturating_add(with as u128);
                } else {
                    average_non_directory_tokens_avoided =
                        average_non_directory_tokens_avoided.saturating_add(delta);
                }
                continue;
            }
            let entry = modeled_baselines
                .entry(ModeledBaselineKey::from_event(event))
                .or_default();
            entry.calls = entry.calls.saturating_add(1);
            entry.baseline_without_projectatlas = entry.baseline_without_projectatlas.max(without);
            entry.emitted_with_projectatlas =
                entry.emitted_with_projectatlas.saturating_add(with as u128);
        }

        let mut deduped_modeled_tokens_avoided = event_scoped_modeled_tokens_avoided;
        let mut repeated_baselines_deduped = 0usize;
        for (key, totals) in &modeled_baselines {
            if totals.calls > 1 {
                repeated_baselines_deduped =
                    repeated_baselines_deduped.saturating_add(totals.calls.saturating_sub(1));
            }
            let delta = aggregate_token_delta_wide(
                totals.baseline_without_projectatlas as u128,
                totals.emitted_with_projectatlas,
            );
            deduped_modeled_tokens_avoided = deduped_modeled_tokens_avoided.saturating_add(delta);
            if key.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
                average_directory_without = average_directory_without
                    .saturating_add(totals.baseline_without_projectatlas as u128);
                average_directory_with =
                    average_directory_with.saturating_add(totals.emitted_with_projectatlas);
            } else {
                average_non_directory_tokens_avoided =
                    average_non_directory_tokens_avoided.saturating_add(delta);
            }
        }
        let average_directory_tokens_avoided = aggregate_token_delta_wide(
            average_modeled_baseline_tokens(
                TOKEN_BASELINE_DIRECTORY_WALK,
                average_directory_without,
            ),
            average_directory_with,
        );
        let average_modeled_tokens_avoided =
            average_non_directory_tokens_avoided.saturating_add(average_directory_tokens_avoided);
        let average_tokens_avoided =
            measured_tokens_saved.saturating_add(average_modeled_tokens_avoided);
        let maximum_tokens_avoided =
            measured_tokens_saved.saturating_add(deduped_modeled_tokens_avoided);
        let likely_file_reads_avoided =
            observed_file_read_replacements.saturating_add(modeled_file_reads_avoided);
        Self {
            measured_tokens_saved: saturating_i128_to_isize(measured_tokens_saved),
            gross_modeled_tokens_avoided: saturating_i128_to_isize(gross_modeled_tokens_avoided),
            deduped_modeled_tokens_avoided: saturating_i128_to_isize(
                deduped_modeled_tokens_avoided,
            ),
            average_modeled_tokens_avoided: saturating_i128_to_isize(
                average_modeled_tokens_avoided,
            ),
            average_tokens_avoided: saturating_i128_to_isize(average_tokens_avoided),
            maximum_tokens_avoided: saturating_i128_to_isize(maximum_tokens_avoided),
            repeated_baselines_deduped,
            observed_file_read_replacements,
            modeled_file_reads_avoided,
            likely_file_reads_avoided,
        }
    }
}

impl ModeledBaselineKey {
    /// Build a dedupe key from persisted event metadata with legacy fallback.
    fn from_event(event: &UsageEvent) -> Self {
        let identity = if event.baseline_identity.is_empty() {
            default_baseline_identity(
                &event.command,
                event.path.as_deref(),
                event.query.as_deref(),
                &event.baseline_kind,
            )
        } else {
            event.baseline_identity.clone()
        };
        let fingerprint = if event.baseline_fingerprint.is_empty() {
            identity.clone()
        } else {
            event.baseline_fingerprint.clone()
        };
        Self {
            session_id: event.session_id.clone(),
            baseline_identity: identity,
            baseline_fingerprint: fingerprint,
            denominator_kind: event.denominator_kind.clone(),
        }
    }
}

impl TokenBucketKey {
    /// Build a grouping key from one usage event.
    fn from(event: &UsageEvent) -> Self {
        let observed = is_observed_event(event);
        Self {
            token_savings_bucket: event.token_savings_bucket.clone(),
            provider: event.provider.clone(),
            model: event.model.clone(),
            tokenizer_backend: event.tokenizer_backend.clone(),
            accuracy: event.accuracy.clone(),
            baseline_kind: event.baseline_kind.clone(),
            confidence: event.confidence.clone(),
            accounting_layer: if observed {
                TOKEN_ACCOUNTING_OBSERVED_DELTA.to_string()
            } else {
                event.accounting_layer.clone()
            },
            estimate_method: event.estimate_method.clone(),
            denominator_kind: if observed {
                TOKEN_BASELINE_FULL_FILE.to_string()
            } else {
                event.denominator_kind.clone()
            },
            dedupe_scope: if observed {
                TOKEN_DEDUPE_SCOPE_EVENT.to_string()
            } else {
                event.dedupe_scope.clone()
            },
        }
    }

    /// Convert an aggregate bucket into a report row.
    fn into_overview(self, calls: u128, without: u128, with: u128) -> TokenBucketOverview {
        TokenBucketOverview::from_totals(
            self.token_savings_bucket,
            self.provider,
            self.model,
            self.tokenizer_backend,
            self.accuracy,
            self.baseline_kind,
            self.confidence,
            self.accounting_layer,
            self.estimate_method,
            self.denominator_kind,
            self.dedupe_scope,
            calls,
            without,
            with,
        )
    }
}

/// Create a usage event from response text and baseline text.
#[must_use]
pub fn usage_from_text(
    session_id: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    projectatlas_text: &str,
) -> UsageEvent {
    let without = estimate_tokens(baseline_text);
    let with = estimate_tokens(projectatlas_text);
    usage_from_estimates_with_accounting(
        session_id,
        command,
        path,
        query,
        without,
        with,
        TOKEN_BUCKET_FULL_FILE_COMPRESSION,
        TOKEN_BASELINE_FULL_FILE,
        TOKEN_CONFIDENCE_OBSERVED,
        TOKEN_ACCOUNTING_OBSERVED_DELTA,
        TOKEN_BASELINE_FULL_FILE,
        TOKEN_DEDUPE_SCOPE_EVENT,
    )
}

/// Create a usage event from already-computed token estimates.
#[must_use]
pub fn usage_from_estimates(
    session_id: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    estimated_with_projectatlas: usize,
) -> UsageEvent {
    usage_from_estimates_with_accounting(
        session_id,
        command,
        path,
        query,
        estimated_without_projectatlas,
        estimated_with_projectatlas,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_CONFIDENCE_INFERRED,
        TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
        TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_DEDUPE_SCOPE_SESSION,
    )
}

/// Create a usage event from token estimates and explicit baseline semantics.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn usage_from_estimates_with_context(
    session_id: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    estimated_with_projectatlas: usize,
    token_savings_bucket: &str,
    baseline_kind: &str,
    confidence: &str,
) -> UsageEvent {
    usage_from_estimates_with_accounting(
        session_id,
        command,
        path,
        query,
        estimated_without_projectatlas,
        estimated_with_projectatlas,
        token_savings_bucket,
        baseline_kind,
        confidence,
        if token_savings_bucket == TOKEN_BUCKET_FULL_FILE_COMPRESSION {
            TOKEN_ACCOUNTING_OBSERVED_DELTA
        } else {
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE
        },
        baseline_kind,
        if token_savings_bucket == TOKEN_BUCKET_FULL_FILE_COMPRESSION {
            TOKEN_DEDUPE_SCOPE_EVENT
        } else {
            TOKEN_DEDUPE_SCOPE_SESSION
        },
    )
}

/// Create a usage event from token estimates and explicit accounting semantics.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn usage_from_estimates_with_accounting(
    session_id: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    estimated_with_projectatlas: usize,
    token_savings_bucket: &str,
    baseline_kind: &str,
    confidence: &str,
    accounting_layer: &str,
    denominator_kind: &str,
    dedupe_scope: &str,
) -> UsageEvent {
    let baseline_identity =
        default_baseline_identity(command, path.as_deref(), query.as_deref(), baseline_kind);
    let baseline_fingerprint = baseline_identity.clone();
    UsageEvent {
        session_id: session_id.to_string(),
        command: command.to_string(),
        path,
        query,
        estimated_tokens_without_projectatlas: Some(estimated_without_projectatlas),
        estimated_tokens_with_projectatlas: Some(estimated_with_projectatlas),
        estimated_tokens_saved: Some(token_delta(
            estimated_without_projectatlas,
            estimated_with_projectatlas,
        )),
        token_savings_bucket: token_savings_bucket.to_string(),
        provider: default_token_provider(),
        model: default_token_model(),
        tokenizer_backend: default_tokenizer_backend(),
        accuracy: default_token_accuracy(),
        baseline_kind: baseline_kind.to_string(),
        confidence: confidence.to_string(),
        calculation_trace: default_token_trace(),
        accounting_layer: accounting_layer.to_string(),
        estimate_method: default_estimate_method(),
        denominator_kind: denominator_kind.to_string(),
        baseline_identity,
        baseline_fingerprint,
        dedupe_scope: dedupe_scope.to_string(),
    }
}

/// Default token savings bucket for legacy usage events.
#[must_use]
pub fn default_token_savings_bucket() -> String {
    TOKEN_BUCKET_NAVIGATION_AVOIDANCE.to_string()
}

/// Default token provider for legacy usage events.
#[must_use]
pub fn default_token_provider() -> String {
    TOKEN_PROVIDER_HEURISTIC.to_string()
}

/// Default token model for legacy usage events.
#[must_use]
pub fn default_token_model() -> String {
    TOKEN_MODEL_UNKNOWN.to_string()
}

/// Default tokenizer backend for legacy usage events.
#[must_use]
pub fn default_tokenizer_backend() -> String {
    TOKENIZER_BACKEND_HEURISTIC.to_string()
}

/// Default accuracy label for legacy usage events.
#[must_use]
pub fn default_token_accuracy() -> String {
    TOKEN_ACCURACY_HEURISTIC.to_string()
}

/// Default baseline kind for legacy usage events.
#[must_use]
pub fn default_token_baseline_kind() -> String {
    TOKEN_BASELINE_SELECTED_CANDIDATES.to_string()
}

/// Default confidence label for legacy usage events.
#[must_use]
pub fn default_token_confidence() -> String {
    TOKEN_CONFIDENCE_INFERRED.to_string()
}

/// Default calculation trace for legacy usage events.
#[must_use]
pub fn default_token_trace() -> String {
    TOKEN_TRACE_HEURISTIC.to_string()
}

/// Default accounting layer for legacy usage events.
#[must_use]
pub fn default_accounting_layer() -> String {
    TOKEN_ACCOUNTING_MODELED_AVOIDANCE.to_string()
}

/// Default estimate method for legacy usage events.
#[must_use]
pub fn default_estimate_method() -> String {
    TOKEN_ESTIMATE_METHOD_HEURISTIC.to_string()
}

/// Default denominator kind for legacy usage events.
#[must_use]
pub fn default_denominator_kind() -> String {
    TOKEN_BASELINE_SELECTED_CANDIDATES.to_string()
}

/// Default dedupe scope for legacy usage events.
#[must_use]
pub fn default_dedupe_scope() -> String {
    TOKEN_DEDUPE_SCOPE_SESSION.to_string()
}

/// Default read-avoidance scope for legacy serialized overviews.
#[must_use]
pub fn default_read_avoidance_scope() -> String {
    READ_AVOIDANCE_SCOPE.to_string()
}

/// Default read-avoidance confidence for legacy serialized overviews.
#[must_use]
pub fn default_read_avoidance_confidence() -> String {
    READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string()
}

/// Build a stable baseline identity from existing event context.
#[must_use]
pub fn default_baseline_identity(
    command: &str,
    path: Option<&str>,
    query: Option<&str>,
    baseline_kind: &str,
) -> String {
    format!(
        "{baseline_kind}:command={command}:path={path}:query={query}",
        path = path.unwrap_or("*"),
        query = query.unwrap_or("*")
    )
}

/// Return a saturating signed token delta.
fn token_delta(without: usize, with: usize) -> isize {
    let without = isize::try_from(without).unwrap_or(isize::MAX);
    let with = isize::try_from(with).unwrap_or(isize::MAX);
    without.saturating_sub(with)
}

/// Return the signed aggregate token delta.
fn aggregate_token_delta(without: u128, with: u128) -> isize {
    saturating_i128_to_isize(aggregate_token_delta_wide(without, with))
}

/// Return a wide signed aggregate token delta and saturate only at the wide boundary.
fn aggregate_token_delta_wide(without: u128, with: u128) -> i128 {
    if without >= with {
        let delta = without - with;
        if delta > i128::MAX as u128 {
            i128::MAX
        } else {
            delta as i128
        }
    } else {
        let delta = with - without;
        if delta > i128::MAX as u128 {
            i128::MIN
        } else {
            -(delta as i128)
        }
    }
}

/// Apply the fixed average policy to one modeled baseline.
#[must_use]
pub fn average_modeled_baseline_tokens(denominator_kind: &str, without: u128) -> u128 {
    if denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
        without / 2
    } else {
        without
    }
}

/// Convert a wide aggregate count to `usize` with saturation.
fn saturating_u128_to_usize(value: u128) -> usize {
    if value > usize::MAX as u128 {
        usize::MAX
    } else {
        value as usize
    }
}

/// Convert a wide signed aggregate to `isize` with saturation.
fn saturating_i128_to_isize(value: i128) -> isize {
    if value > isize::MAX as i128 {
        isize::MAX
    } else if value < isize::MIN as i128 {
        isize::MIN
    } else {
        value as isize
    }
}

/// Sum observed saved-token buckets.
fn measured_tokens_saved_from_buckets(buckets: &[TokenBucketOverview]) -> i128 {
    buckets
        .iter()
        .filter(|bucket| is_observed_bucket(bucket))
        .fold(0i128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_saved as i128)
        })
}

/// Sum modeled avoided-token buckets.
fn modeled_tokens_saved_from_buckets(buckets: &[TokenBucketOverview]) -> i128 {
    buckets
        .iter()
        .filter(|bucket| is_modeled_bucket(bucket))
        .fold(0i128, |acc, bucket| {
            acc.saturating_add(bucket.estimated_saved as i128)
        })
}

/// Sum modeled avoided-token buckets with the average directory-walk policy.
fn average_modeled_tokens_saved_from_buckets(buckets: &[TokenBucketOverview]) -> i128 {
    let mut non_directory_tokens_avoided = 0i128;
    let mut directory_without = 0u128;
    let mut directory_with = 0u128;
    for bucket in buckets.iter().filter(|bucket| is_modeled_bucket(bucket)) {
        if bucket.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
            directory_without =
                directory_without.saturating_add(bucket.estimated_without_projectatlas as u128);
            directory_with =
                directory_with.saturating_add(bucket.estimated_with_projectatlas as u128);
        } else {
            non_directory_tokens_avoided =
                non_directory_tokens_avoided.saturating_add(bucket.estimated_saved as i128);
        }
    }
    let directory_tokens_avoided = aggregate_token_delta_wide(
        average_modeled_baseline_tokens(TOKEN_BASELINE_DIRECTORY_WALK, directory_without),
        directory_with,
    );
    non_directory_tokens_avoided.saturating_add(directory_tokens_avoided)
}

/// Whether an event represents observed before/after source compression.
fn is_observed_event(event: &UsageEvent) -> bool {
    event.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA
        || event.token_savings_bucket == TOKEN_BUCKET_FULL_FILE_COMPRESSION
        || event.confidence == TOKEN_CONFIDENCE_OBSERVED
}

/// Whether an event represents modeled counterfactual navigation avoidance.
fn is_modeled_event(event: &UsageEvent) -> bool {
    event.accounting_layer == TOKEN_ACCOUNTING_MODELED_AVOIDANCE || !is_observed_event(event)
}

/// Whether a raw observed event is strong evidence for replacing a whole-file read.
fn is_observed_read_replacement_event(event: &UsageEvent, baseline_tokens: usize) -> bool {
    baseline_tokens > 0
        && matches!(
            event.command.as_str(),
            TOKEN_COMMAND_SUMMARY
                | TOKEN_COMMAND_OUTLINE
                | TOKEN_COMMAND_SLICE
                | TOKEN_COMMAND_SYMBOL_SLICE
                | TOKEN_COMMAND_MCP_FILE_SUMMARY
                | TOKEN_COMMAND_MCP_OUTLINE
                | TOKEN_COMMAND_MCP_SLICE
        )
}

/// Whether a raw modeled event is strong evidence for avoiding a broad file read.
fn is_modeled_read_avoidance_event(event: &UsageEvent, baseline_tokens: usize) -> bool {
    baseline_tokens > 0
        && matches!(
            event.command.as_str(),
            TOKEN_COMMAND_SEARCH | TOKEN_COMMAND_MCP_SEARCH
        )
        && event.denominator_kind == TOKEN_BASELINE_SELECTED_CANDIDATES
}

/// Return the confidence label for read-avoidance counters.
fn read_avoidance_confidence_for(
    observed_file_read_replacements: usize,
    modeled_file_reads_avoided: usize,
) -> &'static str {
    if observed_file_read_replacements == 0 && modeled_file_reads_avoided == 0 {
        READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED
    } else if modeled_file_reads_avoided == 0 {
        READ_AVOIDANCE_CONFIDENCE_OBSERVED
    } else {
        READ_AVOIDANCE_CONFIDENCE_MODELED
    }
}

/// Whether a bucket represents observed before/after source compression.
fn is_observed_bucket(bucket: &TokenBucketOverview) -> bool {
    bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA
        || bucket.token_savings_bucket == TOKEN_BUCKET_FULL_FILE_COMPRESSION
        || bucket.confidence == TOKEN_CONFIDENCE_OBSERVED
}

/// Whether a bucket represents modeled counterfactual navigation avoidance.
fn is_modeled_bucket(bucket: &TokenBucketOverview) -> bool {
    bucket.accounting_layer == TOKEN_ACCOUNTING_MODELED_AVOIDANCE || !is_observed_bucket(bucket)
}

#[cfg(test)]
mod tests {
    use super::{
        AgentEfficiencyEvidenceState, READ_AVOIDANCE_CONFIDENCE_MODELED,
        READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED, READ_AVOIDANCE_CONFIDENCE_OBSERVED,
        READ_AVOIDANCE_SCOPE, TOKEN_AVERAGE_DIRECTORY_WALK_PERCENT, TOKEN_BASELINE_DIRECTORY_WALK,
        TOKEN_BUCKET_FULL_FILE_COMPRESSION, TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_DEDUPE_SCOPE_EVENT, TOKEN_ESTIMATE_KIND, TOKEN_ESTIMATE_SCOPE, TOKEN_ESTIMATOR,
        TelemetryContractError, TokenAccountingTotals, TokenOverview, TokenTrendReport,
        TokenTrendWindow, UsageDetailAvailability, UsageInstanceId, UsageInstanceOwner,
        usage_from_estimates, usage_from_text,
    };
    use std::io;

    fn require_eq<T: std::fmt::Debug + PartialEq>(
        actual: &T,
        expected: &T,
        label: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }

    #[test]
    fn usage_instance_ids_validate_and_round_trip() {
        let bytes = [7; 16];
        let identity = UsageInstanceId::from_bytes(bytes);
        assert_eq!(identity.map(UsageInstanceId::as_bytes), Ok(bytes));
        assert_eq!(
            UsageInstanceId::from_bytes([0; 16]),
            Err(TelemetryContractError::ZeroUsageInstanceId)
        );
    }

    #[test]
    fn usage_states_parse_and_missing_report_state_fails_honest()
    -> Result<(), Box<dyn std::error::Error>> {
        for (value, expected) in [
            ("cli_invocation", UsageInstanceOwner::CliInvocation),
            ("mcp_process", UsageInstanceOwner::McpProcess),
            ("library_handle", UsageInstanceOwner::LibraryHandle),
            ("migrated_legacy", UsageInstanceOwner::MigratedLegacy),
        ] {
            require_eq(
                &UsageInstanceOwner::parse(value),
                &Some(expected),
                "usage instance owner parse",
            )?;
            require_eq(&expected.as_str(), &value, "usage instance owner encoding")?;
        }
        require_eq(
            &UsageInstanceOwner::parse("unknown"),
            &None,
            "unknown usage instance owner",
        )?;

        for (value, expected) in [
            ("retained", UsageDetailAvailability::Retained),
            ("partial", UsageDetailAvailability::Partial),
            ("expired", UsageDetailAvailability::Expired),
            ("unavailable", UsageDetailAvailability::Unavailable),
        ] {
            require_eq(
                &UsageDetailAvailability::parse(value),
                &Some(expected),
                "detail availability parse",
            )?;
            require_eq(&expected.as_str(), &value, "detail availability encoding")?;
        }
        require_eq(
            &UsageDetailAvailability::parse("unknown"),
            &None,
            "unknown detail availability",
        )?;
        require_eq(
            &UsageDetailAvailability::default(),
            &UsageDetailAvailability::Unavailable,
            "default detail availability",
        )?;

        let overview = TokenOverview::from_events(&[]);
        require_eq(
            &overview.detail_availability,
            &UsageDetailAvailability::Retained,
            "new overview detail availability",
        )?;
        let mut overview_value = serde_json::to_value(overview)?;
        let overview_object = overview_value
            .as_object_mut()
            .ok_or_else(|| io::Error::other("serialized token overview was not an object"))?;
        overview_object.remove("detail_availability");
        overview_object.remove("agent_efficiency");
        overview_object.remove("average_modeled_tokens_avoided");
        overview_object.remove("average_tokens_avoided");
        overview_object.remove("maximum_tokens_avoided");
        overview_object.remove("average_policy");
        let decoded_overview: TokenOverview = serde_json::from_value(overview_value)?;
        require_eq(
            &decoded_overview.detail_availability,
            &UsageDetailAvailability::Unavailable,
            "missing overview detail availability",
        )?;
        require_eq(
            &decoded_overview.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Unavailable,
            "missing agent-efficiency evidence state",
        )?;
        require_eq(
            &decoded_overview.agent_efficiency.baselines,
            &Vec::new(),
            "missing agent-efficiency baseline rows",
        )?;
        require_eq(
            &decoded_overview.average_tokens_avoided,
            &0,
            "missing average tokens avoided",
        )?;
        require_eq(
            &decoded_overview.maximum_tokens_avoided,
            &0,
            "missing maximum tokens avoided",
        )?;
        require_eq(
            &decoded_overview
                .average_policy
                .directory_walk_baseline_percent,
            &TOKEN_AVERAGE_DIRECTORY_WALK_PERCENT,
            "missing average policy",
        )?;
        require_eq(
            &AgentEfficiencyEvidenceState::Partial.as_str(),
            &"partial",
            "agent-efficiency evidence encoding",
        )?;

        let trends = TokenTrendReport::new(None, TokenTrendWindow::Day, Vec::new());
        require_eq(
            &trends.detail_availability,
            &UsageDetailAvailability::Retained,
            "new trend detail availability",
        )?;
        let mut trends_value = serde_json::to_value(trends)?;
        let trends_object = trends_value
            .as_object_mut()
            .ok_or_else(|| io::Error::other("serialized token trends were not an object"))?;
        trends_object.remove("detail_availability");
        let decoded_trends: TokenTrendReport = serde_json::from_value(trends_value)?;
        require_eq(
            &decoded_trends.detail_availability,
            &UsageDetailAvailability::Unavailable,
            "missing trend detail availability",
        )?;
        Ok(())
    }

    #[test]
    fn modeled_baseline_keys_preserve_legacy_fallback_and_component_boundaries() {
        let event = usage_from_estimates(
            "session",
            "search",
            Some("src/lib.rs".to_string()),
            Some("needle".to_string()),
            100,
            20,
        );
        let expected_key = event.modeled_baseline_key();
        assert_eq!(
            event.effective_baseline_identity().as_ref(),
            event.baseline_identity
        );
        assert_eq!(
            event.effective_baseline_fingerprint().as_ref(),
            event.baseline_fingerprint
        );

        let mut legacy = event.clone();
        legacy.baseline_identity.clear();
        legacy.baseline_fingerprint.clear();
        assert_eq!(legacy.modeled_baseline_key(), expected_key);

        let mut changed_fingerprint = event.clone();
        changed_fingerprint
            .baseline_fingerprint
            .push_str("-changed");
        assert_ne!(changed_fingerprint.modeled_baseline_key(), expected_key);

        let mut left = event.clone();
        left.baseline_identity = "ab".to_string();
        left.baseline_fingerprint = "c".to_string();
        let mut right = event;
        right.baseline_identity = "a".to_string();
        right.baseline_fingerprint = "bc".to_string();
        assert_ne!(left.modeled_baseline_key(), right.modeled_baseline_key());
    }

    #[test]
    fn wide_accounting_totals_narrow_only_at_the_report_boundary() {
        let mut overview = TokenOverview::from_events(&[]);
        overview.apply_accounting_totals(TokenAccountingTotals {
            measured_tokens_saved: 7,
            gross_modeled_tokens_avoided: 100,
            deduped_modeled_tokens_avoided: 30,
            average_modeled_tokens_avoided: 10,
            repeated_baselines_deduped: 2,
            observed_file_read_replacements: 1,
            modeled_file_reads_avoided: 3,
        });
        assert_eq!(overview.measured_tokens_saved, 7);
        assert_eq!(overview.gross_modeled_tokens_avoided, 100);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 30);
        assert_eq!(overview.average_modeled_tokens_avoided, 10);
        assert_eq!(overview.average_tokens_avoided, 17);
        assert_eq!(overview.maximum_tokens_avoided, 37);
        assert_eq!(overview.tokens_avoided, overview.average_tokens_avoided);
        assert_eq!(overview.repeated_baselines_deduped, 2);
        assert_eq!(overview.observed_file_read_replacements, 1);
        assert_eq!(overview.modeled_file_reads_avoided, 3);
        assert_eq!(overview.likely_file_reads_avoided, 4);
        assert_eq!(
            overview.read_avoidance_confidence,
            READ_AVOIDANCE_CONFIDENCE_MODELED
        );

        overview.apply_accounting_totals(TokenAccountingTotals {
            measured_tokens_saved: i128::MAX,
            gross_modeled_tokens_avoided: i128::MIN,
            deduped_modeled_tokens_avoided: i128::MAX,
            average_modeled_tokens_avoided: i128::MAX,
            repeated_baselines_deduped: u128::MAX,
            observed_file_read_replacements: u128::MAX,
            modeled_file_reads_avoided: u128::MAX,
        });
        assert_eq!(overview.measured_tokens_saved, isize::MAX);
        assert_eq!(overview.gross_modeled_tokens_avoided, isize::MIN);
        assert_eq!(overview.deduped_modeled_tokens_avoided, isize::MAX);
        assert_eq!(overview.average_modeled_tokens_avoided, isize::MAX);
        assert_eq!(overview.average_tokens_avoided, isize::MAX);
        assert_eq!(overview.maximum_tokens_avoided, isize::MAX);
        assert_eq!(overview.tokens_avoided, isize::MAX);
        assert_eq!(overview.repeated_baselines_deduped, usize::MAX);
        assert_eq!(overview.observed_file_read_replacements, usize::MAX);
        assert_eq!(overview.modeled_file_reads_avoided, usize::MAX);
        assert_eq!(overview.likely_file_reads_avoided, usize::MAX);
    }

    #[test]
    fn usage_from_text_tracks_positive_and_negative_savings() {
        let positive = usage_from_text("s", "outline", None, None, "abcdefghijkl", "abcd");
        assert_eq!(positive.estimated_tokens_without_projectatlas, Some(3));
        assert_eq!(positive.estimated_tokens_with_projectatlas, Some(1));
        assert_eq!(positive.estimated_tokens_saved, Some(2));

        let negative = usage_from_estimates("s", "overview", None, None, 1, 4);
        assert_eq!(negative.estimated_tokens_saved, Some(-3));
    }

    #[test]
    fn huge_estimates_use_saturating_signed_delta() {
        let event = usage_from_estimates("s", "large-repo", None, None, usize::MAX, 0);
        assert_eq!(event.estimated_tokens_saved, Some(isize::MAX));
    }

    #[test]
    fn overview_recomputes_saved_from_aggregate_without_and_with() {
        let mut first = usage_from_estimates("s", "a", None, None, 20, 50);
        first.estimated_tokens_saved = Some(999);
        let mut second = usage_from_estimates("s", "b", None, None, 0, 10);
        second.estimated_tokens_saved = Some(999);
        let overview = TokenOverview::from_events(&[first, second]);

        assert_eq!(overview.estimate_kind, TOKEN_ESTIMATE_KIND);
        assert_eq!(overview.estimator, TOKEN_ESTIMATOR);
        assert_eq!(overview.estimate_scope, TOKEN_ESTIMATE_SCOPE);
        assert_eq!(overview.calls, 2);
        assert_eq!(overview.estimated_without_projectatlas, 20);
        assert_eq!(overview.estimated_with_projectatlas, 60);
        assert_eq!(overview.estimated_saved, -40);
        assert_eq!(overview.savings_rate, Some(-2.0));
    }

    #[test]
    fn overview_keeps_source_compression_and_navigation_buckets_separate() {
        let overview = TokenOverview::from_events(&[
            usage_from_text("s", "summary", None, None, "abcdefghijkl", "abcd"),
            usage_from_estimates("s", "search", None, None, 100, 20),
        ]);

        assert_eq!(overview.calls, 2);
        assert_eq!(overview.buckets.len(), 2);
        assert_eq!(
            overview.buckets[0].token_savings_bucket,
            TOKEN_BUCKET_FULL_FILE_COMPRESSION
        );
        assert_eq!(
            overview.buckets[1].token_savings_bucket,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE
        );
        assert_eq!(overview.observed_file_read_replacements, 1);
        assert_eq!(overview.modeled_file_reads_avoided, 1);
        assert_eq!(overview.likely_file_reads_avoided, 2);
        assert_eq!(
            overview.read_avoidance_confidence,
            READ_AVOIDANCE_CONFIDENCE_MODELED
        );
        assert_eq!(overview.read_avoidance_scope, READ_AVOIDANCE_SCOPE);
    }

    #[test]
    fn observed_only_overview_reports_observed_read_avoidance_confidence() {
        let overview = TokenOverview::from_events(&[usage_from_text(
            "s",
            "summary",
            None,
            None,
            "abcdefghijkl",
            "abcd",
        )]);

        assert_eq!(overview.observed_file_read_replacements, 1);
        assert_eq!(overview.modeled_file_reads_avoided, 0);
        assert_eq!(overview.likely_file_reads_avoided, 1);
        assert_eq!(
            overview.read_avoidance_confidence,
            READ_AVOIDANCE_CONFIDENCE_OBSERVED
        );
    }

    #[test]
    fn bucket_overview_does_not_infer_read_avoidance_without_raw_events() {
        let event_overview = TokenOverview::from_events(&[
            usage_from_text("s", "summary", None, None, "abcdefghijkl", "abcd"),
            usage_from_estimates("s", "search", None, None, 100, 20),
        ]);
        let bucket_overview = TokenOverview::from_buckets(event_overview.buckets);

        assert_eq!(bucket_overview.observed_file_read_replacements, 0);
        assert_eq!(bucket_overview.modeled_file_reads_avoided, 0);
        assert_eq!(bucket_overview.likely_file_reads_avoided, 0);
        assert_eq!(
            bucket_overview.read_avoidance_confidence,
            READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED
        );
    }

    #[test]
    fn non_file_read_navigation_events_do_not_increment_read_avoidance() {
        let overview = TokenOverview::from_events(&[
            usage_from_estimates("s", "overview", None, None, 100, 20),
            usage_from_estimates("s", "folders", None, None, 100, 20),
            usage_from_estimates("s", "files", None, None, 100, 20),
            usage_from_estimates("s", "mcp.atlas_health", None, None, 100, 20),
            usage_from_estimates("s", "mcp.atlas_purpose_queue", None, None, 100, 20),
        ]);

        assert_eq!(overview.modeled_file_reads_avoided, 0);
        assert_eq!(overview.likely_file_reads_avoided, 0);
    }

    #[test]
    fn zero_baseline_events_do_not_increment_read_avoidance() {
        let overview = TokenOverview::from_events(&[
            usage_from_estimates("s", "search", None, None, 0, 20),
            usage_from_text("s", "summary", None, None, "", "summary"),
        ]);

        assert_eq!(overview.observed_file_read_replacements, 0);
        assert_eq!(overview.modeled_file_reads_avoided, 0);
        assert_eq!(overview.likely_file_reads_avoided, 0);
    }

    #[test]
    fn average_policy_halves_only_deduped_directory_walk_baselines() {
        let mut first_folder =
            usage_from_estimates("s", "folders", Some("src".to_string()), None, 101, 20);
        first_folder.denominator_kind = TOKEN_BASELINE_DIRECTORY_WALK.to_string();
        first_folder.baseline_identity = "directory:src".to_string();
        first_folder.baseline_fingerprint = "directory:src@1".to_string();
        let mut second_folder = first_folder.clone();
        second_folder.estimated_tokens_with_projectatlas = Some(10);
        second_folder.estimated_tokens_saved = Some(91);

        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcdabcd",
                "ab",
            ),
            first_folder,
            second_folder,
            usage_from_estimates("s", "search", None, Some("token".to_string()), 80, 20),
        ]);

        assert_eq!(overview.measured_tokens_saved, 1);
        assert_eq!(overview.gross_modeled_tokens_avoided, 232);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 131);
        assert_eq!(overview.average_modeled_tokens_avoided, 80);
        assert_eq!(overview.average_tokens_avoided, 81);
        assert_eq!(overview.maximum_tokens_avoided, 132);
        assert_eq!(overview.tokens_avoided, overview.average_tokens_avoided);
        assert_eq!(overview.repeated_baselines_deduped, 1);
    }

    #[test]
    fn average_directory_walk_policy_preserves_signed_payload_cost() {
        let mut event = usage_from_estimates("s", "folders", Some("src".to_string()), None, 5, 4);
        event.denominator_kind = TOKEN_BASELINE_DIRECTORY_WALK.to_string();
        let overview = TokenOverview::from_events(&[event]);

        assert_eq!(overview.average_modeled_tokens_avoided, -2);
        assert_eq!(overview.average_tokens_avoided, -2);
        assert_eq!(overview.maximum_tokens_avoided, 1);
        assert_eq!(overview.tokens_avoided, -2);
    }

    #[test]
    fn raw_accounting_narrows_once_after_wide_signed_aggregation() {
        let bound = isize::MAX as usize;
        let mut events = vec![
            usage_from_estimates("s", "search", None, Some("a".to_string()), bound, 0),
            usage_from_estimates("s", "search", None, Some("b".to_string()), bound, 0),
            usage_from_estimates("s", "search", None, Some("c".to_string()), 0, bound),
        ];
        for event in &mut events {
            event.dedupe_scope = TOKEN_DEDUPE_SCOPE_EVENT.to_string();
        }

        let overview = TokenOverview::from_events(&events);

        assert_eq!(overview.gross_modeled_tokens_avoided, isize::MAX);
        assert_eq!(overview.deduped_modeled_tokens_avoided, isize::MAX);
        assert_eq!(overview.average_modeled_tokens_avoided, isize::MAX);
        assert_eq!(overview.average_tokens_avoided, isize::MAX);
        assert_eq!(overview.maximum_tokens_avoided, isize::MAX);
    }

    #[test]
    fn bucket_accounting_narrows_once_after_wide_signed_aggregation() {
        let modeled_bucket = |saved| {
            let mut bucket = TokenOverview::from_estimated_totals(1, 1, 1)
                .buckets
                .remove(0);
            bucket.estimated_saved = saved;
            bucket
        };
        let expected = isize::MAX - 1;
        for order in [
            [isize::MAX, isize::MAX, isize::MIN],
            [isize::MAX, isize::MIN, isize::MAX],
        ] {
            let overview = TokenOverview::from_buckets(
                order.into_iter().map(&modeled_bucket).collect::<Vec<_>>(),
            );
            assert_eq!(overview.gross_modeled_tokens_avoided, expected);
            assert_eq!(overview.average_modeled_tokens_avoided, expected);
            assert_eq!(overview.average_tokens_avoided, expected);
            assert_eq!(overview.maximum_tokens_avoided, expected);
        }
    }

    #[test]
    fn overview_dedupes_repeated_modeled_baselines_without_hiding_measured_savings() {
        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcdabcd",
                "ab",
            ),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 40),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 30),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 20),
        ]);

        assert_eq!(overview.estimated_saved, 1111);
        assert_eq!(overview.legacy_gross_estimated_saved, 1111);
        assert_eq!(overview.measured_tokens_saved, 1);
        assert_eq!(overview.gross_modeled_tokens_avoided, 1110);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 310);
        assert_eq!(overview.tokens_avoided, 311);
        assert_eq!(overview.repeated_baselines_deduped, 2);
        assert_eq!(overview.observed_file_read_replacements, 1);
        assert_eq!(overview.modeled_file_reads_avoided, 3);
        assert_eq!(overview.likely_file_reads_avoided, 4);
    }
}
