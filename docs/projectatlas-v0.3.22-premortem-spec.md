# ProjectAtlas v0.3.22 Pre-Mortem And Issue Specification

Date: 2026-07-04

## Purpose

This document defines the pre-implementation scope for the next ProjectAtlas
release after v0.3.21. It covers every open GitHub issue at the time of the
goal reset, classifies each issue, lists failure modes before coding, and
defines the required unit and end-to-end tests.

No issue in this release may be implemented only because it is open. It must
either fix a real defect, improve agent workflow measurably, or be moved out of
the release scope.

## Current Issue Decisions

| Issue | Decision | Rationale |
| --- | --- | --- |
| #270 `feat(telemetry): measure whole-file reads avoided by ProjectAtlas` | Implement in v0.3.22 | Directly addresses user-facing token-savings clarity and read-avoidance reporting. |
| #282 `fix(plugin): repair stale ProjectAtlas skill and instruction pointers across hosts` | Implement in v0.3.22 | Same drift class as v0.3.21 marketplace/MCP repair; stale instructions can make agents follow old workflows. |
| #283 `fix(installer): prevent downstream workflow ProjectAtlas release pins from drifting` | Implement in v0.3.22 | Downstream workflows can pin old ProjectAtlas release assets after plugin/runtime updates; ProjectAtlas should detect or guide repair. |
| #281 `feat(mcp): close complete CLI command parity gaps` | Backlog | Valuable but broad. Needs a dedicated command-family parity design and should not be rushed into the token/plugin drift release. |
| #279 `chore(plugin): apply installer convergence checks to OpenCode` | Backlog | Still useful as broad host convergence work after the shared stale instruction pointer fix. |
| #278 `chore(plugin): apply installer convergence checks to Claude Code` | Backlog | Still useful as broad host convergence work after the shared stale instruction pointer fix. |
| #276 `feat(mcp): auto-detect nearest ProjectAtlas DB for addressed paths` | Backlog | Useful ergonomic idea, but path-root auto-switching has ambiguity and isolation risk. Keep out of this release. |
| #271 `feat(mcp): make MCP parity first-class for navigation improvements` | Close as superseded by #281 after adding a closing comment | The broader #281 now covers command-family parity, including normal navigation parity. Keeping both creates duplicate planning surfaces. |
| #269 `feat(output): explain why ranked ProjectAtlas results were selected` | Backlog | Useful, but should follow ranking service work so reasons explain real scoring rather than current incidental ordering. |
| #268 `chore(purpose): reduce low-value purpose and health noise` | Backlog | Useful cleanup, but it changes lint/health policy and needs its own risk review. |
| #267 `feat(navigation): add a next-step recommendation command and MCP tool` | Backlog | High-value, but depends on ranking and reason output. |
| #266 `feat(ranking): improve file and folder ranking for agent navigation` | Backlog | Foundational and high-value, but larger than the release-drift and token-reporting fixes selected for v0.3.22. |

## Global Pre-Mortem

### What Can Go Wrong

- The release ships user-facing metrics that overclaim precision. Token and
  read-avoidance numbers are workflow heuristics, not provider billing totals.
- Installer fixes mutate host state too aggressively and break managed Codex,
  Claude Code, or OpenCode environments.
- Cross-host support guesses at Claude/OpenCode behavior instead of only using
  verified host surfaces.
- Downstream workflow drift detection becomes noisy and flags historical docs
  or test fixtures.
- A narrow lint/check hardcodes one consumer repository convention as a
  ProjectAtlas product invariant.
- Fixes are tested only on the local Windows shell and fail on Linux/macOS
  release runners.
- Open issues are left half-classified, so the next agent repeats the same
  triage.

### Mitigations

- Label derived metrics as heuristic/product-health signals.
- Add tests before implementation for each selected behavior.
- Keep installer mutations ProjectAtlas-scoped and add opt-out or warning-only
  paths where host state is not safely repairable.
- Prefer generated configs and documented warnings over silent edits in
  downstream projects.
- Run both focused tests and full Rust workspace gates before release.
- Update GitHub issue metadata after the spec, before implementation commit.

## #270 Specification: Token Savings And Read Avoidance

### Summary

Make `projectatlas token`, `projectatlas token --view tui`, and
`atlas_token_report` explain ProjectAtlas value in plain terms:
total tokens avoided, measured summary/slice savings, navigation tokens avoided,
and likely whole-file reads avoided.

