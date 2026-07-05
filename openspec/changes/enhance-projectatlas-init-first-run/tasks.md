## 1. Spec, Issue, and Planning

- [x] 1.1 Create GitHub issue for one-call `projectatlas init` first-run setup and assign it to v0.3.26.
- [x] 1.2 Map `enhance-projectatlas-init-first-run` in `openspec/issue-map.json`.
- [x] 1.3 Mirror this task list into the GitHub issue under `OpenSpec Tasks`.
- [x] 1.4 Validate the OpenSpec change before implementation.
- [x] 1.5 Send the init spec/design/tasks to a subagent for review and disposition findings.

## 2. Current Implementation Inspection

- [x] 2.1 Inspect current `projectatlas init`, `init_project`, MCP `atlas_init`, scan pipeline, and purpose queue APIs.
- [x] 2.2 Confirm CLI/MCP report formats and existing output formatting patterns.
- [x] 2.3 Identify where plugin/skill guidance should describe low-reasoning subagent purpose curation.

## 3. Init Bootstrap Implementation

- [x] 3.1 Add typed init setup report structs/enums shared by CLI and MCP output.
- [x] 3.2 Expand CLI `projectatlas init` options for scan control and first-run behavior.
- [x] 3.3 Make init create/verify `.projectatlas/`, config, non-source TOON, and DB idempotently.
- [x] 3.4 Integrate default deep scan/index into init, with `--no-scan` and `--force-rescan` controls.
- [x] 3.5 Add purpose queue/handoff output with low-reasoning subagent instructions for plugin/agent harnesses.
- [x] 3.6 Expand MCP `atlas_init` parameters and result to match CLI behavior.
- [x] 3.7 Preserve wrong-root and nearest-project safety: init mutates only the explicit/current project root.
- [x] 3.8 Generate and report `.projectatlas/projectatlas.mcp.json`, `.projectatlas/projectatlas.claude.mcp.json`, and `.projectatlas/projectatlas.opencode.json`.

## 4. Plugin/Docs Guidance

- [x] 4.1 Update ProjectAtlas plugin/skill guidance so agents run `projectatlas init` for first-run setup.
- [x] 4.2 Document that purpose creation/correction is delegated to a low-reasoning subagent after init returns the handoff.
- [x] 4.3 Document that subagents apply reviewed purposes through ProjectAtlas purpose APIs and that API-written purposes count as approved.
- [x] 4.4 Update README/docs/CI examples so first-run setup does not immediately double-run `projectatlas scan` unless an explicit refresh is intended.

## 5. Tests

- [x] 5.1 Add tests for new repo init creating config, non-source TOON, DB, and scan report.
- [x] 5.2 Add tests for existing config/DB preservation and idempotent second run.
- [x] 5.3 Add tests for `--no-scan` and `--force-rescan`.
- [x] 5.4 Add tests for MCP `atlas_init` parity, explicit project path handling, and no active-default switch.
- [x] 5.5 Add tests for purpose handoff output without direct subagent spawning.
- [x] 5.6 Add tests for wrong-root/missing-index edge cases.
- [x] 5.7 Add tests that generated host MCP configs are created/reported by init.

## 6. Verification

- [x] 6.1 Run focused init/scan/MCP tests.
- [x] 6.2 Run `cargo fmt --check`.
- [x] 6.3 Run `cargo check --workspace --all-targets --all-features --locked`.
- [x] 6.4 Run `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- [x] 6.5 Run `cargo test --workspace --all-features --locked`.
- [x] 6.6 Run `openspec validate --all --strict --no-interactive`.
- [x] 6.7 Run `.github/scripts/issue-checklists.py` and update the GitHub checklist before closure.
