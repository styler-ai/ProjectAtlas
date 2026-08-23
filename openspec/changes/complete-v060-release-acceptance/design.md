## Context

#310 and #314 are the initial accepted v0.6 implementation issues. #310 is the foundation; #314 is directly blocked by #310 because its public adapters must target the accepted installed surface. #493 is the release-acceptance root: it is the native parent of both children, is directly blocked by both, and gains any later accepted milestone issue as another direct child and blocker. Hierarchy expresses scope/progress; dependency edges express execution order.

The v0.6 release boundary is a packaged command-line product with CLI, MCP, generated host, plugin/skill, SQLite, installer, and hosted release contracts. Unit tests and schema/help inventories cannot prove their integration.

## Goals / Non-Goals

**Goals:**

- Maintain one visible native release hierarchy and an acyclic direct-blocker graph.
- Freeze and build one exact revision/artifact set only after all child issues and reviews are accepted.
- Safely execute every supported CLI command/nested command and MCP tool, including unchanged routes.
- Prove one installed workflow across #310 and #314 positive, negative, failure, recovery, compatibility, privacy, concurrency, pressure, and platform boundaries.
- Independently read back prerelease and stable hosted truth and close #493 last.

**Non-goals:**

- Feature, bug, migration, or compatibility implementation.
- Duplicate owning tests or a second release/test framework.
- Unisolated mutation, direct stable publication, or carrying a confirmed defect as release-owned remediation.

## Decisions

### Native hierarchy and dependency graph have different jobs

#493 is the sole native parent for every other accepted `v0.6.0-00` issue. Each child has exactly one parent and #493 has none. Direct `blocked by` edges remain the execution authority: #310 has no blocker, #314 is blocked by #310, and #493 is blocked by both. No duplicated children list is stored in OpenSpec; `release_issue` plus the graph issue keys imply the hierarchy.

### The release owner is feature-free

#493 may add or maintain acceptance harness/workflow proof, manifests, release metadata, and architecture documentation, but it SHALL NOT fix a product defect. A confirmed failure is returned to its owning sanitized IssueOps/OpenSpec issue, reviewed there, merged to `main`, and the complete candidate proof restarts on the new exact input.

### Public-surface regression executes behavior

The candidate manifest is derived from the installed runtime and accepted specs, then reconciled with CLI help/nested commands, MCP discovery schemas, aliases, generated hosts, formats, and compatibility fixtures. Every executable route is invoked safely. Mutating and administrative routes use disposable fixtures; deliberate removals must match #310. Root/worktree selection, freshness, JSON/TOON, stdout/stderr, exit/error schemas, source evidence, task status/cancellation, concurrency, and compatibility are all observable.

### Holistic proof uses the installed product

The E2E begins from isolated homes/config/cache/repository state and exact candidate artifacts. It covers installation or upgrade, init, scan, purpose-led navigation, graph/source evidence, #310 routing, #314 typed storage, conditional atomic writes, bounded recovery after resume/compaction, documented host-hook or manual fallback, pressure/conflict/privacy, failure recovery, uninstall, and compatible rollback. It never substitutes checkout binaries or a developer database.

### Prerelease and stable truth are independently read back

`v0.6.0-rc1` is a non-draft prerelease and stable v0.5.0 remains Latest. Readback independently verifies tag/revision, metadata, assets, checksums, installers, runtime/plugin/skill/MCP identity, and acceptance results. Any changed revision or artifact invalidates prior proof. Stable v0.6.0 repeats installed and hosted verification before Latest/downstream/milestone finalization.

## Risks / Trade-offs

- Inventory drift could hide an unchanged route. Mitigation: derive and reconcile independent CLI, MCP, host, compatibility, and accepted-spec inventories, then execute behavior.
- A release harness could become a feature worktree. Mitigation: return defects to owners and permit only acceptance/release proof in #493.
- Mutating routes could damage ambient state. Mitigation: isolated homes, roots, databases, registries, caches, hosts, exact cleanup, and typed refusal.
- Hosted workflow success could be mistaken for product truth. Mitigation: independent installed and hosted readback at RC and stable boundaries.

## Migration Plan

1. Land and accept #310.
2. Refresh/rebase #314 onto the accepted #310 baseline, then land and accept #314.
3. Add any later accepted v0.6 issue as a direct #493 child/blocker and recompute the graph.
4. Freeze one exact revision after every child/review is complete; run gates, full public-surface execution, and holistic installed E2E.
5. Publish/read back RC1 only with explicit authorization. Return blockers to owners and restart on any changed input.
6. Repeat proof for stable, verify Latest/downstream/milestone state, and close #493 last.

Rollback leaves stable v0.5.0 as Latest, retains failed candidates as prereleases, and refuses incompatible runtime/database/plugin/host state without mutation.

## Dependencies / Cross-Issue Impact

- Foundation wave: #310 has no direct blocker.
- Authored-context wave: #314 is blocked by #310 and lands its SQLite boundary before service/adapters.
- Release-acceptance wave: #493 is blocked by #310, #314, and every future accepted v0.6 child; it has no parent and closes last.
- #493 creates no SQLite schema. It consumes migration, query-plan, concurrency, backup/restore, privacy, and compatibility proof owned by #314 and installed-surface proof owned by #310.

## Open Questions

None.