### Goal

A nontechnical user should be able to open the token dashboard and understand:

- how many tokens ProjectAtlas likely kept out of the chat,
- which portion came from measured whole-file compression,
- which portion came from narrowing to the right folders/files,
- how many likely whole-file reads were avoided,
- that the numbers are local workflow estimates, not billing totals.

### Non-Goals

- Do not call model-provider token APIs.
- Do not store private source content beyond existing telemetry.
- Do not require agent harness changes.
- Do not add a database migration unless existing telemetry cannot express the
  counters.
- Do not hide the existing detailed bucket fields from JSON/TOON consumers.

### Requirements

- Add report-level read-avoidance counters derived from raw telemetry events
  only when command evidence is still available:
  - observed ProjectAtlas full-file replacements,
  - modeled search calls likely avoiding broad reads,
  - total likely whole-file reads avoided.
- Keep `tokens_avoided`, `measured_tokens_saved`, and modeled-token totals
  backward compatible.
- Add a `read_avoidance` section to TOON output and read-avoidance fields to
  JSON output.
- Rewrite TUI copy so the first screen uses plain language instead of raw
  accounting labels such as `observed_delta` and `deduped modeled`.
- Render the main token view as a vertical with/without ProjectAtlas comparison,
  followed by a cake-style file-read-avoidance mix for observed versus
  search-modeled reads.
- Keep detailed bucket data available, but present it as "How ProjectAtlas
  helped" with short explanations.
- Preserve `PROJECTATLAS_NO_TELEMETRY` behavior.

### Edge Cases

- No telemetry events: report zero counters and an explanatory empty state.
- Only observed events: confidence should be observed, not modeled.
- Only modeled navigation events: clearly label likely/heuristic.
- Negative savings: keep signed values and avoid treating regressions as saved
  tokens.
- Duplicate modeled baselines: token dedupe should not hide the number of
  ProjectAtlas calls used for likely read avoidance.
- Aggregate-only bucket reports: do not infer read-avoidance counts because the
  original command name is unavailable.
- Overview/folders/files/health/purpose queue events: they have navigation
  value, but do not count them as likely whole-file reads avoided.
- Very large totals: keep saturating arithmetic and formatting safe.
- Tokenizer calibration: keep calibration separate from read-avoidance counts.
- MCP chart output: chart and structured data must agree.

### Implementation Plan

1. Add derived read-avoidance counters to `TokenOverview`.
2. Populate counters from raw events only for eligible commands:
   `summary`, `outline`, `slice`, `symbols slice`, `mcp.atlas_file_summary`,
   `mcp.atlas_outline`, `mcp.atlas_slice`, and `mcp.atlas_search`.
3. Keep aggregate-only `TokenOverview::from_buckets` read-avoidance counters at
   zero with `not_recorded` confidence.
4. Render a `read_avoidance` section in `render_token_overview`.
5. Rework the Ratatui overview dashboard labels, vertical comparison chart,
   file-read avoidance mix, and bucket table.
6. Update CLI/MCP tests and add an e2e behavior check.
7. Run the dashboard on ProjectAtlas itself and inspect the output.

### Code Touch Points

- `crates/projectatlas-core/src/telemetry.rs`
- `crates/projectatlas-core/src/toon.rs`
- `crates/projectatlas-cli/src/token_tui.rs`
- `crates/projectatlas-cli/src/main.rs`
- `crates/projectatlas-cli/src/mcp.rs`
- `crates/projectatlas-cli/tests/e2e.rs`

### Unit Tests

- `TokenOverview::from_events` counts observed replacements, modeled avoided
  reads, and total likely reads.
- `TokenOverview::from_buckets` reports zero read avoidance with `not_recorded`
  confidence because aggregate buckets do not preserve command names.
- Eligible observed commands count as observed replacements.
- Eligible search commands count as modeled avoided reads.
- Overview/folders/files/health/purpose queue events do not count as likely
  full-file reads avoided.
- Zero-baseline events do not count as likely full-file reads avoided.
- Duplicate modeled baselines still dedupe tokens but retain modeled call count.
- Negative savings do not produce false positive saved-token claims.
- `render_token_overview` includes `read_avoidance` fields and keeps old token
  totals.
- `token_tui` renders plain-language labels and no longer relies on raw
  accounting labels for the primary view.

### End-To-End Tests

