## ADDED Requirements

### Requirement: Document files own exact static documentation relations
ProjectAtlas SHALL publish every admitted Markdown documentation candidate as one canonical `documents` relation whose source is the owning document file and whose target resolution is derived only from exact indexed repository evidence.

#### Scenario: File anchor exposes a heading-contained source reference
- **WHEN** a fully scanned document contains an admitted repository-relative source reference beneath a Markdown heading
- **THEN** an outbound file-anchored `documents` query returns the canonical relation to the exact source file, symbol, or heading target

#### Scenario: Repeated references deduplicate without losing evidence
- **WHEN** several headings in one document contain admitted references to the same canonical target
- **THEN** ProjectAtlas stores one logical document-to-target relation and retains the bounded exact occurrence spans for every admitted reference

#### Scenario: Long prose creates no speculative edges
- **WHEN** a long document discusses source concepts without a parser destination or complete repository-path code span
- **THEN** ProjectAtlas creates no `documents` relation from that prose, name similarity, or topic proximity

#### Scenario: In-document heading target stays navigable
- **WHEN** an admitted fragment resolves to an exact heading in the same or another indexed document
- **THEN** the canonical document-file source resolves to that heading entity without a self-edge or guessed fragment winner

### Requirement: Inbound documentation navigation is one canonical view
ProjectAtlas SHALL expose `documented_by` only as the inbound view of the stored outbound `documents` fact.

#### Scenario: Source discovers its governing document
- **WHEN** an exact source entity is the resolved target of a canonical document relation
- **THEN** an inbound `documents` query returns the owning document as `documented_by` without storing a duplicate inverse relation

#### Scenario: Direction and occurrence provenance agree
- **WHEN** the same canonical relation is queried outbound from the document and inbound from the target
- **THEN** both views retain the same relation key, resolution, confidence, completeness, and source occurrence evidence

### Requirement: Zero-candidate document coverage is explicit and trusted
ProjectAtlas SHALL expose `no_candidates` coverage when supported document extraction completes successfully and admits no static target candidate. The durable representation MAY normalize this state to the existing trusted complete-zero invariant when reads reconstruct the public state deterministically.

#### Scenario: Document has no static target
- **WHEN** complete Markdown extraction admits zero documentation candidates for an indexed document
- **THEN** its `documents` coverage state is `no_candidates` with zero covered, zero omitted, no failure reason, no reached limit, and trusted health semantics

#### Scenario: Admitted target cannot resolve
- **WHEN** extraction admits a static candidate whose exact target is missing, ignored, outside the root, case-conflicting, ambiguous, or unsupported
- **THEN** ProjectAtlas emits a privacy-safe typed unresolved relation and does not report `no_candidates`

#### Scenario: Extraction is incomplete
- **WHEN** a declared byte, count, evidence, cancellation, or parser boundary prevents complete document extraction
- **THEN** ProjectAtlas reports the applicable partial, failed, ignored, oversized, quarantined, or stale coverage instead of `no_candidates`

### Requirement: Coverage remains durable through publication transitions
ProjectAtlas SHALL retain schema-18 compatibility and publish relation plus coverage changes atomically with the complete graph generation.

#### Scenario: Existing RC1 database is reopened
- **WHEN** a valid schema-18 database is opened by the RC2 runtime
- **THEN** no coverage-table migration is required
- **AND** the older projection-contract fingerprint requires one typed full refresh before stale heading-owned graph rows can be served as current
- **AND** after refresh trusted complete-zero `documents` rows reconstruct as `no_candidates` through write, read, reopen, and filtered discovery
- **AND** non-document complete-zero rows and positive complete rows remain `complete`

#### Scenario: Publication fails
- **WHEN** constraint validation, cancellation, busy handling, or graph publication fails
- **THEN** no partial generation becomes active and the previously complete database remains recoverable

#### Scenario: Full and incremental scans converge
- **WHEN** document references change through add, edit, rename, delete, case, fragment, or ignore transitions
- **THEN** incremental publication produces the same canonical relations, unresolved reasons, occurrences, and coverage as a clean full scan

### Requirement: Agent adapters expose bounded file-scoped graph truth
CLI and MCP relation tools SHALL return equivalent file-anchored documentation results, coverage, exact next calls, limits, and errors without implicit mutation or root fallback.

#### Scenario: CLI and MCP full-scan regression
- **WHEN** a real fixture is fully scanned and queried through CLI and MCP from its exact initialized root
- **THEN** both adapters return equivalent resolved and unresolved document relations plus explicit no-candidate coverage

#### Scenario: Wrong root or missing index
- **WHEN** a request addresses a different root or a root without an initialized compatible index
- **THEN** ProjectAtlas returns the existing typed routing or missing-index error and does not read, initialize, migrate, or mutate another project's database implicitly

#### Scenario: Bounded high-reference document
- **WHEN** a document contains more admitted references or occurrences than one request may return
- **THEN** indexed pagination, continuation identity, cancellation, occurrence ceilings, and output limits remain deterministic and disclose truncation without dropping the canonical edge set silently

#### Scenario: Mandatory packaged release proof
- **WHEN** the RC2 candidate runs in hosted mandatory CI
- **THEN** one packaged full scan and generated MCP startup prove file-outbound, source-inbound, unresolved, and no-candidate behavior before release publication
