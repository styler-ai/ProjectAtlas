## Context

Current agent startup usually calls `atlas_settings`, `atlas_overview`, `atlas_folders`, `atlas_files`, and sometimes `atlas_health` before reading source. The workflow is correct but verbose. The brief should be a read-only orchestration layer over existing ProjectAtlas services, not a second ranking or health implementation.

## Goals / Non-Goals

**Goals:**
- Provide a compact, deterministic startup payload for agent harnesses.
- Surface selected project identity, index availability, scan freshness signals, bounded blockers, query-relevant folders/files, and recommended next calls.
- Keep all output typed with serializable structs and enums.
- Preserve the existing atlas-first workflow by recommending lower-level calls rather than replacing them.

**Non-Goals:**
- Do not auto-scan or mutate the index from the brief call.
- Do not read arbitrary source contents.
- Do not remove or hide the existing MCP tools.
- Do not include human marketing prose in the contract.

## Decisions

- Compose existing services instead of duplicating logic. Folder/file recommendations should call the same ranking paths used by `atlas_folders` and `atlas_files`; blockers should use existing health query code.
- Use strict bounds. The default response should include only a small number of folders, files, blockers, and next calls with truncation metadata where applicable.
- Model recommendations as enum-backed records such as `scan`, `folders`, `files`, `summary`, `slice`, `health`, or `filesystem_tools`, with reason fields. This lets harnesses inspect the payload without parsing prose.
- Keep the tool project-isolated. `project_path` may select a project for the call, but the brief should not change active MCP state.

## Risks / Trade-offs

- Brief grows too large -> keep hard limits and include counts/truncation flags.
- Brief masks stale index state -> expose freshness/index state first and recommend scan when needed.
- Brief becomes a second implementation of ranking/health -> wire through existing service functions and add regression tests.
- Recommendations become hard to trust -> include reason codes and source signals for each recommendation.
