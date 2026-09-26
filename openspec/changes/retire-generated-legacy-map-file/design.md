## Context

The explicit `projectatlas map` and `atlas_map` routes call one `write_map` owner: it builds a current snapshot, writes `.projectatlas/projectatlas.toon`, and optionally writes adjacent JSON. Normal graph/navigation uses SQLite and does not read that export. Separate legacy-purpose import can still read an existing map, and `.projectatlas/projectatlas-nonsource-files.toon` is an authored input. The map command/tool and TOON response format must remain.

## Goals / Non-Goals

**Goals:** Return the selected root's current map directly through CLI and MCP, without generating a TOON snapshot at the configured `map_path`; preserve explicit JSON sidecar export, read-only legacy-purpose import, and supported root isolation.

**Non-Goals:** Remove map navigation, TOON, SQLite, the non-source TOON input, or user-owned historical snapshots; silently change unrelated scan/publication behavior; stable promotion.

## Decisions

### Reuse the existing snapshot and renderers

Replace only the TOON disk-writer boundary with a renderer returning map content. CLI emits it on stdout; MCP includes it in the map response. Keep the existing snapshot/purpose computation rather than creating another map model. `--json`/`json: true` retain the explicit adjacent JSON export and also select JSON response content. `--force`/`force: true` retain their CI bypass for that JSON write, but cannot cause a TOON file write. Retain the global CLI format contract where it can be reconciled without duplicate serialization.

### Preserve legacy input, never touch existing output

Keep `map_path` as the configured read-only import location for old purpose snapshots, including its publication fingerprint. Do not remove or overwrite a pre-existing TOON file at the default or a configured alternate path. Stop all writes of that snapshot, but leave the separate explicit JSON sidecar behavior and non-source input unchanged. The MCP response reports TOON `written: false` and JSON sidecar state separately during RC3.

### Keep the map an explicit full-snapshot operation

The route already walks the selected source tree. Preserve its existing admission and project-root rules, and enforce a finite response-size limit before emitting an inline map, with typed refusal rather than partial data or a fabricated complete result. Routine bounded navigation remains the session-brief/search/slice path.

## Risks / Trade-offs

- [Old consumers expect a TOON file after `map --force`] → Keep command/flags and explicit JSON sidecar behavior, but document the TOON response migration and verify no TOON write at default and alternate `map_path` in installed CLI/MCP tests.
- [An inline map floods an agent context] → Finite response ceiling and explicit oversized refusal; normal navigation remains bounded.
- [Legacy purpose metadata is lost] → Retain read-only import and fingerprint tests for a pre-existing snapshot.
- [Wrong-root MCP request leaks another project's map] → Exercise selected-root and missing-index paths without implicit initialization or mutation.

## Migration Plan

Update docs/skill and callers to consume CLI stdout or MCP response. Existing TOON files at the default or configured alternate `map_path` remain untouched and may still be imported; users may remove their own old file after reviewing its purpose content. No database migration. Rollback to RC2 restores the explicit TOON file-writing behavior, so RC3 release notes must identify that compatibility difference.

## Dependencies / Cross-Issue Impact

#627 is independent of the plugin and document fixes and is a direct child/blocker of RC3 release owner #492. The release owner verifies the installed map contract after #627 merges. No new dependency on #620, #624, or #625 is required.

## Open Questions

None.
