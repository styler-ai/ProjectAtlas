## ADDED Requirements

### Requirement: Map routes return a current map without a legacy file write
`projectatlas map` and MCP `atlas_map` SHALL remain available. They SHALL render the selected project's current map as a CLI/MCP response in TOON or JSON, and MUST NOT create, overwrite, or delete a TOON snapshot at the default or a configured alternate `map_path`. Explicit `--json`/`json: true` SHALL retain its existing adjacent JSON sidecar export behavior. An oversized response MUST fail explicitly before emission, not claim a complete partial map.

#### Scenario: CLI map from a project without a legacy snapshot
- **WHEN** a user runs `projectatlas map` or `projectatlas map --json` in a supported project
- **THEN** stdout contains the selected format's map data, the TOON snapshot is not created, and the JSON sidecar is written only when explicitly requested

#### Scenario: Existing legacy snapshot
- **WHEN** a user runs the map command while a legacy snapshot already exists
- **THEN** the current map is returned and the existing TOON file bytes remain unchanged, including at a configured alternate `map_path`

#### Scenario: MCP map and compatibility flags
- **WHEN** `atlas_map` is called with the selected root and optional `json` or `force` compatibility flags
- **THEN** the response reports the correct map format/content and no TOON file write; `json` retains its explicit JSON sidecar behavior and `force` only bypasses the existing CI skip policy for that sidecar

#### Scenario: Response exceeds a finite limit
- **WHEN** the full map cannot fit the supported response budget
- **THEN** CLI/MCP return an explicit typed limit without a partial map or file mutation

### Requirement: Legacy inputs and project isolation remain intact
ProjectAtlas SHALL continue to read an existing legacy TOON map at the configured `map_path` for purpose import without writing it. It MUST retain `.projectatlas/projectatlas-nonsource-files.toon` as a distinct input and SHALL honor exact selected-root isolation for MCP map calls.

#### Scenario: Legacy purpose import
- **WHEN** a pre-existing map contains valid purpose rows and the selected project is scanned
- **THEN** those rows remain eligible for the existing read-only import path and the map file bytes do not change

#### Scenario: Wrong root or missing index
- **WHEN** an MCP map request selects the wrong root or a root without a published index
- **THEN** it reports that selected root's exact state without borrowing another root's database or implicitly initializing/mutating project state

#### Scenario: Non-source input remains separate
- **WHEN** the project has `.projectatlas/projectatlas-nonsource-files.toon`
- **THEN** map rendering and purpose import retain that input's existing semantics and never treat it as the retired output file
