## 1. Review

- [x] 1.1 Capture the repeated-field and header-spacing issues from the current `projectatlas token --view tui` output.
- [x] 1.2 Define the visible accounting equations for conservative saved tokens and file reads avoided.

## 2. Implementation

- [x] 2.1 Remove the duplicate headline gauge row from the overview dashboard.
- [x] 2.2 Keep Ratatui `Block`, `Paragraph`, `Chart`, `Gauge`, and `Table` widgets as the rendering foundation.
- [x] 2.3 Replace raw bucket rows with an aggregated source table whose visible rows add up to the conservative saved-token and file-read totals.
- [x] 2.4 Improve table header labels, spacing, and styling so headers are visually separated from data rows.
- [x] 2.5 Keep saved-token trends visible for day, week, month, and year.
- [x] 2.6 Consolidate the file-handling equation, source rows, and gauge into one section so the dashboard does not repeat the same accounting fields.
- [x] 2.7 Hide gross period saved numbers from overview trend titles so the overview has one visible saved-token total.
- [x] 2.8 Widen compact token columns so billion-scale grouped values remain readable at 80 columns.
- [x] 2.9 Keep one compact equation strip and remove the visible total source row so observed/modeled rows sum exactly once.

## 3. Verification

- [x] 3.1 Add or update tests proving visible saved-token and file-read equations add up.
- [x] 3.2 Add or update tests preventing repeated removed labels such as `Measured summaries`, `Narrowed files`, and duplicated `Fewer candidates` rows.
- [x] 3.3 Run a local `projectatlas token --view tui` visual inspection after rebuilding.
- [x] 3.4 Run focused token TUI tests and the full Rust workspace gate set.
- [x] 3.5 Add regression coverage that rejects the duplicate gross-summary line and the separate repeated savings-source section.
- [x] 3.6 Add regression coverage that rejects gross saved trend totals in the overview.
- [x] 3.7 Add compact-width regression coverage for large grouped token values.
- [x] 3.8 Add regression coverage that signed negative savings stay visible in trend charts and token-mix labels.
- [x] 3.9 Add regression coverage that rejects a visible total source row and keeps the explicit equations correlated with source rows.
