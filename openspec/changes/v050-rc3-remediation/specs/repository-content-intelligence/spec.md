## MODIFIED Requirements

### Requirement: v0.5 document extraction supports only PDF and DOCX
#465 SHALL pin and audit the fixed `pdf-extract` 0.12.0+projectatlas guest with `lopdf` 0.44.0, the `wasmi`/`wasmi_core` 2.0.0 host, `quick-xml` 0.42.0, and `zip` 0.6.6 (ZIP default features disabled, only `deflate` enabled) plus their exact locked transitive trees. It SHALL admit only PDF content streams and stored or DEFLATE DOCX WordprocessingML parts reached through validated in-package story relationships, reject encrypted or unsupported compression as typed unsupported input before text publication, and invoke no OCR, legacy DOC, spreadsheet/presentation formats, macros, scripts, remote references, arbitrary processes, or embedded recursive parsers.

#### Scenario: Canonical guest source proof
- **WHEN** CI verifies the fixed PDF guest on Linux x86-64 with the pinned Rust toolchain and locked sources
- **THEN** the rebuilt WASI bytes must exactly match the checked-in guest
- **AND** every supported native platform executes that same embedded guest through document, CLI/MCP, and resource proof; noncanonical hosts validate guest sources without claiming cross-host compiler-byte equality

#### Scenario: Valid PDF
- **WHEN** PDF magic and all input/time/memory/output limits pass
- **THEN** ProjectAtlas publishes bounded text with exact page and text-span locator, parser/version provenance, completeness, and coverage

- **AND** Type 0 fonts admit Identity-H with ToUnicode mapping; vertical and custom stream encoding CMaps return typed unsupported input rather than fabricating character codes or positions; horizontal text measures its baseline and height independently under scaling and rotation
- **AND** CID width ranges apply the declared width to every character through the inclusive last CID, preserving text spacing
- **AND** word and line gaps use the previous glyph's transformed text frame, preserving exact logical text across rotation, nonuniform scaling, reflection, and shear; degenerate frames emit literal characters without inferred separators
- **AND** partial ToUnicode maps use a known font encoding for missing entries, including built-in Base-14 encodings independently of explicit widths; used characters with no known fallback and unmapped CID characters return typed unsupported input without publishing partial text
- **AND** simple fonts use the font descriptor's MissingWidth for codes outside their explicit width range, default to zero when absent, and reject malformed descriptors or nonnumeric/nonfinite widths
- **AND** Type 3 widths use the horizontal component of their required finite six-component FontMatrix to convert glyph-space widths into text displacement, including rotated and reflected matrices; malformed matrices fail before publication
- **AND** named DeviceCMYK and Pattern color-space aliases preserve text extraction; used undefined font-encoding slots return typed unsupported input without overriding explicit ToUnicode mappings or defined WinAnsi bullet codes
- **AND** ExtGState font selections update the font and size with scoped dictionary identity, preserve graphics-state save/restore and Form resources, and reject malformed font entries before publication
- **AND** marked-content ActualText replacements and ReversedChars text ordering return typed unsupported input, including named properties and Form-local resource scopes, rather than publishing the underlying glyphs as complete text; a bounded preflight over structure-root child links also refuses structure-element ActualText, preserves ordinary tagged content without replacements, and fails closed on uninspectable or cyclic child links without interpreting logical order or ParentTree mappings
- **AND** quote text-showing operators preserve the corresponding line movement, spacing changes, and text emission
- **AND** configured character and word spacing contributes to the transformed glyph endpoint without fabricating extra spaces between adjacent text operations; genuine geometric gaps remain word boundaries
- **AND** q/Q save and restore the text and text-line matrices together, including within Form-local execution, so subsequent positioning uses the restored origin
- **AND** inherited page rotation uses validated integer quarter turns before text layout; malformed rotation fails before publication
- **AND** Form XObjects inherit graphics state and compose their matrix with the caller transform, preserving positioned and nested text evidence; malformed matrices fail before publication

#### Scenario: Empty document replacement
- **WHEN** an indexed document is replaced by an admitted PDF or DOCX with no text, including a PDF with a structurally valid zero-page tree
- **THEN** refresh publishes empty text and no document blocks, regenerates the content summary and any suggested purpose without removed blocks, and preserves authored purposes
- **AND** malformed page-tree counts remain typed failures that preserve the previous complete publication

#### Scenario: Bounded concurrent document admission
- **WHEN** concurrent CLI, MCP, or source-parser jobs request document extraction in one process
- **THEN** only one PDF or DOCX parser executes at a time, preventing their heavyweight memory envelopes from multiplying by the ordinary source-worker count
- **AND** queued document work observes the caller's cancellation and deadline before parser allocation, and every success or failure releases admission

#### Scenario: Generated configuration upgrade
- **WHEN** an existing configuration contains the exact ordered pre-document generated source-extension defaults
- **THEN** runtime normalization admits PDF and DOCX without rewriting the configuration
- **AND** absent settings use current defaults while reordered, shortened, extended, or empty custom lists remain authoritative

