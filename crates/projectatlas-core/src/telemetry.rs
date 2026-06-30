//! Purpose: Track `ProjectAtlas` token savings telemetry.

use crate::outline::estimate_tokens;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// Token savings event for a funnel command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageEvent {
    /// Session identifier.
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
    /// Conservative headline tokens avoided estimate.
    pub tokens_avoided: isize,
    /// Legacy all-bucket gross estimate retained for migration diagnostics.
    pub legacy_gross_estimated_saved: isize,
    /// Number of duplicate modeled baseline events collapsed by dedupe.
    pub repeated_baselines_deduped: usize,
    /// Optional local tokenizer calibration for indexed UTF-8 files.
    pub calibration: Option<TokenCalibrationOverview>,
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
    /// Optional session filter.
    pub session: Option<String>,
    /// Grouping window.
    pub window: TokenTrendWindow,
    /// Period aggregates ordered oldest to newest.
    pub periods: Vec<TokenTrendPeriod>,
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
        let measured_tokens_saved = measured_tokens_saved_from_buckets(&buckets);
        let gross_modeled_tokens_avoided = modeled_tokens_saved_from_buckets(&buckets);
        let tokens_avoided =
            saturating_isize_add(measured_tokens_saved, gross_modeled_tokens_avoided);
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
            tokens_avoided,
            legacy_gross_estimated_saved: saved,
            repeated_baselines_deduped: 0,
            calibration: None,
            buckets,
        }
    }

    /// Attach a local tokenizer calibration section.
    pub fn set_calibration(&mut self, calibration: TokenCalibrationOverview) {
        self.calibration = Some(calibration);
    }

    /// Apply separated measured/modeled accounting totals from raw usage events.
    pub fn apply_accounting_from_events(&mut self, events: &[UsageEvent]) {
        let summary = TokenAccountingSummary::from_events(events);
        self.measured_tokens_saved = summary.measured_tokens_saved;
        self.gross_modeled_tokens_avoided = summary.gross_modeled_tokens_avoided;
        self.deduped_modeled_tokens_avoided = summary.deduped_modeled_tokens_avoided;
        self.tokens_avoided = summary.tokens_avoided;
        self.repeated_baselines_deduped = summary.repeated_baselines_deduped;
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
        }
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
    /// Conservative headline tokens avoided.
    tokens_avoided: isize,
    /// Number of duplicate modeled baseline events collapsed by dedupe.
    repeated_baselines_deduped: usize,
}

impl TokenAccountingSummary {
    /// Build separated accounting totals from raw usage events.
    fn from_events(events: &[UsageEvent]) -> Self {
        let mut measured_tokens_saved = 0isize;
        let mut gross_modeled_tokens_avoided = 0isize;
        let mut modeled_baselines = BTreeMap::<ModeledBaselineKey, ModeledBaselineTotals>::new();

        for event in events {
            let (Some(without), Some(with)) = (
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
            ) else {
                continue;
            };
            let delta = token_delta(without, with);
            if is_observed_event(event) {
                measured_tokens_saved = saturating_isize_add(measured_tokens_saved, delta);
                continue;
            }
            if !is_modeled_event(event) {
                continue;
            }
            gross_modeled_tokens_avoided =
                saturating_isize_add(gross_modeled_tokens_avoided, delta);
            let entry = modeled_baselines
                .entry(ModeledBaselineKey::from_event(event))
                .or_default();
            entry.calls = entry.calls.saturating_add(1);
            entry.baseline_without_projectatlas = entry.baseline_without_projectatlas.max(without);
            entry.emitted_with_projectatlas =
                entry.emitted_with_projectatlas.saturating_add(with as u128);
        }

        let mut deduped_modeled_tokens_avoided = 0isize;
        let mut repeated_baselines_deduped = 0usize;
        for totals in modeled_baselines.values() {
            if totals.calls > 1 {
                repeated_baselines_deduped =
                    repeated_baselines_deduped.saturating_add(totals.calls.saturating_sub(1));
            }
            let delta = aggregate_token_delta(
                totals.baseline_without_projectatlas as u128,
                totals.emitted_with_projectatlas,
            );
            deduped_modeled_tokens_avoided =
                saturating_isize_add(deduped_modeled_tokens_avoided, delta);
        }
        let tokens_avoided =
            saturating_isize_add(measured_tokens_saved, deduped_modeled_tokens_avoided);
        Self {
            measured_tokens_saved,
            gross_modeled_tokens_avoided,
            deduped_modeled_tokens_avoided,
            tokens_avoided,
            repeated_baselines_deduped,
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
    if without >= with {
        let delta = without - with;
        if delta > isize::MAX as u128 {
            isize::MAX
        } else {
            delta as isize
        }
    } else {
        let delta = with - without;
        if delta > isize::MAX as u128 {
            isize::MIN
        } else {
            -(delta as isize)
        }
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

/// Add signed token totals without overflowing.
fn saturating_isize_add(left: isize, right: isize) -> isize {
    left.saturating_add(right)
}

/// Sum observed saved-token buckets.
fn measured_tokens_saved_from_buckets(buckets: &[TokenBucketOverview]) -> isize {
    buckets
        .iter()
        .filter(|bucket| is_observed_bucket(bucket))
        .fold(0isize, |acc, bucket| {
            saturating_isize_add(acc, bucket.estimated_saved)
        })
}

/// Sum modeled avoided-token buckets.
fn modeled_tokens_saved_from_buckets(buckets: &[TokenBucketOverview]) -> isize {
    buckets
        .iter()
        .filter(|bucket| is_modeled_bucket(bucket))
        .fold(0isize, |acc, bucket| {
            saturating_isize_add(acc, bucket.estimated_saved)
        })
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
        TOKEN_BUCKET_FULL_FILE_COMPRESSION, TOKEN_BUCKET_NAVIGATION_AVOIDANCE, TOKEN_ESTIMATE_KIND,
        TOKEN_ESTIMATE_SCOPE, TOKEN_ESTIMATOR, TokenOverview, usage_from_estimates,
        usage_from_text,
    };

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
            usage_from_estimates("s", "folders", None, None, 100, 20),
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
            usage_from_estimates("s", "folders", None, Some("token".to_string()), 400, 40),
            usage_from_estimates("s", "folders", None, Some("token".to_string()), 400, 30),
            usage_from_estimates("s", "folders", None, Some("token".to_string()), 400, 20),
        ]);

        assert_eq!(overview.estimated_saved, 1111);
        assert_eq!(overview.legacy_gross_estimated_saved, 1111);
        assert_eq!(overview.measured_tokens_saved, 1);
        assert_eq!(overview.gross_modeled_tokens_avoided, 1110);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 310);
        assert_eq!(overview.tokens_avoided, 311);
        assert_eq!(overview.repeated_baselines_deduped, 2);
    }
}
