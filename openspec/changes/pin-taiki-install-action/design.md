## Context

`ci.yml` and `release.yml` each install `cargo-deny` through `taiki-e/install-action`. Both currently use the old immutable `v2.86.6` commit. The live Dependabot branch carries the same two-line update to the `v2.87.5` commit, and the action tag has been independently resolved to that commit. This is a workflow maintenance boundary with no Rust, database, installer, or runtime change.

## Goals / Non-Goals

**Goals:**

- Keep both existing action uses pinned to the exact reviewed `v2.87.5` commit.
- Preserve the `cargo-deny@0.20.2` tool, conditions, job order, permissions, and workflow behavior.
- Make the exact two-use source diff and affected hosted checks the proof boundary.

**Non-Goals:**

- No workflow redesign, action configuration change, or Dependabot policy change.
- No runtime, CLI, MCP, database, installer, dependency, or release-owner implementation.
- No new test framework or generated evidence ledger.

## Decisions

- Update only the two existing `uses: taiki-e/install-action@...` values to `5bf6ce016fd2e72eefc647cbca1e4213f65955b8`, the immutable commit for `v2.87.5`. A tag-only reference would weaken the repository's existing pinning contract.
- Leave the adjacent `cargo-deny@0.20.2` values and all surrounding YAML unchanged. A workflow parser or new abstraction would add no proof for this two-line maintenance change; exact source comparison and the existing affected CI jobs are sufficient.
- Keep architecture diagrams unchanged. `N/A: workflow pin maintenance does not change product or runtime architecture.`

## Risks / Trade-offs

- One workflow use could retain the old SHA → search both named files and require exactly two new `5bf6ce0...` uses with no `6cd1350...` use.
- The resolved action could differ from the claimed release → verify the upstream `v2.87.5` tag resolves to the exact commit before acceptance.
- An incidental workflow edit could alter behavior → review the workflow diff as exactly two SHA changes; the owning OpenSpec and issue-map metadata are the only additional files.

## Migration Plan

Apply the two-line pin update on the existing Dependabot PR branch, add the owning issue/OpenSpec mapping, run the normal repository IssueOps and affected CI gates, obtain one independent review, and let the repository owner perform the protected merge. Rollback is a reviewed revert of the two pin lines if the existing checks expose an incompatibility; no data or runtime migration is involved.

## Dependencies / Cross-Issue Impact

This issue is one direct child of release owner #492 in milestone v0.5.0-00 and has no implementation blocker. It touches only the existing Dependabot PR #538; #560 metadata-event routing, #566, #465, and #358 are outside this boundary. The release owner remains blocked by this child until its accepted head is merged.

## Open Questions

None.
