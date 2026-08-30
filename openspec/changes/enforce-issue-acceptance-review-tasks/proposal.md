## Why

ProjectAtlas IssueOps currently treats the OpenSpec implementation checklist as the only authoritative task field, so an issue can appear complete without a separate, visible judgment that the delivered result fulfills the complete issue intent and that its source, specifications, architecture, and tests are sound. The issue contract needs two distinct task authorities without weakening the substantive issue packet or encoding agent models in CI.

## What Changes

- **BREAKING for open mapped issue bodies:** rename the authoritative `OpenSpec Tasks` field to `Implementation Tasks` while continuing to mirror the mapped `tasks.md` text, order, ownership, and checked state exactly.
- Require one separate `Acceptance and Review Tasks` field on every open mapped issue, with independently checked intent/outcome, implementation/source, specification/architecture, test/proof, and final-readiness review gates.
- Stop requiring future implementation task lists to end with the historical architecture-review row because specification/architecture reconciliation now belongs to the acceptance checklist; preserve every such row already present in an existing issue/OpenSpec task list.
- Require exactly one issue complexity label on every open issue: `complexity:low`, `complexity:medium`, `complexity:high`, or `complexity:very-high`. IssueOps validates only the label contract; external agent routing may use the value without putting model policy into CI.
- Keep IssueOps model-blind: it validates task structure and completion only; reviewer-model routing remains outside the repository checker.
- Preserve the complete existing issue packet—`Why`, `What Changes`, `Capabilities`, `Architecture Diagrams`, `Release Scope`, `Non-Goals`, and `Pre-Mortem`—and retain every existing diagram, mitigation, relationship, publication, and release gate.
- Require all implementation and acceptance/review tasks to be checked before issue closure or milestone completion; ordinary in-progress pull requests may retain unchecked tasks.
- Make pull-request validation branch-aware so immediate live task progress on one issue cannot fail an unrelated concurrent branch: compare the owning issue's candidate task slice with live state, require unrelated task slices to remain unchanged from the pull-request base, and keep `main` and release validation global and fail-closed.
- Migrate every open mapped issue before activating the new gate. Preserve closed historical issue bodies and keep their legacy task headings readable for historical validation.
- Record the only legacy-contract exceptions as per-issue `legacy_closed_issues` provenance in `openspec/issue-map.json`; mapped issues outside that explicit repository-controlled set use the new contract, and pull-request validation freezes the set after its initial introduction.
- Update issue templates, repository workflow guidance, pull-request guidance, and behavior-focused contract tests to describe and enforce the same two-list model.

## Capabilities

### New Capabilities

- `issue-implementation-acceptance-contract`: Two authoritative issue task fields that separate OpenSpec-backed implementation work from holistic issue acceptance and review.

### Modified Capabilities

None.

## Impact

- IssueOps parser and self-test: `.github/scripts/issue-checklists.py`.
- Issue event and CI/release enforcement: `.github/workflows/issueops.yml`, `.github/workflows/ci.yml`, existing release callers, and behavior-focused workflow tests.
- Open issue bodies mapped by `openspec/issue-map.json`, complexity labels on every open issue, and unchanged closed issue history.
- Applicable issue and pull-request templates plus `docs/workflow.md`, `docs/agent-integration.md`, repository instructions, and the version-matched ProjectAtlas plugin guidance when it repeats the issue contract.
- No Rust product/runtime, CLI/MCP, crate, dependency, schema, migration, query, transaction, or SQLite behavior changes.

This change is ready for implementation after its issue packet, design, specification, task plan, v0.5 dependency relationship, and open-issue migration boundary are accepted.

## Non-Goals

- Encoding Sol, Luna, Terra, model names, reasoning levels, or semantic reviewer judgments in IssueOps.
- Treating issue complexity as priority, duration, story points, or changed-line count, or asking IssueOps to infer whether the selected complexity is semantically correct.
- Replacing issue-specific product intent with generic review boilerplate or weakening any current issue section, OpenSpec requirement, architecture view, test obligation, or release proof.
- Creating task receipts, SHA evidence ledgers, one test per checkbox, automated semantic scoring, or an LLM CI gate.
- Rewriting closed issue history or requiring completed historical issues to adopt the new headings.
- Making every open pull request wait for final issue acceptance when the owning issue intentionally spans additional delivery work.
- Accepting unrelated task-list edits in a pull request or weakening complete live validation on `main` and release gates.
