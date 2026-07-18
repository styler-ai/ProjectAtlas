# ProjectAtlas v0.3.23 Pre-Mortem And Issue Specification

Date: 2026-07-04

## Release Goal

Ship a patch release after `v0.3.22` that fixes the two concrete regressions
found during Codex plugin/runtime startup:

- #285 `fix(plugin): detect stale Codex marketplace source manifests`
- #286 `fix(token): make token TUI accounting and Ratatui layout clear`

The release also adds a reusable Codex plugin engineering skill so future plugin
work keeps installer, manifest, runtime, MCP registry, and host instruction
drift in one stability model.

## Scope Decision

| Issue | Decision | Rationale |
| --- | --- | --- |
| #285 `fix(plugin): detect stale Codex marketplace source manifests` | Implement in v0.3.23 | Official Codex ProjectAtlas plugin cache can report the current plugin version while the source manifest path is stale. Installer must repair the cache instead of trusting the reported version alone. |
| #286 `fix(token): make token TUI accounting and Ratatui layout clear` | Implement in v0.3.23 | Token dashboard correctness is user-facing. The math must add up, and the TUI must remain understandable while using standard Ratatui widgets. |
| #266 `feat(ranking): improve file and folder ranking for agent navigation` | Backlog, first in next tranche | Foundational for #267 and #269, but larger than a patch release. Needs deterministic scoring design and broader ranking tests. |
| #267 `feat(navigation): add a next-step recommendation command and MCP tool` | Backlog after #266 and #269 shape | Depends on better ranking and explainable reasons so `next` recommendations are not opaque or incidental. |
| #268 `chore(purpose): reduce low-value purpose and health noise` | Backlog, can run parallel after output scope is clear | Worth doing, but changes health/lint policy and must not encode local workspace names as product invariants. |
| #269 `feat(output): explain why ranked ProjectAtlas results were selected` | Backlog after #266 scoring | Reasons should explain real ranking signals, not today's incidental ordering. |
| #276 `feat(mcp): auto-detect nearest ProjectAtlas DB for addressed paths` | Backlog, independent but risky | Useful multi-project ergonomics, but root switching can break isolation if precedence rules are wrong. |
| #278 `chore(plugin): apply installer convergence checks to Claude Code` | Backlog after shared installer convergence | Host-specific convergence should build on stable shared installer drift repair. |
| #279 `chore(plugin): apply installer convergence checks to OpenCode` | Backlog after shared installer convergence | Same host-convergence class as #278. |
| #281 `feat(mcp): close complete CLI command parity gaps` | Backlog, broad parity tranche | Valuable but too broad for the token/plugin patch release. Needs command-family inventory and safe MCP write boundaries. |

## Global Pre-Mortem

### What Can Go Wrong

- Installer trusts `codex plugin list` version and marketplace ref, then installs
  a runtime while Codex still exposes a stale ProjectAtlas skill or plugin
  manifest from the reported source path.
- Installer reacts to an official stale cache by hard-failing only, leaving the
  user with a stale plugin when Codex can repair the cache through remove/add.
- Installer mutates non-official or intentionally managed marketplaces.
- Token TUI mixes gross token estimates with conservative deduped accounting,
  so `without - with` does not equal the visible saved value.
- Token TUI becomes prettier but loses old information such as conservative
  tokens avoided, file reads avoided, observed vs modeled split, repeated
  baselines, calibration guidance, or bucket plain meanings.
- Token TUI uses custom hand-drawn layout where Ratatui standard widgets would
  give better table spacing, charting, and style semantics.
- Four trend windows make the overview cramped or unreadable.
- Tests assert only labels and miss the actual accounting invariants or rendered
  layout shape.
- Release workflow fails because version pins are not bumped from `0.3.22` to
  `0.3.23` before dispatch.

### Release Constraints

- Keep token telemetry data structures and JSON/TOON output schemas unchanged.
- Keep installer behavior conservative for non-official Codex marketplaces.
- Use Ratatui widgets for the TUI where they fit: `Chart`, `Gauge`, and
  `Table`.
- Keep `projectatlas token --view tui --trend day|week|month|year` as the
  detailed trend mode; do not add interactive click/keyboard selection in this
  patch release.
- Preserve deterministic string rendering for CLI/MCP chart snapshots and tests.
- Bump all workspace and plugin manifest version pins to `0.3.23` before
  release.

## #285 Stale Codex Source Manifest Repair

### Summary

The ProjectAtlas installer must verify the Codex plugin source manifest at the
path reported by `codex plugin list`, not just the installed plugin version and
marketplace ref.

### Expected Behavior