- Run a tiny temp repo through scan plus summary/search/slice-like ProjectAtlas
  commands, then assert `projectatlas token` includes read-avoidance counters.
- Call `atlas_token_report` through the MCP smoke path and assert the same
  fields are present.
- Run `projectatlas token --view tui` and assert the dashboard includes:
  `Total tokens avoided`, `Tokens: with vs without ProjectAtlas`,
  `File reads avoided`, `Measured from summaries`, and
  `Narrowed to right files`.

### Acceptance Criteria

- CLI, MCP, JSON, TOON, and TUI surfaces expose the same read-avoidance counts.
- Existing token totals remain present and compatible.
- Tests cover aggregation, rendering, e2e CLI behavior, and MCP parity.
- Documentation/skill wording explains the metric as heuristic and local.

## #282 Specification: Stale Skill And Instruction Pointers

### Summary

Prevent plugin updates from leaving agents on stale ProjectAtlas skills or
instructions after the runtime/plugin version changes.

### Severity / Impact

High for agent correctness. If the runtime is v0.3.21 but the loaded skill or
host instruction pointer is from v0.3.6 or another stale cache, agents may use
obsolete MCP setup, pathing, installer, or workflow rules.

### Repro Context

- A downstream SwipingGale session found stale `0.3.6` ProjectAtlas wording and
  stale workflow pins while the installed plugin/runtime was v0.3.21.
- This ProjectAtlas session initially received a stale ProjectAtlas skill path
  in tool-provided skill metadata, while the active plugin cache contained
  v0.3.21.

### Expected vs Actual

- Expected: after installer/update, ProjectAtlas runtime, Codex marketplace,
  MCP registry, ProjectAtlas skill/instructions, and generated host configs are
  aligned to the verified plugin version or report a host limitation.
- Actual: v0.3.21 runtime and plugin can coexist with stale skill/instruction
  references in a running or downstream host environment.

### Non-Goals

- Do not rewrite unrelated user skills.
- Do not mutate unknown host state.
- Do not pretend a running Codex/Claude/OpenCode process can always refresh
  in-memory skills without restart support.
- Do not invent a native OpenCode plugin surface if only generated MCP config
  is supported.

### Requirements

- Installer output must verify or report the ProjectAtlas skill/instruction
  surface per host:
  - Codex plugin cache and ProjectAtlas skill path,
  - Claude Code plugin metadata or generated instruction/config surface,
  - OpenCode generated config/template surface.
- If a stale official ProjectAtlas host pointer can be safely repaired, repair
  it.
- If the host exposes no repairable pointer, print a clear limitation and
  restart/reinstall guidance.
- Keep the verified runtime path, DB path, config path, and `--require-version`
  unchanged.
- Keep all behavior OS-agnostic.
- Add managed-environment skip variables only if actual mutation is performed.

### Edge Cases

- Running host has stale in-memory skill metadata but persistent plugin cache is
  current: report restart requirement, do not fail install.
- Multiple ProjectAtlas plugin cache versions exist: active/installed version
  must win.
- Non-official ProjectAtlas-like host entry exists: do not mutate it.
- Host command is missing: skip with a warning, not failure.
- Claude/OpenCode expose only generated config, not a plugin marketplace:
  verify generated config and document no-op host cache repair.
- Windows path normalization must not compare mixed slash forms as different
  versions.
- POSIX and PowerShell installers must behave equivalently.

### Fix Plan

1. Identify persistent ProjectAtlas skill/instruction surfaces that installers
   can safely inspect.
2. Add read-only verification after runtime/MCP config generation.
3. Add guarded repair only for official ProjectAtlas-owned entries.
4. Add clear installer output for current, repaired, skipped, and restart-needed
   states.
5. Add fake Codex e2e fixtures and host-config output checks for Claude Code
   and OpenCode surfaces.
6. Update plugin skill docs and AGENTS templates with the restart limitation.

### Code Touch Points

- `plugins/projectatlas/scripts/install-runtime.ps1`
- `plugins/projectatlas/scripts/install-runtime.sh`
- `plugins/projectatlas/skills/projectatlas/SKILL.md`
- `plugins/projectatlas/.claude-plugin/plugin.json`
- `plugins/projectatlas/opencode/opencode.json`
- `docs/agent-integration.md`
- `README.md`
- `AGENTS.md`
- `templates/AGENTS.md`
- `crates/projectatlas-cli/tests/e2e.rs`

### Unit Tests

