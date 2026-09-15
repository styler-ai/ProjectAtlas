## Context

The existing release matrix binds every packaged test to an exact installed candidate. Its CLI case table owns top-level commands and its MCP case table owns the live tool inventory. Nested CLI help coverage and synthetic predecessor schemas leave two distinct acceptance gaps in the same installed-product boundary.

## Goals / Non-Goals

Prove complete installed CLI route execution and real v0.4.5-to-candidate continuity on all four supported native tuples. Reuse the current runner, installers, artifact servers, snapshots, and assertions. Do not add runtime behavior, dependencies, an alternative test framework, npm delivery, or automatic stable promotion.

## Decisions

- Extend the existing CLI behavior owner with a closed nested-route case set and compare it to the current frozen fixture's subcommands and actions. Each case executes the installed runtime with bounded output and causal filesystem/SQLite assertions. Unsupported parser-pack operations must execute and return the documented typed platform refusal without changing state; supported operations use local verified fixtures or test their specific invalid-input refusal. Help output cannot satisfy a behavior case.
- Keep the dynamic MCP behavior table unchanged as the authority for every advertised tool. Reuse the existing root, missing-index, mutation, and canary checks instead of copying the tool inventory.
- Make the published predecessor executable and checksum explicit workflow inputs to one environment-bound contract in the existing delivery/lifecycle runner. The release workflow obtains the platform-matched v0.4.5 asset and published checksum; no ambient PATH runtime or synthetic database substitutes for it.
- Exercise the released executable and installer in an isolated project and host profile. Create source evidence, authored purpose, a real telemetry event, and worktree registration through public routes. Capture a consistent closed/backup snapshot and read-only state before upgrading that same project.
- Reuse the local release-asset server to inject an invalid candidate checksum, prove atomic refusal, then retry the verified exact candidate. Compare identity, root/database selection, authored purposes, telemetry, worktrees, generation, and source evidence across the supported migration. Verify generated MCP/host bindings select the candidate. Where parser provenance changes invalidate derived evidence, require explicit typed refresh and successful rebuild while preserving authored authority; never present stale derived evidence as current.
- Preserve the predecessor runtime and valid predecessor state for recovery. An incompatible old runtime must refuse the upgraded database without changing bytes or sidecars; rollback is only accepted with a compatible retained state and must not discard later authored changes. This does not claim that a schema downgrade is supported.
- Run the added contract in the existing Unix and Windows prepublish jobs, so it is a publication prerequisite on every tuple. Network downloads happen before the bounded test, while candidate update faults use the existing loopback fixture. Missing predecessor identity fails the required workflow instead of becoming a skip.

## Risks / Trade-offs

- Ambient runtime or host state could create false proof: pass explicit executable, plugin, database, and isolated profile paths and preserve unrelated canaries.
- A help-only case or wrong nested dispatch could pass: reconcile case coverage with the frozen route inventory and assert state/output for the actual route.
- Schema-specific snapshots could reject legitimate migration or hide loss: compare authored authority and stable identity separately from derived freshness, and retain a complete predecessor backup for refusal/recovery assertions.
- Network failure could masquerade as compatibility failure: verify predecessor download and checksum before invoking the contract; never fall back to Cargo or synthetic schema setup.

## Migration Plan

This is acceptance hardening. No user database migration is introduced. Any product defect exposed by the real predecessor must be fixed in its owning boundary and revalidated before this issue can close. Final release acceptance restarts from the resulting exact main revision.

## Architecture

The [installed release acceptance flow](../../../docs/v050-release-architecture.md#installed-release-acceptance) owns the artifact, fixture, update, and publication-gate boundary.

## Dependencies / Cross-Issue Impact

Issue #593 is a direct child and blocker of #492. It reuses the accepted packaged test and installer boundaries already on main; no open implementation prerequisite remains. The final candidate freeze and clean parser/prepublish sequence restart after this proof repair merges.

## Open Questions

None.
