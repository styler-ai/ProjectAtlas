## ADDED Requirements

### Requirement: Bounded document failures are file-specific and truthful
DOCX/PDF extraction SHALL retain finite time, memory, fuel, input, and output bounds. Every syntactically valid rendered DOCX `w:sym` in package-reachable document stories—including the main body, headers, footers, footnotes, endnotes, referenced comments, frames, and nested text boxes—and within those bounds SHALL retain its exact font/code, story-part, and occurrence identity in durable queryable evidence; a verified font mapping SHALL produce Unicode text, while an unknown mapping SHALL identify unresolved text coverage without discarding surrounding content. Unreferenced glossary content is non-rendered; a referenced glossary, subdocument, or other rendered story that cannot be safely examined SHALL be recorded as explicitly incomplete, not silently treated as symbol-free. Story relationships SHALL be validated for type, existence, and package containment without external fetch or path escape. An accepted parser output, fact, memory, or repeated-decompression-work limit after safe package admission SHALL produce typed document-local incomplete coverage without claiming unexamined symbols were retained. The extractor SHALL batch reachable note/comment IDs before reading their shared story parts where possible; the finite work ceiling is a fail-safe, not a substitute for supported-story extraction. Unsafe package/ZIP input limits, cancellation, and the shared indexing deadline SHALL remain generation-fatal. PDF fuel use SHALL be measured on valid and adversarial fixtures; a finite budget increase is allowed only when it demonstrably admits valid input under retained bounds. Remaining PDF fuel exhaustion SHALL identify its file and typed reason. The system MUST NOT guess missing text, silently omit it from a complete-coverage claim, or conflate PDF parser fuel with the per-file text-index byte cap.

Relationship attribute values SHALL be XML-decoded before type, target, and internal/external target-mode validation.
For `w:sym`, exact font/code identity means retaining each supplied decoded value and retaining absence when `w:font` or `w:char` is omitted; a missing attribute is unresolved text coverage, not malformed package input.

For RC3, the modified `repository-content-intelligence` requirements supersede #465's main-part-only, font-symbol, footnote/endnote no-read, and unconditional bound-failure publication rules. They retain the PDF extraction and package-safety contract and do not permit OCR, macros, scripts, remote references, arbitrary processes, recursive embedded parsers, or unsafe ZIP admission. A reference marker or live page-number/date field needing dynamic evaluation has typed file-local incomplete text coverage rather than fabricated text, even when its in-package note story is examined.

#### Scenario: Rendered DOCX font-specific symbol
- **WHEN** an otherwise valid DOCX contains a rendered `w:sym` glyph in any supported story part whose font mapping is not known
- **THEN** the result preserves its exact font/code, story-part identity, and occurrence locator through publication and reopen, retains verifiable surrounding text, identifies unresolved Unicode-text coverage, and never advertises the glyph as decoded

#### Scenario: Symbol identity attribute is omitted
- **WHEN** a rendered `w:sym` omits `w:font`, `w:char`, or both
- **THEN** its occurrence and supplied attributes remain queryable, omitted attributes remain absent rather than invented, and Unicode-text coverage is incomplete without rejecting the otherwise valid package

#### Scenario: Symbol identity attribute is duplicated
- **WHEN** a `w:sym` supplies duplicate font or character attributes by different prefixes bound to the same WordprocessingML namespace
- **THEN** the malformed document is rejected without publishing an order-dependent symbol identity

#### Scenario: Rendered symbols outside the main body
- **WHEN** a safely admitted DOCX references rendered headers, footers, footnotes, endnotes, comments, frames, or nested text boxes containing `w:sym` within retained parser bounds
- **THEN** every such symbol is admitted with exact story-part provenance; an incomplete-coverage fallback does not satisfy this supported-story case

#### Scenario: Special note separator symbols
- **WHEN** a footnote or endnote story is admitted and the document-wide note properties list a special separator or continuation item by ID
- **THEN** its text and symbols are admitted once with exact note-part provenance; an unlisted special item is not treated as rendered, and pagination-dependent continuation content has conditional coverage

