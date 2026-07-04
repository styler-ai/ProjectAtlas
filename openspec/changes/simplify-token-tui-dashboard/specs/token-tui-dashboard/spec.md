## ADDED Requirements

### Requirement: Concise Token Overview Dashboard
The token TUI overview SHALL avoid duplicate sections that repeat the same saved-token and file-read accounting facts.

#### Scenario: Repeated headline gauge labels
- **WHEN** `projectatlas token --view tui` renders the overview dashboard
- **THEN** it SHALL NOT include a separate duplicate gauge row for `Measured summaries`, `Narrowed files`, or repeated `Fewer candidates` rows.

#### Scenario: Repeated accounting sections
- **WHEN** `projectatlas token --view tui` renders the overview dashboard
- **THEN** it SHALL NOT render a competing gross saved total beside the conservative avoided-token headline
- **AND** it SHALL keep the file-handling equation, source rows, and ratio gauge in one section instead of splitting them into repeated sections.

### Requirement: Visible Conservative Accounting
The token TUI overview SHALL show conservative saved-token accounting as `measured_tokens_saved + deduped_modeled_tokens_avoided = tokens_avoided`.

#### Scenario: Conservative saved-token equation
- **WHEN** the overview has observed replacements and modeled navigation avoidance
- **THEN** the dashboard SHALL show a saved-token equation whose values add up to `tokens_avoided`.

#### Scenario: Source table token total
- **WHEN** the dashboard renders the source table
- **THEN** the visible source rows SHALL sum to `tokens_avoided`.

### Requirement: Visible File-Read Accounting
The token TUI overview SHALL show file-read avoidance as `observed_file_read_replacements + modeled_file_reads_avoided = likely_file_reads_avoided`.

#### Scenario: File-read equation
- **WHEN** the overview has observed and search-modeled read avoidance
- **THEN** the dashboard SHALL show both parts and their total.

#### Scenario: Source table file-read total
- **WHEN** the dashboard renders the source table
- **THEN** visible source row file-read counts SHALL sum to `likely_file_reads_avoided`.

### Requirement: Ratatui Widget Composition
The token TUI overview SHALL be built with Ratatui standard widgets and light-terminal-friendly styling.

#### Scenario: Header spacing
- **WHEN** a table is rendered in the overview dashboard
- **THEN** the header SHALL use styled text, visible column spacing, and a blank margin before the first row.

#### Scenario: Trend windows
- **WHEN** token trend history exists
- **THEN** the overview SHALL show day, week, month, and year saved-token trend windows.
- **AND** the overview trend titles SHALL NOT print gross period saved totals as competing numeric saved-token values.

#### Scenario: Negative savings remain signed
- **WHEN** a period or accounting operand has negative saved tokens
- **THEN** trend visuals SHALL preserve the negative sign instead of converting the value to a positive magnitude.
- **AND** the token-mix label SHALL show signed observed/modeled operands and the signed net value.
