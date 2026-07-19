## ADDED Requirements

### Requirement: Generated Honest Capability Registry

One versioned registry SHALL generate language identifiers, aliases, detection precedence, parser ownership, optional pack ownership, fixtures, support tiers, settings, and documentation inputs. Support SHALL be reported independently as detected, parsed, symbols, semantic, and benchmarked. Counts or aliases SHALL NOT hide a missing required capability.

#### Scenario: Parser exists without semantic resolution
- **WHEN** a language parses but lacks validated project-wide resolution
- **THEN** settings and documentation report only its achieved tier

#### Scenario: Registry entries conflict
- **WHEN** two entries claim the same detection precedence without an explicit rule
- **THEN** generation fails with both owning entries

### Requirement: Existing Parser Behavior Remains Compatible

Generated selection SHALL preserve current exact-filename, compound-extension, extension, content/dialect, and explicit-override behavior before new modes are enabled. Built-in parsers remain closed compile-time choices. Fallback parsing SHALL be identified honestly and SHALL NOT be presented as grammar-backed symbol support.

For every accepted embedded-language capability, extraction SHALL be bounded and SHALL reconcile embedded byte, line, and source spans back to the host file. The registry SHALL identify the host/embedded pairing and its natural fixtures; malformed or truncated embedded content SHALL return honest partial coverage rather than host-relative fabricated spans.

#### Scenario: Existing fixture is rescanned
- **WHEN** registry-driven selection replaces hand-maintained selection
- **THEN** every current fixture selects the same effective built-in parser and compatible output

#### Scenario: Component host contains an embedded language
- **WHEN** a supported template or component file contains an accepted embedded-language region
- **THEN** definitions and relationships use exact reusable host-file spans and bounded partial coverage is reported when reconciliation is incomplete

### Requirement: Accepted Language Capability Cannot Shrink Silently

The versioned registry SHALL contain an explicit accepted capability-set manifest. Each accepted row SHALL declare required membership and tier, natural positive/negative fixtures, provenance and license inputs, and required-platform applicability. Generated runtime tables, settings, validation, and documentation SHALL derive from those rows, including their counts. Removing or weakening an accepted row SHALL require an explicit compatibility decision and capability-set version change; validation SHALL fail when generated output omits or understates a still-accepted row. Product Rust and tests SHALL NOT duplicate mutable accepted membership or totals as literals.

#### Scenario: A generator drops an accepted language row
- **WHEN** generated parser tables, settings, fixtures, or documentation omit a still-accepted capability or advertise a lower tier
- **THEN** validation fails with the owning capability row instead of accepting a smaller advertised set

### Requirement: Advertised Semantic Families Are Independently Validated

Project-wide resolution SHALL use small language-owned providers over normalized registries and candidates. Resolved, ambiguous, unresolved, and external outcomes SHALL remain distinct. Every advertised language/relation family SHALL have non-vacuous positive, negative, malformed, duplicate-name, and ambiguity fixtures; aggregate success SHALL NOT mask one failing family.

#### Scenario: Static source cannot choose one target
- **WHEN** several scoped candidates remain valid
- **THEN** the provider abstains with bounded candidates rather than choosing the first name match

### Requirement: Compiler Metadata Is Bounded Non-Executable Input

Translation-unit providers MAY consume typed working-directory, include-root, dialect/target, forced-include, and opaque define identity data when required for correct resolution. ProjectAtlas SHALL parse that metadata as data, SHALL NOT execute compilers, shells, response-file commands, builds, or repository code, and SHALL NOT persist, log, serialize, snapshot, benchmark, or return secret-bearing raw define values.

#### Scenario: Metadata requires execution or contains a secret
- **WHEN** a compiler entry requires command execution, unsafe indirection, an out-of-scope path, or a secret-bearing raw value
- **THEN** ProjectAtlas returns a typed partial/unsupported outcome, uses only safe opaque invalidation identity, and emits no secret value

### Requirement: Optional Parser Breadth Does Not Burden Core

Broad parser capability SHALL be installed, verified, enabled, updated, rolled back, disabled, and removed explicitly. Normal core scan/query SHALL not download, compile, link, initialize, or execute an absent optional pack. Pack work SHALL be bounded and isolated from the long-lived service.

#### Scenario: No optional parser pack is installed
- **WHEN** normal ProjectAtlas scan and navigation run
- **THEN** existing built-in language support remains fully functional without network or pack-runtime cost

### Requirement: Installed Parser Packs Are Contained And Non-Executable

Every installed parser pack SHALL bind pinned provenance, digest, license, ABI/runtime compatibility, and accepted capability rows. Normal pack use SHALL be offline and SHALL run through a supervised out-of-process boundary or a capability-denied WASM/native boundary that cannot execute repository code, shell commands, builds, compilers, or network requests. Hard time, process-tree memory, output, and cancellation limits SHALL apply. Pack crash, timeout, invalid output, or containment failure SHALL leave the MCP process responsive and the active structural generation unchanged.

#### Scenario: Optional parser exceeds its boundary
- **WHEN** a pack crashes, hangs, exceeds a resource limit, requests a forbidden capability, or emits invalid output
- **THEN** the pack operation fails with bounded diagnostics while normal built-in navigation and the active generation remain available
