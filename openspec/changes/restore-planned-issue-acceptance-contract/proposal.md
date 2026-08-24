## Why

ProjectAtlas issue forms still require observable acceptance criteria and type-specific problem context, but the canonical planned-issue checker omits acceptance and accepts headings whose prose does not explain the actor, current failure, intended behavior, ownership, or release role. The drift lets a mechanically valid packet reach implementation while its product intent still has to be reconstructed from OpenSpec tasks.

## What Changes

- Restore one canonical planned-issue order: `Why`, `What Changes`, `Capabilities`, `Architecture Diagrams`, `Release Scope`, `Acceptance Criteria`, `Non-Goals`, `Pre-Mortem`, then exactly one OpenSpec task section.
- Define acceptance criteria as two to five plain observable behavior bullets, never task checkboxes, OpenSpec task IDs, evidence receipts, or a second implementation checklist.
- Preserve concise applicable intake context: bugs retain enough trigger, environment, actual, and expected behavior to reproduce or truthfully identify proof-gap hardening; improvements retain the affected actor or surface and the before-to-after agent workflow.
- Extend IssueOps only with objective section, ordering, bullet-count, and forbidden-content checks while preserving the existing task, diagram, pre-mortem, release-graph, and publication gates.
- Require Sol semantic reconciliation before readiness, handoff, candidate acceptance, and stable acceptance; IssueOps proves structure and synchronization, not comprehension.
- Check in one deterministic `candidate-issue-bodies.json` containing the complete sanitized exact Markdown, issue number, and SHA-256 for every one of the thirty bodies. It is the sole publication source; ignored `.tmp` copies are non-authoritative and must match it byte-for-byte. After exact-head independent Sol and hosted Codex review, primary Sol validates the manifest, publishes only the exact UTF-8/LF body bytes reconstructed from it while the planning PR remains open, and reads the live bodies back by normalized bytes and hash with every mismatch or race fail-closed.
- Keep the four candidate OpenSpec task mappings in `openspec/issue-map.json` as required ownership declarations, but stage their exact twenty-nine-child future release graph and #499 campaign declaration in the separate tracked `candidate-issue-map.json` promotion manifest while authoritative `release_graphs.v0.5.0-00` continues to describe only current live GitHub state. After body publication makes normal unfiltered IssueOps/CI green, trusted-default-branch repository dispatch remains the sole normal merge route. If and only if this exact planning PR is the proven self-repair that the old trusted authorizer cannot consume, primary Sol may use one non-reusable bootstrap: pin the accepted immutable head, base/`main`, expected result tree, full protection state, and required check/app bindings; verify `issueops-merge-authorized` is the sole authorization blocker; temporarily remove only administrator enforcement; admin-squash only the pinned head inside mandatory finally restoration; then require exact restored protection, squash parent/tree/`main`, and repaired-`main` idempotent-dispatch readback. Any mismatch stops in restoration/recovery, and no required check may be removed, rebound, or forged. After exact planning-main readback, require #497, the shared #498/#499 change, and #500 to contain only accepted checked contract/specification work in section 1 while every implementation, test, campaign/GitHub synchronization, and final diagram-review task lives in section 2 or later and is checked only after substantive completion. Only then may the independent shared task assign #497-#500 their milestone/status/native release ownership and promote the future graph through a separate narrow atomic publication that removes both candidate manifests. Exact hosted/published/task/dependency/semantic readback precedes shared-task synchronization and every implementation handoff, with each issue's delivery and closure continuing normally.
- Migrate the complete v0.5.0 issue set, add #497-#500 as open executable direct #492 children only after their checked specification-task classification and truthful later-section states are exact, preserve all four issues' incomplete delivery tasks and real blocker edges after promotion, and let each close only after its own implementation, tests, GitHub synchronization, and final review are substantively complete.

## Capabilities

### New Capabilities

- `planned-issue-acceptance-contract`: Canonical explanatory, behavioral-acceptance, structural validation, semantic-review, migration, and release-readback rules for planned ProjectAtlas issues.

### Modified Capabilities

None. The v0.5 release-readiness change is synchronized in place because it is still active and owns the release-wide publication and acceptance contract.

## Impact

- Primary Sol has completed the personal global `issue-spec-writing` contract and remains its sole mutation owner; this repository change reads it as authoritative and does not edit it.
- Later Luna implementation updates `.github/scripts/issue-checklists.py`, its self-test, repository issue forms or repository guidance where needed, and the version-matched ProjectAtlas plugin guidance only if ownership review requires it; Luna may verify but never mutates the personal global skill, and no Rust product or database behavior changes.
- Sol owns the specification, exact issue-body migration, tracked candidate body and release-graph manifests, native hierarchy and milestone activation, authoritative release-graph promotion, semantic audit, publication, and readback.
- `openspec/changes/complete-v050-release-readiness`, the change-local `candidate-issue-bodies.json` and `candidate-issue-map.json`, `openspec/issue-map.json`, and `docs/v050-release-architecture.md` gain the staged release-wide contract without making candidate declarations active before hosted reconciliation.
- Pre-merge body publication exists only to make the reviewed planning PR executable under normal IssueOps; the temporary body-to-`main` architecture-link gap authorizes no readiness. The change remains backlog until its tracked planning artifacts are merged and read back from exact `main`, Luna's objective integration is accepted, the hosted graph and authoritative map agree, and the packet is semantically reconciled.

## Non-Goals

- No word counts, keyword or jargon rules, semantic score, LLM CI gate, proof ledger, per-body task IDs, or duplicate checklist.
- No fabricated reproduction evidence, forced raw intake fields where inapplicable, or broad blocker edges from #500 to product issues.
- No Rust crate, runtime, MCP, CLI, schema, migration, query, transaction, or SQLite change.
