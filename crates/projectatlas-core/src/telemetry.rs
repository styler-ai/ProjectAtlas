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
        Self::from_buckets(buckets)
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
        Self {
            estimate_kind: TOKEN_ESTIMATE_KIND.to_string(),
            estimator: TOKEN_ESTIMATOR.to_string(),
            estimate_scope: TOKEN_ESTIMATE_SCOPE.to_string(),
            calls: saturating_u128_to_usize(calls),
            estimated_without_projectatlas: saturating_u128_to_usize(without),
            estimated_with_projectatlas: saturating_u128_to_usize(with),
            estimated_saved: saved,
            savings_rate,
            buckets,
        }
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
}

impl TokenBucketKey {
    /// Build a grouping key from one usage event.
    fn from(event: &UsageEvent) -> Self {
        Self {
            token_savings_bucket: event.token_savings_bucket.clone(),
            provider: event.provider.clone(),
            model: event.model.clone(),
            tokenizer_backend: event.tokenizer_backend.clone(),
            accuracy: event.accuracy.clone(),
            baseline_kind: event.baseline_kind.clone(),
            confidence: event.confidence.clone(),
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
    usage_from_estimates_with_context(
        session_id,
        command,
        path,
        query,
        without,
        with,
        TOKEN_BUCKET_FULL_FILE_COMPRESSION,
        TOKEN_BASELINE_FULL_FILE,
        TOKEN_CONFIDENCE_OBSERVED,
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
    usage_from_estimates_with_context(
        session_id,
        command,
        path,
        query,
        estimated_without_projectatlas,
        estimated_with_projectatlas,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_CONFIDENCE_INFERRED,
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
}