#### Scenario: Valid DOCX
- **WHEN** a ZIP container passes entry/path/compressed/expanded/recursion limits and contains admitted `word/document.xml` with Transitional or Strict WordprocessingML namespace identity, independent of its prefix
- **THEN** ProjectAtlas publishes bounded text with exact part, paragraph, run, and text-span locator plus parser/version provenance for validated package-reachable rendered stories
- **AND** admitted text leaves honor inherited `xml:space`: default mode removes only leading and trailing XML whitespace after complete entity/CDATA decoding, preserve mode retains it, and invalid modes fail as malformed input; interior whitespace, nonbreaking spaces, and exact logical run spans remain intact
- **AND** live ruby annotations return typed unsupported input rather than a malformed-input error; deleted ruby content remains excluded
- **AND** alternate-format chunks return typed unsupported input rather than omitting referenced content
- **AND** bounded UTF-16 LE/BE XML parts are normalized before parsing, including unambiguous BOM-less parts as an explicit compatibility tolerance; declared encodings must match the source bytes, while unsupported encodings return typed unsupported input and mismatches return malformed input without replacing the last complete publication
- **AND** foreign-namespace character data requiring unsupported semantic decoding returns typed unsupported input, while recognized Word text boxes remain supported through drawing wrappers
- **AND** nested text boxes preserve document order and resume outer runs with exact fragment offsets
- **AND** explicit hyphens, tabs, and saved page breaks retain their text/separator characters and exact UTF-8 spans; rendered font-coded symbols retain durable font/code/part/occurrence identity and produce Unicode only for verified mappings, with unknown mappings marked as incomplete text coverage
- **AND** field instructions and deleted text are validated without execution or publication, while cached field results and instruction text outside field-code regions remain literal text
- **AND** directly `w:vanish`-hidden run payload is excluded from normal rendered text, symbols, and story references without losing field-state transitions; unresolved style-inherited visibility or standalone `w:specVanish` renderer differences are reported as typed file-local incomplete coverage rather than complete rendered text
- **AND** live page-number and date blocks requiring evaluation remain unresolved with typed file-local incomplete text coverage; footnote/endnote references admit their validated in-package note stories without fabricating evaluated marker text; deleted, moved-from, and unselected blocks remain excluded
- **AND** deleted and moved-from revision containers suppress all text leaves, separators, and field-state changes while preserving source paragraph/run numbering
- **AND** field nesting is bounded independently of XML depth and isolated within each text container
- **AND** Markup Compatibility alternatives emit only the first choice requiring understood WordprocessingML namespaces, or its fallback; unselected branches cannot change extraction context or duplicate evidence
- **AND** root `mc:Ignorable` declarations admit at most 64 distinct resolved namespace URIs and suppress unknown extension subtrees without suppressing understood Word content; aliases follow URI identity, while nested policies and nonempty `mc:ProcessContent` or `mc:MustUnderstand` return typed unsupported input before publication
- **AND** bounded, validated relationships admit every rendered header, footer, footnote, endnote, referenced comment, frame, and text box within retained parser bounds; only safe referenced unsupported story types outside that supported set may remain unexamined with explicit incomplete coverage, while unsafe or malformed relationship targets fail closed

#### Scenario: Explicit language overrides a document extension
- **WHEN** an accepted language override selects another language for a `.pdf` or `.docx` path
- **THEN** text and symbol navigation honor the selected language rather than invoking the document parser from its extension
- **AND** map purpose-header reads use the same exact-filename and longest-extension override precedence, allowing text headers for text overrides and requiring database purposes for document overrides

#### Scenario: Document input exceeds the ordinary source ceiling
- **WHEN** a valid PDF or DOCX is larger than the ordinary source/text-index ceiling but within the declared document input and extraction bounds
- **THEN** normal scans publish its extracted text and locator-bearing blocks without requiring a source-limit override
- **AND** aggregate retained-text capacity charges extracted document text, independently of bounded container input bytes
- **AND** ordinary source retains its existing ceiling, while a document above 8 MiB fails before replacing the prior complete generation

#### Scenario: Malformed, encrypted, bomb, oversized, unsupported, or canceled input
- **WHEN** magic mismatches, PDF is malformed/encrypted/password-protected, DOCX has duplicate/unsafe/recursive/expansive entries, non-accepted unsupported content such as live ruby or an unsupported XML encoding is encountered, or package input bounds/cancellation trigger
- **THEN** extraction returns typed bounded/unsupported coverage, publishes no truncated-complete text or new generation, and never invokes external code/network
- **AND** accepted parser output/fact/memory/work or PDF-fuel limits after safe package admission publish only with durable file-specific incomplete coverage under the RC3 document-index-continuity contract; a safely referenced alternate-format chunk is unexamined story coverage

### Requirement: Document evidence is exact, sparse, and atomic
PDF/DOCX extracted text, locators, provenance, coverage, and any document/source relations SHALL publish through existing indexed-text/graph authority when representable; otherwise the smallest constrained SQLite delta SHALL land first. Relations SHALL require exact typed evidence and SHALL not fan a long document out by topical similarity.

#### Scenario: Literal document summary
- **WHEN** an admitted PDF or DOCX publishes extracted text
- **THEN** its content summary and generated purpose use a bounded literal text excerpt rather than synthetic graph block names, without inferring headings or titles
- **AND** empty extracted text produces an explicit empty summary, while authored purposes remain unchanged

#### Scenario: Existing storage is sufficient
- **WHEN** current text/occurrence/coverage rows express the locator and hot queries within bounds
- **THEN** no new schema or index is added

#### Scenario: Accepted document-local incomplete coverage
- **WHEN** a safely admitted document has an accepted parser output/fact/memory/work or PDF-fuel limit, an unresolved DOCX symbol mapping or visibility difference, or live page-number/date field requiring evaluation, or a safe referenced unsupported story type outside the named supported set
- **THEN** the new generation may publish only with verified source and durable, queryable file-specific incomplete coverage; it MUST NOT advertise omitted document evidence as complete

#### Scenario: Publication or replacement fails
- **WHEN** malformed, unsafe, or non-accepted unsupported extraction, source/I/O change, incremental replace/delete, database write, cancellation, or shared deadline fails
- **THEN** the prior complete generation remains current and no partial extracted evidence is advertised
