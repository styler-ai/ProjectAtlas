## ADDED Requirements

### Requirement: Every admitted file has one closed content classification
ProjectAtlas SHALL publish exactly one `source`, `documentation`, `configuration_data`, `other_text`, or `opaque` classification for every admitted indexed file. Known classifications MUST come from the typed language registry; otherwise eligible valid UTF-8 SHALL be `other_text` and invalid/binary content SHALL be `opaque`. Purpose text, filename resemblance, and unverified generated-file guesses MUST NOT override classification.

#### Scenario: Known source and documentation families use registry truth
- **WHEN** the scanner admits a Rust file, Markdown document, MDX document, or declared configuration/data family
- **THEN** its stored and returned classification matches the registry-owned role without adapter inference

#### Scenario: Unknown text remains useful without becoming source
- **WHEN** an eligible extensionless or unknown-format file is valid UTF-8 and has no registry row
- **THEN** ProjectAtlas classifies it as `other_text`, keeps it boundedly searchable/summarizable, and never presents it as source

#### Scenario: Unknown binary content is opaque
- **WHEN** an admitted file has no registry row and is invalid UTF-8 or binary under the existing bounded admission checks
- **THEN** ProjectAtlas classifies it as `opaque`, retains eligible metadata, and does not persist searchable text or fabricate document facts

#### Scenario: Known families retain registry authority when text is unavailable
- **WHEN** an admitted file has a known registry family but bounded text indexing skips it as too large, binary, or invalid UTF-8
- **THEN** ProjectAtlas retains the registry-owned classification while omitting text-derived facts it could not safely extract

#### Scenario: Ignored and ineligible files stay absent
- **WHEN** `.gitignore`, ProjectAtlas ignore policy, root/privacy rules, vendor/cache policy, or size admission excludes a path
- **THEN** no classification row or content-derived graph fact is published for that path

#### Scenario: Purpose and classification are independent
- **WHEN** an agent suggests, approves, stales, or changes a file purpose
- **THEN** the derived classification remains unchanged and every purpose-bearing result still exposes it

### Requirement: Content selection is closed and backwards compatible
Affected queries SHALL accept only `source`, `documentation`, or `both` when selection is explicit. Omission MUST remain a distinct legacy-compatible state that preserves the previous candidate universe, ordering, defaults, and unsupported-value behavior except for additive classification fields.

#### Scenario: Omitted selection preserves legacy results
- **WHEN** an existing caller omits content selection
- **THEN** files, search, purpose, summary, and graph candidates retain their prior inclusion and ranking, including configuration/data and other text already admitted

#### Scenario: Source selection is exact
- **WHEN** a caller selects `source`
- **THEN** only file candidates classified as source enter ordinary candidate ranking or traversal frontiers

#### Scenario: Documentation selection is exact
- **WHEN** a caller selects `documentation`
- **THEN** only documentation candidates enter ordinary candidate ranking or traversal frontiers

#### Scenario: Both selection remains bounded
- **WHEN** a caller selects `both`
- **THEN** source and documentation candidates are admitted under the same existing row/byte/deadline bounds while configuration/data, other text, and opaque candidates are excluded

#### Scenario: Unsupported selection fails typed validation
- **WHEN** CLI or MCP receives an empty, unknown, repeated, or malformed content selection
- **THEN** both adapters return the same typed allowed-value error without querying or changing the database

### Requirement: Classification persists and queries transactionally
The active atlas SHALL store classifications in one constrained table owned by the current complete derived generation, enforce the five-value domain and file-node ownership, index classification/path access, and batch endpoint reads without per-result database round trips.

#### Scenario: Full publication is coherent
- **WHEN** a full scan publishes nodes, classifications, symbols, and graph facts
- **THEN** one transaction exposes either the prior complete generation or the new complete generation with exactly one classification for every admitted file

#### Scenario: Incremental deletion removes classification
- **WHEN** an admitted file is deleted, ignored, renamed, or becomes ineligible
- **THEN** its old classification and dependent document facts disappear in the same complete publication that updates the file inventory

#### Scenario: Indexed selection uses the intended plan
- **WHEN** a bounded classification/path query is prepared for a representative repository
- **THEN** query-plan assertions show the declared classification/path index and no per-row follow-up query

#### Scenario: Interrupted publication rolls back
- **WHEN** classification publication encounters busy state, cancellation, constraint failure, or process interruption before commit
- **THEN** no partial classification/document generation becomes active and restart follows existing typed recovery
