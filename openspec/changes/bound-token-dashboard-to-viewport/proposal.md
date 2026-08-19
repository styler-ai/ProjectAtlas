# Change: Bound Token Dashboards To The Terminal Viewport

## Why

The token overview and trend TUI snapshots render fixed buffers with an 80-column minimum and fixed heights. A smaller terminal therefore wraps rows or scrolls earlier dashboard content out of view. The overview also measures width separately for graph loading and rendering, allowing resize-dependent output and unnecessary graph work.

## What Changes

- Capture one validated terminal viewport for each token TUI invocation.
- Preserve the existing full dashboard when its minimum dimensions fit.
- Render a bounded compact Ratatui snapshot when width or height is insufficient.
- Use the same viewport to decide whether the optional Atlas preview is loaded and rendered.
- Add mandatory unit, CLI, platform, and installed-candidate regressions at the exact dimension boundaries.

## Capabilities

### New Capabilities

- `terminal-bounded-token-dashboard`: Token overview and trend snapshots remain within the selected terminal viewport and retain their highest-priority facts as space permits.

### Modified Capabilities

- `token-impact-estimate-reporting`: Full-size dashboard semantics remain unchanged while compact output gains explicit two-dimensional bounds.

## Impact

The change is limited to the existing CLI token TUI adapter, its tests, mandatory CI selection, packaged release verification, and durable terminal guidance. It does not alter token telemetry storage, calculation, MCP payloads, or database schemas.
