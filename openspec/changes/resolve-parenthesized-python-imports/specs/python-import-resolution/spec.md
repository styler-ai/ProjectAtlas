## ADDED Requirements

### Requirement: Parenthesized Python named imports retain static semantics

The existing Python provider SHALL normalize a single flat parenthesized from-import list equivalently to its ordinary named-import form, including multiline layout, one trailing comma and optional per-name aliases. It SHALL preserve module-relative resolution and conservative rejection of malformed, nested, wildcard or comment-bearing lists or otherwise unsupported syntax without guessing targets or executing Python.

#### Scenario: Parenthesized names and aliases resolve
- **WHEN** an indexed Python file imports local declarations using a flat parenthesized list, with single-line or multiline layout and optional trailing comma or aliases
- **THEN** detailed import relations resolve to the indexed local module and supported calls through imported names or aliases resolve to their declarations
- **AND** the equivalent ordinary form retains its behavior

#### Scenario: Unsupported input cannot fabricate references
- **WHEN** an import list is empty, unbalanced, nested, contains empty items, invalid names or aliases, or unsupported wildcard syntax
- **THEN** semantic normalization abstains rather than inventing a local target

### Requirement: Existing semantic projections are repaired safely

The implementation SHALL advance the existing semantic resolution contract and use existing fingerprint invalidation and publication ownership to repair unchanged compatible indexes. It SHALL preserve project identity and approved purposes without a schema change or index reset. Required one-time semantic upgrade work SHALL be distinguished from steady-state open performance.

#### Scenario: Refresh upgrades an unchanged prior-contract index
- **WHEN** a compatible index created under the previous semantic contract contains an unresolved valid parenthesized Python import and current sources are unchanged
- **THEN** ordinary refresh publishes corrected import and supported call relations while preserving project identity and approved purposes
- **AND** a subsequent unchanged refresh does not republish, and incremental results agree with full rebuilding

#### Scenario: Existing route and isolation behavior remains
- **WHEN** CLI or MCP requests relations from an exact selected checkout
- **THEN** existing output, missing-state, mutation, root-isolation and relation-bound contracts remain unchanged by the syntax fix