#### Scenario: Duplicate selected story ID
- **WHEN** a selected note, comment, or special separator ID appears twice in its story part
- **THEN** the malformed package is refused without publishing duplicated text or symbols

#### Scenario: Duplicate section story variant
- **WHEN** one section declares more than one header or footer reference for the same page variant
- **THEN** the malformed section is refused before non-rendered story content can be duplicated

#### Scenario: Compatibility-wrapped document settings
- **WHEN** supported markup compatibility selects document-wide settings that list a special note item
- **THEN** the selected settings govern story loading; an unknown potentially selected branch produces incomplete story coverage rather than a false complete result

#### Scenario: Compatibility-wrapped story item
- **WHEN** a selected markup-compatibility branch wraps a footnote, endnote, or comment item
- **THEN** story IDs are selected at their logical depth, unreferenced items remain excluded, and conditional continuation-note coverage is scoped to that note rather than later items

#### Scenario: Malformed note or comment item structure
- **WHEN** a footnote, endnote, or comment story root contains a direct WordprocessingML child other than its item type, including inside a selected compatibility branch, or any note/comment item appears nested or outside its matching collection root
- **THEN** extraction rejects the malformed story before its text or symbols can enter a new publication

#### Scenario: Section properties outside the main document structure
- **WHEN** section properties and header/footer references appear outside the main document's body or a paragraph-properties child of a paragraph within that body, including in a separate text-box story, accounting for selected compatibility wrappers
- **THEN** extraction rejects the malformed section before its header or footer story can enter a new publication

#### Scenario: Referenced story is not examined
- **WHEN** a valid DOCX references an in-package subdocument, glossary, or other rendered story type that the bounded extractor cannot examine
- **THEN** it publishes typed incomplete story coverage without claiming that part is symbol-free or fetching external content

#### Scenario: Unsafe or malformed story relationship
- **WHEN** a story relationship has an external or escaping target, a missing target, or a wrong-type part
- **THEN** extraction fails closed without reading outside the package or publishing a new generation

#### Scenario: Undeclared attribute prefix in package metadata or suppressed history
- **WHEN** a content-type root or record, relationship element, non-rendered section-revision element, or skipped compatibility branch in a story or settings part contains an attribute with an undeclared namespace prefix
- **THEN** the malformed DOCX is rejected before publication, even when the attribute does not select rendered content

#### Scenario: Invalid entity in ignored XML
- **WHEN** a story, settings, content-type, or relationship attribute contains an undefined or invalid XML entity, including an attribute in an unselected compatibility branch, or ignored settings text contains one
- **THEN** the malformed DOCX is rejected before publication rather than treating ignored content as well-formed

#### Scenario: Encoded whitespace between story elements
- **WHEN** valid XML character references or CDATA inside the story root contain only spaces, tabs, carriage returns, or line feeds outside rendered text leaves
- **THEN** extraction treats them as inter-element whitespace while still rejecting non-whitespace, invalid references, or CDATA outside the root

#### Scenario: Text payload inside non-rendered metadata
- **WHEN** a run or paragraph, including one nested under an otherwise supported wrapper, appears inside paragraph or structured-document properties instead of rendered content
- **THEN** extraction rejects the malformed story before its text, symbols, or references can enter a new publication, while retaining valid block, table-row, and table-cell content wrappers

#### Scenario: BOM-less UTF-16 XML with leading whitespace
- **WHEN** an unambiguous BOM-less UTF-16 LE or BE DOCX XML part without an XML declaration starts with XML whitespace before its root
- **THEN** the bytes are decoded as an explicit compatibility tolerance and the resulting XML is validated like the same part without leading whitespace

#### Scenario: Verified symbol mapping
- **WHEN** a rendered `w:sym` has a verified mapping for its font and code
- **THEN** its Unicode text and exact symbol provenance are indexed without an unsupported-symbol error

#### Scenario: Live field requires evaluation
- **WHEN** a safely admitted DOCX contains a live page-number or date field whose value requires evaluation and has no admitted literal cached result
- **THEN** surrounding literal text remains indexed and the field has durable file-local incomplete text coverage, without fabricated field text or a whole-document completeness claim

