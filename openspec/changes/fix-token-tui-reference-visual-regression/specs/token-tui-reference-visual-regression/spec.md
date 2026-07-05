## ADDED Requirements

### Requirement: Reference Screenshot Visual Parity Gate
The token overview TUI SHALL be accepted only when a rebuilt real terminal screenshot materially matches `docs/design/token-impact-tui-reference.png` in composition, hierarchy, spacing, semantic colors, and information layout, except for documented terminal limitations and the approved ProjectAtlas-origin ivory color correction.

#### Scenario: Real screenshot review before closure
- **WHEN** issue #304 is marked ready for closure or a release PR claims the token TUI regression is fixed
- **THEN** the implementer SHALL capture a real screenshot of the rebuilt `projectatlas token --view tui` overview
- **AND** the screenshot SHALL be compared to `docs/design/token-impact-tui-reference.png`
- **AND** the comparison SHALL be recorded in the issue or PR before merge/release.

#### Scenario: Current monochrome variant is rejected
- **WHEN** the rendered dashboard has the same sections but mostly uniform pale text, pale borders, weak value hierarchy, and no strong blue/ivory/green/yellow semantic palette
- **THEN** the dashboard SHALL NOT satisfy this requirement.

#### Scenario: Reference section composition
- **WHEN** `projectatlas token --view tui` renders the overview dashboard at a normal desktop terminal size
- **THEN** it SHALL render a header band, hero saved-token panel, file-read strip, composition/signal row, savings source table, calibration/notes panel, and footer/status row in that order
- **AND** those sections SHALL visually resemble the reference layout rather than a plain bordered text report.

### Requirement: Semantic Color Roles
The overview dashboard SHALL use semantic style roles for critical values so users can distinguish original baseline, ProjectAtlas-origin values, saved values, and modeled values at a glance.

#### Scenario: ProjectAtlas identity and ProjectAtlas-origin metrics use ivory
- **WHEN** the overview dashboard renders
- **THEN** `ProjectAtlas` in the title SHALL use the ivory/off-white identity role
- **AND** `With ProjectAtlas` values/bars SHALL use the ivory/off-white identity role
- **AND** the file reads avoided total SHALL use the ivory/off-white identity role
- **AND** observed summaries/slices values/bars SHALL use the ivory/off-white identity role
- **AND** measured summaries/slices composition values/bars SHALL use the ivory/off-white identity role
- **AND** the `Summaries and slices` source-table row SHALL use the ivory/off-white identity role.

#### Scenario: Original baseline uses blue
- **WHEN** the overview dashboard renders the `Without ProjectAtlas` operand
- **THEN** the operand label, value, and bar SHALL use the baseline blue role.

#### Scenario: Net saved values use green
- **WHEN** the overview dashboard renders headline saved tokens or `Saved by ProjectAtlas`
- **THEN** the saved values and saved bars SHALL use the green saved/success role.

#### Scenario: Modeled/search values use yellow
- **WHEN** the overview dashboard renders search-modeled narrowing, modeled confidence, or explicitly modeled source rows
- **THEN** those values and bars SHALL use the yellow modeled/search role.

#### Scenario: Style roles are tested in the Ratatui buffer
- **WHEN** token TUI tests render representative overview data through Ratatui `TestBackend`
- **THEN** tests SHALL inspect foreground/background styles for the baseline blue, ProjectAtlas-origin ivory, saved green, and modeled yellow roles on critical labels or values.

### Requirement: Ani Mascot Image Treatment
The overview dashboard SHALL show Ani as a deliberate small Ratatui widget near the ProjectAtlas title, derived from the committed transparent Ani raster asset and source SVG.

#### Scenario: Ani is recognizable and image-derived
- **WHEN** the overview dashboard renders at a normal desktop terminal size
- **THEN** it SHALL display a small Ani mark that visually reads as a mascot/pirate-cartographer identity element
- **AND** it SHALL be implemented with a tiny Ratatui `Widget` backed by `ratatui-image` halfblock rendering
- **AND** it SHALL use `docs/design/ani-mascot-reference.png` as the raster runtime asset
- **AND** it SHALL keep `docs/design/projectatlas-mascot-clean-transparent.svg` as the vector source asset
- **AND** it SHALL include recognizable hat, face, and repository-map cues
- **AND** it SHALL NOT require sixel, Kitty graphics, or iTerm image protocols.

#### Scenario: Ani presence is tested
- **WHEN** token TUI tests render the normal-width overview
- **THEN** tests SHALL assert that Ani's expected label, symbols, or styled cells are present.

#### Scenario: Compact terminals preserve core information
- **WHEN** the overview renders on compact terminal dimensions
- **THEN** decorative Ani detail MAY compress
- **BUT** the title, headline saved value, equation, file-read total, observed/modeled split, source table heading, and footer SHALL remain visible.

### Requirement: Ratatui Widget-Based Dashboard
The overview dashboard SHALL use Ratatui standard widgets and style primitives for structure, styling, bars, and tables.

