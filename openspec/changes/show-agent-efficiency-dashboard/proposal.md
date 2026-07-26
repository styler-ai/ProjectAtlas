## Why

ProjectAtlas v0.4 now has a reproducible three-arm navigation benchmark, but the existing token report cannot show that comparison without mixing controlled evidence into live telemetry. Users need one typed, honest view that keeps observed and modeled session accounting separate from a validated benchmark comparison.

## What Changes

- Add an optional, bounded, read-only benchmark-result input to the existing token overview request.
- Validate the supported benchmark schema, candidate/runtime identity, run completeness, comparison arms, required metrics, and numeric bounds before exposing any comparison values.
- Extend the authoritative typed token overview with explicit unavailable, incompatible, partial, failed, and compatible comparison states.
- Render compatible tool-call, navigation-visit, broad/full-read, backtrack, context, setup/runtime, and break-even evidence through the existing CLI JSON/TOON, MCP, and Ratatui dashboard paths.
- Keep provider token counters descriptive and separate from causal navigation savings.
- Preserve the existing conservative token/file-read arithmetic, trend mode, semantic palette, compact layout, and backward-compatible output fields.

## Capabilities

### New Capabilities

- `agent-efficiency-comparison`: Validate one versioned navigation benchmark artifact and project its matched candidate/baseline evidence into a bounded typed report without persisting it.

### Modified Capabilities

- `token-impact-reference-dashboard`: Add a distinct benchmark-comparison panel and compact fallback while preserving the accepted reference hierarchy and accounting semantics.

## Impact

The change affects the core token-report types, the transport-independent token service, existing CLI and MCP token parameters, TOON/JSON serialization, the Ratatui overview, focused smoke/E2E tests, and the owning telemetry architecture documentation. It adds no crate, dependency, MCP tool, SQLite schema, migration, write path, or background work and is ready for implementation in v0.4.0.

## Non-Goals

- Do not infer saved calls, wrong selections, or backtracks from ordinary telemetry.
- Do not persist benchmark artifacts or copy their counters into SQLite.
- Do not treat provider billing counters as causal navigation savings.
- Do not add a second dashboard, analytics framework, or benchmark collector.
- Do not alter the dedicated trend dashboard or the established conservative token headline.
