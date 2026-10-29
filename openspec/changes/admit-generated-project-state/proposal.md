# Proposal

## Why

Installed 0.5.0 initialization can create default configuration, nonsource metadata,
a database and host files before a later existing policy rejects its generated-state
destination. A failed init must not write outside its automatic project-state owner.
This confirmed self-hosting/security-triage finding is required for RC1 as #741.

## What Changes

- Admit automatic generated-state destinations before initialization writes, using
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

- `project-state-ownership`: destination admission and safe automatic state creation
  for initialization and generated project host files.

### Modified Capabilities

None. The canonical capability inventory contains only token-impact-estimate-reporting.
This adds no source semantic provider, index schema or CLI/MCP wire field.

## Impact

Existing CLI init and generated MCP-config writers, shared runtime bootstrap,
atlas_map default/nonsource creation, MCP registered-worktree initialization and
hydration activation, existing filesystem/project-binding helpers, native init tests,
installed selector inventory and #715 release acceptance. No dependency is selected
in this plan; prefer existing concrete helpers and standard atomic file operations.

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