#### Scenario: Standard widgets are used for fitting sections
- **WHEN** the dashboard implementation is reviewed
- **THEN** section layout SHALL be composed with Ratatui `Layout`
- **AND** panels SHALL be composed with `Block`
- **AND** title/equation/metadata text SHALL be composed with `Paragraph`, `Line`, and `Span`
- **AND** source data SHALL be composed with `Table`, `Row`, and `Cell`
- **AND** progress/contribution bars SHALL use `Gauge`, `LineGauge`, or a tested span/cell bar when that better matches the reference.

#### Scenario: No broad custom renderer
- **WHEN** the dashboard implementation is reviewed
- **THEN** it SHALL NOT replace the overview with a bespoke full-screen renderer or broad bitmap/image rendering system
- **AND** the accepted image dependency SHALL remain limited to Ani's `ratatui-image` halfblock widget
- **AND** any direct `Buffer` writes SHALL be limited to small exact-cell details such as icons or tested bars.

### Requirement: Accounting Relationships Remain Correct
The overview dashboard SHALL preserve all visible token and file-read accounting relationships while reducing duplicated fields.

#### Scenario: Token equation reconciles
- **WHEN** the overview dashboard renders representative token data
- **THEN** the displayed `Without ProjectAtlas` value minus the displayed `With ProjectAtlas` value SHALL equal the displayed `Saved by ProjectAtlas` value
- **AND** the displayed `Saved by ProjectAtlas` value SHALL equal `tokens_avoided`.

#### Scenario: File-read split reconciles
- **WHEN** the file-read strip renders observed and modeled read counts
- **THEN** displayed observed reads plus displayed modeled reads SHALL equal displayed likely file reads avoided.

#### Scenario: Source table reconciles
- **WHEN** the savings source table renders observed and modeled rows
- **THEN** visible row tokens SHALL sum to the displayed saved-token headline
- **AND** visible row steps SHALL sum to the displayed lookup count when all lookup buckets are visible.

#### Scenario: Source table uses a real remainder row when attribution is incomplete
- **WHEN** observed and modeled telemetry buckets do not fully explain `tokens_avoided` or visible lookup steps
- **THEN** the source table SHALL render an `Unattributed savings` or `Other savings` row for the real remainder
- **AND** it SHALL NOT fabricate source-specific rows that are unsupported by telemetry bucket data.

#### Scenario: No competing saved-token headline
- **WHEN** gross compatibility totals differ from the conservative avoided-token total
- **THEN** the overview dashboard SHALL NOT render a second competing gross saved-token headline.

### Requirement: Visual Regression Tests Cover Bars And Spacing
The token TUI test suite SHALL cover the visual mechanics that made the previous implementation pass text checks but fail screenshot review.

#### Scenario: Bar ratios are tested
- **WHEN** representative ratios are rendered for baseline, ProjectAtlas-origin, saved, observed, and modeled bars
- **THEN** tests SHALL verify filled versus empty portions for zero, partial, full, and overflow-clamped values where the helper supports deterministic inspection.

#### Scenario: Table header spacing is tested
- **WHEN** the `WHERE THE SAVINGS CAME FROM` table renders
- **THEN** tests SHALL verify a styled header and a visible bottom margin, separator, or row gap before the first data row.

#### Scenario: Duplicate field clutter is rejected
- **WHEN** the overview dashboard renders
- **THEN** tests SHALL verify that the same headline values are not repeated in multiple competing panels in a way that contradicts the reference dashboard.

#### Scenario: Trend mode remains separate
- **WHEN** a user runs `projectatlas token --view tui --trend <window>`
- **THEN** the existing dedicated trend dashboard SHALL still render
- **AND** the overview dashboard SHALL remain focused on the reference KPI composition.

### Requirement: Review And Release Gates
Issue #304 SHALL remain open until OpenSpec tasks, subagent review, tests, and screenshot review are complete.

#### Scenario: OpenSpec tasks are mirrored
- **WHEN** the OpenSpec task list changes for `fix-token-tui-reference-visual-regression`
- **THEN** issue #304 SHALL show the same task checklist under an `OpenSpec Tasks` heading
- **AND** checklist drift SHALL block closure/release.

#### Scenario: Canonical screenshot dimensions are recorded
- **WHEN** visual evidence is captured for issue #304
- **THEN** the primary screenshot SHALL use a 140-column by 47-row terminal where possible
- **AND** compact smoke evidence SHALL use an 80-column by 47-row terminal or deterministic buffer equivalent.

#### Scenario: Subagent review findings are dispositioned
- **WHEN** subagents review the spec, code, tests, or screenshot
- **THEN** their valid findings SHALL be fixed before merge
- **AND** any rejected or deferred findings SHALL be recorded with rationale.

#### Scenario: Release waits for visual evidence
- **WHEN** the next ProjectAtlas version is prepared
- **THEN** issue #304 SHALL NOT be closed and the release SHALL NOT be cut until the real screenshot review task is checked.
