## Context

Issue #308 and its feature proof are closed, #340 and #341 are merged and closed, and the first #311 reconciliation reached `dev`. Fresh Codex review then found five blockers: `02-Release` did not publish the supported optional-parser archives, federated rendezvous discovery reapplied trust filters without preserving the primary anchor and direction, the published MCP composition evaluation named a stale raw-input digest, rendezvous database reads lacked the service-owned deadline, and exact federation byte accounting allocated avoidable duplicate encodings. Later review found that published benchmark identities predated release-affecting runtime, packaged-skill, MCP, relation-service, and repository-graph changes. Only proof affected by those changes must be reopened.

The existing release path already has the required mechanics:

- `01-CI` owns Rust quality and source-built CLI/MCP E2E smoke on Linux, Windows, macOS x64, and macOS arm64.
- `optional-parser-pack` owns explicit cache-free Linux and Windows optional-pack construction, full runtime proof, and the clean all-platform handoff consumed by release.
- `02-Release` owns version validation, package construction, installer smoke, release assets, and publication; `prepublish_only=true` exercises the package, optional-pack handoff, and installer path without publishing.
- `03-Auto-Release` dispatches `02-Release` after an eligible version reaches `main` and supplies a successful clean optional-pack run whose behavior-relevant release inputs still match.
- `02-Release` requires every issue in milestone `v0.4.0-00` to be mapped, checked, and closed before publication.

The user also requires branch, worktree, and external ProjectAtlas checkout cleanup only after v0.4.0 is published and independently verified. Release readiness and post-release cleanup therefore need separate owners even though both remain part of the same active release goal.

## Goals / Non-Goals

**Goals:**

- Make #311 a concise, mapped, mechanically synchronized readiness owner.
- Prove one clean final `dev` candidate through the existing local and hosted release surfaces.
- Fix and prove every fresh actionable Codex finding before candidate reconciliation.
- Publish both supported optional-parser archives only after binding them to one clean all-platform run and unchanged behavior-relevant release inputs.
- Keep federated rendezvous evidence inside the primary anchor/direction result without changing SQLite schema or query ownership.
- Carry the same caller-or-service deadline into rendezvous SQLite reads and measure retained federation state without an additional encoded buffer.
- Retain published system-scale and agent-navigation campaigns when their behavior-relevant inputs and measured artifact identities remain valid, and rerun only affected campaigns.
- Preserve cumulative token-impact history when a released v0.3.26 database is upgraded to v0.4.0 and through later compatible migrations.
- Keep the token-impact TUI focused on reconciled persisted impact data, including the observed and modeled file-read split plus broad-folder-walk and candidate-open activity/token-impact charts; keep comparison evidence in structured CLI/MCP output even when an explicit benchmark artifact is supplied; and use only real resolved SQLite relations for its bounded connected, clustered, depth-cued static wide-layout atlas.
- Keep `main`, tags, and GitHub releases untouched until all prepublication evidence is green.
- Prepare a merge-commit `dev`-to-`main` promotion that can pass the existing milestone gate.
- Preserve a durable post-release owner for independent publication verification and safe workspace consolidation.

**Non-Goals:**

- Add a crate, dependency, SQLite schema, migration, write path, identity-specific database query, CLI/MCP schema, second graph authority, or second release path.
- Copy or link the GPL `graf-rs` application.
- Create a second release workflow, test framework, evidence ledger, or task-specific receipt scheme.
- Reopen completed #308 work, absorb #314 into v0.4.0, or claim hosted success from local tests.
- Delete any branch, worktree, or checkout before publication verification and unique-work inventory.

## Decisions

### Extend the existing release workflows

The change keeps `01-CI`, `optional-parser-pack`, `02-Release`, and `03-Auto-Release` as the only release path. `optional-parser-pack` emits one clean release handoff only for an explicit all-platform clean construction. `02-Release` requires the referenced successful run, verifies the same repository/workflow/event and unchanged behavior-relevant inputs, then validates the aggregate proof, clean receipts, archive sizes, and archive digests before staging versioned release assets. `03-Auto-Release` selects the newest successful, unexpired, input-compatible handoff and passes its run identifier.

A new release driver was rejected because the current workflows already own the real package, installer, platform, and publication boundaries. Duplicating them would create drift without increasing proof.

### Scope rendezvous to the primary traversal

The primary project remains the only anchor and direction authority. Its already bounded detailed traversal yields the exact typed external identities eligible for cross-root rendezvous. Secondary projects may contribute evidence only for that bounded identity set. Analysis retains the same derived set from its existing detailed traversal so it does not repeat the database read. An empty set returns immediately.