- If official `projectatlas` marketplace ref, installed plugin version, and the
  reported source manifest all match the expected release, the installer can
  skip plugin cache repair.
- If the official marketplace ref is current but the reported plugin source
  manifest is stale, the installer repairs the plugin cache through Codex plugin
  remove/add and verifies the manifest again.
- If the cache still reports a stale source manifest after repair, the installer
  reports a clear mismatch with the manifest path and expected version.
- If the marketplace is not the official ProjectAtlas marketplace, the installer
  does not mutate it.

### Implementation Touch Points

- `plugins/projectatlas/scripts/install-runtime.ps1`
- `plugins/projectatlas/scripts/install-runtime.sh`
- `crates/projectatlas-cli/tests/e2e.rs`
- `.github/workflows/ci.yml`

### Test Plan

- Fake Codex reports current plugin version and current marketplace ref, but its
  `source.path/.codex-plugin/plugin.json` contains an old version.
- Fake `codex plugin add` rewrites that manifest to the current version.
- Assert installer runs `plugin remove projectatlas --marketplace projectatlas`
  and `plugin add projectatlas --marketplace projectatlas`.
- Assert installer does not mutate the marketplace source when the marketplace
  ref is already current.
- Keep existing tests for non-official marketplace skip, marketplace restore on
  reinstall failure, matching runtime version, and current cache no-op.

### Acceptance Criteria

- Installer repairs an official stale source manifest without requiring manual
  cache deletion.
- Installer output names the stale source manifest mismatch before repair.
- Installer output verifies the ProjectAtlas skill after repair.
- CI runs the new repair e2e smoke.

## #286 Token TUI Accounting And Ratatui Layout

### Summary

The token TUI should keep all old token-savings information, make every visible
field mathematically consistent, and present the dashboard with standard
Ratatui widgets.

### Expected Accounting

- `estimated_without_projectatlas - estimated_with_projectatlas =
  legacy_gross_estimated_saved`
- `legacy_gross_estimated_saved` remains a telemetry compatibility value, but
  the human overview does not render it as a competing saved-token headline.
- Conservative `tokens_avoided` is the visible TUI saved-token total, where
  `measured_tokens_saved + deduped_modeled_tokens_avoided = tokens_avoided`.
- The file-handling section shows the same conservative saved-token equation as
  `Tokens avoided = measured_tokens_saved + deduped_modeled_tokens_avoided`
  so avoided navigation tokens are visibly included alongside observed
  summary/slice savings.
- `observed_file_read_replacements + modeled_file_reads_avoided =
  likely_file_reads_avoided`.
- Each bucket row satisfies
  `estimated_without_projectatlas - estimated_with_projectatlas =
  estimated_saved`.

### Expected TUI Shape

- The overview still shows:
  - lookup count,
  - tokens avoided,
  - likely file reads avoided,
  - observed summaries/slices,
  - search-modeled narrowing,
  - confidence,
  - source rows and plain meanings,
  - repeated baseline dedupe note,
  - tokenizer calibration guidance.
- The trend section shows compact `day`, `week`, `month`, and `year`
  signed saved-token `Chart`s directly in the overview.
- The file section is titled `File Handling Optimization Overview` and uses a
  `Gauge` plus a `Table`; it shows `Tokens avoided`, `File reads avoided`,
  source labels, read counts, token totals, and meaning.
- The source table uses a `Table` header with visible separation before the
  first data row and compact labels that remain readable at 80 columns.

### Non-Goals

- No interactive click, mouse, tab, or keyboard selector in `v0.3.23`.
- No schema changes to token telemetry.
- No provider API calls for token counting or trend rendering.

### Test Plan

- Unit test data where gross saved and conservative avoided differ.
- Assert the gross compatibility value does not appear as a competing TUI
  saved-token total when it differs from conservative avoided tokens.
- Assert the file-read equation and bucket equations.
- Assert rendered dashboard contains the new trend windows and file-handling
  panel.
- Assert negative saved-token periods and operands remain visibly signed in the
  trend charts and token-mix label.
- Assert rendered buffer cells use Ratatui styles for key headers.
- Run CLI smoke for:
  - `projectatlas token --view tui`
  - `projectatlas token --view tui --tokenizer cl100k_base`
  - `projectatlas token --view tui --trend month`
- Manually render the dashboard and inspect a screenshot before release.

### Acceptance Criteria

- A user can read the dashboard without seeing contradictory math.
- The dashboard is more engaging and easier to scan, but keeps the old
  information.
- Day/week/month/year trend context is visible in the overview.
- Tests cover math, layout, and style shape.

