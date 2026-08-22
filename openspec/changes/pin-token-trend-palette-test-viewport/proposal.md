## Why

`token_tui::tests::trend_dashboard_light_theme_remaps_semantic_palette` renders through the production entry point, so the test inherits the host terminal viewport. A normal non-TTY run selects the full trend layout and passes, while an approximately 80-by-24 PTY correctly selects the compact layout and then fails assertions that belong to the full layout. Production viewport behavior is correct; the layout-specific regression does not own its test viewport.

## What Changes

- Render only the full-layout light-theme trend regression through the existing viewport-injected helper with `test_viewport(140, 30)`.
- Retain the complete semantic palette assertion set and all production viewport behavior.
- Verify the regression repeatedly in non-TTY and approximately 80-by-24 PTY execution, then run the focused token TUI and repository-required gates.
- Document the existing separation between production live-viewport selection and deterministic test viewport injection in one focused Mermaid view.

## Capabilities

### New Capabilities

- `token-dashboard-test-determinism`: Layout-specific token-dashboard regressions select an explicit qualifying viewport while real TUI invocations continue to use the captured terminal viewport.

### Modified Capabilities

None.

## Impact

The implementation is limited to one test in `crates/projectatlas-cli/src/token_tui.rs`, its verification, this OpenSpec change, the issue mapping, and the focused architecture view in `docs/projectatlas-3-architecture.md`. It changes no production code, public interface, dependency, telemetry calculation, persistence, CLI/MCP payload, or terminal-layout boundary.

## Non-Goals

- Change production viewport capture or the 80-by-30 full-trend boundary.
- Force the production renderer into the full layout in a small terminal.
- Weaken or remove semantic palette assertions.
- Add a renderer abstraction, dependency, committed PTY harness, or architecture documentation beyond the single focused ownership view.
- Change telemetry calculations, persistence, CLI/MCP payloads, or public interfaces.

This test-only correction is ready for implementation after its specification is accepted.
