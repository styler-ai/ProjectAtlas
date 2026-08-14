## ADDED Requirements

### Requirement: Markdown structure extraction is parser-owned, exact, and bounded
ProjectAtlas SHALL use the workspace-pinned `pulldown-cmark` parser to extract Markdown and supported Markdown portions of MDX into bounded `heading` symbols and explicit reference candidates with exact byte/line selectors, parser provenance, coverage, completeness, and truncation state. The existing structural summary MUST reuse the same heading facts.

#### Scenario: Headings become exact symbols
- **WHEN** Markdown contains ATX or setext headings, duplicate labels, Unicode, inline formatting, code spans, or nested sections
- **THEN** ProjectAtlas emits deterministic `heading` symbols with level, compact label, exact occurrence/range, and disambiguating selector identity

#### Scenario: Code-fence headings are not symbols
- **WHEN** text resembling a heading appears inside a fenced or indented code block or HTML/MDX expression
- **THEN** the parser does not publish it as a document heading

#### Scenario: Parser limits are visible
- **WHEN** admitted bytes, heading count, link/path candidates, label bytes, or retained evidence reach a configured hard bound
- **THEN** extraction stops within the bound and records partial/truncated coverage without claiming completeness

#### Scenario: Unsupported prose formats remain honest
- **WHEN** RST, raw JSX/HTML, or another unsupported document structure is admitted as text
- **THEN** classification/search/summary remain available as applicable but no unsupported heading or relationship parser is claimed

### Requirement: Only explicit static repository-local references become candidates
ProjectAtlas SHALL admit relation candidates only from parser-emitted Markdown link destinations or a complete inline-code span that is itself one valid repository-relative file selector. It MUST reject images, external/absolute/drive/UNC URLs, dynamic or templated destinations, fragment-only anchors, directories, ordinary identifiers/calls/commands, and prose resemblance.

#### Scenario: Relative Markdown link is resolved from its document
- **WHEN** a document at `docs/guide.md` links to `../src/lib.rs#entry`
- **THEN** resolution starts at `docs/`, normalizes the repository-relative file selector, retains the fragment as bounded target evidence, and never uses the process working directory

#### Scenario: Reference links use parser destinations
- **WHEN** CommonMark reference syntax resolves to one static repository-local destination
- **THEN** ProjectAtlas treats the resolved parser destination exactly like an inline link without re-parsing prose

#### Scenario: Whole code-span path is accepted conservatively
- **WHEN** one inline code span contains exactly a normalized path such as `src/lib.rs` with an optional supported line or heading selector
- **THEN** it becomes a bounded candidate, while `foo()`, `cargo test`, `../`, URLs, mixed prose, and shell fragments do not

#### Scenario: Images and dynamic targets do not create edges
- **WHEN** a document contains an image destination, raw HTML link, JSX expression, template variable, or runtime-generated path
- **THEN** no `documents` fact is fabricated and coverage records the applicable unsupported state

### Requirement: Resolution is exact-root, case-safe, and privacy-safe
Document candidates SHALL resolve only to admitted indexed file or heading identities under the selected exact root. Root/symlink escape, ignored targets, missing targets, case-only ambiguity, unsupported target types, and absent static identity MUST remain closed typed unresolved evidence using only bounded repository-relative selectors or digests.

#### Scenario: Exact indexed target resolves
- **WHEN** the normalized selector identifies one admitted exact-case file or heading under the selected root
- **THEN** ProjectAtlas emits one resolved relation with both endpoint classifications and exact next-call selectors

#### Scenario: Case conflict does not choose a winner
- **WHEN** case folding could identify more than one path or the requested case differs on a case-sensitive checkout
- **THEN** ProjectAtlas emits `case_conflict` evidence and no resolved edge on Windows, Linux, or macOS

#### Scenario: Root or symlink escape is refused
- **WHEN** normalization or symlink resolution would leave the selected repository root
- **THEN** ProjectAtlas emits `outside_root` without exposing the absolute destination or reading its content

#### Scenario: Missing and ignored remain distinct
- **WHEN** a selector names a nonexistent path or a path excluded by effective ignore/admission policy
- **THEN** ProjectAtlas records `missing` or `ignored` respectively without importing excluded content

#### Scenario: Unsupported and non-static remain distinct
- **WHEN** a selector names a directory/unsupported target or no static selector can be obtained
- **THEN** ProjectAtlas records `unsupported` or `no_static_target` and no edge

### Requirement: One canonical documents fact supports both directions
ProjectAtlas SHALL store one canonical `documents` relation from a documentation file/heading to its validated file/heading target. `documented_by` SHALL be only the inbound view of that fact. Duplicate references collapse by canonical key, self-references do not create redundant navigation, and cycles remain bounded by existing traversal controls.

#### Scenario: Documentation reaches source
- **WHEN** a document explicitly resolves to a source file or symbol
- **THEN** outbound `documents` traversal returns the source endpoint, classifications, evidence, confidence/completeness, and exact next call

#### Scenario: Source reaches documentation
- **WHEN** the same source endpoint is queried inbound for `documents`
- **THEN** the adapter labels the view `documented_by` and returns the original document without storing a second relation

#### Scenario: Duplicate links remain one graph fact
- **WHEN** one or several documents repeat the same canonical source-target/evidence identity
- **THEN** stable relation keys deduplicate identical facts while preserving distinct source headings or selectors that carry different evidence

#### Scenario: Document cycles terminate
- **WHEN** documents explicitly reference each other in a cycle
- **THEN** bounded traversal reports the cycle through existing visited/depth/row/byte controls without recursion or duplicate inverse facts

### Requirement: Incremental publication invalidates the exact document closure
Full and incremental publication SHALL produce equivalent classification, heading, and document relations. Add/change/delete/rename/ignore/case changes MUST remove old facts and recompute changed-document candidates plus the existing indexed inbound affected closure in one complete generation.

#### Scenario: Changed document replaces old edges
- **WHEN** a document changes one explicit target
- **THEN** the prior heading/link facts are removed and only the newly parsed/resolved facts become active

#### Scenario: Target rename invalidates inbound documents
- **WHEN** a source/document target is renamed, deleted, ignored, or changes case
- **THEN** every indexed inbound document candidate is recomputed into resolved or the correct typed unresolved state

#### Scenario: Incremental equals clean rebuild
- **WHEN** the same final repository is reached through a sequence of adds, changes, renames, and deletions or through one clean full scan
- **THEN** canonical classifications, headings, relations, completeness, and unresolved evidence are identical

#### Scenario: Failure retains prior generation
- **WHEN** parsing, resolution, database write, cancellation, or crash interrupts affected-closure publication
- **THEN** readers keep the prior complete generation and recovery cannot mix old headings with new relations