## Codex Plugin Engineering Skill

### Summary

Stable ProjectAtlas plugin engineering lessons should be durable in-repo
instructions, not only chat history.

### Scope

- Add `skills/codex-coding-plugin/SKILL.md`.
- Add an agent prompt under `skills/codex-coding-plugin/agents/openai.yaml`.
- Cover release pins, plugin manifest drift, runtime installer verification,
  Codex plugin cache repair, MCP registry repair, and host-specific config
  convergence.

### Acceptance Criteria

- Skill validates with the local Codex skill validator.
- Skill avoids stale absolute workspace assumptions.
- ProjectAtlas purposes are set for the skill folder and files.

## Backlog Tranche Pre-Mortem

### #266 Ranking Improvement

Goal: improve deterministic folder/file ranking for agent navigation without
adding embeddings or new external dependencies.

Risks:

- New scoring overfits current repo paths.
- Rankings become less predictable for small repos.
- Search/content fallback makes queries slower or noisier.

Next spec needs:

- Central scoring contract.
- Bounded signals for path, purpose, content summary, text match, symbols, and
  paired tests.
- Golden ranking fixtures across small and medium repos.

### #267 Next-Step Recommendation

Goal: add `projectatlas next <query>` and `atlas_next`.

Dependencies:

- #266 ranking must produce stable candidates.
- #269 reasons should explain recommendations.

Risks:

- Recommendations become vague task advice instead of atlas-grounded next file
  or folder actions.
- MCP and CLI shapes drift.

### #268 Purpose And Health Noise

Goal: reduce low-value purpose/health noise without hiding real drift.

Risks:

- Health demotion can hide stale runtime/plugin problems.
- Local editor, cache, or agent folder names can accidentally become product
  invariants.

Constraint:

- `.gitignore` remains the broad project policy; ProjectAtlas ignore config is
  only a stricter atlas layer.

### #269 Ranked Result Reasons

Goal: explain why ranked folders/files were selected.

Dependencies:

- #266 should define real scoring signals first.

Risks:

- Reasons expose noisy implementation details.
- Reasons become too verbose for token-saving workflows.

### #276 Nearest DB Routing

Goal: auto-detect the nearest `.projectatlas/projectatlas.db` for addressed
paths without creating databases.

Risks:

- Wrong project root selection in shared MCP sessions.
- Implicit routing undermines `project_path` isolation.

Constraints:

- Explicit `project_path` always wins.
- No auto-create behavior.
- Clear error when no indexed DB exists.

### #278 Claude Code Convergence

Goal: extend installer convergence checks to Claude Code generated config and
runtime pointers.

Risks:

- Host-specific config writes break user-managed Claude settings.
- Generated config status is confused with global host registry status.

### #279 OpenCode Convergence

Goal: extend installer convergence checks to OpenCode generated config and
runtime pointers.

Risks:

- OpenCode command array handling differs from Codex/Claude config shape.
- Host-specific repair creates stale cwd or DB assumptions.

### #281 CLI/MCP Parity

Goal: inventory CLI commands and close safe MCP gaps.

Risks:

- Unsafe write/admin commands get exposed over MCP without guardrails.
- Parity work becomes a large refactor instead of command-family increments.

Next spec needs:

- Command inventory.
- Safe read-only first tranche.
- Explicit write-tool safety policy.
- CLI/MCP shape tests.

## Release Plan

1. Finish #285 and #286 implementation.
2. Bump workspace and plugin versions to `0.3.23`.
3. Run focused gates:
   - `cargo fmt --check`
   - `cargo check --locked -p projectatlas-cli --all-targets`
   - `cargo test --locked -p projectatlas-cli token_tui::tests`
   - focused plugin installer e2e tests for Codex cache repair
   - `projectatlas lint --report-untracked --purpose-level low`
   - `git diff --check`
4. Render and inspect `projectatlas token --view tui`.
5. Open PR for `v0.3.23`.
6. Merge after CI.
7. Let `03-Auto-Release` dispatch `02-Release`, or manually dispatch
   `release.yml` with `version=v0.3.23` if needed.
8. Verify GitHub release `v0.3.23` exists with expected assets.
9. Run installer/runtime verification for the new release.
10. Close #285 and #286 with release and verification notes.

## Rollback Plan

- If installer repair fails in CI, revert #285 changes and publish no release.
- If token TUI layout fails or looks worse, keep the accounting fix but reduce
  the layout to standard Ratatui table/gauge sections before release.
- If `v0.3.23` release automation fails after tag creation, rerun
  `02-Release` for the same tag only when the tag points at the intended
  commit.
