## ADDED Requirements

### Requirement: Connected MCP input schemas are self-contained

ProjectAtlas SHALL advertise every MCP tool input as a concrete self-contained JSON Schema that does not require the connected Codex bridge to resolve a local `$defs` or `$ref`.

#### Scenario: Complete input-schema audit

- **WHEN** a packaged ProjectAtlas server returns `tools/list`
- **THEN** every advertised input schema contains no local definition or reference
- **AND** nested request structs and enums remain concrete at their owning fields

#### Scenario: Missing project index

- **WHEN** a host lists MCP tools before the selected project index exists
- **THEN** ProjectAtlas returns the same concrete input schemas without creating or migrating the index

### Requirement: Purpose-review items remain discoverable and rejectable

The connected `atlas_purpose_review` schema SHALL expose each `items` element as an object with required string `path` and optional `purpose`, `confirm_existing`, `task`, `work_key`, and `state_token` fields.

#### Scenario: Missing required path

- **WHEN** a host validates `{"apply":false,"items":[{}]}` against the advertised schema
- **THEN** validation rejects the item for missing required `path` before runtime invocation

#### Scenario: Optional review fields

- **WHEN** a host inspects the concrete item object
- **THEN** `purpose`, `confirm_existing`, `task`, `work_key`, and `state_token` are discoverable
- **AND** none of those five fields is required

### Requirement: Runtime and adapter compatibility are preserved

ProjectAtlas SHALL retain typed Rust request models, Serde deserialization, MCP routing, CLI behavior, project-root isolation, and purpose-review admission checks independently of host-side schema validation.

#### Scenario: Runtime defense for invalid purpose review

- **WHEN** a raw MCP caller bypasses host validation and submits an item without `path`
- **THEN** typed runtime deserialization rejects the request
- **AND** no purpose metadata is changed

#### Scenario: Conditional-write validation

- **WHEN** a raw MCP caller submits stale or incomplete `task`, `work_key`, and `state_token` identity
- **THEN** the existing purpose-review admission behavior rejects the request without mutation

#### Scenario: Wrong root or missing index

- **WHEN** a connected or raw MCP request addresses a wrong root or a missing index
- **THEN** the existing typed error and no-implicit-mutation behavior is unchanged

### Requirement: Upstream ownership is explicit

ProjectAtlas SHALL describe general Codex support for standards-valid local JSON Schema references as an upstream host dependency while providing a narrow self-contained compatibility representation for its own affected tools.

#### Scenario: Local compatibility proof

- **WHEN** ProjectAtlas validates its packaged MCP contract
- **THEN** it reports reference-free schema evidence as local compatibility proof
- **AND** it does not claim that ProjectAtlas tests or fixes Codex's general schema renderer
