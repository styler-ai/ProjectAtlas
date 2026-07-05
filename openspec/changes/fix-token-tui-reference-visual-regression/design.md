## Context

The current overview renderer in `crates/projectatlas-cli/src/token_tui.rs` is Ratatui-based and already renders most required data fields, but the live screenshot does not match `docs/design/token-impact-tui-reference.png`. The problem is visual execution, not missing telemetry.

The current screenshot differs from the reference in these concrete ways:

- It lacks the reference window-like frame and warm dark panel hierarchy.
- Most labels, borders, and numbers use the same pale blue/gray style.
- `With ProjectAtlas`, file-read totals, observed reads, and measured summaries/slices do not use the requested ivory ProjectAtlas-origin role.
- The saved headline is not visually dominant enough and lacks the reference-like icon treatment.
- Ani/pixel-art rendering proved distracting and unreadable in the live TUI, so v0.3.26 defers Ani entirely instead of shipping a broken placeholder.
- The file-read strip and composition panels are flatter than the reference and do not use strong visual separators.
- The savings table has minimal row separation and reads like plain text inside a bordered box.
- Footer/status content exists but does not feel integrated into the reference frame.

The fix must be judged from a real screenshot, not just test output.

## Design Goals

1. Match the approved reference composition closely enough that a first-glance screenshot comparison shows the same dashboard family: header, hero, file-read strip, composition/signal panels, savings table, notes, and footer.
2. Preserve the existing fields and simple math so the dashboard remains understandable.
3. Make semantic colors unambiguous:
   - ivory/off-white: ProjectAtlas identity and values produced by ProjectAtlas,
   - blue: original/counterfactual baseline,
   - green: net saved/success,
   - yellow: modeled/search/confidence,
   - warm muted gray: labels/body text.
4. Use Ratatui widgets and style primitives. Custom drawing is allowed only for small exact-cell details such as icons and tested bars. Ani is deferred from the v0.3.26 TUI and must not add an image-rendering dependency in this release.
5. Add tests that make the visual contract harder to regress: important style roles, ratios, spacing, and absence of duplicated fields.

## Layout Contract

Target normal dashboard size for screenshot review: **140 columns by 48 terminal rows**. The height keeps the KPI panels, table, notes, and footer readable without needing a mascot column. A compact smoke target of **80 columns by 48 rows** must keep core fields visible. The renderer may adapt below that, but the reference-size layout should allocate vertical space in this order:

1. Header band, about 7 rows:
   - no Ani or mascot placeholder in v0.3.26,
   - `ProjectAtlas` in ivory/off-white,
   - `Token Impact` in blue,
   - `Real savings` in green inside the supporting line,
   - right metadata: session, lookups, estimate.
2. Divider row.
3. Hero savings panel, about 10 rows:
   - panel title `TOTAL TOKENS AVOIDED`,
   - dominant green saved-token number,
   - optional success/check mark in green,
   - equation row: `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas`,
   - three bars under the operands.
4. File-read strip, about 5 rows:
   - document/file icon area,
   - total file reads avoided in ivory,
   - observed summaries/slices count and bar in ivory,
   - search-modeled narrowing count and bar in yellow,
   - confidence in yellow.
5. Composition and signal row, about 6 rows:
   - `SAVINGS COMPOSITION` with measured/projectatlas ivory and navigation/modeling green or yellow as appropriate,
   - `SIGNAL` with three concise rows.
6. Savings source table, flexible but usually 7 to 9 rows:
   - `WHERE THE SAVINGS CAME FROM`,
   - styled header row,
   - visible separator or bottom margin before data,
   - row separators or muted divider lines.
7. Calibration and notes, about 4 rows.
8. Footer/status row, 1 row:
   - ProjectAtlas version left,
   - key hints and auto/time right.

When width is constrained, shorten labels and decorative icons before removing core values. At normal/reference width, the design should not look sparse or monochrome.

## Ratatui Widget Plan

Use the global Ratatui skill guidance and Context7 for API details before changing widget code. This repository currently uses Ratatui `0.30.2`, so `Fill`, `Cell::column_span`, and `symbols::Marker::Custom` are available if needed, but the implementation should still prefer simple stable widgets.

Implementation mapping:

- Terminal canvas: do not paint a full-screen ProjectAtlas background; preserve the user's shell/terminal background outside panels.
- Window/frame feel: outer `Block` with subtle border and reset/default background.
- Panels: `Block::bordered()` with themed border/title styles.
- Header/title: `Paragraph` with styled `Line`/`Span`.
- Values/equation: `Paragraph` and `Line`/`Span`.
- Bars: prefer `Gauge` or `LineGauge` where it matches the reference; use small span/block bars only when exact segmented appearance is needed and covered by tests.
- File/composition bars: `Gauge`, `LineGauge`, or tested span bars with filled/empty cell counts.
- Savings table: `Table`, `Row`, `Cell`, `header(...).bottom_margin(1)`, `column_spacing(2)`, semantic row styles.
- Signal icons: small styled `Span`s or exact-cell helper rendering.
- Ani: deferred from the v0.3.26 TUI. Keep `docs/design/ani-mascot-reference.png` and `docs/design/projectatlas-mascot-clean-transparent.svg` as design references for a future scoped mascot issue, but do not load them or depend on `image`/`ratatui-image` at runtime.
- Trend mode: preserve the existing dedicated trend dashboard and avoid forcing a large trend chart into overview mode.

