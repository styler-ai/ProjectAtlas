## 1. Lean IssueOps

- [x] 1.1 Map every active OpenSpec task list to an authoritative GitHub issue through `openspec/issue-map.json`.
- [x] 1.2 Compare authoritative task text, order, ownership, and checked state exactly, and reject closed issues with unchecked tasks.
- [x] 1.3 Support split issue ownership only through ordered, disjoint, gap-free ranges that cover every local task exactly once.
- [x] 1.4 Run mapped checklist synchronization for ordinary pull requests and reserve full milestone completion for release.

## 2. Proven v0.3.26 Engineering Loop

- [x] 2.1 Let the smallest meaningful behavior, integration, E2E, smoke, or validation test prove a coherent implementation slice, including several related tasks when appropriate.
- [x] 2.2 Keep formatting, locked workspace check, strict Clippy, workspace tests, stable doctests, warning-free rustdoc, source lints, dependency policy, and affected behavior checks blocking.
- [x] 2.3 Keep ordinary issue references, milestone assignment, and actionable review-thread resolution, and integrate significant compiling v0.4 slices into `dev` before touching `main` or release publication.

## 3. Remove Evidence Ceremony

- [x] 3.1 Remove per-task test identifiers, verification plans, evidence ledgers, commit receipts, rendered evidence workflows, task-level links/path declarations, issue sealing, and the unfinished repository-wide coverage/mutation campaign.
- [x] 3.2 Align the pre-push hook, pull-request template, normal CI, IssueOps script, and workflow documentation with the lean checklist-and-tests contract while preserving real integrity controls.
- [x] 3.3 Prove the restored workflow with the single IssueOps self-test, focused workflow-policy E2E coverage, strict OpenSpec validation, ordinary workspace gates, and synchronized local/GitHub #309 checklists.

## 4. Restore The v0.3.26 Issue Contract

- [x] 4.1 Require every open mapped issue to contain the concise #305 structure: why, what changes, capabilities, release scope, non-goals, pre-mortem with likely failures and mitigations, and exactly one authoritative OpenSpec task section; keep closed historical issues compatible.
- [x] 4.2 Require each pre-mortem mitigation checkbox to reference owned OpenSpec task IDs and to be checked exactly when all referenced tasks are checked, reusing shared behavior tests rather than adding mitigation-specific proof.
- [x] 4.3 Remove residual issue-level SHA/commit-link evidence requirements and backfill #308, #309, #314, issue templates, `AGENTS.md`, and workflow guidance without weakening SHA-pinned Actions or release checksum/signature integrity.
- [x] 4.4 Extend the existing IssueOps self-test and shared workflow-policy E2E test for required sections, mitigation mapping/state, historical compatibility, and the absence of issue evidence ceremony; pass strict OpenSpec and live checklist synchronization.
  - Shared tests: [IssueOps parser and contract self-test](../../../.github/scripts/issue-checklists.py) and [`issueops_and_workflows_use_behavior_focused_quality_gates`](../../../crates/projectatlas-cli/tests/e2e.rs).
