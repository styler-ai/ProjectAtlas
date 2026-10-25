## Context

See proposal.md for scope. Stable 0.5.0 is published and verified. The user has moved the complete large-repository performance topic forward from 0.6.0 to 0.5.1, including initial indexing and full reindexing. #708 owns implementation; #715 owns integrated release acceptance.

## Goals / Non-Goals

Use existing release and installed-product gates to verify the entire accepted product and the measured improvements. Keep performance algorithms, benchmark construction and their causal tests in #708. This planning change creates no product code, version bump, new workflow, schema or dependency.

## Decisions

### Keep implementation and acceptance separate

The existing `v0.5.1-00` milestone has #715 as its sole release owner. #708 is its required direct child and blocker for the complete indexing/refresh performance work. Remove #708 from #493's 0.6.0 child/blocker set while preserving every other accepted child and task owner. The 0.6.0 features consume the accepted indexing baseline later.

Additional confirmed fixes need their own accepted owners. #715 implements no product change and cannot start candidate acceptance until the complete #708 work and every other accepted child are implemented, reviewed and closed.

### Reuse the existing candidate and stable pipeline

Freeze accepted main after all children close. Use the existing clean parser handoff, native package and installer jobs, complete CLI/MCP inventory, real host readers and supported predecessor upgrade tests. Match the exact candidate revision, artifacts, runtime, plugin, skill, hook, host configuration, project and database identity throughout. Use isolated fixtures for mutations, administration and failure injection.

Run the existing RC-first process with independently verified tag/head, non-draft metadata, assets and SHA256SUMS. RCs preserve the preceding stable as Latest. Stable promotion repeats proof and verifies Latest and supported downstream pins. Preserve exact published tags; later documentation closeout commits do not move them. Scheduling this work grants no immediate publication authority.

### Consume measured performance without weakening correctness

#708 preregisters numerical latency/resource targets and supplies representative large-project fixtures and work counts. Release acceptance runs those cases against the optimized installed candidate with normal telemetry: initial/full indexing, cold CLI, warm MCP, unchanged projects, mixed sparse/dense changes and required exact reconciliation. Report distributions and work separately for each path. A warm result cannot conceal slow fresh CLI or full reindexing.

The integrated scenario performs an initial index, approves purposes, applies mixed changes, refreshes through CLI/MCP, verifies current source and affected graph results, exercises unchanged refresh and explicit full reindexing, injects cancellation/failure and concurrent changes, and retries safely. Include offline/same-metadata edits, lost events, ignore/parser/compiler changes, worktree isolation and a plain non-Git project. Preserve typed uncertainty and compatible rollback; do not reset user databases for acceptance.

## Risks / Trade-offs

- Partial optimization could be called complete: require separate targets and correctness results for every indexing path before freeze.
- A larger default budget could mask expensive work: inspect visited/read/hashed/parsed/written work alongside elapsed time and explicit-limit behavior.
- Release-only changes could bypass owning review: return confirmed defects to an implementation issue and restart affected proof after its fix lands.
- Planning could be mistaken for a release: leave all future acceptance/publication tasks unchecked and retain 0.5.0 identities until implementation and explicit publication work occur.

## Migration Plan

The plan changes only release tracking. Preserve 0.5.0 artifacts, native project databases and authored state. Admit #708 to 0.5.1 after its full plan is synchronized, then complete its implementation and review before candidate acceptance. Any required runtime/storage migration must be specified and proved by its implementation owner before this release consumes it. Close #715 only after successful stable readback; close the milestone afterward.

## Architecture Disposition

N/A: release and runtime ownership are unchanged. Reuse the existing pipeline; #708 owns the indexing architecture and its reviewed diagrams. This release owner composes installed proof rather than introducing another system.

## Dependencies / Cross-Issue Impact

#708 is the required implementation owner, with no new dependency on 0.6.0 features. #708 moves from #493 to #715 as a direct release blocker. #715 does not start candidate acceptance until that complete work and every later accepted child are closed. Preserve the separate 0.6.0 language, agent-continuity and distribution plans.

## Open Questions

None.
