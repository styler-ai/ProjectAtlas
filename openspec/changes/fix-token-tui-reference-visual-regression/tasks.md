## 1. Spec, Issue, and Review Setup

- [x] 1.1 Map `fix-token-tui-reference-visual-regression` to GitHub issue #304 in `openspec/issue-map.json`.
- [x] 1.2 Mirror this task list into issue #304 under `OpenSpec Tasks`.
- [x] 1.3 Run OpenSpec validation for the new change before implementation.
- [x] 1.4 Send the OpenSpec proposal/design/spec/tasks to a subagent for read-only review and disposition findings.

## 2. Reference And Current-State Inspection

- [x] 2.1 Inspect `docs/design/token-impact-tui-reference.png` and the current bad screenshot as visual references.
- [x] 2.2 Inspect `crates/projectatlas-cli/src/token_tui.rs` and existing token TUI tests before editing.
- [x] 2.3 Record the concrete visual mismatches to address: window frame, palette, failed Ani treatment deferral, bar hierarchy, table spacing, terminal background behavior, and footer integration.

## 3. Ratatui Implementation

- [x] 3.1 Define semantic theme constants for background, panel, border, identity ivory, baseline blue, saved green, modeled yellow, text, muted text, and danger.
- [x] 3.2 Rework the overview frame/background so the real screenshot resembles the reference window/panel composition.
- [x] 3.3 Rework the header without Ani for v0.3.26, with ivory `ProjectAtlas`, blue `Token Impact`, green `Real savings`, and right metadata.
- [x] 3.4 Rework the hero panel with dominant green saved-token value, clear equation roles, and tested bars.
- [x] 3.5 Rework the file-read strip with ivory total/observed ProjectAtlas-origin metrics, yellow modeled metric, confidence, and ratio bars.
- [x] 3.6 Rework the savings composition and signal panels so they match the reference hierarchy without duplicating headline fields.
- [x] 3.7 Rework the savings source table using Ratatui `Table`, styled headers, visible header separation, row separators, and semantic row styles.
- [x] 3.8 Rework calibration/notes and footer/status row so the bottom is readable and not cramped.
- [x] 3.9 Preserve dedicated trend dashboard behavior for `--trend <window>`.
- [x] 3.10 Add a real source-table remainder row when visible buckets do not fully attribute the saved-token headline.

## 4. Tests

- [x] 4.1 Add or update math tests for `without - with = saved`, file-read split sum, and source-row sum.
- [x] 4.2 Add or update Ratatui `Buffer` style tests for baseline blue, ProjectAtlas-origin ivory, saved green, and modeled yellow.
- [x] 4.3 Add or update bar-ratio tests for zero, partial, full, and clamped values where deterministic.
- [x] 4.4 Add or update tests for Ani/image deferral and terminal-background preservation at normal width.
- [x] 4.5 Add or update table spacing/header tests so headers cannot run into the first row.
- [x] 4.6 Add compact-width smoke coverage that keeps core fields visible and avoids panic/overlap.
- [x] 4.7 Add or update tests for source-table remainder reconciliation when bucket attribution is incomplete.

## 5. Visual QA And Reviews

- [x] 5.1 Run focused token TUI tests and CLI token TUI smoke tests.
- [x] 5.2 Capture a real screenshot/rendered visual artifact of the rebuilt `projectatlas token --view tui` overview at 140 columns by 47 rows where possible.
- [x] 5.3 Compare the real screenshot to `docs/design/token-impact-tui-reference.png`, fix visible mismatches, and record remaining accepted differences.
- [x] 5.4 Run a subagent code/test review and a subagent visual review after screenshot capture; disposition findings.

## 6. Release Gates

- [x] 6.1 Run `cargo fmt --check`.
- [x] 6.2 Run `cargo check --workspace --all-targets --all-features --locked`.
- [x] 6.3 Run `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- [x] 6.4 Run `cargo test --workspace --all-features --locked`.
- [x] 6.5 Run `openspec validate --all --strict --no-interactive`.
- [x] 6.6 Run `.github/scripts/issue-checklists.py`.
- [x] 6.7 Update issue #304 checklist with completed tasks before closure/release.
