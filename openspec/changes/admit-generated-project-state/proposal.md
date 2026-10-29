# Proposal

## Why

Installed 0.5.0 can create automatic state outside its intended owner during both
initialization and ordinary database-backed navigation. SQLite read-only opens may
initialize WAL shared memory before root metadata is checked; bootstrap can write
default configuration and host files before a later destination refusal.
This confirmed self-hosting/security-triage finding is required for RC1 as #741.

## What Changes

- Admit conventional generated databases and WAL sidecars before every SQLite open,
  including root discovery and read-only navigation; admit bootstrap outputs before writes, using
  existing bootstrap/filesystem owners and the selected canonical project root.
- Cover ordinary CLI/MCP init, registered-worktree hydration, root bind/move/detach and generated host files;
  preserve identity, existing authored configuration/purposes and valid reinitialization.
- Distinguish automatic project outputs from intentionally selected external database,
  configuration and source-root paths. Preserve those supported operator capabilities.
- Prevent a generated file write from following an unsafe pre-existing leaf alias;
  preserve valid files and the last valid state on refusal or failed publication.
- Trace analogous producer/write/admission paths and prove positive compatibility,
  causal failures and real installed CLI/MCP behavior on supported platforms.

## Capabilities

### New Capabilities

- `project-state-ownership`: admission of automatic database access and safe project-state
  creation for initialization and generated host files.

### Modified Capabilities

None. The canonical capability inventory contains only token-impact-estimate-reporting.
This adds no source semantic provider, index schema or CLI/MCP wire field.

## Impact

Existing CLI/MCP database selection and root discovery, normal read/write store
wrappers and early metadata reads, CLI init and generated MCP-config writers, shared runtime bootstrap,
atlas_map default/nonsource creation, MCP registered-worktree initialization and
hydration activation, existing filesystem/project-binding helpers, native init tests,
installed selector inventory and #715 release acceptance. Reuse existing same-file/tempfile helpers and the already locked canonical
winapi-util as a direct Windows-only dependency for stable hard-link counts; add
no transitive package, unsafe binding or schema change.

## Release Scope

High-priority #741 is required for v0.5.1-rc1, directly under and blocking #715 in
v0.5.1-00. It starts from accepted main3ef603a and has no direct implementation blocker;
#738/#739/#731 are independent owners and must be combined before release proof.
Ready for implementation only after independent plan review, strict OpenSpec,
scoped IssueOps and the synchronized native milestone/parent/blocker graph.

## Non-Goals

No blanket denial of explicit external config/database paths, selected-root aliases,
Git metadata or normal indexing/plugins; no schema reset, new path framework,
scanner-driven speculative restrictions or stable publication. Do not claim a
universal defense against a hostile process racing every ancestor in the filesystem.