The existing indexed relation-family read remains the storage boundary. A bounded ordered set membership check fixes the correctness defect in `O(primary rows log primary rows + bounded secondary rows log primary rows)` time and bounded memory. A new SQLite query shape or index was rejected because this finding does not establish a schema or plan deficit; that change would require separate query-plan and representative-scale evidence.

The existing relation request control remains the cancellation and deadline owner. Rendezvous derives one control whose deadline is the earlier of the caller deadline and the federation service deadline, then passes it through every family read so the existing SQLite progress handler can stop late work. Exact serialized-equivalent byte counts reuse the service's streaming counter instead of allocating a second JSON buffer. This preserves query shape, indexes, read-only snapshot ownership, transaction behavior, output compatibility, and `O(serialized bytes)` counting work while removing the duplicate `O(serialized bytes)` allocation.

### Bind published benchmark metadata to its raw input

Both the human-readable and machine-readable MCP composition evaluations name the SHA-256 of the same raw JSON input. The existing release gate computes and compares that digest directly; no benchmark receipt framework or task-specific identifier is added.

### Bind published campaigns to measured behavior and artifacts

The system-scale and agent-navigation preregistrations bind the behavior they measure plus their runtime, skill, tool-inventory, source-state, platform, and environment identities. Commit SHAs remain provenance only. Benchmark lock, raw-result, evaluation, landing-copy, and finite checklist commits do not invalidate a publication by themselves.

Any later change to an owning behavior or measured artifact identity invalidates the affected publication and requires a fresh lock and rerun. Unrelated behavior or metadata changes reuse the passed publication. Reusing the existing harnesses and publication formats was selected over a new benchmark framework.

### Separate provenance from proof invalidation

First, lock one clean release candidate after #341 is closed and every v0.4.0 implementation and readiness artifact is merged. Run the complete local gates, ordinary CI, explicit clean optional-parser construction when its inputs changed, prepublish release packaging, and review disposition.

Once those tasks are true, update the #311 OpenSpec and mirrored GitHub task state. Rerun cheap strict OpenSpec, IssueOps, ProjectAtlas low lint, review, topology, and release-policy checks. The shared classifier carries the clean optional-parser handoff across commit-only or behavior-neutral metadata changes and fails closed on source, dependency, toolchain, workflow, packaging, configuration, parser-pack, or unknown changes. Other proof owners apply the same relevant-input rule.

Promote only when `main` is an ancestor of the candidate, and use a merge commit rather than squash or rebase. The resulting `main` commit must have the candidate as a parent and the same Git tree. The actual `02-Release` run still repeats verification, packaging, and installer smoke before publication.

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

### Preserve telemetry and keep the dashboard truthful

The existing append-only SQLite migration owner remains responsible for compatible upgrades. A released v0.3.26 schema fixture is opened through the normal v0.4.0 path, and the test compares cumulative overview and trend results before migration, after migration, and after reopen. A malformed-row case must roll back atomically. No replacement database, reset, export/import detour, schema addition, or new telemetry authority is introduced unless this compatibility proof exposes a concrete defect.

The TUI consumes the same `TokenOverview` used by structured output, so all numeric fields remain derived rather than independently recalculated or fabricated. It never renders benchmark comparison content, even when a benchmark artifact populates the typed report for CLI JSON/TOON or MCP consumers. The navigation section preserves the report's file-read total and observed/modeled split. It reuses the exact reconciled source rows to show broad-folder-walk and selected-candidate activity shares against persisted source steps plus token-impact shares against reconciled tokens avoided; the source table remains the exact accounting ledger. Wide terminals additionally seed from bounded resolved non-containment relation-family reads, expand through two rounds of the existing indexed inbound and outbound adjacency API, retain one connected component under 48-node, 64-edge, and per-node degree ceilings, and fail closed when the optional graph read is unavailable. The renderer stores only those sampled visual nodes and edges, labels the result as a bounded live snapshot, and hides it at narrower widths. Unresolved or unavailable graph data is shown as empty or unavailable rather than replaced with demo content.

One small in-file deterministic Fruchterman-Reingold layout settles the already bounded projection from graph-derived starting positions. Its quadratic pairwise force work is capped at 48 nodes and 120 iterations; rendering remains a Ratatui `Canvas` concern with graph-distance draw order, cluster colors, and muted edges. The published MIT `fdg-sim` crate was evaluated first, but its only release introduces an old duplicate `hashlink` line rejected by the repository dependency policy. Keeping the standard bounded calculation locally is smaller than weakening that gate or importing the GPL `graf-rs` interactive application.

A new graph query, schema, index, dependency, write path, interactive state model, second graph authority, or full graph explorer was rejected. The existing indexed relation readers own SQLite access, the token TUI owns only the bounded in-memory one-shot layout, and Ratatui owns drawing. The separate interactive explorer remains post-v0.4.0 issue #357.

### Preserve the promotion rollback boundary

