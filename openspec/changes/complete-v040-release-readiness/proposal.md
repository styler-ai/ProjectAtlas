## Why

ProjectAtlas v0.4.0 has completed its feature program, but review of the promotion candidate found release blockers after the first readiness reconciliation: supported optional-parser archives were not published, federated rendezvous evidence could escape the anchored traversal, a published benchmark digest was stale, rendezvous database reads lacked the service deadline, and exact byte accounting allocated avoidable duplicate encodings. Later review also found that published benchmark identities predated release-affecting runtime, packaged-skill, MCP, relation-service, and repository-graph changes. Release issue #311 must remain the truthful owner while those findings are fixed, affected proof is refreshed, and publication verification plus workspace cleanup remain with the deliberately post-release owner.

## What Changes

- Replace #311's stale initiative checklist with a mapped release-readiness checklist whose state can be verified by IssueOps.
- Make milestone IssueOps reject open issues before publication.
- Land the remaining installer trust-boundary correction and close every other v0.4.0 milestone issue against affected local, hosted, and review evidence.
- Prove one clean release candidate, carry passed proof across commit-only or behavior-neutral metadata changes, and rerun only gates whose behavior-relevant inputs changed.
- Run `02-Release` in `prepublish_only` mode so the real package and installer paths are proven without creating a tag or GitHub release.
- Carry only an explicit clean, all-platform optional-parser handoff into `02-Release`, bind its run, candidate tree, aggregate proof, clean receipts, archive sizes, and archive digests, and publish both supported pack archives with the release.
- Restrict cross-root rendezvous evidence to exact external identities reached by the primary anchored traversal in the requested direction.
- Bind every rendezvous database read to the earlier caller or service deadline and count serialized-equivalent federation state without retaining another encoded copy.
- Correct the MCP composition evaluation's raw-input digest in both published representations and verify the binding directly.
- Relock and rerun the system-scale and agent-navigation publications whenever later runtime, packaged-skill, MCP, relation-service, or repository-graph behavior invalidates their recorded candidate identities.
- Preserve cumulative token-impact history through the released v0.3.26-to-v0.4.0 database upgrade and every later compatible migration.
- Keep the human token-impact TUI truthful and focused on persisted impact data, with a bounded connected and clustered static preview of real resolved repository-graph relations in wide terminals.
- Prepare the `dev`-to-`main` promotion while keeping `main` and publication untouched until readiness is complete.
- Create a separate non-milestone post-release issue that owns published-release verification and the user-requested safe branch, worktree, and external ProjectAtlas checkout cleanup.

## Capabilities

### New Capabilities

- `v040-release-readiness`: Define the candidate, prepublication, milestone-reconciliation, promotion, and post-release-handoff requirements for ProjectAtlas v0.4.0.

### Modified Capabilities

- `cross-repository-intelligence`: Require federated rendezvous evidence to remain inside the primary anchored and directed traversal, the request deadline, and the aggregate intermediate-state ceiling.
- `language-intelligence-registry`: Require supported optional-parser archives to ship only through an exact clean-run release handoff.
- `repository-intelligence-benchmarks`: Require published evaluation digests to match their named raw input and published system-scale and agent-navigation results to measure the final functional release candidate.
- `token-telemetry`: Preserve cumulative usage history across compatible upgrades and require every human-dashboard value to derive from persisted telemetry or be explicitly unavailable.
- `token-tui-dashboard`: Remove release/control comparisons and add a bounded connected, clustered, non-interactive wide-layout atlas drawn only from resolved relations in the active project database.

## Impact

This change affects OpenSpec release artifacts, GitHub issue #311, federated service filtering, deadline propagation, byte accounting and focused tests, the MCP composition evaluation metadata, the system-scale and agent-navigation preregistrations and publications, compatible telemetry-migration verification, the human token dashboard, the optional-parser architecture diagram, the final `dev` candidate proof, and the existing `optional-parser-pack`, `02-Release`, and `03-Auto-Release` workflows. It adds no crate, dependency, SQLite schema or migration unless the released-schema preservation test exposes a real compatibility defect, SQLite write path, CLI/MCP schema, or second release workflow.

## Non-Goals

- Do not add another release workflow, installer architecture, generic evidence framework, runtime feature, crate, dependency, graph application, database migration, or MCP tool.
- Do not copy or link GPL `graf-rs` code.
- Do not add an identity-specific database query or index without measured evidence that the existing bounded family reads plus anchored identity filtering are insufficient.
- Do not weaken the milestone checklist gate or mark publication, verification, or cleanup complete before it happens.
- Do not reopen completed #308 feature work or move #314 into v0.4.0.
- Do not delete branches, worktrees, or external ProjectAtlas checkouts before the release is published, independently verified, and their unique work is inventoried.
- Do not invent dashboard numbers, reset cumulative telemetry during upgrade, or present sampled graph decoration as complete graph analysis.

This change is ready for implementation as the v0.4.0 release-readiness owner.
