## ADDED Requirements

### Requirement: Agent Efficiency Comparison Stays Out Of The Human Overview
The token overview TUI SHALL render only live token-impact and repository context from the active project. Validated benchmark comparison state SHALL remain available to structured CLI JSON/TOON and MCP consumers, but SHALL NOT add a panel, row, label, value, reason, or reserved space to the Ratatui overview.

#### Scenario: Compatible or partial evidence is attached
- **WHEN** `projectatlas token --view tui --benchmark-results <path>` receives validated benchmark comparison rows
- **THEN** the dashboard SHALL omit the comparison state, artifact identity, frozen-v0.3.26 row, and plain-control row
- **AND** its human layout and rendered live values SHALL match the same overview without attached benchmark evidence
- **AND** the benchmark values SHALL NOT alter `tokens_avoided`, `likely_file_reads_avoided`, source rows, or composition.

#### Scenario: No evidence is requested
- **WHEN** no benchmark path is supplied and comparison state is unavailable
- **THEN** the dashboard SHALL render the same live-only hierarchy
- **AND** it SHALL reserve no comparison space.

#### Scenario: Requested evidence is failed or incompatible
- **WHEN** an explicitly requested comparison produces a typed failed or incompatible state
- **THEN** the TUI SHALL remain live-only
- **AND** structured output SHALL retain the bounded state and reason without fabricating zero-valued efficiency rows or savings percentages.

### Requirement: The Live Dashboard Preserves Meaningful Navigation Reporting
The human overview SHALL preserve the accepted token-impact hierarchy, semantic palette, terminal-background behavior, trend mode, and compact-width contract. Its navigation-work chart SHALL present file reads avoided, while broad folder walks skipped and candidate files not opened SHALL remain distinct persisted measures in the source table without duplicative expanded charts.

#### Scenario: Normal-width overview renders
- **WHEN** the overview renders at the canonical normal width
- **THEN** file reads avoided SHALL show the exact observed and modeled split with proportional bars
- **AND** broad folder walks and each candidate source group SHALL show exact reconciled step and token values in the source table below
- **AND** the dashboard SHALL NOT duplicate those source rows as expanded activity-share or token-impact-share charts.

#### Scenario: Compact-width overview renders
- **WHEN** the overview renders at 80 columns
- **THEN** the file-read chart and the exact broad-walk and candidate source rows SHALL remain inside the dashboard bounds
- **AND** labels SHALL shorten without changing source meaning or adding duplicate detail blocks.

#### Scenario: Trend mode is selected
- **WHEN** a user requests the dedicated token trend dashboard
- **THEN** trend mode SHALL remain unchanged
- **AND** it SHALL NOT add benchmark comparison content.

### Requirement: Dashboard Values Come From The Typed Live Report
The Ratatui dashboard SHALL render live values from the authoritative typed token overview and SHALL NOT parse benchmark JSON or maintain independent comparison arithmetic.

#### Scenario: Ratatui buffer tests attach representative comparison states
- **WHEN** tests render unavailable, compatible, partial, failed, and incompatible comparison reports at compact, normal, and wide widths
- **THEN** attached comparison state SHALL NOT alter the human buffer
- **AND** existing conservative token, file-read, source-step, and source-token equations SHALL continue to reconcile.

#### Scenario: Real visual review is performed
- **WHEN** the implementation is ready for acceptance
- **THEN** real 80-, 140-, and 200-column dark, light, and terminal-background renders SHALL be compared to the approved token-impact reference
- **AND** the navigation-work chart, source table, footer, and optional wide atlas SHALL remain readable and bounded.
