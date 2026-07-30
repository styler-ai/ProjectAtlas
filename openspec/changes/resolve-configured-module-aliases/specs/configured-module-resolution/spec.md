## ADDED Requirements

### Requirement: Applicable ECMAScript configuration resolves repository modules
ProjectAtlas SHALL load bounded, validated direct `compilerOptions.baseUrl` and `compilerOptions.paths` values from the nearest applicable `tsconfig.json` or `jsconfig.json`. JavaScript, JSX, TypeScript, TSX, and embedded Vue imports SHALL use the shared ECMAScript semantic resolver rather than parser-local alias handling.

#### Scenario: TypeScript and Vue use tsconfig aliases
- **WHEN** a containing `tsconfig.json` maps an alias to an indexed TypeScript module and TypeScript, TSX, or Vue source imports and calls an exported declaration through that alias
- **THEN** file-level import and exact-symbol call relations resolve to the same targets as the equivalent relative import

#### Scenario: JavaScript uses jsconfig aliases
- **WHEN** a containing `jsconfig.json` maps an alias to an indexed JavaScript or JSX module
- **THEN** file-level and exact-symbol inbound and outbound traversal resolve the configured alias

#### Scenario: Extension and package entry inference
- **WHEN** a mapped target is referenced with or without its supported source extension or resolves to an `index` module
- **THEN** existing source export aliases select the indexed file and declaration without inventing a second resolution model

#### Scenario: No applicable configuration
- **WHEN** a repository has no applicable compiler configuration
- **THEN** relative ECMAScript imports and all Python, Rust, and Cargo provider behavior remain compatible and an unconfigured alias remains typed unresolved

### Requirement: Configuration precedence and outcomes remain deterministic
ProjectAtlas SHALL choose the nearest containing configuration and the most-specific matching `paths` pattern, normalize targets within the repository, sort and deduplicate emitted candidate scopes, and preserve existing typed resolution outcomes.

#### Scenario: Nested configuration overrides a parent
- **WHEN** parent and nested configuration files could map the same module specifier for a nested caller
- **THEN** only the nearest applicable configuration supplies candidates

#### Scenario: Multiple configured targets exist
- **WHEN** the selected mapping expands to more than one distinct indexed target
- **THEN** the relation is typed ambiguous and no arbitrary first target is reported as resolved

#### Scenario: Configured target is absent
- **WHEN** the selected mapping produces no indexed target
- **THEN** the relation is typed unresolved and impact or dead-code analysis does not claim a resolved caller

#### Scenario: Configuration is malformed or escapes the repository
- **WHEN** an applicable configuration cannot be parsed, exceeds a bound, or contains an absolute or repository-escaping target
- **THEN** derivation fails before publication and the last complete graph generation remains readable

### Requirement: Configuration freshness invalidates derived graph state
ProjectAtlas SHALL treat creation, edits, renames, and removal of `tsconfig.json` and `jsconfig.json` at any repository depth as route-affecting graph inputs requiring a complete derived refresh.

#### Scenario: Alias mapping changes
- **WHEN** a mapping changes from one indexed target to another
- **THEN** the next refresh removes the old file and symbol edges and publishes only the new resolved targets in one generation

#### Scenario: Alias configuration is removed
- **WHEN** the applicable configuration is deleted
- **THEN** the next refresh removes configuration-derived edges and retains the same imports as typed unresolved unless another applicable configuration resolves them

#### Scenario: Configuration changes during publication
- **WHEN** configuration bytes differ from the scanned node or change before final source revalidation
- **THEN** publication fails without partial rows or generation advancement

### Requirement: Alias resolution preserves bounded adapter-equivalent analysis
Configured resolution SHALL preserve row, input-byte, retained-memory, deadline, cancellation, and output limits. CLI and MCP graph traversal SHALL read the same generation-bound graph and return equivalent resolved, ambiguous, or unresolved states for the same project and selector.

#### Scenario: File and symbol views agree
- **WHEN** a configured import calls one exported declaration
- **THEN** file inbound, exact-symbol inbound, outbound, impact, and conservative dead-code views use the same resolved edge

#### Scenario: CLI and MCP query the same alias edge
- **WHEN** CLI and MCP relation calls address the same project database and exact selector
- **THEN** both adapters return the same target identity, resolution state, generation, and exact total

#### Scenario: Configuration fan-out exceeds a bound
- **WHEN** admitted configuration count, bytes, mappings, targets, or per-fact canonical keys exceed the owning limit
- **THEN** the operation returns a typed resource failure, observes cancellation and deadline checks, and does not publish partial graph state
