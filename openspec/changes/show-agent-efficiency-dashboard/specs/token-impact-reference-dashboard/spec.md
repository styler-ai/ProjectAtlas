## ADDED Requirements

### Requirement: Agent Efficiency Comparison Remains Visually Distinct
The token overview TUI SHALL render explicitly requested validated benchmark evidence in a bounded agent-efficiency panel that is visually and semantically separate from observed and modeled live telemetry. The normal no-artifact dashboard SHALL remain focused on live token impact and SHALL omit release-version and plain-control comparisons.

#### Scenario: Compatible or partial evidence is available
- **WHEN** `projectatlas token --view tui --benchmark-results <path>` receives validated benchmark comparison rows
- **THEN** the dashboard SHALL show the comparison state and artifact identity
- **AND** it SHALL show distinct frozen-v0.3.26 and plain-control rows with matched and failed trial counts
- **AND** it SHALL NOT add benchmark values to `tokens_avoided` or `likely_file_reads_avoided`.

#### Scenario: No evidence is requested
- **WHEN** no benchmark path is supplied and comparison state is unavailable
- **THEN** the dashboard SHALL omit the comparison panel
- **AND** the live token-impact hierarchy SHALL not reserve empty comparison space.

#### Scenario: Requested evidence is invalid
- **WHEN** an explicitly requested comparison is failed or incompatible
- **THEN** the dashboard SHALL show that explicit state and a bounded reason
- **AND** it SHALL NOT render fabricated zero-valued efficiency rows or savings percentages.

### Requirement: Comparison Layout Preserves The Accepted Dashboard
The agent-efficiency panel SHALL preserve the accepted token-impact hierarchy, semantic palette, terminal-background behavior, trend mode, and compact-width contract.

#### Scenario: Normal-width overview renders
- **WHEN** the overview renders at the canonical normal width
- **THEN** the existing headline, file-read, composition, signal, source, calibration, and footer sections SHALL retain their accounting and semantic roles
- **AND** the agent-efficiency panel SHALL show total calls, broad/full reads, net navigation context, runtime, and workload break-even without hidden overflow.

#### Scenario: Compact-width overview renders
- **WHEN** the overview renders at 80 columns
- **THEN** comparison state, both baseline identities, matched/failed trials, and the principal call/read/context result SHALL remain visible
- **AND** the layout SHALL use bounded short labels rather than truncating into adjacent columns.

#### Scenario: Trend mode is selected
- **WHEN** a user requests the dedicated token trend dashboard
- **THEN** trend mode SHALL remain unchanged
- **AND** it SHALL NOT duplicate the agent-efficiency comparison panel.

### Requirement: Dashboard Values Come From The Typed Report
The Ratatui dashboard SHALL render comparison values and states from the authoritative typed token overview and SHALL NOT parse benchmark JSON or maintain independent comparison arithmetic.

#### Scenario: Ratatui buffer tests render representative states
- **WHEN** tests render the hidden no-artifact state plus explicitly requested compatible, partial, failed, and incompatible comparison reports
- **THEN** labels, semantic styles, bounded values and reasons, absent values, failed counts, normal layout, and compact layout SHALL match the typed values
- **AND** existing conservative token and file-read equations SHALL continue to reconcile.

#### Scenario: Real visual review is performed
- **WHEN** the implementation is ready for acceptance
- **THEN** a real normal-width dashboard render SHALL be compared to the approved token-impact reference
- **AND** the new panel SHALL remain readable without weakening the existing visual hierarchy.
