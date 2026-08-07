## Why

ProjectAtlas currently presents the all-files folder-navigation estimate as the primary tokens-avoided value, which is a useful maximum but can be mistaken for expected agent behavior. The v0.4.4 transparency release should preserve that evidence while adding a clearly labeled 50% average policy estimate as the main headline.

## What Changes

- Report both average and maximum tokens avoided from the same telemetry events.
- Calculate average modeled folder-navigation savings from 50% of the deduped aggregate all-files folder baseline before subtracting the unchanged aggregate Atlas payload; keep measured compression and every other savings category unchanged.
- Retain the current all-files folder-navigation calculation as maximum tokens avoided.
- Keep the token TUI layout substantially unchanged: show average tokens avoided as the primary hero, then render the complete without-minus-with equation once for the average and once for the maximum, with no policy subtext in the hero panel.
- Preserve compatibility for existing telemetry rows, SQLite databases, CLI/MCP callers, TOON/JSON consumers, trend reports, and non-folder accounting; label predecessor overflow rows whose denominator can no longer be classified.

### Non-Goals

- Claiming that the 50% policy is benchmark-derived, provider-billed, or a measurement of one specific agent run.
- Changing search baselines, measured full-file compression, candidate-file modeling, deduplication, tokenizer behavior, or read-avoidance counts.
- Redesigning the dashboard, adding explanatory policy prose to the hero panel, adding configuration for the fixed v0.4.4 policy, or adding a database schema/migration; one bounded directory-walk overflow discriminator is permitted so new retained aggregates remain truthful.

This focused bug fix is ready for implementation as the only product issue in v0.4.4.

## Capabilities

### New Capabilities

- `token-impact-estimate-reporting`: Defines compatible average and maximum token-impact calculations and their CLI, MCP, structured-report, and TUI presentation.

### Modified Capabilities

## Impact

- Expected code touch points: shared telemetry aggregation and serialization, the existing token TUI, and their owning tests.
- Existing SQLite event storage and schema remain authoritative and unchanged; average and maximum values are derived at report time, with at most one extra reserved overflow dimension preserving the folder discriminator after normal dimension capacity is reached.
- No new crate, dependency, service, command, or configuration surface is expected.
