## Context

The current startup path is correct but verbose: agents call settings/root, overview, folders, files, and sometimes health before they know where to inspect source. `atlas_session_brief` should be a small orchestration layer over existing indexed metadata, not a second source of truth.

## Contract

`atlas_session_brief` accepts:

- `project_path`: optional root override for this call only.
- `query`: optional task text for ranking folders and files.
- `folder_limit`: optional, clamped to a small maximum.
- `file_limit`: optional, clamped to a small maximum.
- `blocker_limit`: optional, clamped to a small maximum.

The response includes:

- `project`: root, DB path, config path, and active/missing-index status.
- `policy`: path scope and nearest-project startup policy visible enough for startup decisions.
- `overview`: file/folder/purpose counts when the index exists.
- `index`: `available` or `missing`, plus scan recommendation when missing.
- `folders`: bounded ranked folder candidates from existing ranking helpers.
- `files`: bounded ranked file candidates from existing ranking helpers.
- `blockers`: bounded unresolved health/purpose blockers from existing health query paths.
- `recommendations`: typed next calls such as `atlas_scan`, `atlas_folders`, `atlas_files`, `atlas_file_summary`, `atlas_health`, or `filesystem_tools`.
- `limits`: requested/effective counts and truncation flags.

## Implementation Notes

- Use `state_for_project_path` to resolve the selected project without mutating active state.
- If the DB does not exist, return a typed missing-index payload without `open_atlas_store` and without creating `.projectatlas`.
- If the DB exists, open it read-only through `open_store`.
- Use `store.overview()`, `ranked_folder_nodes_with_reasons`, `ranked_file_nodes_with_reasons`, and bounded `unresolved_health_findings_page`.
- Do not record token telemetry from this tool.
- Encode with the existing `encode_named_payload` helper.

## Edge Cases

- Empty query: still return bounded default candidates and generic recommendations.
- Missing index: do not create directories/files; recommend scan or explicit project selection.
- Health-heavy project: cap blocker rows and expose truncation.
- Stale or wrong project config: reuse existing selected-project validation paths.
- Absolute paths outside selected project: recommend normal filesystem tools unless the selected indexed project is explicitly changed.

## Pre-Mortem

Risk: the brief duplicates `atlas_next`.
Mitigation: `atlas_next` remains a query-specific navigation report; the brief adds project/index/policy/blocker context and typed startup recommendations.

Risk: test fixtures pass with empty health because no blocker is created.
Mitigation: include a fixture with a missing purpose or unresolved health finding and assert a blocker row.

Risk: payload shape is too verbose.
Mitigation: keep field names stable and compact, and do not include source text.
