## ADDED Requirements

### Requirement: Qualified symbol parents always satisfy the graph identity contract
The system SHALL convert every valid parser-admitted containment chain into a deterministic `GraphIdentityText` parent no larger than the shared graph identity byte ceiling.

#### Scenario: Readable chain fits
- **WHEN** a qualified `parent::child` containment identity fits the byte ceiling
- **THEN** projection retains the exact readable qualification

#### Scenario: Exact byte boundary
- **WHEN** a qualified parent is exactly at the byte ceiling
- **THEN** projection admits it without compaction or rejection

#### Scenario: Qualification exceeds the byte ceiling
- **WHEN** valid nested components compose beyond the byte ceiling
- **THEN** projection uses a bounded deterministic derived scope identity and preserves the symbol

### Requirement: Deep scope identity remains stable and distinct
The system SHALL keep deeply nested symbols under different ancestor chains distinct without using mutable source positions.

#### Scenario: Equal suffixes under different deep ancestors
- **WHEN** two overbound containment chains end in the same local parent and symbol names but have different ancestors
- **THEN** their derived parent identities and canonical symbol keys remain distinct

#### Scenario: Literal source identity resembles a compact scope
- **WHEN** a parser or directly constructed symbol graph supplies a raw name or parent in the reserved compact-scope namespace
- **THEN** source admission omits it or the shared projection boundary rejects it before it can collide with a derived identity

#### Scenario: Repeated projection
- **WHEN** the same deep symbol graph is projected in full, incremental, and clean rebuild modes
- **THEN** it produces the same bounded parent identities and canonical graph keys

### Requirement: Raw symbol admission remains strict
The system MUST NOT use derived qualification compaction to admit an empty, malformed, control-bearing, or overbound raw symbol name or immediate parent.

#### Scenario: Invalid raw sibling
- **WHEN** one invalid raw declaration appears beside valid shallow and deep declarations
- **THEN** the invalid declaration is omitted according to symbol admission policy while valid siblings remain publishable

### Requirement: Deep nesting cannot abort repository publication
The system SHALL apply bounded parent qualification before entity construction for every source language and publication mode.

#### Scenario: Language-neutral nested graph
- **WHEN** any tree-sitter parser emits enough valid nested declarations to exceed the composed parent ceiling
- **THEN** the scan publishes a complete generation and the nested symbols remain queryable

#### Scenario: Incremental deep nesting change
- **WHEN** a current repository adds or removes an overbound nested scope and runs one incremental refresh
- **THEN** publication remains atomic, unrelated graph facts are retained, and a subsequent clean scan converges to the same graph
