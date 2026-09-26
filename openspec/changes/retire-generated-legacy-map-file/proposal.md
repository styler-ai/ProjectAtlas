## Why

ProjectAtlas's map is a core live capability, but its explicit CLI/MCP map routes still generate a legacy `.projectatlas/projectatlas.toon` file. That snapshot duplicates the SQLite-backed current view and can go stale. Earlier removal of the committed-file requirement did not remove the generator. RC3 should make map access fileless without removing the map.

## What Changes

- `projectatlas map` and `atlas_map` remain available and return the current map in their response, without creating or updating `.projectatlas/projectatlas.toon`.
- Preserve TOON and JSON response formats, truthful output bounds, existing project-root isolation, and read-only import of pre-existing legacy files.
- Reconcile compatibility flags, config/report fields, tests, skill guidance, documentation, and installed-product proof with the new behavior.
- Do not remove or change `.projectatlas/projectatlas-nonsource-files.toon`, the separate authored input.

## Capabilities

### New Capabilities

- `fileless-map-response`: transient, bounded CLI/MCP map output with no legacy snapshot generation and safe legacy-input compatibility.

### Modified Capabilities

None.

## Impact

CLI map rendering and MCP map payloads, map configuration/report semantics, legacy purpose import, tests and docs. No database reset, new parser/dependency, removal of map/navigation, or stable promotion. This change is ready for issue-scoped implementation under #627 after its issue/OpenSpec mirror and RC3 release graph are synchronized.
