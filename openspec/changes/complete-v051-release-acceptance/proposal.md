## Why

Large repositories need the complete indexing and refresh performance work before 0.6.0. Version 0.5.1 needs one release owner to verify those improvements in the installed product, alongside the existing CLI, MCP, plugin, upgrade and recovery behavior.

## What Changes

- Establish #715 as the feature-free release owner for `v0.5.1-00`. Every accepted implementation issue has #715 as its sole direct parent and blocks release acceptance.
- Admit the complete #708 performance work through its updated 0.5.1 specification: initial indexing, full reindexing, one-shot refresh, unchanged projects, large change sets and bounded work budgets.
- Freeze accepted main only after every implementation issue and required review closes. Verify the complete installed command/tool inventory and measured large-project improvements on all supported platforms.
- Use the existing package and release workflow for an RC, remediation and later stable promotion. Verify assets, checksums, installed identities, preserved state and GitHub Latest independently; close the release owner last.

## Capabilities

### New Capabilities

- `v051-release-acceptance`: exact installed-product and performance acceptance, publication and final closure for 0.5.1.

### Modified Capabilities

None. Implementation behavior belongs to its accepted child specification.

## Impact

Release planning, native milestone hierarchy and blockers, the existing release workflow, complete installed CLI/MCP/host acceptance, and the performance measurements supplied by #708. No product code, version bump, dependency or schema change belongs to this planning change.

## Release Scope

Planning only. Establish the release owner first; #708 enters this graph when its full 0.5.1 scope and task mirror are synchronized. The initial graph contains only #715 and cannot authorize publication of an unimplemented performance fix.

## Non-Goals

No indexing implementation in #715, timeout-only substitute, new release pipeline, database reset, or inclusion of the 0.6.0 language, Memory Atlas or npm-distribution work. Scheduling does not publish a candidate or stable release.
