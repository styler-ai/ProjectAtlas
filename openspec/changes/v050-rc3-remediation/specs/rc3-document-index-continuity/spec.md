## ADDED Requirements

### Requirement: Bounded document failures are file-specific and truthful
DOCX/PDF extraction SHALL retain finite time, memory, fuel, input, and output bounds. Every syntactically valid rendered DOCX `w:sym` in the main body, headers, footers, footnotes, endnotes, or rendered nested text boxes and within those bounds SHALL retain its exact font/code, story-part, and occurrence identity in durable queryable evidence; a verified font mapping SHALL produce Unicode text, while an unknown mapping SHALL identify unresolved text coverage without discarding surrounding content. Package-reachable rendered story parts SHALL be examined through validated relationships or recorded as explicitly incomplete; absence of symbols in an unexamined part MUST NOT be inferred. Reaching a retained input, output, fact, or memory limit SHALL produce a typed outcome without claiming that unexamined symbols were retained. Cancellation and the shared indexing deadline SHALL remain generation-fatal. PDF fuel use SHALL be measured on valid and adversarial fixtures; a finite budget increase is allowed only when it demonstrably admits valid input under retained bounds. Remaining PDF fuel exhaustion SHALL identify its file and typed reason. The system MUST NOT guess missing text, silently omit it from a complete-coverage claim, or conflate PDF parser fuel with the per-file text-index byte cap.

#### Scenario: Rendered DOCX font-specific symbol
- **WHEN** an otherwise valid DOCX contains a rendered `w:sym` glyph in any supported story part whose font mapping is not known
- **THEN** the result preserves its exact font/code, story-part identity, and occurrence locator through publication and reopen, retains verifiable surrounding text, identifies unresolved Unicode-text coverage, and never advertises the glyph as decoded

#### Scenario: Rendered symbols outside the main body
- **WHEN** a DOCX references rendered headers, footers, footnotes, endnotes, or nested text boxes containing `w:sym`
- **THEN** their symbols are admitted with exact story-part provenance, or that document exposes explicit incomplete coverage rather than claiming full symbol/text coverage

#### Scenario: Verified symbol mapping
- **WHEN** a rendered `w:sym` has a verified mapping for its font and code
- **THEN** its Unicode text and exact symbol provenance are indexed without an unsupported-symbol error

#### Scenario: Symbol extraction reaches a retained limit
- **WHEN** a valid DOCX exceeds a retained input, output, fact, or memory ceiling before all rendered story parts are processed
- **THEN** the document has a typed limit outcome and no unexamined symbol is claimed as retained; file-local publication requires an explicit incomplete-coverage record

#### Scenario: Cancellation or shared deadline
- **WHEN** cancellation or the shared indexing deadline stops DOCX extraction
- **THEN** the generation fails without publishing partial document evidence

#### Scenario: Non-rendered symbol
- **WHEN** the same symbol is inside deleted or otherwise non-rendered content
- **THEN** it does not degrade the rendered document's coverage

#### Scenario: PDF exhausts parser fuel
- **WHEN** a PDF beside ordinary source consumes its finite Wasmi execution-fuel budget during text indexing
- **THEN** the published generation retains verified source, identifies that PDF with durable file-local incomplete coverage and a typed execution-fuel limit, and retains cancellation and other resource ceilings

#### Scenario: Valid PDF exceeds the old fuel ceiling
- **WHEN** a reproducible valid PDF exceeds the RC2 fuel ceiling but completes under a measured finite budget within wall-time, memory, output, and cancellation limits
- **THEN** that justified budget is admitted and its exact text is indexed, while adversarial or larger inputs still stop at a finite limit

### Requirement: Repository publication is atomic and coverage-aware
A scan SHALL publish a navigable generation only when all accepted source facts are verified and every document-local incomplete outcome is explicitly represented in persisted/queryable coverage. Queries MUST NOT infer absence of facts from an incomplete document. A malformed package, wrong root, source change, I/O fault, cancellation, or publication failure SHALL retain the previous complete generation and authored state.

#### Scenario: Initial scan with one unsupported document
- **WHEN** a repository contains ordinary source plus one document with an accepted unsupported/limit outcome
- **THEN** the published generation exposes verified source and an explicit document-level incomplete result through CLI and MCP navigation

#### Scenario: Repair and incremental retry
- **WHEN** the document is replaced with a supported valid version after an incomplete publication
- **THEN** incremental refresh replaces its incomplete status with exact indexed text and facts without retaining stale rows

#### Scenario: Malformed document or interrupted publication
- **WHEN** the document package is malformed or the scan is cancelled or faults during publication
- **THEN** no new generation is advertised and the prior complete generation and authored data remain intact

#### Scenario: Wrong root, missing index, and no implicit mutation
- **WHEN** MCP navigation selects the wrong root, has no published index, or encounters a stale document
- **THEN** it reports the exact verification state for that root without borrowing another root's database or silently initializing/mutating it