## Theme

Use semantic constants rather than raw color literals in render functions:

- `THEME_BG = Rgb(4, 10, 18)` or close deep ink navy may remain as a reserved semantic role, but the overview/trend renderers must not apply it as a full-canvas background.
- `THEME_PANEL = Rgb(7, 20, 33)` with a slightly warmer alternate panel if needed.
- `THEME_BORDER = Rgb(35, 62, 90)` and `THEME_BORDER_ACTIVE = Rgb(73, 119, 200)`.
- `THEME_IDENTITY = Rgb(238, 234, 224)` for ProjectAtlas identity, future Ani treatment, and ProjectAtlas-origin values.
- `THEME_TEXT = Rgb(218, 214, 204)` for readable warm body text.
- `THEME_MUTED = Rgb(158, 151, 139)` for secondary labels.
- `THEME_BASELINE = Rgb(93, 143, 255)` for original/counterfactual baseline.
- `THEME_SAVED = Rgb(111, 216, 100)` for net saved/success.
- `THEME_MODELED = Rgb(230, 179, 55)` for modeled/search/confidence.
- `THEME_DANGER = Rgb(235, 95, 95)` for negative savings.

The dashboard may use close values if terminal contrast requires it, but the role mapping must remain.

## Accounting And Data Rules

The TUI should keep the simple displayed math:

- `saved_by_projectatlas = overview.tokens_avoided`.
- `with_projectatlas = overview.estimated_with_projectatlas`.
- `without_projectatlas = with_projectatlas + saved_by_projectatlas`.

This avoids showing an older gross compatibility total as a competing headline.

File-read math:

- total `likely_file_reads_avoided`,
- observed `observed_file_read_replacements`,
- modeled `modeled_file_reads_avoided`,
- displayed observed plus modeled must equal displayed total.

Source table math:

- table rows must reconcile to the displayed saved-token headline,
- observed summaries/slices row uses ProjectAtlas-origin ivory,
- modeled/search rows use yellow or green according to the existing semantic contract,
- if telemetry buckets are missing or do not fully attribute the headline total, render a real `Unattributed savings`/`Other savings` remainder row rather than silently letting the table total diverge.

## Test Strategy

Add or strengthen unit tests using Ratatui `TestBackend`/`Buffer`:

- required sections appear in reference order,
- the visible equation adds up,
- the visible file-read split adds up,
- source-row tokens sum to the headline saved total,
- source-row steps sum to visible lookup count when all rows render,
- `Without ProjectAtlas` value/bar uses baseline blue,
- `With ProjectAtlas`, file-read total, observed reads, measured summaries/slices, and `Summaries and slices` row use identity ivory,
- saved headline/value/bar uses green,
- modeled/search/confidence values use yellow,
- table header has a styled header and at least one visible margin/separator before first data row,
- bar fill proportions are correct for representative 0%, partial, full, and overflow-clamped ratios,
- Ani/image rendering is absent from the v0.3.26 overview, and the title starts in the header without a mascot column,
- overview/trend frames preserve reset/default terminal background outside panels,
- compact-width smoke does not panic and keeps core fields visible.

Do not rely only on string snapshots. Where symbols are non-ASCII, use direct cell assertions instead of byte-length string matching.

## Visual QA

Before issue closure:

1. Build/run the current binary from the working tree.
2. Render `projectatlas token --view tui` at a reference-like terminal size.
3. Capture a real screenshot.
4. Compare it against `docs/design/token-impact-tui-reference.png`.
5. Record mismatches and either fix them or explicitly document the terminal limitation.

The accepted differences from the reference for v0.3.26 are terminal font/rendering differences, the user-approved color-semantic correction where ProjectAtlas-origin metrics use ivory instead of blue, preserving the user's terminal background outside panels instead of painting the full canvas, and deferring Ani from the TUI until a future focused mascot issue.

## Subagent Review Gate

Use subagents for independent review:

- spec/design review before implementation or early in implementation,
- code/test correctness review before commit,
- visual review after the real screenshot is captured.

Findings must be dispositioned before merge. Valid findings should be fixed; false positives or deferred findings should be explained in the issue or PR.

## Pre-Mortem

Risk: the new dashboard still looks monochrome because colors are applied only to section titles.
Mitigation: tests assert styles on values and key labels, and screenshot review checks first-glance palette.

Risk: the release spends more time on mascot rendering than on readability, math, and background correctness.
Mitigation: defer Ani from v0.3.26, remove image-runtime dependencies, and keep the mascot assets as future design references.

Risk: table readability regresses at narrow widths.
Mitigation: use Ratatui table spacing/header margin and compact-width smoke tests.

Risk: tests become brittle because they assert every decorative border cell.
Mitigation: assert semantic cells, section order, ratios, and spacing, not every full-frame character.

Risk: implementation hand-rolls too much layout.
Mitigation: keep sections on Ratatui `Layout`, `Block`, `Paragraph`, `Gauge`/`LineGauge`, and `Table`, with custom buffer work only for tiny art/icons.

Risk: release pressure causes acceptance before a real screenshot.
Mitigation: make screenshot capture/review an explicit OpenSpec task and GitHub checklist item.
