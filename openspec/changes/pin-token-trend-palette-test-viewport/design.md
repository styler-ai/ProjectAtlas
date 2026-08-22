## Context

The production trend renderer captures one terminal viewport and deliberately selects the full layout only at 80 by 30 or larger. The palette regression asserts cells emitted only by that full layout but currently calls the production entry point, so a real short PTY changes the branch under test. The existing `render_token_trend_dashboard_with_theme_in_viewport` and `test_viewport` helpers already provide the required deterministic test seam.

## Goals / Non-Goals

**Goals:**

- Make the full-layout palette regression independent of the host terminal.
- Preserve every existing light-theme semantic palette assertion.
- Preserve production live-viewport capture and compact/full selection unchanged.
- Prove the correction in both non-TTY and approximately 80-by-24 PTY execution.

**Non-Goals:**

- Change a production renderer, threshold, palette, or terminal contract.
- Add an abstraction, dependency, environment override, or committed PTY harness.
- Expand this one-test correction into a token TUI redesign.

## Decisions

### Inject the viewport at the existing test seam

Change only `trend_dashboard_light_theme_remaps_semantic_palette` to call `render_token_trend_dashboard_with_theme_in_viewport` with `test_viewport(140, 30)`. This viewport is explicitly large enough for the established full trend layout and keeps the test focused on palette semantics rather than terminal discovery.

Alternative rejected: set `COLUMNS` and `LINES`. Environment mutation is process-global, can race parallel tests, and still exercises viewport discovery rather than the layout-specific rendering contract.

Alternative rejected: change the production wrapper or full-layout threshold. The approximately 80-by-24 PTY is supposed to select compact output; changing production to satisfy the test would regress issue #462's accepted behavior.

### Keep the assertions and renderer ownership unchanged

The test retains its ANSI, light panel background, saved-green, identity-color, hard-coded-cyan absence, and dark-background absence assertions. The existing renderer remains the sole palette owner; the test supplies only its input viewport.

### Record production and test viewport ownership once

The focused Mermaid view in `docs/projectatlas-3-architecture.md` shows production live viewport capture and its full/compact branch separately from deterministic test injection. It documents the existing control flow and does not introduce another renderer or architecture layer.

## Risks / Trade-offs

- **[A fixed viewport hides production small-terminal defects]** -> Existing compact/full boundary tests and production entry points remain unchanged; this regression owns only full-layout palette semantics.
- **[The test passes after assertions are weakened]** -> Retain the complete existing assertion set verbatim.
- **[Only non-TTY execution is verified]** -> Repeat the exact regression in both non-TTY and approximately 80-by-24 PTY contexts.
- **[Documentation implies a second renderer]** -> Show both paths converging on the existing trend rendering boundary and inspect the rendered Mermaid for semantic truth.

## Migration Plan

No migration or rollback path is required. The change modifies only a test call site and specification documentation. Reverting that call restores the prior test behavior without affecting production or stored data.

## Open Questions

None. The existing viewport-injected helper and 140-by-30 test viewport settle the implementation boundary.