- Installer helper parsing recognizes current and stale ProjectAtlas skill paths.
- Non-official host entries are skipped.
- Current entries are no-ops.
- Stale official entries either repair or produce a restart/limitation warning.

### End-To-End Tests

- Fake Codex cache with a ProjectAtlas skill artifact is verified.
- Fake Codex current cache is left untouched.
- Fake Claude/OpenCode generated configs are verified against the runtime
  version and absolute paths.
- Claude/OpenCode generated-config integrations report the host restart/cache
  limitation instead of pretending to repair native host state.
- Windows and POSIX installer test selections both exercise the new checks.

### Acceptance Criteria

- Installer smoke output reports host instruction/skill verification status.
- An update cannot silently leave persistent official ProjectAtlas host
  instructions pointing at an older plugin version when repair is possible.
- Running-host restart limitations are documented, not hidden.
- Tests cover stale, current, non-official, and host-missing cases.

## #283 Specification: Downstream Workflow Release Pin Drift

### Summary

Detect or prevent downstream GitHub workflow references to stale ProjectAtlas
release assets after plugin/runtime updates.

### Severity / Impact

Medium. A consumer repo can pass local ProjectAtlas checks while CI continues
to download an old ProjectAtlas release such as v0.3.1, producing inconsistent
CI behavior and stale agent context.

### Repro Context

A downstream SwipingGale workflow pass found several GitHub Actions downloading
`ProjectAtlas/releases/download/v0.3.1/...` while the installed and latest
ProjectAtlas plugin/runtime was v0.3.21.

### Expected vs Actual

- Expected: ProjectAtlas install/update guidance makes stale workflow pins
  visible and points to the verified release tag.
- Actual: stale downstream workflow pins can survive plugin/runtime repair.

### Non-Goals

- Do not blindly rewrite arbitrary downstream workflows.
- Do not flag historical docs or release-note tests.
- Do not require network calls during lint unless the current release tag is
  already known from installer/plugin metadata.

### Requirements

- Add a deterministic stale-pin detector for downstream workflow files under
  `.github/workflows`.
- Detect URLs matching official ProjectAtlas release asset downloads where the
  tag/asset version does not equal the verified installer release tag.
- Report file path, line number, found version, and expected version.
- Offer a repair command or guidance, but do not auto-edit by default.
- Ensure ProjectAtlas's own release workflow continues deriving assets from
  `$RELEASE_VERSION`.
- Avoid false positives in historical docs and tests.

### Edge Cases

- Asset URL version and filename version disagree.
- URL points to a fork or non-official ProjectAtlas repo: ignore it to avoid
  false noise.
- Workflow uses a variable such as `$RELEASE_VERSION`: should pass.
- Workflow has multiple pins, mixed current/stale.
- Windows CRLF workflows should preserve line counting.
- Current release tag is unavailable: detector should say it cannot compare
  rather than guessing.
- Downstream repo intentionally pins an old migration version: leave warning-only
  behavior and let the user decide whether to keep or update the pin.

### Fix Plan

1. Add a small reusable workflow-pin scanner.
2. Call it from installer verification or a ProjectAtlas lint/report command
   where the expected release tag is known.
3. Print actionable warnings, not silent edits.
4. Add fixture workflows for stale, current, variable, and fork cases.
5. Document the repair guidance in plugin skill/AGENTS docs.

### Code Touch Points

- `plugins/projectatlas/scripts/install-runtime.ps1`
- `plugins/projectatlas/scripts/install-runtime.sh`
- `crates/projectatlas-cli/tests/e2e.rs`
- `plugins/projectatlas/skills/projectatlas/SKILL.md`
- `docs/agent-integration.md`
- `AGENTS.md`
- `templates/AGENTS.md`

### Unit Tests

- Scanner detects stale official release URL.
- Scanner accepts current release URL.
- Scanner accepts variable-derived release URL.
- Scanner ignores non-official repositories and historical docs.
- Scanner reports line number and expected version.

### End-To-End Tests

- Installer run in a temp project with stale workflow pin emits a warning.
- The same temp-project fixture includes current official and forked
  ProjectAtlas-looking URLs to verify they are not warned on.
- POSIX and PowerShell paths are covered by the installer contract test; the
  host platform executes the concrete installer e2e.

### Acceptance Criteria

- ProjectAtlas can detect stale official downstream workflow ProjectAtlas asset
  pins during install/update verification.
