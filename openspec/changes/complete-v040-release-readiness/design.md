## Context

Issue #308 and its feature proof are closed, #340 is merged and closed, and #341 has only its final explicit empty-cache construction proof left. Issue #311 still contains an obsolete split of the pre-closure #308 program, is not mapped in `openspec/issue-map.json`, and includes a post-release cleanup checkbox that cannot truthfully be completed before publication.

The existing release path already has the required mechanics:

- `01-CI` owns Rust quality and source-built CLI/MCP E2E smoke on Linux, Windows, macOS x64, and macOS arm64.
- `optional-parser-pack` owns explicit cache-free Linux and Windows optional-pack construction and full runtime proof.
- `02-Release` owns version validation, package construction, installer smoke, release assets, and publication; `prepublish_only=true` exercises the package and installer path without publishing.
- `03-Auto-Release` dispatches `02-Release` after an eligible version reaches `main`.
- `02-Release` requires every issue in milestone `v0.4.0-00` to be mapped, checked, and closed before publication.

The user also requires branch, worktree, and external ProjectAtlas checkout cleanup only after v0.4.0 is published and independently verified. Release readiness and post-release cleanup therefore need separate owners even though both remain part of the same active release goal.

## Goals / Non-Goals

**Goals:**

- Make #311 a concise, mapped, mechanically synchronized readiness owner.
- Prove one exact final `dev` head through the existing local and hosted release surfaces.
- Keep `main`, tags, and GitHub releases untouched until all prepublication evidence is green.
- Prepare an exact `dev`-to-`main` promotion that can pass the existing milestone gate.
- Preserve a durable post-release owner for independent publication verification and safe workspace consolidation.

**Non-Goals:**

- Add or change product runtime behavior, Rust APIs, crate boundaries, SQLite state, CLI/MCP schemas, package formats, or dependencies.
- Create a second release workflow, test framework, evidence ledger, or task-specific receipt scheme.
- Reopen completed #308 work, absorb #314 into v0.4.0, or claim hosted success from local tests.
- Delete any branch, worktree, or checkout before publication verification and unique-work inventory.

## Decisions

### Reuse the existing release workflows

The change records and executes the existing `01-CI`, `optional-parser-pack`, `02-Release`, and `03-Auto-Release` contracts. No workflow or release orchestrator is added.

A new release driver was rejected because the current workflows already own the real package, installer, platform, and publication boundaries. Duplicating them would create drift without increasing proof.

### Bind readiness to one exact promotion head

First, lock one release-content head after #341 is closed and every v0.4.0 implementation and readiness artifact is merged. Run the complete local gates, ordinary CI, explicit clean optional-parser construction, prepublish release packaging, and review disposition on that head.

Once those tasks are true, one bounded reconciliation commit may change only the #311 OpenSpec task checkbox state. Mirror that state to the GitHub issue. Because `dev` is protected, land the task-state commit through a merge commit whose parent is that commit and whose Git tree is identical; the resulting `dev` merge SHA is the exact promotion head. Clean optional-parser and prepublish proof carry forward because the verified tree change affects no runtime, workflow, package, installer, or documentation input. Ordinary exact-head CI, strict OpenSpec, IssueOps, ProjectAtlas low lint, and review checks then run as post-commit closure gates on that `dev` SHA outside the reconciled checklist; #311 remains open until they pass. Any other path change, non-identical protection merge, or later tree change invalidates the affected proof and restarts the boundary at the release-content lock.

Promote only when `main` is an ancestor of the promotion head, and use a merge commit rather than squash or rebase. The resulting `main` commit must have the promotion head as a parent and the same Git tree. Its new SHA is therefore not a content change, and the actual `02-Release` run still repeats verification, packaging, and installer smoke on that `main` SHA before publication.

### Separate prepublication readiness from post-release operations

Issue #311 remains in milestone `v0.4.0-00` and owns only work that can be completed before `main` promotion. Before #311 closes, a dedicated non-milestone v0.4.0 post-release issue must exist and own:

- published tag, GitHub release, asset, checksum, and installer verification;
- installed runtime, plugin, MCP, CLI, and representative real E2E smoke;
- branch and worktree inventory;
- removal of only merged, obsolete, or superseded ProjectAtlas lanes;
- confirmation that the primary repository is the only long-term ProjectAtlas root.

Keeping that task unchecked inside #311 was rejected because `02-Release` correctly blocks publication when a milestone issue is open or incomplete. Checking cleanup before publication was rejected as false evidence.

### Keep evidence behavior-focused

OpenSpec and GitHub tasks state the behavior and gate to complete. Existing Actions runs, test definitions, workflow artifacts, release checksums, and review threads remain the evidence sources. The issue will not grow per-task SHA receipts, bespoke test identifiers, or duplicate status comments.

### Preserve the promotion rollback boundary

Until readiness is complete, `main`, tags, and releases remain untouched. If any candidate gate fails, fix the owning branch, produce a new exact candidate, and rerun affected proof. If the release workflow fails after `main` promotion, do not clean worktrees or delete branches; diagnose and retry the owning release path while the post-release issue remains open.

## Risks / Trade-offs

- **Checklist reconciliation advances the commit after expensive proof** → Permit one verified task-state-only commit, carry forward only unaffected behavioral proof, and run ordinary exact-head gates on its promotion head before closure.
- **Protected `dev` landing creates an unproven promotion SHA** → Require the protection merge to contain the task-state commit as a parent with an identical tree, then run closure gates on the resulting `dev` SHA.
- **Promotion changes the verified tree or hides it behind squash/rebase** → Require `main` ancestry, a merge commit with the promotion head as a parent, identical Git trees, and actual release verification on the resulting `main` SHA.
- **A green older run is mistaken for final evidence** → Compare every required run's `headSha` with the locked candidate before closure.
- **The milestone gate becomes circular around post-release cleanup** → Keep #311 prepublication-only and create the non-milestone post-release owner before closure.
- **A prepublish run is mistaken for publication** → Require `prepublish_only=true`, verify no tag or release was created, and leave publication to the existing main-triggered workflow.
- **Installer or plugin state passes in source but fails when packaged** → Require real package/installer smoke and installed CLI/MCP behavior from the release workflow.
- **Cleanup removes unique or dirty work** → Inventory dirtiness, unique commits, PR ownership, and merge/supersession status before each removal; retain uncertain lanes.

## Migration Plan

1. Land the bounded installer trust fix and reconcile all live review feedback.
2. Add this change to `openspec/issue-map.json` and replace #311's obsolete body with the exact local checklist.
3. Complete and close #341 after its Linux and Windows empty-cache proof and land its reconciled checklist state.
4. Merge all remaining readiness artifacts into `dev`, then lock the release-content head.
5. Run the complete local gates, exact-head `01-CI`, clean optional-parser proof, and `02-Release` with `prepublish_only=true` on that head.
6. Reconcile #311 in one task-state-only commit and mirror the GitHub checklist; its resulting `dev` SHA is the exact promotion head.
7. Run ordinary exact-head CI, strict OpenSpec, IssueOps, ProjectAtlas low lint, and review checks on the promotion head, close #311, and then pass milestone IssueOps.
8. Promote with a merge commit after verifying `main` ancestry and identical promotion/main trees; let the existing release workflow verify the resulting `main` SHA and publish v0.4.0.
9. Independently verify the published release and only then perform the post-release cleanup.

Rollback before promotion is ordinary correction followed by a new release-content lock and reconciliation. After promotion, retain all source lanes and release artifacts until the publishing failure is understood; never use workspace cleanup as rollback.

## Open Questions

None.
