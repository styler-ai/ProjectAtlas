## 1. Spec and Issue Setup

- [x] 1.1 Create the OpenCode OpenSpec proposal, design, spec delta, and task list with pre-mortem risks.
- [x] 1.2 Map `converge-opencode-installer` to GitHub issue #279 in `openspec/issue-map.json`.
- [x] 1.3 Mirror this task list into #279 under `OpenSpec Tasks` and assign the release milestone.

## 2. Installer Implementation

- [x] 2.1 Add PowerShell generated OpenCode config validation for runtime path, version guard, DB path, optional config path, final `mcp`, `type`, `enabled`, and `cwd`.
- [x] 2.2 Add POSIX generated OpenCode config validation with the same field contract.
- [x] 2.3 Ensure OpenCode convergence output is explicit about generated config verification and required host restart.

## 3. Tests and Documentation

- [x] 3.1 Add installer e2e coverage that validates the generated OpenCode config after a runtime update.
- [x] 3.2 Update README, agent integration docs, plugin skill, AGENTS, and templates with the OpenCode convergence contract.
- [x] 3.3 Run OpenSpec, issue-checklist, installer, and Rust verification gates.
