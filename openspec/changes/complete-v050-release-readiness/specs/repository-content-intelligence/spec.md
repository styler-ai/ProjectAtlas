## ADDED Requirements

### Requirement: PHP 8 is a pinned built-in language
#477 SHALL pin `tree-sitter-php` 0.24.2 against the workspace Tree-sitter 0.26.9 contract and SHALL add PHP to the existing `LanguageCapability` and built-in parser dispatch without a provider/process framework.

#### Scenario: Supported PHP source
- **WHEN** PHP 8 source contains namespaces, classes/interfaces/traits/enums, functions/methods/properties/constants, `use` aliases, or static include/require/call syntax
- **THEN** ProjectAtlas emits the accepted kinds/parents/signatures, exact byte/line/column spans, parser provenance, and conservative exact relations through existing graph publication

#### Scenario: Dynamic or mixed PHP
- **WHEN** variable names, runtime includes, `eval`, ambiguous calls, unsupported constructs, malformed recovery trees, or mixed HTML/PHP exceed exact static semantics
- **THEN** valid PHP spans remain navigable and unsupported/dynamic regions return typed partial/fallback coverage without invented relations

#### Scenario: Large or canceled parse
- **WHEN** file/node/output/time/cancellation bounds are reached
- **THEN** parsing terminates within the accepted budget and never aborts unrelated repository publication

### Requirement: PHP guidance follows accepted runtime evidence
After #477 acceptance, #339 SHALL publish exactly one v0.5 PHP guidance profile derived from `LANGUAGE_CAPABILITIES`, generated language-support data, fixtures, representative Composer repositories, CLI/MCP results, and installed version-matched skill behavior.

#### Scenario: Evidence establishes a workflow
- **WHEN** registry, parser, graph, representative task, and installed skill agree
- **THEN** guidance routes overview -> folders -> files -> summary/outline/search/detailed graph -> exact slice and states reliable syntax/relation/fallback boundaries

#### Scenario: Framework or dynamic claim lacks evidence
- **WHEN** behavior has not been established
- **THEN** guidance explicitly abstains or selects lexical/fallback evidence and does not generalize PHP into a plural language campaign

### Requirement: v0.5 document extraction supports only PDF and DOCX
#465 SHALL pin and audit the fixed `pdf-extract` 0.12.0+projectatlas guest with `lopdf` 0.44.0, the `wasmi`/`wasmi_core` 2.0.0 host, `quick-xml` 0.42.0, and `zip` 0.6.6 (ZIP default features disabled, only `deflate` enabled) plus their exact locked transitive trees. It SHALL admit only PDF content streams and stored or DEFLATE DOCX `word/document.xml`, reject encrypted or unsupported compression as typed unsupported input before text publication, and invoke no OCR, legacy DOC, spreadsheet/presentation formats, macros, scripts, remote references, arbitrary processes, or embedded recursive parsers.

#### Scenario: Canonical guest source proof
- **WHEN** CI verifies the fixed PDF guest on Linux x86-64 with the pinned Rust toolchain and locked sources
- **THEN** the rebuilt WASI bytes must exactly match the checked-in guest
- **AND** every supported native platform executes that same embedded guest through document, CLI/MCP, and resource proof; noncanonical hosts validate guest sources without claiming cross-host compiler-byte equality

#### Scenario: Valid PDF
- **WHEN** PDF magic and all input/time/memory/output limits pass
- **THEN** ProjectAtlas publishes bounded text with exact page and text-span locator, parser/version provenance, completeness, and coverage