- Detection is warning-only unless an explicit repair command is later added.
- Tests cover stale/current/fork behavior and keep detection scoped to official
  ProjectAtlas release URLs.
- Docs tell users how to update stale workflow pins safely.

## Backlog Issue Pre-Mortems

### #281 Complete CLI/MCP Command Parity

Failure risk: implementing every CLI command through MCP at once can expose
unsafe mutating admin behavior or long-running watch loops through request/response
MCP calls. The issue is valid, but it needs a command-family inventory, reviewed
exceptions, and lifecycle design for long-running commands. Backlog.

Future tests: static CLI-to-MCP inventory test, route existence tests, isolated
mutating-command temp-project tests, and docs verifying CLI-only exceptions.

### #279 OpenCode Convergence Checks

Failure risk: assuming OpenCode has a native plugin/cache surface when the repo
currently only ships a generated config template. The issue is valid as a host
audit, but implementation should wait for verified OpenCode APIs or CLI
behavior. Backlog.

Future tests: fake OpenCode config/current/stale/non-official cases only after
the real host contract is known.

### #278 Claude Code Convergence Checks

Failure risk: copying Codex marketplace repair logic into Claude Code without a
matching Claude host API. The issue is valid, but broader than stale instruction
pointer verification selected in #282. Backlog.

Future tests: fake Claude config/current/stale/non-official cases after the real
host contract is known.

### #276 Nearest Ancestor DB Auto-Detection

Failure risk: auto-switching projects based on path ancestors can route reads or
writes to the wrong project in nested repositories, symlinked worktrees, or
multi-client MCP sessions. The idea is useful but intentionally deferred.
Backlog.

Future tests: nearest ancestor, nested project, symlink/canonicalization,
config-root mismatch, explicit `project_path` precedence, and no-db cases.

### #271 MCP Navigation Parity

Failure risk: duplicate issue management. #281 now covers parity more completely.
Close as superseded after adding a comment linking #281. No implementation.

### #269 Ranked Result Reasons

Failure risk: reasons that do not reflect actual ranking can create false
confidence. Implement after #266 ranking has stable scoring signals. Backlog.

Future tests: deterministic reason ordering, bounded reason count, JSON/TOON/MCP
shape checks, and no full-file reads for reason generation.

### #268 Purpose And Health Noise

Failure risk: demoting health findings can hide real release/runtime drift.
This needs a careful impact-classification design and should not be mixed with
plugin drift fixes. Backlog.

Future tests: high-impact stale purpose remains blocking, low-impact findings
are advisory, generated/assets stay opt-in, and MCP/CLI scopes match.

### #267 Next-Step Recommendation Command

Failure risk: a recommendation command built before ranking/reasons can automate
bad navigation. It should depend on #266 and #269. Backlog.

Future tests: CLI/MCP parity, bounded output, suggested commands only, no source
mutation, and realistic query fixtures.

### #266 Ranking Improvements

Failure risk: ranking can overfit ProjectAtlas fixtures or become too expensive
on large repos. It is foundational and useful, but not part of this release.
Backlog.

Future tests: path, purpose, summary, indexed text, symbol, and source/test
pairing signals; top-5 realistic query expectations; performance guard.

## Verification Plan For v0.3.22

### Focused Checks

- `cargo fmt --check`
- `cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- strict-strings`
- `cargo test --locked -p projectatlas-core telemetry --lib`
- `cargo test --locked -p projectatlas-core toon --lib`
- `cargo test --locked -p projectatlas-cli token_dashboard --bin projectatlas`
- Focused e2e tests for #270, #282, and #283.

### Full Release Gates

- `cargo check --workspace --all-targets --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo test --doc --all-features`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- `cargo run -p projectatlas-cli -- lint --report-untracked --purpose-level low`
- ProjectAtlas installer smokes on Linux, Windows, macOS x64, and macOS arm64
  through GitHub Actions.
- Release workflow for v0.3.22 with assets verified.

## Release Exit Criteria

- #270, #282, and #283 have passing tests and closing comments with commit and
  verification references.
- #271 is closed as superseded by #281, unless new evidence shows it needs to
  stay separate.
- All other open issues are labelled `status:backlog` with no milestone.
- v0.3.22 release assets exist for Linux, Windows, macOS x64, macOS arm64, and
  `SHA256SUMS`.
- Local installer re-run verifies Codex marketplace, MCP registry, generated
  configs, host instruction/skill status, and ProjectAtlas runtime version.
