## Why

The token overview dashboard is a human-facing trust surface. The previous v0.3.25 cleanup made the accounting less repetitive, but the user supplied a stronger visual reference for the target experience: a professional Ratatui dashboard with one dominant savings equation, a clear file-read-avoidance strip, compact composition/signal panels, a readable source table, a status row, and a small ProjectAtlas mascot mark.

ProjectAtlas also now has a named mascot, Ani. Ani should appear as a small recognizable terminal-native pixel/block mark near the ProjectAtlas title in the token overview, and the README/docs should identify Ani as the ProjectAtlas mascot.

## What Changes

- Add the supplied token dashboard reference image at `docs/design/token-impact-tui-reference.png`.
- Add the supplied Ani mascot reference image at `docs/design/ani-mascot-reference.png`.
- Rework `projectatlas token --view tui` overview mode to match the reference screenshot's visible layout, color palette, widget hierarchy, labels, section order, and information density. Do not substitute a materially different dashboard design:
  - top title `ProjectAtlas Token Impact` with a small Ani pixel/block mascot beside it,
  - right-side metadata for session, lookup count, and estimator,
  - dominant `TOTAL TOKENS AVOIDED` panel,
  - visible equation `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas`,
  - `File Reads Avoided` strip with observed summary/slice reads, search-modeled narrowing, and confidence,
  - `Savings Composition` and `Signal` panels,
  - `Where The Savings Came From` table with styled headers and clear spacing,
  - `Calibration & Notes` panel.
- Match the screenshot's dark navy background, blue/cyan headings and borders, green saved-token values, yellow modeled/confidence values, muted gray body text, segmented bar styling, and compact footer/status row. Use ivory/off-white for ProjectAtlas identity and ProjectAtlas-origin values so Ani, the `ProjectAtlas` title, `With ProjectAtlas`, file reads avoided, observed summaries/slices, measured summaries/slices, and the `Summaries and slices` row share one visual family; reserve blue for original/counterfactual baseline values such as `Without ProjectAtlas`. The soft glow in the generated image is the only explicit visual non-goal because Ratatui cannot guarantee it portably.
- Keep the dashboard math internally consistent: visible token totals and source rows must reconcile to `tokens_avoided`, and visible file-read totals must reconcile to `likely_file_reads_avoided`.
- Continue using Ratatui standard widgets and style primitives rather than a custom renderer or bitmap dependency.
- Keep the detailed `projectatlas token --view tui --trend <window>` dashboard intact; the overview screen itself is a KPI dashboard and does not need a large chart.
- Update README/docs so the token TUI and Ani mascot references are discoverable.

## Capabilities

### New Capabilities

- `token-impact-reference-dashboard`: Defines the visual and accounting contract for the reference-matched token overview TUI, including Ani mascot treatment.

### Modified Capabilities

- `token-tui-dashboard`: The existing token overview dashboard contract is strengthened from a simplified dashboard to a reference-matched Ratatui dashboard.
- Public documentation now includes the versioned design references and the Ani mascot note.

## Release Scope

This change is scheduled for v0.3.25 and is ready for implementation in the current release PR. It is a CLI/TUI and documentation change only; MCP token report payloads remain unchanged.

## Non-Goals

- Do not add image protocol support such as sixel/iTerm graphics or a bitmap terminal renderer.
- Do not make Ani large enough to compete with the token headline.
- Do not change the token telemetry storage schema.
- Do not expose gross compatibility totals as a second competing saved-token headline.
- Do not remove trend support from the dedicated trend dashboard.

## Pre-Mortem

Likely failure modes:

- The dashboard copies the screenshot visually but reintroduces incorrect arithmetic where `without - with` does not equal the headline saved value.
- The Ani mascot becomes too large, unreadable, or dependent on terminal image features that are not portable.
- The implementation hand-draws the whole dashboard and bypasses Ratatui widgets, making future layout changes brittle.
- The table headers again run into first-row content at narrow terminal widths.
- The new design hides file-read avoidance, observed-vs-modeled split, confidence, or calibration notes that existed in the previous dashboard.
- Dark styling looks good in a screenshot but becomes unreadable in a normal terminal color palette.
- Docs mention Ani or the design reference but the assets are not versioned or linked.

Mitigations:

- Treat the visible equation as a reconciled conservative baseline: `without_projectatlas = with_projectatlas + tokens_avoided`, so the displayed equation always adds up.
- Render Ani as a tiny Unicode/block/line mascot mark using Ratatui text styling; no bitmap renderer dependency.
- Use Ratatui `Layout`, `Block`, `Paragraph`, `Gauge`, `Table`, and `Chart` widgets for the dashboard sections.
- Add snapshot-style string/style tests for section order, labels, table header spacing, color roles, Ani presence, and accounting equations.
- Preserve all existing token fields in fewer, clearer sections: saved tokens, with/without comparison, file reads avoided, observed and modeled parts, confidence, source rows, signal, and calibration.
- Use the screenshot-matched dark navy, cyan/blue, green, yellow, and gray roles while keeping plain text readable in Ratatui buffer tests.
- Link the design assets from the GitHub issue and README/docs so future reviewers have the same reference.
