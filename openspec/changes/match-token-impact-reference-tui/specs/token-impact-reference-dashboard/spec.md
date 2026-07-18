## ADDED Requirements

### Requirement: Reference-Matched Token Impact Dashboard
The token TUI overview SHALL follow the supplied token impact dashboard reference directly in visible layout, color palette, widget hierarchy, labels, section order, and information content while preserving ProjectAtlas accounting correctness. The implementation SHALL NOT replace the reference with a materially different dashboard design.

#### Scenario: Reference section order
- **WHEN** `projectatlas token --view tui` renders the overview dashboard
- **THEN** it SHALL render a header with `ProjectAtlas Token Impact`
- **AND** it SHALL render the hero `TOTAL TOKENS AVOIDED` panel before the file-read strip
- **AND** it SHALL render `FILE READS AVOIDED` before `SAVINGS COMPOSITION`
- **AND** it SHALL render `SAVINGS COMPOSITION` and `SIGNAL` before `WHERE THE SAVINGS CAME FROM`
- **AND** it SHALL render `CALIBRATION & NOTES` after the source table
- **AND** it SHALL render a compact footer/status row.

#### Scenario: Screenshot-matched color roles
- **WHEN** the overview dashboard renders
- **THEN** the dashboard SHALL use a near-black navy background
- **AND** it SHALL use dark-blue panels with muted blue borders/dividers
- **AND** headings SHALL use bright blue/cyan styling
- **AND** positive saved-token values SHALL use bright green styling
- **AND** original baseline values such as `Without ProjectAtlas` SHALL use the bright blue role
- **AND** ProjectAtlas identity and ProjectAtlas-origin values SHALL use the ivory/off-white role
- **AND** `With ProjectAtlas`, the file reads avoided total, observed summaries/slices values, measured summaries/slices bars, and the `Summaries and slices` source-table row SHALL use the ivory/off-white role
- **AND** modeled/search confidence values SHALL use warm yellow styling
- **AND** body text SHALL use pale blue-gray styling.

#### Scenario: Dominant saved-token equation
- **WHEN** the overview has a conservative avoided-token total
- **THEN** the dashboard SHALL show the `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas` equation
- **AND** the displayed equation operands SHALL satisfy `without_projectatlas - with_projectatlas = saved_by_projectatlas`
- **AND** `saved_by_projectatlas` SHALL equal `tokens_avoided`
- **AND** the dashboard SHALL NOT add extra gross-vs-conservative explanation text to the hero panel.

#### Scenario: No competing gross headline
- **WHEN** gross compatibility saved-token totals differ from the conservative avoided-token total
- **THEN** the overview dashboard SHALL NOT render the gross saved-token total as a competing headline
- **AND** the conservative `tokens_avoided` value SHALL remain the dominant saved-token value.

### Requirement: File Handling Optimization Strip
The token TUI overview SHALL show file-read avoidance in a compact reference-style strip with observed and modeled parts.

#### Scenario: File-read total and split
- **WHEN** the overview has observed and search-modeled file-read avoidance
- **THEN** the dashboard SHALL show `FILE READS AVOIDED`
- **AND** it SHALL show the total `likely_file_reads_avoided`
- **AND** it SHALL show observed summary/slice reads
- **AND** it SHALL show search-modeled narrowing
- **AND** it SHALL show the confidence label.

#### Scenario: File-read equation remains correct
- **WHEN** the file-read strip renders
- **THEN** the displayed observed and search-modeled counts SHALL sum to the displayed file-read total.

### Requirement: Source Table Readability
The token TUI overview SHALL render the savings source table with styled headers, clear spacing, and rows that reconcile to the headline totals.

#### Scenario: Screenshot table labels
- **WHEN** the `WHERE THE SAVINGS CAME FROM` table renders
- **THEN** it SHALL show the columns `Source`, `Steps`, `Tokens Avoided`, and `What it means`
- **AND** it SHALL show screenshot-matched source row labels for `Summaries and slices`, `Skipped broad folder walk`, `Opened fewer candidates (A)`, and `Opened fewer candidates (B)` when the corresponding telemetry bucket exists.

#### Scenario: Header spacing
- **WHEN** the `WHERE THE SAVINGS CAME FROM` table renders
- **THEN** the header row SHALL use styled text
- **AND** there SHALL be visible spacing or a bottom margin between the header and the first row.

#### Scenario: Source rows reconcile
- **WHEN** the source table renders observed and modeled rows
- **THEN** the visible source row tokens SHALL sum to `tokens_avoided`
- **AND** the visible source row steps SHALL sum to the overview lookup count represented by `calls`.

#### Scenario: Modeled rows are telemetry-backed
- **WHEN** telemetry does not provide independent modeled sub-buckets for every screenshot-modeled row
- **THEN** the dashboard SHALL NOT fabricate additional modeled source rows from `modeled_file_reads_avoided`
- **AND** it SHALL allocate `deduped_modeled_tokens_avoided` across the real modeled source buckets so visible source-row tokens still sum exactly to `tokens_avoided`.

### Requirement: Ani Mascot Integration
The token TUI overview and documentation SHALL introduce Ani as the ProjectAtlas mascot without adding terminal bitmap rendering.

#### Scenario: Terminal-native Ani mark
- **WHEN** `projectatlas token --view tui` renders the overview dashboard
- **THEN** it SHALL show a small terminal-native Ani pixel/block mascot mark near the `ProjectAtlas Token Impact` title
- **AND** it SHALL remain recognizable through plain text/Ratatui styling without relying on sixel, iTerm images, or a bitmap protocol.

#### Scenario: Mascot documentation
- **WHEN** a reader opens the README or relevant docs
- **THEN** Ani SHALL be identified as the ProjectAtlas mascot
- **AND** the versioned mascot reference image SHALL be linked from the documentation.

### Requirement: Ratatui Widget Composition
The token TUI overview SHALL use Ratatui standard widgets and style primitives for the reference dashboard.

#### Scenario: Widget-backed dashboard
- **WHEN** the overview dashboard is implemented
- **THEN** it SHALL use Ratatui layout, block, paragraph, gauge or paragraph-backed bars, table, and style primitives where those widgets fit the section
- **AND** it SHALL NOT introduce a bespoke terminal renderer for this dashboard.

#### Scenario: Dedicated trend dashboard remains available
- **WHEN** a user runs `projectatlas token --view tui --trend <window>`
- **THEN** the dedicated trend dashboard SHALL remain available
- **AND** overview mode SHALL remain focused on the reference KPI layout instead of adding a large chart section.

### Requirement: Supersede Previous Overview Trend Grid
The reference-matched token impact overview SHALL supersede the previous overview requirement to show day, week, month, and year trend windows directly inside overview mode.

#### Scenario: Overview does not include the old trend grid
- **WHEN** `projectatlas token --view tui` renders overview mode
- **THEN** it SHALL NOT render the old four-panel day/week/month/year trend grid
- **AND** trend chart rendering SHALL remain available only through `projectatlas token --view tui --trend <window>`.

#### Scenario: Portable terminal effects
- **WHEN** the screenshot includes non-terminal soft glow or bitmap-like effects
- **THEN** the Ratatui dashboard SHALL approximate the visual hierarchy with color, borders, spacing, and bars
- **AND** it SHALL NOT depend on terminal image protocols or glow effects.
