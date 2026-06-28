//! Purpose: Track `ProjectAtlas` token savings telemetry.

use crate::outline::estimate_tokens;
use serde::{Deserialize, Serialize};

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
    /// Number of tracked calls.
    pub calls: usize,
    /// Total baseline estimate.
    pub estimated_without_projectatlas: usize,
    /// Total `ProjectAtlas` estimate.
    pub estimated_with_projectatlas: usize,
    /// Total saved tokens.
    pub estimated_saved: isize,
    /// Savings rate from 0.0 to 1.0.
    pub savings_rate: Option<f64>,
}

impl TokenOverview {
    /// Build an overview from usage events.
    #[must_use]
    pub fn from_events(events: &[UsageEvent]) -> Self {
        let mut without = 0usize;
        let mut with = 0usize;
        let mut saved = 0isize;
        let mut calls = 0usize;
        for event in events {
            let (Some(event_without), Some(event_with), Some(event_saved)) = (
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
                event.estimated_tokens_saved,
            ) else {
                continue;
            };
            calls += 1;
            without += event_without;
            with += event_with;
            saved += event_saved;
        }
        let savings_rate = if without == 0 {
            None
        } else {
            Some(saved as f64 / without as f64)
        };
        Self {
            calls,
            estimated_without_projectatlas: without,
            estimated_with_projectatlas: with,
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

#[cfg(test)]
mod tests {
    use super::{usage_from_estimates, usage_from_text};

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
}