Until readiness is complete, `main`, tags, and releases remain untouched. If any candidate gate fails, fix the owning branch and rerun affected proof. If the release workflow fails after `main` promotion, do not clean worktrees or delete branches; diagnose and retry the owning release path while the post-release issue remains open.

## Risks / Trade-offs

- **Checklist reconciliation advances the commit after expensive proof** → Rerun cheap current-state gates and reuse only proof whose behavior-relevant inputs remain unchanged.
- **Protected `dev` landing changes provenance** → Preserve merge topology and candidate content without treating the new commit SHA as changed behavior.
- **Promotion changes content or hides it behind squash/rebase** → Require `main` ancestry, a merge commit with the candidate as a parent, identical Git trees, and actual release verification.
- **A green older run is mistaken for applicable evidence** → Validate each proof owner's relevant inputs, platform, toolchain, workflow contract, status, and immutable artifacts; unknown changes fail closed.
- **Supported optional-parser archives exist only as expiring workflow artifacts** → Require one explicit clean handoff, verify it against unchanged release inputs, and stage both supported archives plus aggregate proof as versioned release assets.
- **Secondary federation rows escape the requested anchor or direction** → Derive the eligible typed external identities from the bounded primary traversal and reject every other secondary row.
- **A rendezvous family query runs past the service deadline** → Bind every database call to the existing caller-or-service request control.
- **Exact federation accounting doubles retained memory while measuring it** → Stream serialized-equivalent bytes into the existing counter without retaining another encoding.
- **Published MCP composition metadata names stale input** → Compute its raw input SHA-256 and compare both published representations in the release gate.
- **Published system-scale or agent-navigation results measure changed behavior** → Bind both campaigns to their owning behavior plus measured runtime, skill, tool, and platform identities; invalidate and rerun only the affected campaign.
- **A compatible upgrade resets cumulative token history** → Compare released v0.3.26 overview and trend totals before migration, after migration, and after reopen; require atomic rollback on invalid predecessor rows.
- **The dashboard shows plausible but non-authoritative values** → Render numeric fields from the persisted report object, assert arithmetic and labels against it, and show unavailable state instead of fallback values.
- **The miniature atlas implies complete graph analysis or consumes unbounded state** → Read only resolved rows through existing indexed bounded queries, cap nodes and edges, label the static preview, and keep the full explorer in #357.
- **The miniature layout regresses into a clipped or unstable drawing** → Keep deterministic starting positions, force steps, and bounds covered by focused tests plus real wide, narrow, dark, light, and terminal-background review.
- **The milestone gate becomes circular around post-release cleanup** → Keep #311 prepublication-only and create the non-milestone post-release owner before closure.
- **A prepublish run is mistaken for publication** → Require `prepublish_only=true`, verify no tag or release was created, and leave publication to the existing main-triggered workflow.
- **Installer or plugin state passes in source but fails when packaged** → Require real package/installer smoke and installed CLI/MCP behavior from the release workflow.
- **Cleanup removes unique or dirty work** → Inventory dirtiness, unique commits, PR ownership, and merge/supersession status before each removal; retain uncertain lanes.

## Migration Plan

1. Land the bounded installer trust fix and reconcile all live review feedback.
2. Add this change to `openspec/issue-map.json` and replace #311's obsolete body with the exact local checklist.
3. Complete and close #341 after its Linux and Windows empty-cache proof and land its reconciled checklist state.
4. Land the supported optional-parser release handoff, anchored and deadline-bound federation filtering with allocation-free byte accounting, benchmark digest correction, focused tests, specs, and architecture diagram.
5. Prove released-schema telemetry preservation and finish the truthful token-impact TUI plus its bounded live graph preview and visual review.
6. Merge all remaining readiness artifacts into `dev`, retain or rerun each benchmark according to its behavior-relevant inputs and measured artifact identities, then lock the corrected release candidate.
7. Run the complete local gates, `01-CI`, clean optional-parser proof when its inputs changed, and `02-Release` with `prepublish_only=true` and an input-compatible clean handoff.
8. Reconcile #311 and mirror the GitHub checklist, then rerun cheap current-state gates.
9. Close #311 after strict OpenSpec, IssueOps, ProjectAtlas low lint, review checks, applicable CI, and the reusable or refreshed clean optional-parser handoff pass; then pass milestone IssueOps.
10. Promote with a merge commit after verifying `main` ancestry and identical promotion/main trees; let the existing release workflow consume the input-compatible handoff, verify the resulting `main` tree, and publish v0.4.0.
11. Independently verify the published release and only then perform the post-release cleanup.

Rollback before promotion is ordinary correction followed by a new release-content lock and reconciliation. After promotion, retain all source lanes and release artifacts until the publishing failure is understood; never use workspace cleanup as rollback.

## Open Questions

None.