- **AND** Type 0 fonts admit Identity-H with ToUnicode mapping; vertical and custom stream encoding CMaps return typed unsupported input rather than fabricating character codes or positions; horizontal text measures its baseline and height independently under scaling and rotation
- **AND** CID width ranges apply the declared width to every character through the inclusive last CID, preserving text spacing
- **AND** partial ToUnicode maps use a known font encoding for missing entries, including built-in Base-14 encodings independently of explicit widths; used characters with no known fallback and unmapped CID characters return typed unsupported input without publishing partial text
- **AND** simple fonts use the font descriptor's MissingWidth for codes outside their explicit width range, default to zero when absent, and reject malformed descriptors or nonnumeric/nonfinite widths
- **AND** ExtGState font selections update the font and size with scoped dictionary identity, preserve graphics-state save/restore and Form resources, and reject malformed font entries before publication
- **AND** marked-content ActualText replacements return typed unsupported input, including named properties and Form-local resource scopes, rather than publishing the underlying glyphs as complete text
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
- **THEN** ProjectAtlas publishes bounded text with exact part, paragraph, run, and text-span locator plus parser/version provenance
- **AND** live ruby annotations return typed unsupported input rather than a malformed-input error; deleted ruby content remains excluded
- **AND** alternate-format chunks return typed unsupported input rather than omitting referenced content
- **AND** unsupported XML encodings, including UTF-16, return typed unsupported input without replacing the last complete publication
- **AND** foreign-namespace character data requiring unsupported semantic decoding returns typed unsupported input, while recognized Word text boxes remain supported through drawing wrappers
- **AND** nested text boxes preserve document order and resume outer runs with exact fragment offsets
- **AND** explicit hyphens, tabs, and saved page breaks retain their text/separator characters and exact UTF-8 spans; font-coded symbols return typed unsupported input without replacing the last complete publication
- **AND** field instructions and deleted text are validated without execution or publication, while cached field results and instruction text outside field-code regions remain literal text
- **AND** deleted and moved-from revision containers suppress all text leaves, separators, and field-state changes while preserving source paragraph/run numbering
- **AND** field nesting is bounded independently of XML depth and isolated within each text container
- **AND** Markup Compatibility alternatives emit only the first choice requiring understood WordprocessingML namespaces, or its fallback; unselected branches cannot change extraction context or duplicate evidence

#### Scenario: Explicit language overrides a document extension
- **WHEN** an accepted language override selects another language for a `.pdf` or `.docx` path
- **THEN** text and symbol navigation honor the selected language rather than invoking the document parser from its extension

#### Scenario: Document input exceeds the ordinary source ceiling
- **WHEN** a valid PDF or DOCX is larger than the ordinary source/text-index ceiling but within the declared document input and extraction bounds
- **THEN** normal scans publish its extracted text and locator-bearing blocks without requiring a source-limit override
- **AND** aggregate retained-text capacity charges extracted document text, independently of bounded container input bytes
- **AND** ordinary source retains its existing ceiling, while a document above 8 MiB fails before replacing the prior complete generation

#### Scenario: Malformed, encrypted, bomb, oversized, unsupported, or canceled input
- **WHEN** magic mismatches, PDF is malformed/encrypted/password-protected, DOCX has duplicate/unsafe/recursive/expansive entries, or any bound/cancellation triggers
- **THEN** extraction returns typed bounded/unsupported coverage, publishes no truncated-complete text, and never invokes external code/network

### Requirement: Document evidence is exact, sparse, and atomic
PDF/DOCX extracted text, locators, provenance, coverage, and any document/source relations SHALL publish through existing indexed-text/graph authority when representable; otherwise the smallest constrained SQLite delta SHALL land first. Relations SHALL require exact typed evidence and SHALL not fan a long document out by topical similarity.

#### Scenario: Existing storage is sufficient
- **WHEN** current text/occurrence/coverage rows express the locator and hot queries within bounds
- **THEN** no new schema or index is added

#### Scenario: Publication or replacement fails
- **WHEN** extraction, incremental replace/delete, database write, or cancellation fails
- **THEN** the prior complete generation remains current and no partial extracted evidence is advertised

### Requirement: Content intelligence uses normal agent navigation
PHP and PDF/DOCX evidence SHALL be available through the existing overview, folders, files, summary, outline, search, graph, and exact-evidence routes where the evidence kind applies.

#### Scenario: Agent narrows to exact evidence
- **WHEN** an agent investigates supported PHP or document content
- **THEN** bounded navigation reaches the exact source span or document locator without reading unrelated whole files
