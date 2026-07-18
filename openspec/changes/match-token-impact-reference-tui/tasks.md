## 1. Spec, Issue, and Design Assets

- [x] 1.1 Add the supplied token impact dashboard reference and Ani mascot reference under `docs/design/`.
- [x] 1.2 Create the OpenSpec proposal, design, spec delta, and task list with pre-mortem risks.
- [x] 1.3 Create a GitHub issue for the reference-matched Ratatui token dashboard and Ani mascot, assign it to v0.3.25, and embed/link the design reference images.
- [x] 1.4 Map `match-token-impact-reference-tui` in `openspec/issue-map.json`.
- [x] 1.5 Mirror this task list into the GitHub issue under `OpenSpec Tasks`.

## 2. Token TUI Implementation

- [x] 2.1 Rework the overview dashboard header to show a small terminal-native Ani mark, `ProjectAtlas Token Impact`, supporting copy, session, lookup count, and estimator.
- [x] 2.2 Rework the hero panel to show `TOTAL TOKENS AVOIDED` plus the reconciled `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas` equation.
- [x] 2.3 Rework the file-read strip to show total file reads avoided, observed summary/slice reads, search-modeled narrowing, percentages/bars, and confidence.
- [x] 2.4 Rework the middle panels to show savings composition and signal details without repeating the same fields in competing sections.
- [x] 2.5 Rework the source table to use the screenshot columns plus telemetry-backed `Summaries and slices`, `Skipped broad folder walk`, `Opened fewer candidates (A)`, and `Opened fewer candidates (B)` rows when those buckets exist, with styled headers, visible spacing, source steps that sum to lookups, and source tokens that sum to the headline total.
- [x] 2.6 Rework calibration/notes so local-estimate, observed/modeled reads, and tokenizer audit information remain visible.
- [x] 2.7 Add the compact status/footer row from the screenshot, including a clock timestamp, and keep the dedicated saved-token trend dashboard available outside overview mode.

## 3. Documentation

- [x] 3.1 Update README/docs to mention Ani as the ProjectAtlas mascot and link the design references.
- [x] 3.2 Update token dashboard docs to describe the reference-style token impact overview.

## 4. Verification

- [x] 4.1 Add or update token TUI unit tests for required sections, order, Ani mark, Ratatui styles, and table spacing.
- [x] 4.2 Add or update tests proving the displayed token equation and source rows reconcile to `tokens_avoided`.
- [x] 4.3 Add or update tests proving the displayed file-read split reconciles to `likely_file_reads_avoided`.
- [x] 4.4 Run focused token TUI tests and the CLI e2e token TUI smoke.
- [x] 4.5 Run a rebuilt local `projectatlas token --view tui` visual inspection at normal and compact widths and compare against `docs/design/token-impact-tui-reference.png`.
- [x] 4.6 Run OpenSpec validation and issue checklist validation.