#### Scenario: Story anchor inside a field instruction
- **WHEN** a complex-field instruction contains an alternate-format import or subdocument anchor before its result separator, including across paragraphs
- **THEN** the non-rendered anchor selects no relationship or incomplete story coverage, while a literal cached result remains available

#### Scenario: Comment range in non-rendered properties
- **WHEN** a comment range marker appears inside paragraph or other property metadata rather than a supported content parent
- **THEN** the malformed story is rejected before its comment can be selected; a marker under a supported content parent is tolerated but selects no comment story without a rendered comment reference

#### Scenario: Symbol extraction reaches a retained limit
- **WHEN** a safely admitted DOCX exceeds a retained parser output, fact, memory, or repeated-decompression-work ceiling before all rendered story parts are processed
- **THEN** the document publishes a typed file-local incomplete-coverage record and no unexamined symbol is claimed as retained

#### Scenario: Package safety limit
- **WHEN** a DOCX exceeds ZIP entry, path, compressed, expanded, recursion, or other unsafe input limits before safe admission
- **THEN** extraction fails closed without publishing a new generation or changing the last complete one

#### Scenario: Cancellation or shared deadline
- **WHEN** cancellation or the shared indexing deadline stops DOCX extraction
- **THEN** the generation fails without publishing partial document evidence

#### Scenario: Non-rendered symbol
- **WHEN** the same symbol is inside deleted or otherwise non-rendered content
- **THEN** it does not degrade the rendered document's coverage

#### Scenario: Visible controls replace a glossary placeholder
- **WHEN** an SDT placeholder contains actual rendered run controls instead of literal text
- **THEN** those controls count as content, and an absent glossary relationship does not cause false incomplete coverage or package refusal

#### Scenario: Preserved whitespace replaces a glossary placeholder
- **WHEN** an SDT retains a whitespace-only text leaf under `xml:space="preserve"`
- **THEN** the retained text counts as content without selecting a missing glossary relationship

#### Scenario: Opaque content part remains incomplete
- **WHEN** a rendered run contains a `w:contentPart` alternate XML part that this parser does not decode
- **THEN** the glossary placeholder is not selected, and document coverage is explicitly incomplete without reading the target part

#### Scenario: Alternate-format chunk replaces a glossary placeholder
- **WHEN** a non-displayed glossary placeholder has actual SDT content containing a safely referenced alternate-format chunk in a table cell
- **THEN** the chunk counts as SDT content, its reference and incomplete story coverage remain retained, and no unrelated glossary relationship is required

#### Scenario: Field-instruction references are not rendered
- **WHEN** note or comment references appear in a complex field's instruction region before the cached-result separator
- **THEN** their stories are not followed and their markers do not enter rendered text or coverage

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

#### Scenario: Coverage metadata does not consume symbol budgets
- **WHEN** a document with incomplete text coverage is published or bounded impact analysis reads its real symbols
- **THEN** the internal coverage marker remains queryable as coverage but does not consume navigable symbol row or byte limits, inflate reported symbol counts, or hide a later real symbol

#### Scenario: Document language is inferred or mixed case
- **WHEN** an admitted DOCX or PDF is projected with no language hint or a mixed-case hint
- **THEN** its graph and symbols use the admitted format's canonical language so incomplete coverage remains typed and the marker never becomes a navigable symbol

#### Scenario: Repair and incremental retry
- **WHEN** the document is replaced with a supported valid version after an incomplete publication
- **THEN** incremental refresh replaces its incomplete status with exact indexed text and facts without retaining stale rows

#### Scenario: Malformed document or interrupted publication
- **WHEN** the document package is malformed or the scan is cancelled or faults during publication
- **THEN** no new generation is advertised and the prior complete generation and authored data remain intact

#### Scenario: Wrong root, missing index, and no implicit mutation
- **WHEN** MCP navigation selects the wrong root, has no published index, or encounters a stale document
- **THEN** it reports the exact verification state for that root without borrowing another root's database or silently initializing/mutating it
