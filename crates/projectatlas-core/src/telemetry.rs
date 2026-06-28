//! Purpose: Track `ProjectAtlas` token savings telemetry.

use crate::outline::estimate_tokens;
use serde::{Deserialize, Serialize};

/// Token overview counting mode.
pub const TOKEN_ESTIMATE_KIND: &str = "heuristic";
/// Token overview estimator identifier.
pub const TOKEN_ESTIMATOR: &str = "chars_or_bytes_div_ceil_4";
/// Token overview scope label.
pub const TOKEN_ESTIMATE_SCOPE: &str = "workflow_payload_estimate_not_model_billing_tokens";

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
}

impl TokenOverview {
    /// Build an overview from usage events.
    #[must_use]
    pub fn from_events(events: &[UsageEvent]) -> Self {
        let mut without = 0u128;
        let mut with = 0u128;
        let mut calls = 0u128;
        for event in events {
            let (Some(event_without), Some(event_with)) = (
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
            ) else {
                continue;
            };
            calls = calls.saturating_add(1);
            without = without.saturating_add(event_without as u128);
            with = with.saturating_add(event_with as u128);
        }
        Self::from_estimated_totals(calls, without, with)
    }

    /// Build an overview from aggregate heuristic token totals.
    #[must_use]
    pub fn from_estimated_totals(calls: u128, without: u128, with: u128) -> Self {
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
        }
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
    usage_from_estimates(session_id, command, path, query, without, with)
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
    }
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
        TOKEN_ESTIMATE_KIND, TOKEN_ESTIMATE_SCOPE, TOKEN_ESTIMATOR, TokenOverview, UsageEvent,
        usage_from_estimates, usage_from_text,
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
        let overview = TokenOverview::from_events(&[
            UsageEvent {
                session_id: "s".to_string(),
                command: "a".to_string(),
                path: None,
                query: None,
                estimated_tokens_without_projectatlas: Some(20),
                estimated_tokens_with_projectatlas: Some(50),
                estimated_tokens_saved: Some(999),
            },
            UsageEvent {
                session_id: "s".to_string(),
                command: "b".to_string(),
                path: None,
                query: None,
                estimated_tokens_without_projectatlas: Some(0),
                estimated_tokens_with_projectatlas: Some(10),
                estimated_tokens_saved: Some(999),
            },
        ]);

        assert_eq!(overview.estimate_kind, TOKEN_ESTIMATE_KIND);
        assert_eq!(overview.estimator, TOKEN_ESTIMATOR);
        assert_eq!(overview.estimate_scope, TOKEN_ESTIMATE_SCOPE);
        assert_eq!(overview.calls, 2);
        assert_eq!(overview.estimated_without_projectatlas, 20);
        assert_eq!(overview.estimated_with_projectatlas, 60);
        assert_eq!(overview.estimated_saved, -40);
        assert_eq!(overview.savings_rate, Some(-2.0));
    }
}
