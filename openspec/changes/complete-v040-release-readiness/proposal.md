## Why

ProjectAtlas v0.4.0 has completed its feature program, but release issue #311 still mirrors an obsolete pre-closure split of #308 and cannot serve as a truthful release gate. The final candidate needs one concise readiness contract that reconciles the remaining milestone work, proves packages and installed behavior without publishing, and hands publication verification plus workspace cleanup to a deliberately post-release owner.

## What Changes

- Replace #311's stale initiative checklist with a mapped release-readiness checklist whose state can be verified by IssueOps.
- Make milestone IssueOps reject open issues before publication.
- Land the remaining installer trust-boundary correction and close every other v0.4.0 milestone issue against exact-head local, hosted, and review evidence.
- Prove one exact release-content head, carry that proof only across one verified task-state-only reconciliation commit, and require ordinary exact-head gates on the resulting promotion head.
- Run `02-Release` in `prepublish_only` mode so the real package and installer paths are proven without creating a tag or GitHub release.
- Prepare the exact `dev`-to-`main` promotion while keeping `main` and publication untouched until readiness is complete.
- Create a separate non-milestone post-release issue that owns published-release verification and the user-requested safe branch, worktree, and external ProjectAtlas checkout cleanup.

## Capabilities

### New Capabilities

- `v040-release-readiness`: Define the exact-candidate, prepublication, milestone-reconciliation, promotion, and post-release-handoff requirements for ProjectAtlas v0.4.0.

### Modified Capabilities

None.

## Impact

This change affects OpenSpec release artifacts, `openspec/issue-map.json`, GitHub issue #311, the final `dev` candidate proof, the existing `01-CI`, `optional-parser-pack`, `02-Release`, and `03-Auto-Release` workflows, and the later post-release cleanup issue. It changes no Rust API, crate boundary, SQLite schema, CLI/MCP contract, dependency, package format, or runtime behavior.

## Non-Goals

- Do not add another release workflow, installer architecture, evidence framework, runtime feature, crate, dependency, database migration, or MCP tool.
- Do not weaken the milestone checklist gate or mark publication, verification, or cleanup complete before it happens.
- Do not reopen completed #308 feature work or move #314 into v0.4.0.
- Do not delete branches, worktrees, or external ProjectAtlas checkouts before the release is published, independently verified, and their unique work is inventoried.

This change is ready for implementation as the v0.4.0 release-readiness owner.
