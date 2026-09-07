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

#### Scenario: Valid PDF
- **WHEN** PDF magic and all input/time/memory/output limits pass
- **THEN** ProjectAtlas publishes bounded text with exact page and text-span locator, parser/version provenance, completeness, and coverage

#### Scenario: Valid DOCX
- **WHEN** a ZIP container passes entry/path/compressed/expanded/recursion limits and contains admitted `word/document.xml` with Transitional or Strict WordprocessingML namespace identity, independent of its prefix
- **THEN** ProjectAtlas publishes bounded text with exact part, paragraph, run, and text-span locator plus parser/version provenance
- **AND** nested text boxes preserve document order and resume outer runs with exact fragment offsets

#### Scenario: Document input exceeds the ordinary source ceiling
- **WHEN** a valid PDF or DOCX is larger than the ordinary source/text-index ceiling but within the declared document input and extraction bounds
- **THEN** normal scans publish its extracted text and locator-bearing blocks without requiring a source-limit override
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
