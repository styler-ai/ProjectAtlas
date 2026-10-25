## Purpose

Define reliable language-specific source navigation and analysis for coding agents, including legacy systems being understood and migrated to Rust.

## ADDED Requirements

### Requirement: Required language support is demonstrated by navigation
ProjectAtlas SHALL provide source recognition, useful declarations and exact spans, provable static relations, navigation, analysis, and incremental behavior for ABAP, COBOL, Fortran, Ruby, and SQL before accepting this 0.6.0 scope.

#### Scenario: A grammar loads without useful graph facts
- **WHEN** a required language parses but expected declarations, static relations, or navigation results are missing
- **THEN** its implementation remains incomplete rather than being marked fully supported

### Requirement: Source forms and dialect claims are explicit
ProjectAtlas SHALL recognize declared ordinary source forms, distinguish ambiguous extensions, preserve exact source locations, and document and test supported dialect and encoding boundaries.

#### Scenario: Legacy fixed and free forms
- **WHEN** representative COBOL or Fortran fixed/free source is indexed
- **THEN** supported forms yield correct declarations and source spans and unsupported forms return explicit coverage limitations

#### Scenario: SQL dialect syntax
- **WHEN** source uses a claimed SQL dialect
- **THEN** the published feature matrix and fixtures establish its parser, object-dependency, and navigation behavior

### Requirement: Static evidence remains honest under incomplete source
ProjectAtlas SHALL expose proven static calls, imports, references, and dependencies while retaining unresolved, ambiguous, malformed, dynamic, and incomplete evidence through summaries and analysis.

#### Scenario: A dynamic target cannot be resolved
- **WHEN** a required-language call depends on runtime information absent from the source
- **THEN** the result identifies that limitation and dead-code or impact analysis does not treat the missing target as proof of absence

### Requirement: Root and source state remain isolated
Language navigation SHALL use the exact selected project and its compatible published index, preserve current saved-source evidence, and refuse wrong-root, missing-index, and stale-selector requests without implicit setup or mutation.

#### Scenario: Wrong project or missing index
- **WHEN** a CLI or MCP language request addresses another project or an uninitialized root
- **THEN** it returns typed recovery guidance without borrowing a sibling index or creating project state

### Requirement: Graph analyses produce checked language results
For every required language, ProjectAtlas SHALL verify impact sets, dependency paths, cycles, and dead-code candidates against expected source facts in focused fixtures and pinned real projects. Each project SHALL retain revision, license, provenance, and expected-answer evidence.

#### Scenario: A known dependency graph is queried
- **WHEN** a fixture or pinned project contains known callers, a dependency path, and a cycle
- **THEN** bounded impact, path, and cycle results match the expected declarations and exact source references

#### Scenario: An unused declaration has complete eligible coverage
- **WHEN** an unreferenced declaration satisfies the language's documented entry-point and coverage rules
- **THEN** analysis returns the expected dead-code candidate with the evidence and limits needed for review

#### Scenario: Dynamic or unresolved references affect analysis
- **WHEN** possible incoming calls or dependencies cannot be resolved from the available source
- **THEN** analysis exposes that uncertainty and does not turn an incomplete impact/path/cycle result or dead-code candidate into a claim of proven absence

### Requirement: Incremental and failure behavior preserves accepted state
ProjectAtlas SHALL update affected language facts after source/configuration changes and renames/deletions, preserve authored state, and keep the last complete generation readable after cancellation or failure.

#### Scenario: A changed-language refresh is cancelled
- **WHEN** indexing is cancelled before a complete publication
- **THEN** no partial generation is visible and retry/reopen preserves identity, reviewed purposes, and valid prior navigation

### Requirement: Supported surfaces and platforms agree
Required language behavior SHALL pass through installed CLI and MCP on Windows, Linux, macOS Intel, and macOS Apple Silicon with bounded resources and representative positive, negative, malformed, recovery, and compatibility proof.

#### Scenario: One platform lacks a required intelligence route
- **WHEN** a required language works only through a pack unavailable on a supported native platform
- **THEN** the language remains incomplete until an intentional supported route passes that platform's navigation tests

### Requirement: Major-language and migration claims have evidence
ProjectAtlas SHALL audit the major existing languages, including Python, publish capability levels and limits from the runtime authority, and demonstrate a representative legacy-to-Rust planning workflow with exact source evidence and explicit unknowns.

#### Scenario: A migration boundary is selected
- **WHEN** an agent investigates a bounded legacy component for conversion to Rust
- **THEN** it can inspect entry points, declarations, connected callers and data/external boundaries supported by source facts, cite the original spans, and identify unresolved behavior and necessary preservation tests
