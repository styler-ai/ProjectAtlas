## 1. Persistent-Host Reproduction

- [x] 1.1 Map `report-installer-host-restart` to GitHub issue #383 in `openspec/issue-map.json` and keep the issue checklist synchronized with this file.
- [x] 1.2 Extend the Windows locked-mirror regression to model one persistent parent environment, the installer child, and a later sibling bare-CLI probe, proving the child-only success claim before the fix.

## 2. Truthful Installer Readiness

- [x] 2.1 Capture inherited bare-command identity before PATH mutation, report stable-mirror synchronization from the existing helper, and derive restart-required state only when the unchanged parent would remain stale.
- [x] 2.2 Emit one deterministic final runtime/generated-config, installer-CLI, and host-restart readiness record plus clear required warning while preserving exit-zero partial success, persisted User PATH, and absolute Codex/Claude/OpenCode MCP paths.

## 3. Safety and Verification

- [x] 3.1 Cover unlocked mirror, already-current inherited PATH, fresh-host convergence, locked-process survival, no pre-existing or unrelated process termination, bounded owned-probe termination, and unchanged generated-config/registry verification.
- [ ] 3.2 Run focused Windows installer trust/E2E tests, script syntax/static checks, affected Rust check/clippy/test/doc gates, OpenSpec validation, IssueOps, and live review-feedback reconciliation.
