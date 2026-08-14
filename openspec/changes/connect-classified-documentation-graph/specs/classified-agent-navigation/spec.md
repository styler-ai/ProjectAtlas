## ADDED Requirements

### Requirement: Every file-bearing agent result exposes classification
Files, search matches, summaries, purpose queues/reviews/sets, symbols, detailed relations, graph analysis, settings/capability reports, and exact next calls SHALL expose the content classification of every returned file endpoint through typed Rust structures and equivalent CLI JSON/TOON and MCP schemas.

#### Scenario: Mixed file results remain distinguishable
- **WHEN** an omitted-selection query returns source, documentation, configuration/data, other text, or opaque metadata
- **THEN** each row carries its stable classification without inferring from extension, purpose, or summary prose

#### Scenario: Relation endpoints retain both roles
- **WHEN** a document relation connects different content classifications
- **THEN** the relation row exposes both source and target classifications plus direction/view and exact selectors

#### Scenario: Purpose mutation cannot spoof classification
- **WHEN** an agent sets or reviews a purpose
- **THEN** the request has no classification override and the response projects the database-derived role

#### Scenario: JSON, TOON, CLI, and MCP agree
- **WHEN** equivalent requests use supported adapters and formats
- **THEN** classification names, selection defaults, validation, bounds, completeness, and next-call fields are semantically identical

### Requirement: Selection is shared before ranking and traversal expansion
One service-owned content predicate SHALL apply explicit selection before candidate ranking/truncation and before graph anchor/frontier expansion. Explicit `documents`/inbound `documented_by` navigation MAY cross classification to return the selected relation endpoint, but unrelated frontier expansion MUST continue to honor selection.

#### Scenario: Filtering occurs before limit
- **WHEN** many stronger out-of-selection candidates precede matching candidates and a small limit is requested
- **THEN** ProjectAtlas returns the strongest in-selection candidates rather than filtering a truncated mixed page

#### Scenario: Explicit document relation crosses from documentation to source
- **WHEN** selection is `documentation`, the anchor is documentation, and outbound `documents` is explicitly requested
- **THEN** the source target is returned as the relation endpoint with its source classification, but it is not expanded as a documentation frontier

#### Scenario: Inbound document view crosses from source to documentation
- **WHEN** selection is `source`, the anchor is source, and inbound `documents` is explicitly requested
- **THEN** the documentation endpoint is returned under the `documented_by` view without admitting unrelated documentation expansion

#### Scenario: Legacy unfiltered relations exclude new family by default
- **WHEN** an existing relation/analysis caller omits both relation family and content selection
- **THEN** the result preserves the previous relation-family universe and order; `documents` participates only when explicitly requested or a content selection opts into classified traversal

### Requirement: Classified navigation preserves trust and exact next steps
Classification SHALL NOT replace parser provenance, purpose authority, freshness, coverage, confidence, resolution, completeness, or truncation. Agent-facing next calls MUST remain bounded and address the exact selected project, file/heading, relation direction, content selection, and generation needed to continue from documentation to current source evidence.

#### Scenario: Documentation is guidance rather than source truth
- **WHEN** an agent follows `documents` from a specification
- **THEN** the response directs the agent to current source summary/symbol/slice evidence and does not claim the documentation proves runtime behavior

#### Scenario: Stale or partial parse remains visible
- **WHEN** a document summary, parser, relation closure, or index generation is stale, partial, failed, or truncated
- **THEN** the result retains classification but also reports the existing trust/completeness state and a typed refresh or narrower next action

#### Scenario: Exact heading next call is bounded
- **WHEN** a relation resolves to a document heading
- **THEN** the next call includes the exact repository-relative document and disambiguated heading selector/range under existing output ceilings

#### Scenario: Unsupported filter is side-effect free
- **WHEN** adapter validation rejects a selection
- **THEN** it performs no scan, migration, refresh, purpose write, or graph mutation

### Requirement: Existing source workflows remain compatible
When callers omit content selection and do not request document relations, existing source-language detection, symbols, imports/calls/dependencies, rankings, search counters, purpose states, graph results, pagination/cursors, and output bounds MUST remain compatible.

#### Scenario: Frozen source-only fixtures remain exact
- **WHEN** the pre-#440 compatibility corpus runs without new request fields
- **THEN** candidate identities/order, legacy relation rows, counters, pagination, and source next calls match the frozen baseline apart from additive classification fields

#### Scenario: Configuration and other text remain discoverable by default
- **WHEN** an existing unfiltered search targets text already indexed from configuration/data or other-text files
- **THEN** it continues to find that text with unchanged match semantics and an additive non-source classification

#### Scenario: Existing cursors cannot change meaning silently
- **WHEN** classification/selection participates in a paginated request
- **THEN** cursor identity includes the behavior-relevant selection and rejects reuse under a different selection

### Requirement: Shipped guidance teaches the classified funnel
The version-matched ProjectAtlas skill and user documentation SHALL teach agents to choose source, documentation, or both; use classified files/search before exact summaries; follow explicit `documents`/`documented_by`; inspect trust and unresolved reasons; bind shared-host calls with `project_path`; and end at exact current source slices before making implementation claims.

#### Scenario: Ordinary task needs source only
- **WHEN** an agent is implementing code with no documentation question
- **THEN** guidance recommends source selection without forcing document traversal

#### Scenario: Specification task crosses both directions
- **WHEN** an agent starts from documentation or needs owning docs from source
- **THEN** guidance shows the explicit relation and exact next-call sequence without broad repository reads

#### Scenario: Worktree task binds exact root
- **WHEN** several linked worktrees are available through one host
- **THEN** guidance requires per-call `project_path` and warns that each checkout has its own writable classified graph
