//! Bounded, in-process extraction for the supported repository document formats.

use crate::check_parser_iteration;
use projectatlas_core::symbols::{CodeSymbol, ParserKind, SymbolGraph, SymbolKind};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use quick_xml::NsReader;
use quick_xml::events::BytesStart;
use quick_xml::events::{BytesRef, Event};
use quick_xml::name::{QName, ResolveResult};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;
use thiserror::Error;
use zip::{CompressionMethod, ZipArchive};

#[path = "../../../packaging/pdf-parser/limits.rs"]
mod limits;
mod pdf_runtime;

/// The exact audited PDF object parser version used by this boundary.
pub const LOPDF_VERSION: &str = "0.44.0";
/// The upstream text parser version with the contained guest's documented patches.
pub const PDF_EXTRACT_VERSION: &str = "0.12.0+projectatlas";
/// The exact audited XML parser version used by the DOCX boundary.
pub const QUICK_XML_VERSION: &str = "0.42.0";
/// Maximum compressed bytes admitted to one document extraction.
pub const MAX_DOCUMENT_COMPRESSED_BYTES: usize = limits::INPUT_LIMIT;
/// Maximum expanded package bytes admitted to one document extraction.
pub const MAX_DOCUMENT_EXPANDED_BYTES: usize = limits::EXPANDED_LIMIT;
/// Maximum extracted UTF-8 bytes retained from one document.
pub const MAX_DOCUMENT_OUTPUT_BYTES: usize = limits::OUTPUT_LIMIT;
/// Maximum source, parser staging, and retained-output envelope for one extraction.
pub const MAX_DOCUMENT_MEMORY_BYTES: usize = 96 * 1024 * 1024;
/// Maximum ZIP entries inspected in one DOCX package.
pub const MAX_DOCUMENT_ENTRIES: usize = 256;
/// Maximum document-container depth; embedded documents are rejected.
pub const MAX_DOCUMENT_RECURSION_DEPTH: usize = 1;
/// Maximum XML element nesting admitted within the document part.
const MAX_DOCX_XML_DEPTH: usize = 64;
/// Maximum ZIP part-name bytes before locator fanout can exceed the document envelope.
const MAX_DOCX_PART_NAME_BYTES: usize = 1024;
/// XML reference identities are bounded before aggregate set fanout.
const MAX_DOCX_REFERENCE_ID_BYTES: usize = 256;
/// Exact font identity retained on each symbol without multiplying unbounded XML attributes.
const MAX_DOCX_FONT_NAME_BYTES: usize = 256;
/// Maximum one relationship or content-type XML table admitted for bounded metadata maps.
const MAX_DOCX_METADATA_BYTES: usize = 2 * 1024 * 1024;
/// Maximum retained package metadata records from one XML table.
const MAX_DOCX_METADATA_RECORDS: usize = 4096;
/// Repeated decompression is parser work, not additional ZIP admission.
const MAX_DOCX_DECOMPRESSION_WORK_BYTES: usize = MAX_DOCUMENT_EXPANDED_BYTES * 2;
/// Maximum distinct ignorable namespace URIs in the supported root policy.
const MAX_DOCX_IGNORABLE_NAMESPACES: usize = 64;
/// Maximum evidence facts retained from one document.
pub const MAX_DOCUMENT_FACTS: usize = limits::FACT_LIMIT;
/// Main DOCX document part selected by the package relationship.
pub const DOCX_DOCUMENT_PART: &str = "word/document.xml";
/// Reserved derived graph fact carrying document-local text coverage to publication.
pub const DOCUMENT_COVERAGE_SYMBOL: &str = "document-text-coverage";

/// Keep heavyweight document parser envelopes from multiplying by source workers.
/// ponytail: one parser per process; add bounded parallel admission only if measured throughput requires it.
static DOCUMENT_EXECUTION: Mutex<()> = Mutex::new(());

/// Admit one process-local document parser without hiding cancellation while queued.
fn lock_document_execution(
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<MutexGuard<'static, ()>, DocumentExtractionError> {
    loop {
        control.check(stage)?;
        match DOCUMENT_EXECUTION.try_lock() {
            Ok(guard) => return Ok(guard),
            // The lock owns no mutable parser state; each call owns and drops its parser.
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
}

/// A repository document format supported by the bounded extraction boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentFormat {
    /// Portable Document Format, parsed by the fixed contained text extractor.
    Pdf,
    /// Office Open XML Word document, parsed by the direct `quick-xml` boundary.
    Docx,
}

impl DocumentFormat {
    /// Return the canonical language identifier used by the registry.
    #[must_use]
    pub const fn language(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
        }
    }
}

impl fmt::Display for DocumentFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.language())
    }
}

/// Completeness of one bounded extraction result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentCompleteness {
    /// Every supported content item in the admitted format was extracted.
    Complete,
    /// Exact retained evidence has one or more explicitly unresolved text regions.
    Partial {
        /// Distinct reasons why the extracted text is not the complete rendering.
        gaps: Vec<DocumentCoverageGap>,
    },
}

/// A bounded reason that prevents document text from claiming full coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentCoverageGap {
    /// A font-specific glyph has no verified Unicode mapping.
    UnknownSymbolMapping,
    /// A live field or reference marker requires layout-time evaluation.
    UnevaluatedField,
    /// A referenced story could not be safely interpreted as rendered text.
    UnexaminedStory,
    /// A configured story may not appear without pagination evidence.
    ConditionalStory,
    /// Style inheritance or renderer policy may change which runs are visible.
    UnresolvedVisibility,
    /// Parsing stopped at an accepted post-admission resource ceiling.
    ResourceLimit(DocumentLimit),
}

impl DocumentCoverageGap {
    /// Stable persisted spelling for one document-local incomplete reason.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownSymbolMapping => "unknown_symbol_mapping",
            Self::UnevaluatedField => "unevaluated_field",
            Self::UnexaminedStory => "unexamined_story",
            Self::ConditionalStory => "conditional_story",
            Self::UnresolvedVisibility => "unresolved_visibility",
            Self::ResourceLimit(DocumentLimit::OutputBytes) => "resource_limit:output_bytes",
            Self::ResourceLimit(DocumentLimit::FactCount) => "resource_limit:fact_count",
            Self::ResourceLimit(DocumentLimit::MemoryBytes) => "resource_limit:memory_bytes",
            Self::ResourceLimit(DocumentLimit::ParserWorkBytes) => {
                "resource_limit:parser_work_bytes"
            }
            Self::ResourceLimit(_) => "resource_limit:other",
        }
    }
}

/// Exact location of one extracted document text span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentLocator {
    /// A text span in one one-based PDF page.
    Pdf {
        /// One-based page number.
        page: usize,
        /// Inclusive byte offset within the parser's page text.
        text_start: usize,
        /// Exclusive byte offset within the parser's page text.
        text_end: usize,
    },
    /// A text span in one admitted DOCX story part.
    Docx {
        /// Package part containing the source text.
        part: String,
        /// One-based paragraph ordinal in the admitted document body.
        paragraph: usize,
        /// One-based run ordinal within the paragraph.
        run: usize,
        /// Inclusive UTF-8 byte offset within the run text.
        text_start: usize,
        /// Exclusive UTF-8 byte offset within the run text.
        text_end: usize,
    },
}

impl fmt::Display for DocumentLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pdf {
                page,
                text_start,
                text_end,
            } => write!(
                formatter,
                "pdf:page={page};text-span={text_start}..{text_end}"
            ),
            Self::Docx {
                part,
                paragraph,
                run,
                text_start,
                text_end,
            } => write!(
                formatter,
                "docx:part={part};paragraph={paragraph};run={run};text-span={text_start}..{text_end}"
            ),
        }
    }
}

/// Parser provenance attached to every bounded document result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentParserProvenance {
    /// The pinned `pdf-extract` parser with documented guest-local patches.
    PdfExtract,
    /// The pinned direct `quick-xml` parser after strict package admission.
    QuickXml,
}

impl fmt::Display for DocumentParserProvenance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PdfExtract => "pdf-extract-0.12.0+projectatlas",
            Self::QuickXml => "quick-xml-0.42.0",
        })
    }
}

/// One exact text fact retained from a supported document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentFact {
    /// Extracted UTF-8 text for this fact.
    pub text: String,
    /// Exact parser-relative locator for the text.
    pub locator: DocumentLocator,
    /// One-based first line in the emitted document text.
    pub line_start: usize,
    /// One-based last occupied line in the emitted document text.
    pub line_end: usize,
}

/// One rendered DOCX font glyph with exact source identity and optional verified text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSymbol {
    /// Font named by `w:font`, if supplied.
    pub font: Option<String>,
    /// Numeric `w:char` value, if supplied.
    pub code: Option<u16>,
    /// Exact package part, paragraph, run, and in-run occurrence span.
    pub locator: DocumentLocator,
    /// One-based line containing the glyph or unresolved placeholder.
    pub line: usize,
    /// Unicode text only when the font/code mapping is verified.
    pub unicode: Option<char>,
}

/// Complete bounded text and provenance extracted from one document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentFacts {
    /// Format admitted by magic and language/extension checks.
    pub format: DocumentFormat,
    /// Extracted text with preserved paragraph and run layout for the text index.
    pub text: String,
    /// Exact text facts used to build graph evidence.
    pub facts: Vec<DocumentFact>,
    /// Rendered font glyphs, including ones with unresolved Unicode text.
    pub symbols: Vec<DocumentSymbol>,
    /// Whether the supported part was fully examined.
    pub completeness: DocumentCompleteness,
    /// Parser provenance for audit and graph evidence.
    pub provenance: DocumentParserProvenance,
}

impl DocumentFacts {
    /// Project exact document facts into the existing sparse symbol graph.
    /// The admitted format, not a caller's optional language hint, owns the graph language.
    #[must_use]
    pub fn symbol_graph(&self, path: &str, _language: Option<&str>) -> SymbolGraph {
        let language = Some(self.format.language().to_owned());
        let mut symbols: Vec<CodeSymbol> = self
            .facts
            .iter()
            .enumerate()
            .map(|(index, fact)| CodeSymbol {
                path: path.to_owned(),
                language: language.clone(),
                name: format!("document-block-{}", index + 1),
                kind: SymbolKind::Value,
                signature: fact.locator.to_string(),
                exported: false,
                documentation: None,
                line_start: fact.line_start,
                line_end: fact.line_end,
                source_selector: None,
                parent: None,
                parser: ParserKind::Structural,
                detail: Some(format!(
                    "format={};provenance={};completeness={:?};text-bytes={}",
                    self.format,
                    self.provenance,
                    self.completeness,
                    fact.text.len()
                )),
            })
            .collect();
        symbols.extend(self.symbols.iter().enumerate().map(|(index, symbol)| {
            let code = symbol
                .code
                .map_or_else(|| "missing".to_owned(), |code| format!("{code:04X}"));
            CodeSymbol {
                path: path.to_owned(),
                language: language.clone(),
                name: format!("document-symbol-{}", index + 1),
                kind: SymbolKind::Value,
                signature: format!("{};font={:?};code={code}", symbol.locator, symbol.font),
                exported: false,
                documentation: None,
                line_start: symbol.line,
                line_end: symbol.line,
                source_selector: None,
                parent: None,
                parser: ParserKind::Structural,
                detail: Some(format!(
                    "unicode={:?};completeness={:?}",
                    symbol.unicode, self.completeness
                )),
            }
        }));
        if let DocumentCompleteness::Partial { gaps } = &self.completeness {
            symbols.push(CodeSymbol {
                path: path.to_owned(),
                language: language.clone(),
                name: DOCUMENT_COVERAGE_SYMBOL.to_owned(),
                kind: SymbolKind::Unknown,
                signature: gaps
                    .iter()
                    .map(|gap| gap.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Structural,
                detail: Some("document text coverage is incomplete".to_owned()),
            });
        }
        SymbolGraph {
            path: path.to_owned(),
            language,
            parser: ParserKind::Structural,
            symbols,
            relations: Vec::new(),
        }
    }
}

/// Resource ceiling applied by the document boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentLimit {
    /// Raw input bytes for PDF or compressed package bytes for DOCX.
    InputBytes,
    /// Total compressed ZIP member bytes.
    CompressedBytes,
    /// Total uncompressed ZIP member bytes.
    ExpandedBytes,
    /// Total repeated DOCX decompression work after safe package admission.
    ParserWorkBytes,
    /// Retained extracted UTF-8 bytes.
    OutputBytes,
    /// Source and parser staging envelope.
    MemoryBytes,
    /// Number of ZIP entries.
    EntryCount,
    /// Number of retained evidence facts.
    FactCount,
    /// Nested XML elements in a document part.
    NestingDepth,
    /// Interpreter work consumed by the contained PDF parser.
    ExecutionFuel,
}

impl fmt::Display for DocumentLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputBytes => "input_bytes",
            Self::CompressedBytes => "compressed_bytes",
            Self::ExpandedBytes => "expanded_bytes",
            Self::ParserWorkBytes => "parser_work_bytes",
            Self::OutputBytes => "output_bytes",
            Self::MemoryBytes => "memory_bytes",
            Self::EntryCount => "entry_count",
            Self::FactCount => "fact_count",
            Self::NestingDepth => "nesting_depth",
            Self::ExecutionFuel => "execution_fuel",
        })
    }
}

/// Typed failure at the PDF/DOCX trust boundary.
#[derive(Debug, Error)]
pub enum DocumentExtractionError {
    /// The file was not admitted as one of the supported formats.
    #[error("unsupported document format for language {language}")]
    UnsupportedFormat {
        /// Language requested by the caller.
        language: String,
    },
    /// The path/registry admission and file magic disagree.
    #[error("document magic does not match expected {expected} format; found {found}")]
    MismatchedMagic {
        /// Format expected from the path or language registry.
        expected: DocumentFormat,
        /// Format identified by the leading bytes.
        found: &'static str,
    },
    /// A document exceeded one explicit bounded resource.
    #[error("document exceeded {limit}: observed {observed}, limit {maximum}")]
    ResourceLimit {
        /// Resource that exceeded its ceiling.
        limit: DocumentLimit,
        /// First observed value beyond the ceiling.
        observed: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// An encrypted PDF was rejected because credentials are never accepted here.
    #[error("encrypted PDF documents are unsupported")]
    EncryptedPdf,
    /// PDF text semantics require a feature outside the admitted parser subset.
    #[error("unsupported PDF text semantics")]
    UnsupportedPdfInput,
    /// A malformed or unreadable parser input was rejected.
    #[error("malformed {format} document: {message}")]
    Malformed {
        /// Format whose parser reported the error.
        format: DocumentFormat,
        /// Bounded parser diagnostic.
        message: String,
    },
    /// ZIP package structure is unsafe or not a supported DOCX package.
    #[error("invalid DOCX package: {message}")]
    InvalidDocxPackage {
        /// Bounded package diagnostic.
        message: String,
    },
    /// DOCX input requires a document or package feature outside the admitted set.
    #[error("unsupported DOCX package input: {message}")]
    UnsupportedDocxInput {
        /// Bounded package diagnostic.
        message: String,
    },
    /// Shared indexing cancellation or deadline stopped extraction.
    #[error(transparent)]
    Work(#[from] IndexWorkFailure),
}

/// Honor a supplied language; infer from the extension only when no language is supplied.
#[must_use]
pub fn document_format_for_path(path: &str, language: Option<&str>) -> Option<DocumentFormat> {
    let language = language.or_else(|| Path::new(path).extension()?.to_str())?;
    match language.to_ascii_lowercase().as_str() {
        "pdf" => Some(DocumentFormat::Pdf),
        "docx" => Some(DocumentFormat::Docx),
        _ => None,
    }
}

/// Extract bounded text and exact facts for one admitted document.
///
/// # Errors
///
/// Returns a typed admission, parser, resource, cancellation, or deadline
/// failure without returning a partial result.
pub fn extract_document_text_controlled(
    bytes: &[u8],
    path: &str,
    language: Option<&str>,
    control: &IndexWorkControl,
) -> Result<DocumentFacts, DocumentExtractionError> {
    extract_document_controlled_with_stage(
        bytes,
        path,
        language,
        control,
        IndexWorkStage::TextIndex,
    )
}

/// Extract a sparse graph with exact document locator evidence.
///
/// # Errors
///
/// Returns a typed admission, parser, resource, cancellation, or deadline
/// failure without returning a partial graph.
pub fn extract_document_graph_controlled(
    bytes: &[u8],
    path: &str,
    language: Option<&str>,
    control: &IndexWorkControl,
) -> Result<SymbolGraph, DocumentExtractionError> {
    Ok(
        extract_document_symbol_facts_controlled(bytes, path, language, control)?
            .symbol_graph(path, language),
    )
}

/// Extract document text and exact facts for symbol publication and content summaries.
///
/// # Errors
///
/// Returns a typed admission, parser, resource, cancellation, or deadline failure
/// without returning partial facts.
pub fn extract_document_symbol_facts_controlled(
    bytes: &[u8],
    path: &str,
    language: Option<&str>,
    control: &IndexWorkControl,
) -> Result<DocumentFacts, DocumentExtractionError> {
    extract_document_controlled_with_stage(
        bytes,
        path,
        language,
        control,
        IndexWorkStage::SymbolParsing,
    )
}

/// Extract one document while applying the caller-selected work stage.
fn extract_document_controlled_with_stage(
    bytes: &[u8],
    path: &str,
    language: Option<&str>,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    control.check(stage)?;
    if bytes.len() > MAX_DOCUMENT_COMPRESSED_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::InputBytes,
            observed: bytes.len(),
            maximum: MAX_DOCUMENT_COMPRESSED_BYTES,
        });
    }
    let format = document_format_for_path(path, language).ok_or_else(|| {
        DocumentExtractionError::UnsupportedFormat {
            language: language.unwrap_or("unknown").to_owned(),
        }
    })?;
    let _execution = lock_document_execution(control, stage)?;
    let facts = match format {
        DocumentFormat::Pdf => extract_pdf(bytes, control, stage)?,
        DocumentFormat::Docx => extract_docx(bytes, control, stage)?,
    };
    control.check(stage)?;
    Ok(facts)
}

/// Extract PDF page text after magic, encryption, page-content, and memory checks.
fn extract_pdf(
    bytes: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(DocumentExtractionError::MismatchedMagic {
            expected: DocumentFormat::Pdf,
            found: if bytes.starts_with(b"PK\x03\x04") {
                "docx"
            } else {
                "unknown"
            },
        });
    }
    let pages = pdf_runtime::extract_pages(bytes, control, stage)?;
    let mut text = String::new();
    let mut facts = Vec::new();
    for (page_number, page) in &pages {
        control.check(stage)?;
        let mut page_offset = 0usize;
        for line in page.split('\n') {
            let source_start = page_offset;
            page_offset = page_offset.saturating_add(line.len().saturating_add(1));
            if line.trim().is_empty() {
                continue;
            }
            if facts.len() >= MAX_DOCUMENT_FACTS {
                return Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::FactCount,
                    observed: facts.len().saturating_add(1),
                    maximum: MAX_DOCUMENT_FACTS,
                });
            }
            let required = text
                .len()
                .saturating_add(usize::from(!text.is_empty()))
                .saturating_add(line.len());
            if required > MAX_DOCUMENT_OUTPUT_BYTES {
                return Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::OutputBytes,
                    observed: required,
                    maximum: MAX_DOCUMENT_OUTPUT_BYTES,
                });
            }
            if !text.is_empty() {
                push_output_byte(&mut text, b'\n')?;
            }
            text.push_str(line);
            let end = text.len();
            facts.push(DocumentFact {
                line_start: facts.len() + 1,
                line_end: facts.len() + 1,
                text: line.to_owned(),
                locator: DocumentLocator::Pdf {
                    page: usize::try_from(*page_number).map_err(|_error| {
                        DocumentExtractionError::Malformed {
                            format: DocumentFormat::Pdf,
                            message: "PDF page number exceeds host range".to_owned(),
                        }
                    })?,
                    text_start: source_start,
                    text_end: source_start.saturating_add(line.len()),
                },
            });
            debug_assert!(end <= MAX_DOCUMENT_OUTPUT_BYTES);
        }
    }
    Ok(DocumentFacts {
        format: DocumentFormat::Pdf,
        text,
        facts,
        symbols: Vec::new(),
        completeness: DocumentCompleteness::Complete,
        provenance: DocumentParserProvenance::PdfExtract,
    })
}

/// Append one ASCII separator while enforcing the retained-output ceiling.
fn push_output_byte(text: &mut String, byte: u8) -> Result<(), DocumentExtractionError> {
    if text.len().saturating_add(1) > MAX_DOCUMENT_OUTPUT_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::OutputBytes,
            observed: text.len().saturating_add(1),
            maximum: MAX_DOCUMENT_OUTPUT_BYTES,
        });
    }
    text.push(char::from(byte));
    Ok(())
}

/// Enforce the source and parser-staging memory envelope.
fn check_memory_budget(observed: usize) -> Result<(), DocumentExtractionError> {
    if observed > MAX_DOCUMENT_MEMORY_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::MemoryBytes,
            observed,
            maximum: MAX_DOCUMENT_MEMORY_BYTES,
        });
    }
    Ok(())
}

/// Validate a DOCX ZIP and parse only its declared document part with quick-xml.
fn extract_docx(
    bytes: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(DocumentExtractionError::MismatchedMagic {
            expected: DocumentFormat::Docx,
            found: if bytes.starts_with(b"%PDF-") {
                "pdf"
            } else {
                "unknown"
            },
        });
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        DocumentExtractionError::InvalidDocxPackage {
            message: error.to_string(),
        }
    })?;
    if archive.len() > MAX_DOCUMENT_ENTRIES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::EntryCount,
            observed: archive.len(),
            maximum: MAX_DOCUMENT_ENTRIES,
        });
    }
    let mut compressed_bytes = 0usize;
    let mut expanded_bytes = 0usize;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        check_parser_iteration(index, &mut || control.check(stage))?;
        let entry = archive.by_index_raw(index).map_err(|error| {
            DocumentExtractionError::InvalidDocxPackage {
                message: error.to_string(),
            }
        })?;
        if std::str::from_utf8(entry.name_raw()).is_err() {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX package part metadata is not UTF-8".to_owned(),
            });
        }
        let name = entry.name().to_owned();
        if name.len() > MAX_DOCX_PART_NAME_BYTES {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("DOCX package part name exceeds {MAX_DOCX_PART_NAME_BYTES} bytes"),
            });
        }
        let enclosed = entry.enclosed_name().is_some();
        let compression = entry.compression();
        let compressed_size = entry.compressed_size();
        let expanded_size = entry.size();
        if !matches!(
            compression,
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(DocumentExtractionError::UnsupportedDocxInput {
                message: format!("unsupported compression for package part {name}"),
            });
        }
        drop(entry);
        archive.by_index(index).map_err(|error| match error {
            zip::result::ZipError::UnsupportedArchive(message) => {
                DocumentExtractionError::UnsupportedDocxInput {
                    message: message.to_owned(),
                }
            }
            error => DocumentExtractionError::InvalidDocxPackage {
                message: error.to_string(),
            },
        })?;
        if !enclosed || name.contains('\\') || name.starts_with('/') || !names.insert(name.clone())
        {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("unsafe or duplicate package part {name}"),
            });
        }
        if name.ends_with('/') {
            continue;
        }
        let compressed = usize::try_from(compressed_size).map_err(|_error| {
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::CompressedBytes,
                observed: usize::MAX,
                maximum: MAX_DOCUMENT_COMPRESSED_BYTES,
            }
        })?;
        let expanded = usize::try_from(expanded_size).map_err(|_error| {
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ExpandedBytes,
                observed: usize::MAX,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES,
            }
        })?;
        compressed_bytes = compressed_bytes.saturating_add(compressed);
        expanded_bytes = expanded_bytes.saturating_add(expanded);
        if compressed_bytes > MAX_DOCUMENT_COMPRESSED_BYTES {
            return Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::CompressedBytes,
                observed: compressed_bytes,
                maximum: MAX_DOCUMENT_COMPRESSED_BYTES,
            });
        }
        if expanded_bytes > MAX_DOCUMENT_EXPANDED_BYTES {
            return Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ExpandedBytes,
                observed: expanded_bytes,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES,
            });
        }
    }
    let mut read_budget = DocxReadBudget::default();
    if !names.contains("[Content_Types].xml") || !names.contains("_rels/.rels") {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX package is missing content types or root relationships".to_owned(),
        });
    }
    let content_types = if names.contains("[Content_Types].xml") {
        let manifest = read_docx_package_part(
            &mut archive,
            "[Content_Types].xml",
            &mut read_budget,
            control,
            stage,
        )?;
        Some(parse_docx_content_types(&manifest, control, stage)?)
    } else {
        None
    };
    let main_part = if names.contains("_rels/.rels") {
        let package_rels = read_docx_package_part(
            &mut archive,
            "_rels/.rels",
            &mut read_budget,
            control,
            stage,
        )?;
        let relationships = parse_docx_relationships(&package_rels, control, stage)?;
        let mut main = relationships
            .values()
            .filter(|(kind, _, _)| kind == "officeDocument");
        let Some((_, target, external)) = main.next() else {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX package has no unique internal main-document relationship"
                    .to_owned(),
            });
        };
        if main.next().is_some() {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX package has no unique internal main-document relationship"
                    .to_owned(),
            });
        }
        if *external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX main-document relationship is external".to_owned(),
            });
        }
        resolve_docx_story_target(target, "", &names)?
    } else {
        DOCX_DOCUMENT_PART.to_owned()
    };
    if let Some(types) = &content_types {
        validate_docx_content_type(&main_part, "document.main", types)?;
    }
    let xml =
        match read_docx_package_part(&mut archive, &main_part, &mut read_budget, control, stage) {
            Ok(xml) => xml,
            Err(error) => {
                if let Some(limit) = accepted_docx_limit(&error) {
                    return Ok(empty_partial_docx(limit));
                }
                return Err(error);
            }
        };
    control.check(stage)?;
    // Bound live source, one expanded story, parser staging, retained text and facts.
    let memory_check = check_docx_story_memory(bytes.len(), xml.capacity(), 0);
    if let Err(error) = memory_check {
        if let Some(limit) = accepted_docx_limit(&error) {
            return Ok(empty_partial_docx(limit));
        }
        return Err(error);
    }
    let mut references = DocxStoryReferences::default();
    let mut document = match parse_docx_part(
        &xml,
        &main_part,
        "document",
        None,
        None,
        &mut references,
        control,
        stage,
    ) {
        Ok(document) => document,
        Err(error) => {
            if let Some(limit) = accepted_docx_limit(&error) {
                return Ok(empty_partial_docx(limit));
            }
            return Err(error);
        }
    };
    drop(xml);
    let settings = match docx_settings(
        &mut archive,
        &main_part,
        &names,
        content_types.as_ref(),
        &mut read_budget,
        control,
        stage,
    ) {
        Ok(enabled) => enabled,
        Err(error) => {
            if let Some(limit) = accepted_docx_limit(&error) {
                mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                return Ok(document);
            }
            return Err(error);
        }
    };
    if settings.unexamined_compatibility {
        mark_docx_gap(&mut document, DocumentCoverageGap::UnexaminedStory);
    }
    if settings.even_odd_headers && references.has_parity_variant() {
        mark_docx_gap(&mut document, DocumentCoverageGap::ConditionalStory);
    }
    let mut total_references = references.count();
    let mut document_newlines = document.text.bytes().filter(|byte| *byte == b'\n').count();
    let main_relationship_part = docx_relationship_part(&main_part);
    let main_relationships = if names.contains(&main_relationship_part) {
        let xml = read_docx_package_part(
            &mut archive,
            &main_relationship_part,
            &mut read_budget,
            control,
            stage,
        )?;
        Some(parse_docx_relationships(&xml, control, stage)?)
    } else {
        None
    };
    if main_relationships
        .as_ref()
        .is_some_and(|relationships| relationships.values().any(|(kind, _, _)| kind == "styles"))
    {
        mark_docx_gap(&mut document, DocumentCoverageGap::UnresolvedVisibility);
    }
    // Drain linked stories first so their note/comment IDs are batched before item-part inflation.
    let mut selected: BTreeMap<(u8, String), (DocxStoryKind, HashSet<String>)> = BTreeMap::new();
    if let Err(error) = queue_docx_story_references(
        &mut archive,
        &main_part,
        &references,
        &names,
        content_types.as_ref(),
        &main_part,
        main_relationships.as_ref(),
        settings.even_odd_headers,
        &mut read_budget,
        &mut selected,
        control,
        stage,
    ) {
        if let Some(limit) = accepted_docx_limit(&error) {
            mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
            return Ok(document);
        }
        return Err(error);
    }
    let mut parsed: HashMap<String, (DocxStoryKind, HashSet<String>)> = HashMap::new();
    while let Some(((_, part), (kind, ids))) = selected.pop_first() {
        control.check(stage)?;
        let previous = parsed
            .entry(part.clone())
            .or_insert_with(|| (kind, HashSet::new()));
        if previous.0 != kind {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX story part has conflicting relationship types".to_owned(),
            });
        }
        let previous = &mut previous.1;
        let special_ids = if previous.is_empty() {
            match kind {
                DocxStoryKind::Footnotes => Some(&settings.footnote_special_ids),
                DocxStoryKind::Endnotes => Some(&settings.endnote_special_ids),
                _ => None,
            }
        } else {
            None
        };
        let pending_ids = if kind.item_name().is_some() {
            ids.difference(previous).cloned().collect::<HashSet<_>>()
        } else if previous.is_empty() {
            HashSet::new()
        } else {
            continue;
        };
        if kind.item_name().is_some() && pending_ids.is_empty() {
            continue;
        }
        if kind.item_name().is_some() {
            previous.extend(pending_ids.iter().cloned());
        } else {
            previous.insert(String::new());
        }
        let story_xml =
            match read_docx_package_part(&mut archive, &part, &mut read_budget, control, stage) {
                Ok(xml) => xml,
                Err(error) => {
                    if let Some(limit) = accepted_docx_limit(&error) {
                        mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                        break;
                    }
                    return Err(error);
                }
            };
        if let Err(error) =
            check_docx_story_memory(bytes.len(), story_xml.capacity(), document.text.capacity())
        {
            if let Some(limit) = accepted_docx_limit(&error) {
                mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                break;
            }
            return Err(error);
        }
        let mut child_references = DocxStoryReferences::default();
        let story = match parse_docx_part(
            &story_xml,
            &part,
            kind.root_name(),
            kind.item_name().map(|_| &pending_ids),
            special_ids,
            &mut child_references,
            control,
            stage,
        ) {
            Ok(story) => story,
            Err(error) => {
                if let Some(limit) = accepted_docx_limit(&error) {
                    mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                    break;
                }
                return Err(error);
            }
        };
        if let Err(error) = append_docx_story(&mut document, story, &mut document_newlines) {
            if let Some(limit) = accepted_docx_limit(&error) {
                mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                break;
            }
            return Err(error);
        }
        total_references = total_references.saturating_add(child_references.count());
        if total_references > MAX_DOCUMENT_FACTS {
            mark_docx_gap(
                &mut document,
                DocumentCoverageGap::ResourceLimit(DocumentLimit::FactCount),
            );
            break;
        }
        if settings.even_odd_headers && child_references.has_parity_variant() {
            mark_docx_gap(&mut document, DocumentCoverageGap::ConditionalStory);
        }
        if let Err(error) = queue_docx_story_references(
            &mut archive,
            &part,
            &child_references,
            &names,
            content_types.as_ref(),
            &main_part,
            main_relationships.as_ref(),
            settings.even_odd_headers,
            &mut read_budget,
            &mut selected,
            control,
            stage,
        ) {
            if let Some(limit) = accepted_docx_limit(&error) {
                mark_docx_gap(&mut document, DocumentCoverageGap::ResourceLimit(limit));
                break;
            }
            return Err(error);
        }
    }
    Ok(document)
}

/// Classify safe post-admission parser ceilings that may publish local incomplete coverage.
fn accepted_docx_limit(error: &DocumentExtractionError) -> Option<DocumentLimit> {
    match error {
        DocumentExtractionError::ResourceLimit { limit, .. }
            if matches!(
                limit,
                DocumentLimit::OutputBytes
                    | DocumentLimit::FactCount
                    | DocumentLimit::MemoryBytes
                    | DocumentLimit::ParserWorkBytes
            ) =>
        {
            Some(*limit)
        }
        _ => None,
    }
}

/// Charge one live story, parser staging, and retained output to the document envelope.
fn check_docx_story_memory(
    source_bytes: usize,
    story_capacity: usize,
    retained_text_capacity: usize,
) -> Result<(), DocumentExtractionError> {
    check_memory_budget(
        source_bytes
            .saturating_add(story_capacity.saturating_mul(4))
            .saturating_add(retained_text_capacity.saturating_mul(2))
            .saturating_add(MAX_DOCUMENT_ENTRIES.saturating_mul(MAX_DOCX_PART_NAME_BYTES))
            .saturating_add(MAX_DOCUMENT_FACTS.saturating_mul(MAX_DOCX_PART_NAME_BYTES * 2))
            .saturating_add(MAX_DOCUMENT_FACTS.saturating_mul(MAX_DOCX_REFERENCE_ID_BYTES * 4))
            .saturating_add(MAX_DOCX_METADATA_BYTES.saturating_mul(8))
            .saturating_add(MAX_DOCX_IGNORABLE_NAMESPACES * std::mem::size_of::<String>())
            .saturating_add(MAX_DOCUMENT_OUTPUT_BYTES.saturating_mul(4))
            .saturating_add(MAX_DOCX_XML_DEPTH.saturating_mul(
                std::mem::size_of::<(DocxTextContext, usize)>()
                    + MAX_DOCX_XML_DEPTH * std::mem::size_of::<DocxFieldPhase>()
                    + std::mem::size_of::<DocxSimpleField>()
                    + std::mem::size_of::<DocxSdtContext>()
                    + std::mem::size_of::<DocxSdtTag>()
                    + std::mem::size_of::<DocxAlternative>(),
            ))
            .saturating_add(
                MAX_DOCUMENT_FACTS
                    .saturating_mul(std::mem::size_of::<DocumentFact>())
                    .saturating_mul(2),
            ),
    )
}

/// Preserve a typed coverage fact when no main-story evidence fits the retained ceiling.
fn empty_partial_docx(limit: DocumentLimit) -> DocumentFacts {
    DocumentFacts {
        format: DocumentFormat::Docx,
        text: String::new(),
        facts: Vec::new(),
        symbols: Vec::new(),
        completeness: DocumentCompleteness::Partial {
            gaps: vec![DocumentCoverageGap::ResourceLimit(limit)],
        },
        provenance: DocumentParserProvenance::QuickXml,
    }
}

/// Add one distinct reason without discarding already verified text or symbols.
fn mark_docx_gap(document: &mut DocumentFacts, gap: DocumentCoverageGap) {
    if let DocumentCompleteness::Complete = document.completeness {
        document.completeness = DocumentCompleteness::Partial { gaps: Vec::new() };
    }
    if let DocumentCompleteness::Partial { gaps } = &mut document.completeness
        && !gaps.contains(&gap)
    {
        gaps.push(gap);
    }
}

/// OPC relationship-part name adjacent to its source part.
fn docx_relationship_part(source_part: &str) -> String {
    match source_part.rsplit_once('/') {
        Some((directory, filename)) => format!("{directory}/_rels/{filename}.rels"),
        None => format!("_rels/{source_part}.rels"),
    }
}

#[allow(clippy::too_many_arguments)]
/// Resolve story links from their source part and note items from the main part.
fn queue_docx_story_references(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    origin_part: &str,
    references: &DocxStoryReferences,
    names: &HashSet<String>,
    content_types: Option<&DocxContentTypes>,
    main_part: &str,
    main_relationships: Option<&HashMap<String, (String, String, bool)>>,
    even_odd_headers: bool,
    read_budget: &mut DocxReadBudget,
    selected: &mut BTreeMap<(u8, String), (DocxStoryKind, HashSet<String>)>,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<(), DocumentExtractionError> {
    let has_story_links = !references.linked_parts.is_empty()
        || !references.imported_parts.is_empty()
        || !references.content_parts.is_empty()
        || (even_odd_headers && !references.even_parts.is_empty());
    if !has_story_links && references.items.is_empty() && !references.glossary_placeholder {
        return Ok(());
    }
    let relationship_origin = if has_story_links {
        origin_part
    } else {
        main_part
    };
    let relationship_part = docx_relationship_part(relationship_origin);
    let source_relationships = if relationship_origin == main_part {
        None
    } else {
        let xml = read_docx_package_part(archive, &relationship_part, read_budget, control, stage)?;
        Some(parse_docx_relationships(&xml, control, stage)?)
    };
    let missing_main = || DocumentExtractionError::InvalidDocxPackage {
        message: format!(
            "required part {} is missing",
            docx_relationship_part(main_part)
        ),
    };
    let relationships = source_relationships
        .as_ref()
        .or(main_relationships)
        .ok_or_else(missing_main)?;
    let item_relationships = if references.items.is_empty() && !references.glossary_placeholder {
        relationships
    } else {
        main_relationships.ok_or_else(missing_main)?
    };
    let mut relationship_by_kind = HashMap::new();
    for (index, (relationship_kind, target, external)) in item_relationships.values().enumerate() {
        check_parser_iteration(index, &mut || control.check(stage))?;
        relationship_by_kind
            .entry(relationship_kind.as_str())
            .and_modify(|unique| *unique = None)
            .or_insert(Some((target.as_str(), *external)));
    }
    for (kind, id) in references
        .linked_parts
        .iter()
        .chain(references.even_parts.iter().filter(|_| even_odd_headers))
    {
        let (relationship_kind, target, external) =
            relationships
                .get(id)
                .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                    message: format!("referenced DOCX relationship {id} is missing"),
                })?;
        if relationship_kind != kind.relationship_name() || *external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("DOCX relationship {id} has an unsafe or wrong story type"),
            });
        }
        let part = resolve_docx_story_target(target, origin_part, names)?;
        if let Some(types) = content_types {
            validate_docx_content_type(&part, kind.content_type(), types)?;
        }
        if *kind == DocxStoryKind::Subdocument {
            continue;
        }
        let story = selected
            .entry((u8::from(kind.item_name().is_some()), part))
            .or_insert_with(|| (*kind, HashSet::new()));
        if story.0 != *kind {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX story part has conflicting relationship types".to_owned(),
            });
        }
    }
    for id in &references.imported_parts {
        let (kind, target, external) =
            relationships
                .get(id)
                .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                    message: format!("referenced DOCX import relationship {id} is missing"),
                })?;
        if !matches!(kind.as_str(), "aFChunk" | "afChunk") || *external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("DOCX import relationship {id} has an unsafe or wrong type"),
            });
        }
        resolve_docx_story_target(target, origin_part, names)?;
    }
    for id in &references.content_parts {
        let (kind, target, external) =
            relationships
                .get(id)
                .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                    message: format!("referenced DOCX content part relationship {id} is missing"),
                })?;
        if kind != "customXml" || *external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("DOCX content part relationship {id} is unsafe or wrong type"),
            });
        }
        resolve_docx_story_target(target, origin_part, names)?;
    }
    for (kind, id) in &references.items {
        let (target, external) = match relationship_by_kind.get(kind.relationship_name()) {
            None => {
                return Err(DocumentExtractionError::InvalidDocxPackage {
                    message: format!(
                        "referenced DOCX {} part is missing",
                        kind.relationship_name()
                    ),
                });
            }
            Some(None) => {
                return Err(DocumentExtractionError::InvalidDocxPackage {
                    message: format!(
                        "DOCX {} relationship is ambiguous",
                        kind.relationship_name()
                    ),
                });
            }
            Some(Some(relationship)) => *relationship,
        };
        if external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("DOCX {} relationship is external", kind.relationship_name()),
            });
        }
        let part = resolve_docx_story_target(target, main_part, names)?;
        if let Some(types) = content_types {
            validate_docx_content_type(&part, kind.content_type(), types)?;
        }
        let story = selected
            .entry((1, part))
            .or_insert_with(|| (*kind, HashSet::new()));
        if story.0 != *kind {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "DOCX story part has conflicting relationship types".to_owned(),
            });
        }
        story.1.insert(id.clone());
    }
    if references.glossary_placeholder {
        let mut glossary = item_relationships
            .values()
            .filter(|(kind, _, _)| kind == "glossaryDocument");
        let (_, target, external) =
            glossary
                .next()
                .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                    message: "referenced DOCX glossary relationship is missing".to_owned(),
                })?;
        if glossary.next().is_some() {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "referenced DOCX glossary relationship is ambiguous".to_owned(),
            });
        }
        if *external {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: "referenced DOCX glossary relationship is external".to_owned(),
            });
        }
        let part = resolve_docx_story_target(target, main_part, names)?;
        if let Some(types) = content_types {
            validate_docx_content_type(&part, "document.glossary", types)?;
        }
    }
    Ok(())
}

/// Bounded document-wide settings that affect reachable story content.
#[derive(Default)]
struct DocxSettings {
    /// Whether even-page header and footer variants are active.
    even_odd_headers: bool,
    /// Special footnote IDs loaded by the document.
    footnote_special_ids: HashSet<String>,
    /// Special endnote IDs loaded by the document.
    endnote_special_ids: HashSet<String>,
    /// A compatibility branch in settings may select unexamined rendered stories.
    unexamined_compatibility: bool,
}

/// Read validated document-wide settings once before linked-story extraction.
fn docx_settings(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    main_part: &str,
    names: &HashSet<String>,
    content_types: Option<&DocxContentTypes>,
    read_budget: &mut DocxReadBudget,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocxSettings, DocumentExtractionError> {
    let relationship_part = docx_relationship_part(main_part);
    if !names.contains(&relationship_part) {
        return Ok(DocxSettings::default());
    }
    let rel_xml = read_docx_package_part(archive, &relationship_part, read_budget, control, stage)?;
    let relationships = parse_docx_relationships(&rel_xml, control, stage)?;
    let mut settings_links = relationships
        .values()
        .filter(|(kind, _, _)| kind == "settings");
    let Some((_, target, external)) = settings_links.next() else {
        return Ok(DocxSettings::default());
    };
    if settings_links.next().is_some() || *external {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX settings relationship is ambiguous or external".to_owned(),
        });
    }
    let part = resolve_docx_story_target(target, main_part, names)?;
    if let Some(types) = content_types {
        validate_docx_content_type(&part, "settings", types)?;
    }
    let xml = read_docx_package_part(archive, &part, read_budget, control, stage)?;
    if xml.len() > MAX_DOCX_METADATA_BYTES {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX settings exceed the package metadata bound".to_owned(),
        });
    }
    let mut reader = NsReader::from_reader(xml.as_slice());
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut seen_root = false;
    let mut seen_switch = false;
    let mut seen_footnote_properties = false;
    let mut seen_endnote_properties = false;
    let mut settings = DocxSettings::default();
    let mut note_kind = None;
    let mut alternatives: Vec<DocxAlternative> = Vec::new();
    let mut selected_branches: Vec<usize> = Vec::new();
    let mut skipped_branch_depth = None;
    let mut event_index = 0usize;
    loop {
        check_parser_iteration(event_index, &mut || control.check(stage))?;
        event_index = event_index.saturating_add(1);
        let (namespace, event) =
            reader
                .read_resolved_event()
                .map_err(|error| DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                })?;
        let wordprocessing = match &namespace {
            ResolveResult::Bound(namespace) => wordprocessing_namespace(namespace.as_ref()),
            ResolveResult::Unbound => false,
            ResolveResult::Unknown(prefix) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: format!("DOCX XML contained an undeclared namespace prefix: {prefix}"),
                });
            }
        };
        let compatibility = matches!(&namespace, ResolveResult::Bound(namespace)
            if namespace.as_ref() == "http://schemas.openxmlformats.org/markup-compatibility/2006");
        if matches!(event, Event::Decl(_)) && event_index != 1 {
            return Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: "DOCX XML declaration must be the first XML event".to_owned(),
            });
        }
        match event {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_DOCX_XML_DEPTH {
                    return Err(DocumentExtractionError::ResourceLimit {
                        limit: DocumentLimit::NestingDepth,
                        observed: depth,
                        maximum: MAX_DOCX_XML_DEPTH,
                    });
                }
                if skipped_branch_depth.is_some() {
                    continue;
                }
                for (index, attribute) in event.attributes().enumerate() {
                    check_parser_iteration(index, &mut || control.check(stage))?;
                    let attribute =
                        attribute.map_err(|error| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: error.to_string(),
                        })?;
                    if let (ResolveResult::Unknown(prefix), _) =
                        reader.resolver().resolve_attribute(attribute.key)
                    {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: format!(
                                "DOCX XML attribute has an undeclared namespace prefix: {prefix}"
                            ),
                        });
                    }
                }
                let name = event.local_name();
                if depth == 1 && (seen_root || !wordprocessing || name.as_ref() != "settings") {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX settings have an invalid root".to_owned(),
                    });
                }
                if compatibility && matches!(name.as_ref(), "Choice" | "Fallback") {
                    let alternative = alternatives
                        .last_mut()
                        .filter(|item| item.depth + 1 == depth && !item.fallback_seen)
                        .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX settings compatibility branch has invalid placement"
                                .to_owned(),
                        })?;
                    let supported = if name.as_ref() == "Choice" {
                        alternative.choice_seen = true;
                        docx_choice_supported(&reader, &event, control, stage)?
                    } else {
                        if !alternative.choice_seen {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX settings compatibility fallback requires a choice"
                                    .to_owned(),
                            });
                        }
                        alternative.fallback_seen = true;
                        true
                    };
                    if !supported && alternative.selection == DocxAlternativeSelection::Unselected {
                        settings.unexamined_compatibility = true;
                        alternative.selection = DocxAlternativeSelection::Uncertain;
                    }
                    if alternative.selection == DocxAlternativeSelection::Unselected && supported {
                        alternative.selection = DocxAlternativeSelection::Selected;
                        selected_branches.push(depth);
                    } else {
                        skipped_branch_depth = Some(depth);
                    }
                    continue;
                }
                if alternatives
                    .last()
                    .is_some_and(|item| item.depth + 1 == depth)
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX settings alternatives must contain choices or fallback"
                            .to_owned(),
                    });
                }
                if compatibility && name.as_ref() == "AlternateContent" {
                    alternatives.push(DocxAlternative {
                        depth,
                        selection: DocxAlternativeSelection::Unselected,
                        choice_seen: false,
                        fallback_seen: false,
                    });
                    continue;
                }
                let logical_parent = depth - 1 - 2 * selected_branches.len();
                if logical_parent == 0 {
                    seen_root = true;
                } else if logical_parent == 1
                    && wordprocessing
                    && event.local_name().as_ref() == "evenAndOddHeaders"
                {
                    if seen_switch {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX even/odd header setting is duplicated".to_owned(),
                        });
                    }
                    seen_switch = true;
                    settings.even_odd_headers = docx_on_off(&event, &reader)?;
                } else if logical_parent == 1 && wordprocessing {
                    note_kind = match event.local_name().as_ref() {
                        "footnotePr" if !seen_footnote_properties => {
                            seen_footnote_properties = true;
                            Some(DocxStoryKind::Footnotes)
                        }
                        "endnotePr" if !seen_endnote_properties => {
                            seen_endnote_properties = true;
                            Some(DocxStoryKind::Endnotes)
                        }
                        "footnotePr" | "endnotePr" => {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX document-wide note properties are duplicated"
                                    .to_owned(),
                            });
                        }
                        _ => None,
                    };
                } else if logical_parent == 2
                    && wordprocessing
                    && let Some(kind) = note_kind
                    && event.local_name().as_ref()
                        == if kind == DocxStoryKind::Footnotes {
                            "footnote"
                        } else {
                            "endnote"
                        }
                {
                    let id = docx_attribute(&event, &reader, "id", wordprocessing_namespace)?
                        .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX special note setting is missing its id".to_owned(),
                        })?;
                    let id = canonical_docx_story_id(&id)?;
                    let inserted = match kind {
                        DocxStoryKind::Footnotes => settings.footnote_special_ids.insert(id),
                        DocxStoryKind::Endnotes => settings.endnote_special_ids.insert(id),
                        _ => unreachable!(),
                    };
                    if inserted {
                        let observed = settings
                            .footnote_special_ids
                            .len()
                            .saturating_add(settings.endnote_special_ids.len());
                        if observed > MAX_DOCUMENT_FACTS {
                            return Err(DocumentExtractionError::ResourceLimit {
                                limit: DocumentLimit::FactCount,
                                observed,
                                maximum: MAX_DOCUMENT_FACTS,
                            });
                        }
                    }
                }
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX settings have an unmatched closing element".to_owned(),
                    });
                }
                if let Some(skipped) = skipped_branch_depth {
                    if depth == skipped {
                        skipped_branch_depth = None;
                    }
                    depth -= 1;
                    continue;
                }
                if compatibility
                    && event.local_name().as_ref() == "AlternateContent"
                    && alternatives
                        .pop()
                        .is_none_or(|item| item.depth != depth || !item.choice_seen)
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX settings alternatives require a choice".to_owned(),
                    });
                }
                if compatibility
                    && matches!(event.local_name().as_ref(), "Choice" | "Fallback")
                    && selected_branches.last() == Some(&depth)
                {
                    selected_branches.pop();
                }
                if wordprocessing
                    && depth - 2 * selected_branches.len() == 2
                    && matches!(event.local_name().as_ref(), "footnotePr" | "endnotePr")
                {
                    note_kind = None;
                }
                depth -= 1;
            }
            Event::DocType(_) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: "DOCX settings DOCTYPE is unsupported".to_owned(),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_root
        || depth != 0
        || !alternatives.is_empty()
        || !selected_branches.is_empty()
        || skipped_branch_depth.is_some()
    {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX settings ended before the root closed".to_owned(),
        });
    }
    Ok(settings)
}

#[derive(Default)]
/// Bound actual decompression work, including repeated reads of one package part.
struct DocxReadBudget {
    /// Distinct ZIP-member expansion used for fatal package-safety admission.
    unique_expanded: usize,
    /// Total actual inflation work, including repeat reads.
    work_expanded: usize,
    /// Parts already charged to unique ZIP expansion.
    read_parts: HashSet<String>,
}

/// Bounded OPC content types, with exact part overrides taking precedence.
#[derive(Default)]
struct DocxContentTypes {
    /// Types assigned to individual package parts.
    overrides: HashMap<String, String>,
    /// Types assigned by case-insensitive filename extension.
    defaults: HashMap<String, String>,
}

/// Read one previously admitted ZIP member and verify its actual expansion.
fn read_docx_package_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    read_budget: &mut DocxReadBudget,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<Vec<u8>, DocumentExtractionError> {
    let mut entry =
        archive
            .by_name(name)
            .map_err(|_error| DocumentExtractionError::InvalidDocxPackage {
                message: format!("required part {name} is missing"),
            })?;
    let mut xml = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        control.check(stage)?;
        let read = entry.read(&mut chunk).map_err(|error| {
            DocumentExtractionError::InvalidDocxPackage {
                message: error.to_string(),
            }
        })?;
        if read == 0 {
            break;
        }
        let observed = read_budget
            .unique_expanded
            .saturating_add(xml.len())
            .saturating_add(read);
        if !read_budget.read_parts.contains(name) && observed > MAX_DOCUMENT_EXPANDED_BYTES {
            return Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ExpandedBytes,
                observed,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES,
            });
        }
        let work = read_budget
            .work_expanded
            .saturating_add(xml.len())
            .saturating_add(read);
        if work > MAX_DOCX_DECOMPRESSION_WORK_BYTES {
            return Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ParserWorkBytes,
                observed: work,
                maximum: MAX_DOCX_DECOMPRESSION_WORK_BYTES,
            });
        }
        xml.extend_from_slice(&chunk[..read]);
    }
    if read_budget.read_parts.insert(name.to_owned()) {
        read_budget.unique_expanded = read_budget.unique_expanded.saturating_add(xml.len());
    }
    read_budget.work_expanded = read_budget.work_expanded.saturating_add(xml.len());
    decode_docx_xml(xml, control, stage)
}

/// Normalize bounded UTF-16 package XML before the UTF-8 XML reader sees it.
fn decode_docx_xml(
    xml: Vec<u8>,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<Vec<u8>, DocumentExtractionError> {
    let encoding = if xml.starts_with(&[0xff, 0xfe]) {
        Some((true, 2))
    } else if xml.starts_with(&[0xfe, 0xff]) {
        Some((false, 2))
    } else if xml.len() >= 4 && xml[0] == b'<' && xml[1] == 0 && xml[3] == 0 {
        Some((true, 0))
    } else if xml.len() >= 4 && xml[0] == 0 && xml[1] == b'<' && xml[2] == 0 {
        Some((false, 0))
    } else {
        None
    };
    let Some((little_endian, bom_bytes)) = encoding else {
        validate_docx_xml_encoding(&xml, None)?;
        return Ok(xml);
    };
    let (chunks, remainder) = xml[bom_bytes..].as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX UTF-16 XML has an incomplete code unit".to_owned(),
        });
    }
    let units = || {
        chunks.iter().map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
    };
    let decoded_characters = || {
        char::decode_utf16(units()).map(|character| {
            character.map_err(|_error| DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: "DOCX UTF-16 XML contains an invalid surrogate".to_owned(),
            })
        })
    };
    let mut decoded_bytes = 0usize;
    for (index, character) in decoded_characters().enumerate() {
        check_parser_iteration(index, &mut || control.check(stage))?;
        decoded_bytes = decoded_bytes.saturating_add(character?.len_utf8());
        if decoded_bytes > MAX_DOCUMENT_EXPANDED_BYTES {
            return Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::MemoryBytes,
                observed: decoded_bytes,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES,
            });
        }
    }
    // Hold source, raw XML, normalized XML, retained output, and bounded metadata/facts at once.
    check_memory_budget(
        MAX_DOCUMENT_COMPRESSED_BYTES
            .saturating_add(xml.capacity())
            .saturating_add(decoded_bytes)
            .saturating_add(MAX_DOCUMENT_OUTPUT_BYTES)
            .saturating_add(MAX_DOCX_METADATA_BYTES)
            .saturating_add(MAX_DOCUMENT_FACTS * 256 * 4),
    )?;
    let mut decoded = String::with_capacity(decoded_bytes);
    for (index, character) in decoded_characters().enumerate() {
        check_parser_iteration(index, &mut || control.check(stage))?;
        decoded.push(character?);
    }
    let decoded = decoded.into_bytes();
    validate_docx_xml_encoding(&decoded, Some(little_endian))?;
    Ok(decoded)
}

/// Check the original byte encoding before every package XML reader sees normalized text.
fn validate_docx_xml_encoding(
    xml: &[u8],
    utf16_little_endian: Option<bool>,
) -> Result<(), DocumentExtractionError> {
    let mut reader = NsReader::from_reader(xml.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(xml));
    let Event::Decl(declaration) =
        reader
            .read_event()
            .map_err(|error| DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: error.to_string(),
            })?
    else {
        return Ok(());
    };
    let Some(encoding) = declaration.encoding() else {
        return Ok(());
    };
    let encoding = encoding.map_err(|error| DocumentExtractionError::Malformed {
        format: DocumentFormat::Docx,
        message: error.to_string(),
    })?;
    let encoding = encoding.as_ref();
    let utf8 = encoding.eq_ignore_ascii_case("UTF-8");
    let ascii = encoding.eq_ignore_ascii_case("US-ASCII");
    let utf16 = encoding.eq_ignore_ascii_case("UTF-16");
    let utf16_le = encoding.eq_ignore_ascii_case("UTF-16LE");
    let utf16_be = encoding.eq_ignore_ascii_case("UTF-16BE");
    if !(utf8 || ascii || utf16 || utf16_le || utf16_be) {
        return Err(DocumentExtractionError::UnsupportedDocxInput {
            message: "DOCX XML encoding is not supported; UTF-8 or UTF-16 is required".to_owned(),
        });
    }
    let matches_bytes = match utf16_little_endian {
        None => utf8 || (ascii && xml.is_ascii()),
        Some(true) => utf16 || utf16_le,
        Some(false) => utf16 || utf16_be,
    };
    if !matches_bytes {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX XML encoding declaration does not match its bytes".to_owned(),
        });
    }
    Ok(())
}

/// Extract package content-type overrides and extension defaults.
fn parse_docx_content_types(
    xml: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocxContentTypes, DocumentExtractionError> {
    const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
    if xml.len() > MAX_DOCX_METADATA_BYTES {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX content types exceed the package metadata bound".to_owned(),
        });
    }
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut types = DocxContentTypes::default();
    let mut depth = 0usize;
    let mut closed = false;
    let mut events = 0usize;
    loop {
        check_parser_iteration(events, &mut || control.check(stage))?;
        events = events.saturating_add(1);
        let (namespace, event) =
            reader
                .read_resolved_event()
                .map_err(|error| DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                })?;
        let valid_namespace = matches!(namespace, ResolveResult::Bound(namespace)
            if namespace.as_ref() == CONTENT_TYPES_NS);
        match event {
            Event::Start(event) => {
                depth += 1;
                let name = event.local_name();
                if !valid_namespace
                    || !matches!(
                        (depth, name.as_ref()),
                        (1, "Types") | (2, "Override" | "Default")
                    )
                    || closed
                {
                    return Err(DocumentExtractionError::InvalidDocxPackage {
                        message: "DOCX content types have an invalid XML structure".to_owned(),
                    });
                }
                if depth == 2 {
                    if types.overrides.len() + types.defaults.len() == MAX_DOCX_METADATA_RECORDS {
                        return Err(DocumentExtractionError::InvalidDocxPackage {
                            message:
                                "DOCX content type overrides exceed the package metadata bound"
                                    .to_owned(),
                        });
                    }
                    let attribute = |name| -> Result<String, DocumentExtractionError> {
                        let value = event
                            .try_get_attribute(name)
                            .map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?
                            .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                                message: format!("DOCX content type override is missing {name}"),
                            })?;
                        Ok(quick_xml::escape::unescape(&value.value)
                            .map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?
                            .into_owned())
                    };
                    let kind = attribute("ContentType")?;
                    if kind.is_empty() {
                        return Err(DocumentExtractionError::InvalidDocxPackage {
                            message: "DOCX content type is empty".to_owned(),
                        });
                    }
                    if name.as_ref() == "Override" {
                        let part = attribute("PartName")?;
                        if !part.starts_with('/')
                            || part.contains(['\\', '?', '#', ':'])
                            || part[1..].split('/').any(|component| {
                                component.is_empty()
                                    || matches!(component, "." | "..")
                                    || !safe_docx_uri_component(component)
                            })
                            || types.overrides.insert(part.clone(), kind).is_some()
                        {
                            return Err(DocumentExtractionError::InvalidDocxPackage {
                                message: format!("unsafe or duplicate DOCX content type {part}"),
                            });
                        }
                    } else {
                        let extension = attribute("Extension")?.to_ascii_lowercase();
                        if extension.is_empty()
                            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                            || types.defaults.insert(extension.clone(), kind).is_some()
                        {
                            return Err(DocumentExtractionError::InvalidDocxPackage {
                                message: format!("unsafe or duplicate DOCX extension {extension}"),
                            });
                        }
                    }
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(DocumentExtractionError::InvalidDocxPackage {
                        message: "DOCX content types XML has an unmatched end".to_owned(),
                    });
                }
                depth -= 1;
                if depth == 0 {
                    closed = true;
                }
            }
            Event::Text(text) if text.as_ref().chars().all(char::is_whitespace) => {}
            Event::Decl(_) if events == 1 => {}
            Event::Decl(_) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: "DOCX XML declaration must be the first XML event".to_owned(),
                });
            }
            Event::Comment(_) => {}
            Event::Eof => break,
            _ => {
                return Err(DocumentExtractionError::InvalidDocxPackage {
                    message: "DOCX content types contain unsupported XML".to_owned(),
                });
            }
        }
    }
    if !closed || depth != 0 {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX content types XML is incomplete".to_owned(),
        });
    }
    Ok(types)
}

/// Reject a referenced story whose package type contradicts its relationship.
fn validate_docx_content_type(
    part: &str,
    expected_suffix: &str,
    types: &DocxContentTypes,
) -> Result<(), DocumentExtractionError> {
    let expected = format!(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.{expected_suffix}+xml"
    );
    let actual = types.overrides.get(&format!("/{part}")).or_else(|| {
        part.rsplit_once('.')
            .and_then(|(_, extension)| types.defaults.get(&extension.to_ascii_lowercase()))
    });
    if actual.is_none_or(|actual| actual != &expected) {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: format!("DOCX story part {part} has a missing or wrong content type"),
        });
    }
    Ok(())
}

/// Read typed internal/external relationship identities from one package part.
fn parse_docx_relationships(
    xml: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<HashMap<String, (String, String, bool)>, DocumentExtractionError> {
    const RELATIONSHIPS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const OFFICE_RELATIONSHIP_PREFIX: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
    if xml.len() > MAX_DOCX_METADATA_BYTES {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX relationships exceed the package metadata bound".to_owned(),
        });
    }
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut result = HashMap::new();
    let mut depth = 0usize;
    let mut closed = false;
    let mut events = 0usize;
    loop {
        check_parser_iteration(events, &mut || control.check(stage))?;
        events = events.saturating_add(1);
        let (namespace, event) =
            reader
                .read_resolved_event()
                .map_err(|error| DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                })?;
        let valid_namespace = matches!(namespace, ResolveResult::Bound(namespace)
            if namespace.as_ref() == RELATIONSHIPS_NS);
        match event {
            Event::Start(event) => {
                depth += 1;
                let name = event.local_name();
                if !valid_namespace
                    || !matches!(
                        (depth, name.as_ref()),
                        (1, "Relationships") | (2, "Relationship")
                    )
                    || closed
                {
                    return Err(DocumentExtractionError::InvalidDocxPackage {
                        message: "DOCX relationships have an invalid XML structure".to_owned(),
                    });
                }
                if depth == 2 {
                    if result.len() == MAX_DOCX_METADATA_RECORDS {
                        return Err(DocumentExtractionError::InvalidDocxPackage {
                            message: "DOCX relationships exceed the package metadata record bound"
                                .to_owned(),
                        });
                    }
                    let attribute = |name| -> Result<String, DocumentExtractionError> {
                        let value = event
                            .try_get_attribute(name)
                            .map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?
                            .ok_or_else(|| DocumentExtractionError::InvalidDocxPackage {
                                message: format!("DOCX relationship is missing {name}"),
                            })?;
                        Ok(quick_xml::escape::unescape(&value.value)
                            .map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?
                            .into_owned())
                    };
                    let id = attribute("Id")?;
                    let kind = attribute("Type")?;
                    let target = attribute("Target")?;
                    let target_mode = event.try_get_attribute("TargetMode").map_err(|error| {
                        DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: error.to_string(),
                        }
                    })?;
                    let target_mode = target_mode
                        .as_ref()
                        .map(|value| quick_xml::escape::unescape(&value.value))
                        .transpose()
                        .map_err(|error| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: error.to_string(),
                        })?;
                    let external = match target_mode.as_deref() {
                        None | Some("Internal") => false,
                        Some("External") => true,
                        Some(_) => {
                            return Err(DocumentExtractionError::InvalidDocxPackage {
                                message: "DOCX relationship has an invalid TargetMode".to_owned(),
                            });
                        }
                    };
                    let kind = kind
                        .strip_prefix(OFFICE_RELATIONSHIP_PREFIX)
                        .or_else(|| {
                            kind.strip_prefix(
                                "http://purl.oclc.org/ooxml/officeDocument/relationships/",
                            )
                        })
                        .unwrap_or("")
                        .to_owned();
                    if result
                        .insert(id.clone(), (kind, target, external))
                        .is_some()
                    {
                        return Err(DocumentExtractionError::InvalidDocxPackage {
                            message: format!("duplicate DOCX relationship {id}"),
                        });
                    }
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(DocumentExtractionError::InvalidDocxPackage {
                        message: "DOCX relationship XML has an unmatched end".to_owned(),
                    });
                }
                depth -= 1;
                if depth == 0 {
                    closed = true;
                }
            }
            Event::Text(text) if text.as_ref().chars().all(char::is_whitespace) => {}
            Event::Decl(_) if events == 1 => {}
            Event::Decl(_) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: "DOCX XML declaration must be the first XML event".to_owned(),
                });
            }
            Event::Comment(_) => {}
            Event::Eof => break,
            _ => {
                return Err(DocumentExtractionError::InvalidDocxPackage {
                    message: "DOCX relationships contain unsupported XML".to_owned(),
                });
            }
        }
    }
    if !closed || depth != 0 {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: "DOCX relationships XML is incomplete".to_owned(),
        });
    }
    Ok(result)
}

/// Resolve a story-relative target while refusing unsafe URI syntax and package escape.
fn resolve_docx_story_target(
    target: &str,
    origin_part: &str,
    names: &HashSet<String>,
) -> Result<String, DocumentExtractionError> {
    let mut components = if origin_part.is_empty() {
        Vec::new()
    } else {
        let mut components = origin_part
            .split('/')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        components.pop();
        components
    };
    let target = if let Some(absolute) = target.strip_prefix('/') {
        components.clear();
        absolute
    } else {
        target
    };
    if target.is_empty() || target.contains(['\\', '?', '#', ':']) {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: format!("unsafe DOCX story target {target}"),
        });
    }
    for component in target.split('/') {
        let normalized = normalize_docx_target_component(component).ok_or_else(|| {
            DocumentExtractionError::InvalidDocxPackage {
                message: format!("unsafe DOCX story target {target}"),
            }
        })?;
        match normalized.as_str() {
            "" => {
                return Err(DocumentExtractionError::InvalidDocxPackage {
                    message: format!("unsafe DOCX story target {target}"),
                });
            }
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(DocumentExtractionError::InvalidDocxPackage {
                        message: format!("unsafe DOCX story target {target}"),
                    });
                }
            }
            _ => components.push(normalized),
        }
    }
    let part = components.join("/");
    if !names.contains(&part) {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: format!("referenced DOCX story part {part} is missing"),
        });
    }
    Ok(part)
}

/// Admit canonical part-name segments without encoded separators or unreserved aliases.
fn safe_docx_uri_component(component: &str) -> bool {
    normalize_docx_target_component(component).is_some_and(|normalized| normalized == component)
}

/// Normalize URI unreserved escapes while preserving encoded package-name bytes.
fn normalize_docx_target_component(component: &str) -> Option<String> {
    let bytes = component.as_bytes();
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            normalized.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return None;
        }
        let encoded = u8::from_str_radix(&component[index + 1..index + 3], 16).ok()?;
        if matches!(encoded, b'/' | b'\\') {
            return None;
        }
        if encoded.is_ascii_alphanumeric() || b"-._~".contains(&encoded) {
            normalized.push(encoded);
        } else {
            normalized.extend_from_slice(&bytes[index..index + 3]);
        }
        index += 3;
    }
    String::from_utf8(normalized).ok()
}

/// Append exact story facts and adjust their aggregate text line positions.
fn append_docx_story(
    document: &mut DocumentFacts,
    story: DocumentFacts,
    document_newlines: &mut usize,
) -> Result<(), DocumentExtractionError> {
    let DocumentFacts {
        text,
        facts,
        symbols,
        completeness,
        ..
    } = story;
    let separator = usize::from(!document.text.is_empty() && !text.is_empty());
    let total = document
        .text
        .len()
        .saturating_add(separator)
        .saturating_add(text.len());
    if total > MAX_DOCUMENT_OUTPUT_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::OutputBytes,
            observed: total,
            maximum: MAX_DOCUMENT_OUTPUT_BYTES,
        });
    }
    let fact_count = document
        .facts
        .len()
        .saturating_add(document.symbols.len())
        .saturating_add(facts.len())
        .saturating_add(symbols.len());
    if fact_count > MAX_DOCUMENT_FACTS {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::FactCount,
            observed: fact_count,
            maximum: MAX_DOCUMENT_FACTS,
        });
    }
    let line_offset = if document.text.is_empty() {
        0
    } else {
        *document_newlines + separator
    };
    *document_newlines += text.bytes().filter(|byte| *byte == b'\n').count() + separator;
    if separator != 0 {
        document.text.push('\n');
    }
    document.text.push_str(&text);
    document.facts.extend(facts.into_iter().map(|mut fact| {
        fact.line_start += line_offset;
        fact.line_end += line_offset;
        fact
    }));
    document
        .symbols
        .extend(symbols.into_iter().map(|mut symbol| {
            symbol.line += line_offset;
            symbol
        }));
    if let DocumentCompleteness::Partial { gaps } = completeness {
        if let DocumentCompleteness::Complete = document.completeness {
            document.completeness = DocumentCompleteness::Partial { gaps: Vec::new() };
        }
        if let DocumentCompleteness::Partial { gaps: existing } = &mut document.completeness {
            for gap in gaps {
                if !existing.contains(&gap) {
                    existing.push(gap);
                }
            }
        }
    }
    Ok(())
}

/// Decoded run text with Word text-leaf whitespace policy applied before publication.
#[derive(Default)]
struct RawDocxRun {
    /// Logical depth of this run, excluding selected compatibility wrappers.
    depth: usize,
    /// Direct run properties cannot follow already-consumed content.
    content_seen: bool,
    /// Logical depth of active direct run properties.
    properties_depth: Option<usize>,
    /// XML depth of the drawing, picture, or object owning a text box.
    drawing_depth: Option<usize>,
    /// Direct run formatting excludes payload without discarding field transitions.
    hidden: bool,
    /// Renderer-specific hiding needs a coverage gap unless direct vanish resolves it.
    spec_vanish: bool,
    /// Decoded logical run text.
    text: String,
    /// Decoded run bytes already emitted before a nested text container.
    text_start: usize,
}

/// Paragraph, run, and field ownership within one `WordprocessingML` text container.
#[derive(Default)]
struct DocxTextContext {
    /// Document-order paragraph ordinal, including empty paragraphs.
    number: usize,
    /// Run ordinal within this paragraph, including empty runs.
    run_number: usize,
    /// Whether the paragraph element is still open.
    open: bool,
    /// Unpublished fragment of the active run.
    run: Option<RawDocxRun>,
    /// Nested complex-field phases in this text container.
    fields: Vec<DocxFieldPhase>,
}

/// Whether a complex field is still in its instruction or cached-result region.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DocxFieldPhase {
    /// Field instructions are data and are never executed or rendered.
    Instruction,
    /// Existing cached result text may be retained.
    Result {
        /// Whether at least one literal cached result character was retained.
        has_cached_text: bool,
    },
}

/// One simple field whose child runs may contain cached literal result text.
struct DocxSimpleField {
    /// XML depth of the enclosing field element.
    depth: usize,
    /// Whether a literal result was retained from the field's children.
    has_cached_text: bool,
    /// A field inside enclosing code cannot render or require evaluation.
    in_instruction: bool,
}

/// Mark only actual retained field-result content, never field instructions.
fn mark_docx_cached_result(fields: &mut [DocxFieldPhase], simple: &mut [DocxSimpleField]) {
    for field in fields {
        if let DocxFieldPhase::Result { has_cached_text } = field {
            *has_cached_text = true;
        }
    }
    for field in simple {
        field.has_cached_text = true;
    }
}

/// The closed text leaves admitted from the Word document part.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DocxTextCarrier {
    /// Literal document text retained with a locator.
    Rendered,
    /// Field instructions and deleted text are validated without publication.
    Ignored,
}

/// Recognize either supported Word namespace independently of its chosen prefix.
fn wordprocessing_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            | "http://purl.oclc.org/ooxml/wordprocessingml/main"
    )
}

/// Resolve the namespace prefixes required by a Markup Compatibility choice.
fn docx_choice_supported(
    reader: &NsReader<&[u8]>,
    event: &BytesStart<'_>,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<bool, DocumentExtractionError> {
    let requires = event
        .try_get_attribute("Requires")
        .map_err(|error| DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: error.to_string(),
        })?
        .ok_or_else(|| DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX compatibility choice requires namespace prefixes".to_owned(),
        })?;
    let requires = quick_xml::escape::unescape(&requires.value).map_err(|error| {
        DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: error.to_string(),
        }
    })?;
    if requires.split_whitespace().next().is_none() {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX compatibility choice requires namespace prefixes".to_owned(),
        });
    }
    let mut supported = true;
    for (index, prefix) in requires.split_whitespace().enumerate() {
        check_parser_iteration(index, &mut || control.check(stage))?;
        if prefix.contains(':') {
            return Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: format!("DOCX compatibility choice has invalid prefix {prefix}"),
            });
        }
        let qualified = format!("{prefix}:choice");
        let (namespace, _) = reader.resolver().resolve_element(QName(&qualified));
        match namespace {
            ResolveResult::Bound(namespace) if wordprocessing_namespace(namespace.as_ref()) => {}
            ResolveResult::Bound(_) => supported = false,
            ResolveResult::Unbound | ResolveResult::Unknown(_) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: format!("DOCX compatibility choice has undeclared prefix {prefix}"),
                });
            }
        }
    }
    Ok(supported)
}

/// Selection state for one bounded Markup Compatibility alternative.
struct DocxAlternative {
    /// XML depth of the enclosing `AlternateContent` element.
    depth: usize,
    /// Whether a branch is selected or an earlier branch may render in Word.
    selection: DocxAlternativeSelection,
    /// Whether the required first Choice has appeared.
    choice_seen: bool,
    /// Whether the final optional Fallback has appeared.
    fallback_seen: bool,
}

/// Parser confidence about which compatibility branch Word may render.
#[derive(Clone, Copy, Eq, PartialEq)]
enum DocxAlternativeSelection {
    /// No earlier branch can render.
    Unselected,
    /// An understood branch was selected.
    Selected,
    /// An earlier unsupported branch may render.
    Uncertain,
}

/// Closed package stories that can be reached from rendered `WordprocessingML`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocxStoryKind {
    /// Header selected by a section reference.
    Header,
    /// Footer selected by a section reference.
    Footer,
    /// Numbered footnote selected by a rendered marker.
    Footnotes,
    /// Numbered endnote selected by a rendered marker.
    Endnotes,
    /// Comment selected by a rendered marker or range.
    Comments,
    /// Referenced but intentionally unparsed subdocument.
    Subdocument,
}

impl DocxStoryKind {
    /// Expected package content-type suffix for this story.
    fn content_type(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Footnotes => "footnotes",
            Self::Endnotes => "endnotes",
            Self::Comments => "comments",
            Self::Subdocument => "document.main",
        }
    }

    /// Relationship-type suffix for this story.
    fn relationship_name(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Footnotes => "footnotes",
            Self::Endnotes => "endnotes",
            Self::Comments => "comments",
            Self::Subdocument => "subDocument",
        }
    }

    /// Required root element of this story part.
    fn root_name(self) -> &'static str {
        match self {
            Self::Header => "hdr",
            Self::Footer => "ftr",
            Self::Footnotes => "footnotes",
            Self::Endnotes => "endnotes",
            Self::Comments => "comments",
            Self::Subdocument => "document",
        }
    }

    /// Item element name for selected note/comment IDs, if applicable.
    fn item_name(self) -> Option<&'static str> {
        match self {
            Self::Footnotes => Some("footnote"),
            Self::Endnotes => Some("endnote"),
            Self::Comments => Some("comment"),
            Self::Header | Self::Footer | Self::Subdocument => None,
        }
    }
}

#[derive(Default)]
/// Package references observed in rendered XML of one story part.
struct DocxStoryReferences {
    /// Explicit `r:id` references to related parts.
    linked_parts: Vec<(DocxStoryKind, String)>,
    /// A default header or footer may render differently on odd and even pages.
    has_default_header_footer: bool,
    /// Even-page stories are active only when the document setting enables them.
    even_parts: Vec<(DocxStoryKind, String)>,
    /// Imported-content anchors validated without decoding their target.
    imported_parts: Vec<String>,
    /// Alternate XML content anchors validated without decoding their target.
    content_parts: Vec<String>,
    /// Numbered note/comment items whose marker is rendered in this part.
    items: Vec<(DocxStoryKind, String)>,
    /// A rendered structured-document placeholder names glossary content we do not parse.
    glossary_placeholder: bool,
}

impl DocxStoryReferences {
    /// Count retained references before cross-story set fanout.
    fn count(&self) -> usize {
        self.linked_parts
            .len()
            .saturating_add(self.even_parts.len())
            .saturating_add(self.imported_parts.len())
            .saturating_add(self.content_parts.len())
            .saturating_add(self.items.len())
            .saturating_add(usize::from(self.glossary_placeholder))
    }

    /// First-page-only stories do not depend on page parity.
    fn has_parity_variant(&self) -> bool {
        self.has_default_header_footer || !self.even_parts.is_empty()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
/// Configured section page variant for a header or footer reference.
enum DocxHeaderVariant {
    /// Ordinary pages.
    Default,
    /// First page when title-page mode is enabled.
    First,
    /// Even pages when the document-wide setting is enabled.
    Even,
}

/// Deferred section links, resolved after its title-page switch is known.
struct DocxHeaderSection {
    /// XML depth of the current section-properties element.
    depth: usize,
    /// Depth excluding selected compatibility wrappers, for direct children.
    logical_depth: usize,
    /// Whether its first-page stories are displayed.
    title_page: Option<bool>,
    /// Explicit links and their page variants.
    links: Vec<(DocxStoryKind, String, DocxHeaderVariant)>,
}

/// Parent tags needed to distinguish rendered placeholders from `docPartObj` metadata.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum DocxSdtTag {
    #[default]
    /// Any non-SDT element.
    Other,
    /// Structured document tag container.
    Sdt,
    /// SDT properties.
    Properties,
    /// Placeholder properties.
    Placeholder,
    /// Content where a placeholder could be displayed.
    Content,
}

/// Bounded structured-document state for deciding whether a glossary placeholder can render.
struct DocxSdtContext {
    /// XML depth of the containing SDT.
    depth: usize,
    /// Whether a glossary placeholder was named.
    placeholder: bool,
    /// Whether cached placeholder display was requested.
    showing: bool,
    /// XML depth of the optional content element.
    content_depth: Option<usize>,
    /// Whether the content contains visible data.
    content_nonempty: bool,
}

/// Recognize either admitted Office relationship namespace for `r:id`.
fn office_relationship_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            | "http://purl.oclc.org/ooxml/officeDocument/relationships"
    )
}

/// Read one exact namespaced attribute without accepting spoofed local names.
fn docx_attribute(
    event: &BytesStart<'_>,
    reader: &NsReader<&[u8]>,
    local_name: &str,
    namespace_matches: fn(&str) -> bool,
) -> Result<Option<String>, DocumentExtractionError> {
    let mut found = None;
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: error.to_string(),
        })?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if let ResolveResult::Unknown(prefix) = &namespace {
            return Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: format!("DOCX XML attribute has an undeclared namespace prefix: {prefix}"),
            });
        }
        if local.as_ref() == local_name
            && matches!(namespace, ResolveResult::Bound(namespace)
                if namespace_matches(namespace.as_ref()))
        {
            if found.is_some() {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: format!("DOCX {local_name} attribute is duplicated"),
                });
            }
            let value = quick_xml::escape::unescape(&attribute.value).map_err(|error| {
                DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                }
            })?;
            if local_name == "id" && value.len() > MAX_DOCX_REFERENCE_ID_BYTES {
                return Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::MemoryBytes,
                    observed: value.len(),
                    maximum: MAX_DOCX_REFERENCE_ID_BYTES,
                });
            }
            found = Some(value.into_owned());
        }
    }
    Ok(found)
}

/// Match bounded Word story IDs by their XML Schema integer value, not spelling.
fn canonical_docx_story_id(id: &str) -> Result<String, DocumentExtractionError> {
    let id = id.trim_matches([' ', '\t', '\r', '\n']);
    let (negative, digits) = match id.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, id.strip_prefix('+').unwrap_or(id)),
    };
    if digits.is_empty() || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX story item ID is not a decimal integer".to_owned(),
        });
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok("0".to_owned());
    }
    Ok(if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    })
}

/// Read a `WordprocessingML` on/off element, whose absent value means true.
fn docx_on_off(
    event: &BytesStart<'_>,
    reader: &NsReader<&[u8]>,
) -> Result<bool, DocumentExtractionError> {
    match docx_attribute(event, reader, "val", wordprocessing_namespace)?.as_deref() {
        None | Some("true" | "1" | "on") => Ok(true),
        Some("false" | "0" | "off") => Ok(false),
        Some(value) => Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: format!("DOCX on/off value {value:?} is invalid"),
        }),
    }
}

/// Parse body paragraphs and table paragraphs from the admitted XML part.
#[cfg(test)]
fn parse_docx(
    xml: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    let xml = decode_docx_xml(xml.to_vec(), control, stage)?;
    parse_docx_part(
        &xml,
        DOCX_DOCUMENT_PART,
        "document",
        None,
        None,
        &mut DocxStoryReferences::default(),
        control,
        stage,
    )
}

/// Parse one validated `WordprocessingML` story without borrowing package state.
fn parse_docx_part(
    xml: &[u8],
    part: &str,
    root_name: &str,
    selected_ids: Option<&HashSet<String>>,
    selected_special_ids: Option<&HashSet<String>>,
    references: &mut DocxStoryReferences,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut output = String::new();
    let mut output_line = 1usize;
    let mut facts = Vec::new();
    let mut symbols = Vec::new();
    let mut gaps = Vec::new();
    let mut paragraph_number = 0usize;
    let mut paragraph = DocxTextContext::default();
    let mut text_boxes = Vec::new();
    let mut text_carrier = None;
    let mut text_start = 0;
    let mut preserve_space = [false; MAX_DOCX_XML_DEPTH + 1];
    let mut sdt_tags = [DocxSdtTag::Other; MAX_DOCX_XML_DEPTH + 1];
    let mut sdt_stack: Vec<DocxSdtContext> = Vec::new();
    let mut simple_fields: Vec<DocxSimpleField> = Vec::new();
    let mut header_section: Option<DocxHeaderSection> = None;
    let mut inherited_first: [Option<String>; 2] = [None, None];
    let mut inherited_default = [false; 2];
    let mut alternatives: Vec<DocxAlternative> = Vec::new();
    let mut selected_branches: Vec<usize> = Vec::new();
    let mut ignorable_namespaces: Vec<String> = Vec::new();
    let mut skipped_branch_depth = None;
    let mut skipped_text_box = false;
    let mut conditional_note_start = None;
    let mut deleted_depth = None;
    let mut foreign_depth = None;
    let mut foreign_opaque_depth = None;
    let mut element_depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut event_index = 0usize;
    let mut seen_ids = HashSet::new();
    let mut seen_special_ids = HashSet::new();
    loop {
        check_parser_iteration(event_index, &mut || control.check(stage))?;
        event_index = event_index.saturating_add(1);
        let (namespace, event) =
            reader
                .read_resolved_event()
                .map_err(|error| DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                })?;
        let compatibility = matches!(&namespace, ResolveResult::Bound(namespace)
            if namespace.as_ref() == "http://schemas.openxmlformats.org/markup-compatibility/2006");
        let wordprocessing = match &namespace {
            ResolveResult::Bound(namespace) => wordprocessing_namespace(namespace.as_ref()),
            ResolveResult::Unbound => false,
            ResolveResult::Unknown(prefix) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: format!("DOCX XML contained an undeclared namespace prefix: {prefix}"),
                });
            }
        };
        let recognized_drawing = matches!(&namespace, ResolveResult::Bound(namespace)
            if matches!(namespace.as_ref(),
                "http://schemas.openxmlformats.org/drawingml/2006/main"
                    | "http://purl.oclc.org/ooxml/drawingml/main"
                    | "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
                    | "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing"
                    | "http://schemas.openxmlformats.org/drawingml/2006/picture"
                    | "http://purl.oclc.org/ooxml/drawingml/picture"
                    | "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
                    | "urn:schemas-microsoft-com:vml"));
        if matches!(event, Event::Decl(_)) && event_index != 1 {
            return Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: "DOCX XML declaration must be the first XML event".to_owned(),
            });
        }
        if foreign_depth.is_some() && text_carrier.is_none() {
            let has_text = match &event {
                Event::Text(text) => Some(text.as_ref().chars().any(|c| !c.is_ascii_whitespace())),
                Event::CData(text) => Some(text.as_ref().chars().any(|c| !c.is_ascii_whitespace())),
                Event::GeneralRef(reference) => Some(
                    decode_docx_reference(reference)?
                        .chars()
                        .any(|c| !c.is_ascii_whitespace()),
                ),
                _ => None,
            };
            if let Some(has_text) = has_text {
                if has_text && deleted_depth.is_none() && skipped_branch_depth.is_none() {
                    return Err(DocumentExtractionError::UnsupportedDocxInput {
                        message: "foreign-namespace text requires unsupported semantic decoding"
                            .to_owned(),
                    });
                }
                continue;
            }
        }
        match event {
            Event::Start(event) => {
                if text_carrier.is_some() {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX text elements cannot contain nested markup".to_owned(),
                    });
                }
                let name = event.local_name();
                if element_depth == 0 {
                    if root_seen || !wordprocessing || name.as_ref() != root_name {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX XML must contain one WordprocessingML document root"
                                .to_owned(),
                        });
                    }
                    root_seen = true;
                }
                element_depth = element_depth.saturating_add(1);
                if element_depth > MAX_DOCX_XML_DEPTH {
                    return Err(DocumentExtractionError::ResourceLimit {
                        limit: DocumentLimit::NestingDepth,
                        observed: element_depth,
                        maximum: MAX_DOCX_XML_DEPTH,
                    });
                }
                preserve_space[element_depth] = preserve_space[element_depth - 1];
                sdt_tags[element_depth] = DocxSdtTag::Other;
                // Source-part ordinal stays stable when different note IDs are parsed later.
                if wordprocessing
                    && name.as_ref() == "p"
                    && (selected_ids.is_some()
                        || skipped_branch_depth.is_none()
                        || skipped_text_box)
                {
                    paragraph_number += 1;
                }
                if skipped_branch_depth.is_some() {
                    continue;
                }
                if wordprocessing
                    && (foreign_opaque_depth.is_some()
                        || (foreign_depth.is_some()
                            && text_boxes.is_empty()
                            && name.as_ref() != "txbxContent"))
                {
                    return Err(DocumentExtractionError::UnsupportedDocxInput {
                        message: "Word content inside an opaque foreign wrapper cannot be verified as rendered".to_owned(),
                    });
                }
                if wordprocessing
                    && deleted_depth.is_none()
                    && matches!(name.as_ref(), "rStyle" | "pStyle")
                    && !gaps.contains(&DocumentCoverageGap::UnresolvedVisibility)
                {
                    gaps.push(DocumentCoverageGap::UnresolvedVisibility);
                }
                let ignorable = !wordprocessing
                    && !compatibility
                    && matches!(&namespace, ResolveResult::Bound(namespace)
                        if ignorable_namespaces.iter().any(|ignored| ignored == namespace.as_ref()));
                if element_depth == 2 + 2 * selected_branches.len()
                    && let Some(selected_ids) = selected_ids
                    && matches!(root_name, "footnotes" | "endnotes" | "comments")
                {
                    let expected = match root_name {
                        "footnotes" => "footnote",
                        "endnotes" => "endnote",
                        _ => "comment",
                    };
                    if wordprocessing && name.as_ref() == expected {
                        let id = docx_attribute(&event, &reader, "id", wordprocessing_namespace)?
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX story item is missing its id".to_owned(),
                        })?;
                        let id = canonical_docx_story_id(&id)?;
                        let note_type = if matches!(root_name, "footnotes" | "endnotes") {
                            docx_attribute(&event, &reader, "type", wordprocessing_namespace)?
                        } else {
                            None
                        };
                        let selected = selected_ids.contains(&id);
                        if selected && note_type.as_deref().is_some_and(|kind| kind != "normal") {
                            return Err(DocumentExtractionError::InvalidDocxPackage {
                                message: "DOCX note reference targets a non-normal item".to_owned(),
                            });
                        }
                        let special = selected_special_ids.is_some_and(|ids| ids.contains(&id))
                            && matches!(
                                note_type.as_deref(),
                                Some("separator" | "continuationSeparator" | "continuationNotice")
                            );
                        if !selected && !special {
                            skipped_branch_depth = Some(element_depth);
                            continue;
                        }
                        let first_occurrence = if selected {
                            seen_ids.insert(id)
                        } else {
                            seen_special_ids.insert(id)
                        };
                        if !first_occurrence {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX selected story item ID is duplicated".to_owned(),
                            });
                        }
                        if matches!(
                            note_type.as_deref(),
                            Some("continuationSeparator" | "continuationNotice")
                        ) {
                            conditional_note_start =
                                Some((facts.len(), symbols.len(), references.count()));
                        }
                    }
                }
                if compatibility && matches!(name.as_ref(), "Choice" | "Fallback") {
                    let alternative = alternatives
                        .last_mut()
                        .filter(|alternative| {
                            alternative.depth + 1 == element_depth && !alternative.fallback_seen
                        })
                        .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX compatibility branch has invalid placement".to_owned(),
                        })?;
                    let supported = if name.as_ref() == "Choice" {
                        alternative.choice_seen = true;
                        docx_choice_supported(&reader, &event, control, stage)?
                    } else {
                        if !alternative.choice_seen {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX compatibility fallback requires a preceding choice"
                                    .to_owned(),
                            });
                        }
                        alternative.fallback_seen = true;
                        true
                    };
                    if name.as_ref() == "Choice"
                        && !supported
                        && alternative.selection == DocxAlternativeSelection::Unselected
                        && !gaps.contains(&DocumentCoverageGap::UnexaminedStory)
                    {
                        gaps.push(DocumentCoverageGap::UnexaminedStory);
                    }
                    if name.as_ref() == "Choice"
                        && !supported
                        && alternative.selection == DocxAlternativeSelection::Unselected
                    {
                        alternative.selection = DocxAlternativeSelection::Uncertain;
                    }
                    if alternative.selection == DocxAlternativeSelection::Unselected && supported {
                        alternative.selection = DocxAlternativeSelection::Selected;
                    } else {
                        skipped_branch_depth = Some(element_depth);
                        continue;
                    }
                }
                if deleted_depth.is_none()
                    && !(wordprocessing
                        && matches!(name.as_ref(), "del" | "moveFrom" | "sectPrChange"))
                {
                    for (index, attribute) in event.attributes().enumerate() {
                        check_parser_iteration(index, &mut || control.check(stage))?;
                        let attribute =
                            attribute.map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?;
                        let (attribute_namespace, attribute_name) =
                            reader.resolver().resolve_attribute(attribute.key);
                        if let ResolveResult::Unknown(prefix) = &attribute_namespace {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: format!(
                                    "DOCX XML attribute has an undeclared namespace prefix: {prefix}"
                                ),
                            });
                        }
                        if attribute_name.as_ref() == "space"
                            && matches!(&attribute_namespace, ResolveResult::Bound(namespace)
                                if namespace.as_ref() == "http://www.w3.org/XML/1998/namespace")
                        {
                            let value =
                                quick_xml::escape::unescape(&attribute.value).map_err(|error| {
                                    DocumentExtractionError::Malformed {
                                        format: DocumentFormat::Docx,
                                        message: error.to_string(),
                                    }
                                })?;
                            preserve_space[element_depth] = match value.as_ref() {
                                "default" => false,
                                "preserve" => true,
                                _ => {
                                    return Err(DocumentExtractionError::Malformed {
                                        format: DocumentFormat::Docx,
                                        message: "DOCX xml:space must be default or preserve"
                                            .to_owned(),
                                    });
                                }
                            };
                        }
                        if !matches!(attribute_namespace, ResolveResult::Bound(namespace)
                            if namespace.as_ref() == "http://schemas.openxmlformats.org/markup-compatibility/2006")
                            || !matches!(
                                attribute_name.as_ref(),
                                "Ignorable" | "ProcessContent" | "MustUnderstand"
                            )
                        {
                            continue;
                        }
                        let value =
                            quick_xml::escape::unescape(&attribute.value).map_err(|error| {
                                DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: error.to_string(),
                                }
                            })?;
                        if value.split_whitespace().next().is_none() {
                            continue;
                        }
                        if attribute_name.as_ref() != "Ignorable" || element_depth != 1 {
                            return Err(DocumentExtractionError::UnsupportedDocxInput {
                                message: "DOCX compatibility policy supports only root Ignorable namespaces".to_owned(),
                            });
                        }
                        for (index, prefix) in value.split_whitespace().enumerate() {
                            check_parser_iteration(index, &mut || control.check(stage))?;
                            if prefix.contains(':') {
                                return Err(DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: "DOCX Ignorable policy requires namespace prefixes, not qualified names".to_owned(),
                                });
                            }
                            let qualified = format!("{prefix}:ignored");
                            let (resolved, _) =
                                reader.resolver().resolve_element(QName(&qualified));
                            let ResolveResult::Bound(namespace) = resolved else {
                                return Err(DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: "DOCX Ignorable policy names an undeclared namespace"
                                        .to_owned(),
                                });
                            };
                            if ignorable_namespaces
                                .iter()
                                .any(|ignored| ignored == namespace.as_ref())
                            {
                                continue;
                            }
                            if ignorable_namespaces.len() == MAX_DOCX_IGNORABLE_NAMESPACES {
                                return Err(DocumentExtractionError::UnsupportedDocxInput {
                                    message: "DOCX root Ignorable policy exceeds the supported namespace count".to_owned(),
                                });
                            }
                            ignorable_namespaces.push(namespace.as_ref().to_owned());
                        }
                    }
                }
                if ignorable {
                    skipped_branch_depth = Some(element_depth);
                    continue;
                }
                if compatibility && matches!(name.as_ref(), "Choice" | "Fallback") {
                    selected_branches.push(element_depth);
                    continue;
                }
                if alternatives
                    .last()
                    .is_some_and(|alternative| alternative.depth + 1 == element_depth)
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX compatibility alternatives must contain choices and an optional fallback".to_owned(),
                    });
                }
                if compatibility && name.as_ref() == "AlternateContent" {
                    alternatives.push(DocxAlternative {
                        depth: element_depth,
                        selection: DocxAlternativeSelection::Unselected,
                        choice_seen: false,
                        fallback_seen: false,
                    });
                    continue;
                }
                if !wordprocessing && !compatibility {
                    if foreign_depth.is_none() {
                        foreign_depth = Some(element_depth);
                    }
                    if !recognized_drawing && foreign_opaque_depth.is_none() {
                        foreign_opaque_depth = Some(element_depth);
                    }
                }
                let mut parent_depth = element_depth - 1;
                for &branch_depth in selected_branches.iter().rev() {
                    if branch_depth == parent_depth {
                        parent_depth -= 2;
                    }
                }
                if wordprocessing && deleted_depth.is_none() {
                    sdt_tags[element_depth] = match (name.as_ref(), sdt_tags[parent_depth]) {
                        ("sdt", _) => DocxSdtTag::Sdt,
                        ("sdtPr", DocxSdtTag::Sdt) => DocxSdtTag::Properties,
                        ("placeholder", DocxSdtTag::Properties) => DocxSdtTag::Placeholder,
                        ("sdtContent", DocxSdtTag::Sdt) => DocxSdtTag::Content,
                        _ => DocxSdtTag::Other,
                    };
                    if !paragraph.fields.contains(&DocxFieldPhase::Instruction)
                        && (matches!(name.as_ref(), "altChunk" | "subDoc")
                            || (paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                                && matches!(
                                    name.as_ref(),
                                    "sym"
                                        | "drawing"
                                        | "pict"
                                        | "object"
                                        | "annotationRef"
                                        | "contentPart"
                                        | "tab"
                                        | "ptab"
                                        | "br"
                                        | "cr"
                                        | "lastRenderedPageBreak"
                                        | "noBreakHyphen"
                                        | "softHyphen"
                                        | "pgNum"
                                        | "dayShort"
                                        | "dayLong"
                                        | "monthShort"
                                        | "monthLong"
                                        | "yearShort"
                                        | "yearLong"
                                        | "footnoteReference"
                                        | "endnoteReference"
                                        | "commentReference"
                                        | "footnoteRef"
                                        | "endnoteRef"
                                        | "separator"
                                        | "continuationSeparator"
                                )))
                    {
                        for context in &mut sdt_stack {
                            if context
                                .content_depth
                                .is_some_and(|depth| element_depth > depth)
                            {
                                context.content_nonempty = true;
                            }
                        }
                    }
                }
                let logical_depth = element_depth - 2 * selected_branches.len();
                if wordprocessing
                    && matches!(
                        name.as_ref(),
                        "t" | "instrText"
                            | "delText"
                            | "delInstrText"
                            | "fldChar"
                            | "sym"
                            | "drawing"
                            | "pict"
                            | "object"
                            | "contentPart"
                            | "tab"
                            | "ptab"
                            | "br"
                            | "cr"
                            | "lastRenderedPageBreak"
                            | "noBreakHyphen"
                            | "softHyphen"
                            | "pgNum"
                            | "dayShort"
                            | "dayLong"
                            | "monthShort"
                            | "monthLong"
                            | "yearShort"
                            | "yearLong"
                            | "footnoteReference"
                            | "endnoteReference"
                            | "commentReference"
                            | "footnoteRef"
                            | "endnoteRef"
                            | "annotationRef"
                            | "separator"
                            | "continuationSeparator"
                    )
                    && paragraph
                        .run
                        .as_ref()
                        .is_some_and(|run| logical_depth != run.depth + 1)
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX run content is not a direct child of its run".to_owned(),
                    });
                }
                if wordprocessing
                    && paragraph.run.is_some()
                    && matches!(name.as_ref(), "sectPr" | "altChunk" | "subDoc")
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX non-run content appeared inside a run".to_owned(),
                    });
                }
                if wordprocessing
                    && matches!(name.as_ref(), "commentRangeStart" | "commentRangeEnd")
                    && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                    && deleted_depth.is_none()
                    && !paragraph.fields.contains(&DocxFieldPhase::Instruction)
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX comment range appeared inside a rendered run".to_owned(),
                    });
                }
                if wordprocessing && let Some(run) = paragraph.run.as_mut() {
                    if logical_depth == run.depth + 1 {
                        if name.as_ref() == "rPr" {
                            if run.content_seen {
                                return Err(DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: "DOCX run properties follow run content".to_owned(),
                                });
                            }
                            run.properties_depth = Some(logical_depth);
                        } else {
                            run.content_seen = true;
                            if matches!(name.as_ref(), "drawing" | "pict" | "object") {
                                run.drawing_depth = Some(element_depth);
                            }
                        }
                    }
                    if name.as_ref() == "vanish"
                        && run.properties_depth == logical_depth.checked_sub(1)
                        && deleted_depth.is_none()
                    {
                        run.hidden |= docx_on_off(&event, &reader)?;
                    }
                    if name.as_ref() == "specVanish"
                        && run.properties_depth == logical_depth.checked_sub(1)
                        && deleted_depth.is_none()
                    {
                        run.spec_vanish |= docx_on_off(&event, &reader)?;
                    }
                }
                match if wordprocessing { name.as_ref() } else { "" } {
                    "sectPr" if deleted_depth.is_none() => {
                        if header_section.is_some() {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX section properties cannot nest".to_owned(),
                            });
                        }
                        header_section = Some(DocxHeaderSection {
                            depth: element_depth,
                            logical_depth,
                            title_page: None,
                            links: Vec::new(),
                        });
                    }
                    "titlePg" if deleted_depth.is_none() => {
                        let section = header_section
                            .as_mut()
                            .filter(|section| logical_depth == section.logical_depth + 1)
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX title-page setting is outside section properties"
                                    .to_owned(),
                            })?;
                        if section
                            .title_page
                            .replace(docx_on_off(&event, &reader)?)
                            .is_some()
                        {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX section has duplicate title-page settings"
                                    .to_owned(),
                            });
                        }
                    }
                    "fldSimple" if deleted_depth.is_none() => {
                        simple_fields.push(DocxSimpleField {
                            depth: element_depth,
                            has_cached_text: false,
                            in_instruction: paragraph.fields.contains(&DocxFieldPhase::Instruction),
                        });
                    }
                    "sdt" if deleted_depth.is_none() => sdt_stack.push(DocxSdtContext {
                        depth: element_depth,
                        placeholder: false,
                        showing: false,
                        content_depth: None,
                        content_nonempty: false,
                    }),
                    "docPart" if sdt_tags[parent_depth] == DocxSdtTag::Placeholder => {
                        let name =
                            docx_attribute(&event, &reader, "val", wordprocessing_namespace)?
                                .ok_or_else(|| DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: "DOCX placeholder docPart is missing w:val".to_owned(),
                                })?;
                        if name.is_empty() {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX placeholder docPart name is empty".to_owned(),
                            });
                        }
                        if let Some(context) = sdt_stack.last_mut() {
                            context.placeholder = true;
                        }
                    }
                    "showingPlcHdr" if sdt_tags[parent_depth] == DocxSdtTag::Properties => {
                        if let Some(context) = sdt_stack.last_mut() {
                            context.showing = docx_on_off(&event, &reader)?;
                        }
                    }
                    "sdtContent" if sdt_tags[element_depth] == DocxSdtTag::Content => {
                        if let Some(context) = sdt_stack.last_mut() {
                            context.content_depth = Some(element_depth);
                        }
                    }
                    "headerReference" | "footerReference" if deleted_depth.is_none() => {
                        let kind = if name.as_ref() == "headerReference" {
                            DocxStoryKind::Header
                        } else {
                            DocxStoryKind::Footer
                        };
                        let id =
                            docx_attribute(&event, &reader, "id", office_relationship_namespace)?
                                .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX header/footer reference is missing r:id".to_owned(),
                            })?;
                        let variant = match docx_attribute(
                            &event,
                            &reader,
                            "type",
                            wordprocessing_namespace,
                        )?
                        .as_deref()
                        {
                            None | Some("default") => DocxHeaderVariant::Default,
                            Some("first") => DocxHeaderVariant::First,
                            Some("even") => DocxHeaderVariant::Even,
                            Some(value) => {
                                return Err(DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: format!(
                                        "DOCX header/footer type {value:?} is invalid"
                                    ),
                                });
                            }
                        };
                        let section = header_section
                            .as_mut()
                            .filter(|section| logical_depth == section.logical_depth + 1)
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message:
                                    "DOCX header/footer reference is not a direct section child"
                                        .to_owned(),
                            })?;
                        if section
                            .links
                            .iter()
                            .any(|(existing_kind, _, existing_variant)| {
                                *existing_kind == kind && *existing_variant == variant
                            })
                        {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX section has a duplicate header/footer variant"
                                    .to_owned(),
                            });
                        }
                        section.links.push((kind, id, variant));
                    }
                    "subDoc" if deleted_depth.is_none() => {
                        let id =
                            docx_attribute(&event, &reader, "id", office_relationship_namespace)?
                                .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX subdocument reference is missing r:id".to_owned(),
                            })?;
                        references
                            .linked_parts
                            .push((DocxStoryKind::Subdocument, id));
                        if !gaps.contains(&DocumentCoverageGap::UnexaminedStory) {
                            gaps.push(DocumentCoverageGap::UnexaminedStory);
                        }
                    }
                    "altChunk" if deleted_depth.is_none() => {
                        let id =
                            docx_attribute(&event, &reader, "id", office_relationship_namespace)?
                                .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX alternate-format import is missing r:id".to_owned(),
                            })?;
                        references.imported_parts.push(id);
                        if !gaps.contains(&DocumentCoverageGap::UnexaminedStory) {
                            gaps.push(DocumentCoverageGap::UnexaminedStory);
                        }
                    }
                    "ruby" => {
                        if deleted_depth.is_some() {
                            skipped_branch_depth = Some(element_depth);
                        } else {
                            return Err(DocumentExtractionError::UnsupportedDocxInput {
                                message: "ruby annotations require unsupported nested run decoding"
                                    .to_owned(),
                            });
                        }
                    }
                    "del" | "moveFrom" | "sectPrChange" if deleted_depth.is_none() => {
                        deleted_depth = Some(element_depth);
                    }
                    "txbxContent" => {
                        if !paragraph.run.as_ref().is_some_and(|run| {
                            run.drawing_depth.is_some() && run.properties_depth.is_none()
                        }) {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX text box has no rendered drawing owner".to_owned(),
                            });
                        }
                        if paragraph.run.as_ref().is_some_and(|run| run.hidden)
                            || paragraph.fields.contains(&DocxFieldPhase::Instruction)
                        {
                            skipped_branch_depth = Some(element_depth);
                            skipped_text_box = true;
                            continue;
                        }
                        if let Some(run) = paragraph.run.as_mut() {
                            publish_docx_run_fragment(
                                run,
                                part,
                                paragraph.number,
                                paragraph.run_number,
                                &mut output,
                                &mut output_line,
                                &mut facts,
                                symbols.len(),
                            )?;
                        }
                        text_boxes.push((
                            std::mem::take(&mut paragraph),
                            output.len(),
                            facts.len(),
                        ));
                    }
                    "p" if !paragraph.open => {
                        paragraph.open = true;
                        paragraph.number = paragraph_number;
                        paragraph.run_number = 0;
                        if !output.is_empty() && deleted_depth.is_none() {
                            push_output_byte(&mut output, b'\n')?;
                            output_line += 1;
                        }
                    }
                    "r" if paragraph.open && paragraph.run.is_none() => {
                        paragraph.run_number += 1;
                        paragraph.run = Some(RawDocxRun {
                            depth: logical_depth,
                            ..RawDocxRun::default()
                        });
                    }
                    "t" | "instrText" | "delText" | "delInstrText" if paragraph.run.is_some() => {
                        text_start = paragraph.run.as_ref().map_or(0, |run| run.text.len());
                        let ignored = deleted_depth.is_some()
                            || paragraph.run.as_ref().is_some_and(|run| run.hidden)
                            || matches!(name.as_ref(), "delText" | "delInstrText")
                            || paragraph.fields.contains(&DocxFieldPhase::Instruction);
                        text_carrier = Some(if ignored {
                            DocxTextCarrier::Ignored
                        } else {
                            DocxTextCarrier::Rendered
                        });
                    }
                    "fldChar" if paragraph.run.is_some() && deleted_depth.is_some() => {}
                    "fldChar" if paragraph.run.is_some() => {
                        let field_type = docx_attribute(
                            &event,
                            &reader,
                            "fldCharType",
                            wordprocessing_namespace,
                        )?;
                        match field_type.as_deref() {
                            Some("begin") => {
                                if paragraph.fields.len() >= MAX_DOCX_XML_DEPTH {
                                    return Err(DocumentExtractionError::ResourceLimit {
                                        limit: DocumentLimit::NestingDepth,
                                        observed: paragraph.fields.len() + 1,
                                        maximum: MAX_DOCX_XML_DEPTH,
                                    });
                                }
                                paragraph.fields.push(DocxFieldPhase::Instruction);
                            }
                            Some("separate") if !paragraph.fields.is_empty() => {
                                if let Some(phase) = paragraph.fields.last_mut() {
                                    *phase = DocxFieldPhase::Result {
                                        has_cached_text: false,
                                    };
                                }
                            }
                            Some("end") if !paragraph.fields.is_empty() => {
                                if paragraph.fields.pop()
                                    != Some(DocxFieldPhase::Result {
                                        has_cached_text: true,
                                    })
                                    && !paragraph.fields.contains(&DocxFieldPhase::Instruction)
                                    && !gaps.contains(&DocumentCoverageGap::UnevaluatedField)
                                {
                                    gaps.push(DocumentCoverageGap::UnevaluatedField);
                                }
                            }
                            _ => {
                                return Err(DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: "DOCX field marker has invalid type or nesting"
                                        .to_owned(),
                                });
                            }
                        }
                    }
                    "tab"
                    | "ptab"
                    | "br"
                    | "cr"
                    | "lastRenderedPageBreak"
                    | "noBreakHyphen"
                    | "softHyphen"
                        if deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        if let Some(run) = paragraph.run.as_mut() {
                            mark_docx_cached_result(&mut paragraph.fields, &mut simple_fields);
                            append_docx_run_text(
                                run,
                                match name.as_ref() {
                                    "tab" | "ptab" => "\t",
                                    "noBreakHyphen" => "\u{2011}",
                                    "softHyphen" => "\u{00ad}",
                                    _ => "\n",
                                },
                                output.len(),
                            )?;
                        }
                    }
                    "pgNum" | "dayShort" | "dayLong" | "monthShort" | "monthLong" | "yearShort"
                    | "yearLong"
                        if paragraph.run.is_some()
                            && deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        if let Some(run) = paragraph.run.as_mut() {
                            append_docx_run_text(run, "\u{fffc}", output.len())?;
                        }
                        if !gaps.contains(&DocumentCoverageGap::UnevaluatedField) {
                            gaps.push(DocumentCoverageGap::UnevaluatedField);
                        }
                    }
                    "contentPart"
                        if paragraph.run.is_some()
                            && deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        let id =
                            docx_attribute(&event, &reader, "id", office_relationship_namespace)?
                                .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX content part is missing r:id".to_owned(),
                            })?;
                        references.content_parts.push(id);
                        if !gaps.contains(&DocumentCoverageGap::UnexaminedStory) {
                            gaps.push(DocumentCoverageGap::UnexaminedStory);
                        }
                    }
                    "footnoteRef" | "endnoteRef" | "annotationRef"
                        if paragraph.run.is_some()
                            && deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        if (name.as_ref() == "footnoteRef" && root_name != "footnotes")
                            || (name.as_ref() == "endnoteRef" && root_name != "endnotes")
                            || (name.as_ref() == "annotationRef" && root_name != "comments")
                        {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX story reference mark is outside its story"
                                    .to_owned(),
                            });
                        }
                        if let Some(run) = paragraph.run.as_mut() {
                            append_docx_run_text(run, "\u{fffc}", output.len())?;
                        }
                        if !gaps.contains(&DocumentCoverageGap::UnevaluatedField) {
                            gaps.push(DocumentCoverageGap::UnevaluatedField);
                        }
                    }
                    "footnoteReference" | "endnoteReference" | "commentReference"
                        if paragraph.run.is_some()
                            && deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        let kind = match name.as_ref() {
                            "footnoteReference" => DocxStoryKind::Footnotes,
                            "endnoteReference" => DocxStoryKind::Endnotes,
                            _ => DocxStoryKind::Comments,
                        };
                        let id = docx_attribute(&event, &reader, "id", wordprocessing_namespace)?
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX note/comment reference is missing w:id".to_owned(),
                        })?;
                        let id = canonical_docx_story_id(&id)?;
                        references.items.push((kind, id));
                        if let Some(run) = paragraph.run.as_mut() {
                            append_docx_run_text(run, "\u{fffc}", output.len())?;
                        }
                        if !gaps.contains(&DocumentCoverageGap::UnevaluatedField) {
                            gaps.push(DocumentCoverageGap::UnevaluatedField);
                        }
                    }
                    "commentRangeStart" | "commentRangeEnd"
                        if deleted_depth.is_none()
                            && !paragraph.run.as_ref().is_some_and(|run| run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        let id = docx_attribute(&event, &reader, "id", wordprocessing_namespace)?
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX comment range is missing w:id".to_owned(),
                        })?;
                        let id = canonical_docx_story_id(&id)?;
                        references.items.push((DocxStoryKind::Comments, id));
                    }
                    "sym"
                        if paragraph.run.is_some()
                            && deleted_depth.is_none()
                            && paragraph.run.as_ref().is_some_and(|run| !run.hidden)
                            && !paragraph.fields.contains(&DocxFieldPhase::Instruction) =>
                    {
                        let mut font = None;
                        let mut code = None;
                        for (index, attribute) in event.attributes().enumerate() {
                            check_parser_iteration(index, &mut || control.check(stage))?;
                            let attribute =
                                attribute.map_err(|error| DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: error.to_string(),
                                })?;
                            let (namespace, local) =
                                reader.resolver().resolve_attribute(attribute.key);
                            if !matches!(namespace, ResolveResult::Bound(namespace)
                                if wordprocessing_namespace(namespace.as_ref()))
                            {
                                continue;
                            }
                            let value =
                                quick_xml::escape::unescape(&attribute.value).map_err(|error| {
                                    DocumentExtractionError::Malformed {
                                        format: DocumentFormat::Docx,
                                        message: error.to_string(),
                                    }
                                })?;
                            match local.as_ref() {
                                "font" => {
                                    if value.len() > MAX_DOCX_FONT_NAME_BYTES {
                                        return Err(DocumentExtractionError::ResourceLimit {
                                            limit: DocumentLimit::MemoryBytes,
                                            observed: value.len(),
                                            maximum: MAX_DOCX_FONT_NAME_BYTES,
                                        });
                                    }
                                    font = Some(value.into_owned());
                                }
                                "char" => {
                                    if value.len() != 4
                                        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                                    {
                                        return Err(DocumentExtractionError::Malformed {
                                            format: DocumentFormat::Docx,
                                            message: "DOCX symbol code must be four hex digits"
                                                .to_owned(),
                                        });
                                    }
                                    code =
                                        Some(u16::from_str_radix(&value, 16).map_err(|error| {
                                            DocumentExtractionError::Malformed {
                                                format: DocumentFormat::Docx,
                                                message: error.to_string(),
                                            }
                                        })?);
                                }
                                _ => {}
                            }
                        }
                        if facts.len().saturating_add(symbols.len()) >= MAX_DOCUMENT_FACTS {
                            return Err(DocumentExtractionError::ResourceLimit {
                                limit: DocumentLimit::FactCount,
                                observed: facts.len().saturating_add(symbols.len()) + 1,
                                maximum: MAX_DOCUMENT_FACTS,
                            });
                        }
                        // Adobe's Symbol encoding maps byte 0x61 to Greek alpha;
                        // arbitrary font-private codes have no portable Unicode meaning.
                        let unicode = match (font.as_deref(), code) {
                            (Some(name), Some(0x0061 | 0xF061))
                                if name.eq_ignore_ascii_case("Symbol") =>
                            {
                                Some('α')
                            }
                            _ => None,
                        };
                        let Some(run) = paragraph.run.as_mut() else {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX symbol appeared outside an active run".to_owned(),
                            });
                        };
                        let line =
                            output_line + run.text.bytes().filter(|byte| *byte == b'\n').count();
                        let text_start = run.text_start + run.text.len();
                        let rendered = unicode.unwrap_or('\u{fffc}');
                        mark_docx_cached_result(&mut paragraph.fields, &mut simple_fields);
                        append_docx_run_text(run, &rendered.to_string(), output.len())?;
                        symbols.push(DocumentSymbol {
                            font,
                            code,
                            locator: DocumentLocator::Docx {
                                part: part.to_owned(),
                                paragraph: paragraph.number,
                                run: paragraph.run_number,
                                text_start,
                                text_end: text_start + rendered.len_utf8(),
                            },
                            line,
                            unicode,
                        });
                        if unicode.is_none()
                            && !gaps.contains(&DocumentCoverageGap::UnknownSymbolMapping)
                        {
                            gaps.push(DocumentCoverageGap::UnknownSymbolMapping);
                        }
                    }
                    "p" | "r" | "t" | "instrText" | "delText" | "delInstrText" | "fldChar" => {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX paragraph, run, or text nesting is invalid".to_owned(),
                        });
                    }
                    _ => {}
                }
                let reference_count = references.count().saturating_add(
                    header_section
                        .as_ref()
                        .map_or(0, |section| section.links.len()),
                );
                if reference_count > MAX_DOCUMENT_FACTS {
                    return Err(DocumentExtractionError::ResourceLimit {
                        limit: DocumentLimit::FactCount,
                        observed: reference_count,
                        maximum: MAX_DOCUMENT_FACTS,
                    });
                }
            }
            Event::Text(event) => {
                if skipped_branch_depth.is_some() {
                    continue;
                }
                if text_carrier.is_some() {
                    let Some(run) = paragraph.run.as_mut() else {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "text appeared outside a run".to_owned(),
                        });
                    };
                    if text_carrier == Some(DocxTextCarrier::Rendered) {
                        append_docx_run_text(run, event.as_ref(), output.len())?;
                    }
                } else if !event
                    .as_ref()
                    .chars()
                    .all(|character| character.is_ascii_whitespace())
                {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX XML contained text outside its document root".to_owned(),
                    });
                }
            }
            Event::CData(event) => {
                if skipped_branch_depth.is_some() {
                    continue;
                }
                if text_carrier.is_none() {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "CDATA appeared outside a run".to_owned(),
                    });
                }
                let Some(run) = paragraph.run.as_mut() else {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "CDATA appeared outside a run".to_owned(),
                    });
                };
                if text_carrier == Some(DocxTextCarrier::Rendered) {
                    append_docx_run_text(run, event.as_ref(), output.len())?;
                }
            }
            Event::GeneralRef(reference) => {
                if skipped_branch_depth.is_some() {
                    decode_docx_reference(&reference)?;
                    continue;
                }
                if text_carrier.is_none() {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "entity appeared outside a text run".to_owned(),
                    });
                }
                let Some(run) = paragraph.run.as_mut() else {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "entity appeared outside a run".to_owned(),
                    });
                };
                let text = decode_docx_reference(&reference)?;
                if text_carrier == Some(DocxTextCarrier::Rendered) {
                    append_docx_run_text(run, &text, output.len())?;
                }
            }
            Event::DocType(_) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: "DOCX XML DOCTYPE and external declarations are unsupported"
                        .to_owned(),
                });
            }
            Event::End(event) => {
                if element_depth == 0 {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX XML contained an unmatched closing element".to_owned(),
                    });
                }
                if foreign_opaque_depth == Some(element_depth) {
                    foreign_opaque_depth = None;
                }
                if foreign_depth == Some(element_depth) {
                    foreign_depth = None;
                }
                if let Some(depth) = skipped_branch_depth {
                    if element_depth == depth {
                        skipped_branch_depth = None;
                        skipped_text_box = false;
                    }
                    sdt_tags[element_depth] = DocxSdtTag::Other;
                    element_depth -= 1;
                    continue;
                }
                if compatibility && event.local_name().as_ref() == "AlternateContent" {
                    let alternative = alternatives.pop().filter(|alternative| {
                        alternative.depth == element_depth && alternative.choice_seen
                    });
                    if alternative.is_none() {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX compatibility alternatives require at least one choice"
                                .to_owned(),
                        });
                    }
                }
                if compatibility
                    && matches!(event.local_name().as_ref(), "Choice" | "Fallback")
                    && selected_branches.last() == Some(&element_depth)
                {
                    selected_branches.pop();
                }
                let name = event.local_name();
                if wordprocessing
                    && name.as_ref() == "fldSimple"
                    && simple_fields
                        .last()
                        .is_some_and(|field| field.depth == element_depth)
                    && let Some(field) = simple_fields.pop()
                    && !field.has_cached_text
                    && !field.in_instruction
                    && !gaps.contains(&DocumentCoverageGap::UnevaluatedField)
                {
                    gaps.push(DocumentCoverageGap::UnevaluatedField);
                }
                if wordprocessing
                    && name.as_ref() == "sdt"
                    && sdt_stack
                        .last()
                        .is_some_and(|context| context.depth == element_depth)
                {
                    let Some(context) = sdt_stack.pop() else {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX structured tag state is incomplete".to_owned(),
                        });
                    };
                    if !paragraph.fields.contains(&DocxFieldPhase::Instruction)
                        && context.placeholder
                        && (context.showing || !context.content_nonempty)
                    {
                        references.glossary_placeholder = true;
                        if !gaps.contains(&DocumentCoverageGap::UnexaminedStory) {
                            gaps.push(DocumentCoverageGap::UnexaminedStory);
                        }
                    }
                }
                if deleted_depth == Some(element_depth) {
                    deleted_depth = None;
                }
                if wordprocessing
                    && name.as_ref() == "rPr"
                    && let Some(run) = paragraph.run.as_mut()
                    && run.properties_depth == Some(element_depth - 2 * selected_branches.len())
                {
                    run.properties_depth = None;
                    if run.spec_vanish
                        && !run.hidden
                        && !gaps.contains(&DocumentCoverageGap::UnresolvedVisibility)
                    {
                        gaps.push(DocumentCoverageGap::UnresolvedVisibility);
                    }
                }
                if wordprocessing
                    && matches!(name.as_ref(), "drawing" | "pict" | "object")
                    && let Some(run) = paragraph.run.as_mut()
                    && run.drawing_depth == Some(element_depth)
                {
                    run.drawing_depth = None;
                }
                if wordprocessing
                    && name.as_ref() == "sectPr"
                    && header_section
                        .as_ref()
                        .is_some_and(|section| section.depth == element_depth)
                {
                    let Some(section) = header_section.take() else {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX section state is incomplete".to_owned(),
                        });
                    };
                    for (kind, id, variant) in section.links {
                        let index = usize::from(kind == DocxStoryKind::Footer);
                        match variant {
                            DocxHeaderVariant::Default => {
                                inherited_default[index] = true;
                                references.has_default_header_footer = true;
                                references.linked_parts.push((kind, id));
                            }
                            DocxHeaderVariant::First => {
                                inherited_first[index] = Some(id);
                            }
                            DocxHeaderVariant::Even => references.even_parts.push((kind, id)),
                        }
                    }
                    if section.title_page.unwrap_or(false) {
                        if inherited_default.contains(&true)
                            && !gaps.contains(&DocumentCoverageGap::ConditionalStory)
                        {
                            gaps.push(DocumentCoverageGap::ConditionalStory);
                        }
                        for (index, id) in inherited_first.iter().enumerate() {
                            if let Some(id) = id {
                                let kind = if index == 0 {
                                    DocxStoryKind::Header
                                } else {
                                    DocxStoryKind::Footer
                                };
                                references.linked_parts.push((kind, id.clone()));
                            }
                        }
                    }
                }
                match if wordprocessing { name.as_ref() } else { "" } {
                    "t" | "instrText" | "delText" | "delInstrText" => {
                        if text_carrier == Some(DocxTextCarrier::Rendered)
                            && !preserve_space[element_depth]
                            && let Some(run) = paragraph.run.as_mut()
                        {
                            // Trim the complete leaf, not individual XML text/entity/CDATA events.
                            let text = &run.text[text_start..];
                            let trimmed = text.trim_matches([' ', '\t', '\r', '\n']);
                            let leading =
                                text.len() - text.trim_start_matches([' ', '\t', '\r', '\n']).len();
                            let end = text_start + leading + trimmed.len();
                            run.text.truncate(end);
                            run.text.drain(text_start..text_start + leading);
                        }
                        if text_carrier == Some(DocxTextCarrier::Rendered)
                            && paragraph
                                .run
                                .as_ref()
                                .is_some_and(|run| run.text.len() > text_start)
                        {
                            mark_docx_cached_result(&mut paragraph.fields, &mut simple_fields);
                            for context in &mut sdt_stack {
                                if context.content_depth.is_some() {
                                    context.content_nonempty = true;
                                }
                            }
                        }
                        text_carrier = None;
                    }
                    "r" => {
                        if let Some(mut run) = paragraph.run.take() {
                            publish_docx_run_fragment(
                                &mut run,
                                part,
                                paragraph.number,
                                paragraph.run_number,
                                &mut output,
                                &mut output_line,
                                &mut facts,
                                symbols.len(),
                            )?;
                        }
                    }
                    "txbxContent" => {
                        if !paragraph.fields.is_empty() {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX text box ended inside an incomplete field"
                                    .to_owned(),
                            });
                        }
                        let Some((mut outer, previous_bytes, previous_facts)) = text_boxes.pop()
                        else {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX text box had no matching container".to_owned(),
                            });
                        };
                        if facts.len() > previous_facts {
                            mark_docx_cached_result(&mut outer.fields, &mut simple_fields);
                        }
                        paragraph = outer;
                        if output.len() > previous_bytes && !output.ends_with('\n') {
                            push_output_byte(&mut output, b'\n')?;
                            output_line += 1;
                        }
                    }
                    "p" => paragraph.open = false,
                    _ => {}
                }
                if wordprocessing
                    && matches!(name.as_ref(), "footnote" | "endnote")
                    && element_depth == 2 + 2 * selected_branches.len()
                    && let Some((fact_count, symbol_count, reference_count)) =
                        conditional_note_start.take()
                    && (facts.len() > fact_count
                        || symbols.len() > symbol_count
                        || references.count() > reference_count)
                    && !gaps.contains(&DocumentCoverageGap::ConditionalStory)
                {
                    gaps.push(DocumentCoverageGap::ConditionalStory);
                }
                sdt_tags[element_depth] = DocxSdtTag::Other;
                element_depth -= 1;
                if element_depth == 0 {
                    root_closed = true;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen
        || !root_closed
        || element_depth != 0
        || paragraph.open
        || !text_boxes.is_empty()
        || !alternatives.is_empty()
        || !selected_branches.is_empty()
        || skipped_branch_depth.is_some()
        || conditional_note_start.is_some()
        || deleted_depth.is_some()
        || foreign_depth.is_some()
        || foreign_opaque_depth.is_some()
        || paragraph.run.is_some()
        || text_carrier.is_some()
        || !paragraph.fields.is_empty()
        || !simple_fields.is_empty()
        || !sdt_stack.is_empty()
        || header_section.is_some()
    {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX XML ended before all elements were closed".to_owned(),
        });
    }
    if let Some(selected_ids) = selected_ids
        && !selected_ids.is_subset(&seen_ids)
    {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: format!("referenced DOCX story item is absent from {part}"),
        });
    }
    if let Some(selected_special_ids) = selected_special_ids
        && !selected_special_ids.is_subset(&seen_special_ids)
    {
        return Err(DocumentExtractionError::InvalidDocxPackage {
            message: format!("listed DOCX special note is absent from {part}"),
        });
    }
    Ok(DocumentFacts {
        format: DocumentFormat::Docx,
        text: output,
        facts,
        symbols,
        completeness: if gaps.is_empty() {
            DocumentCompleteness::Complete
        } else {
            DocumentCompleteness::Partial { gaps }
        },
        provenance: DocumentParserProvenance::QuickXml,
    })
}

/// Emit a run fragment before leaving its container, preserving its original byte locator.
fn publish_docx_run_fragment(
    run: &mut RawDocxRun,
    part: &str,
    paragraph: usize,
    run_number: usize,
    output: &mut String,
    output_line: &mut usize,
    facts: &mut Vec<DocumentFact>,
    symbol_count: usize,
) -> Result<(), DocumentExtractionError> {
    if run.text.is_empty() {
        return Ok(());
    }
    let observed = facts.len().saturating_add(symbol_count).saturating_add(1);
    if observed > MAX_DOCUMENT_FACTS {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::FactCount,
            observed,
            maximum: MAX_DOCUMENT_FACTS,
        });
    }
    let required = output.len().saturating_add(run.text.len());
    if required > MAX_DOCUMENT_OUTPUT_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::OutputBytes,
            observed: required,
            maximum: MAX_DOCUMENT_OUTPUT_BYTES,
        });
    }
    let text = std::mem::take(&mut run.text);
    let line_start = *output_line;
    *output_line += text.bytes().filter(|byte| *byte == b'\n').count();
    // A terminal newline does not create another occupied slice line.
    let line_end = *output_line - usize::from(text.ends_with('\n'));
    output.push_str(&text);
    let text_end = run.text_start + text.len();
    facts.push(DocumentFact {
        line_start,
        line_end,
        text,
        locator: DocumentLocator::Docx {
            part: part.to_owned(),
            paragraph,
            run: run_number,
            text_start: run.text_start,
            text_end,
        },
    });
    run.text_start = text_end;
    Ok(())
}

/// Append decoded XML text while bounding one retained run before publication.
fn append_docx_run_text(
    run: &mut RawDocxRun,
    text: &str,
    published_bytes: usize,
) -> Result<(), DocumentExtractionError> {
    let required = published_bytes
        .saturating_add(run.text.len())
        .saturating_add(text.len());
    if required > MAX_DOCUMENT_OUTPUT_BYTES {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::OutputBytes,
            observed: required,
            maximum: MAX_DOCUMENT_OUTPUT_BYTES,
        });
    }
    run.text.push_str(text);
    Ok(())
}

/// Decode only XML predefined and character references; external entities are never resolved.
fn decode_docx_reference(reference: &BytesRef<'_>) -> Result<String, DocumentExtractionError> {
    if let Some(character) =
        reference
            .resolve_char_ref()
            .map_err(|error| DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: error.to_string(),
            })?
    {
        let valid = matches!(character, '\u{9}' | '\u{a}' | '\u{d}') || character >= '\u{20}';
        if !valid {
            return Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                message: "DOCX XML contained an invalid character reference".to_owned(),
            });
        }
        return Ok(character.to_string());
    }
    match reference.as_ref() {
        "amp" => Ok("&".to_owned()),
        "apos" => Ok("'".to_owned()),
        "gt" => Ok(">".to_owned()),
        "lt" => Ok("<".to_owned()),
        "quot" => Ok("\"".to_owned()),
        _ => Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX XML contained an unsupported entity reference".to_owned(),
        }),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use projectatlas_core::{IndexCancellation, IndexWorkControl};
    use std::fmt::Write as _;
    use std::io::Write;
    use std::time::{Duration, Instant};
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::FileOptions;

    fn control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), None)
    }

    fn docx_archive(xml: &[u8], method: CompressionMethod) -> Vec<u8> {
        docx_archive_with_parts_and_method(&[(DOCX_DOCUMENT_PART, xml)], method)
    }

    fn docx_archive_with_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
        docx_archive_with_parts_and_method(parts, CompressionMethod::Deflated)
    }

    fn docx_archive_with_parts_and_method(
        parts: &[(&str, &[u8])],
        method: CompressionMethod,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            let main = if parts.iter().any(|(name, _)| *name == DOCX_DOCUMENT_PART) {
                Some(DOCX_DOCUMENT_PART)
            } else if parts.iter().any(|(name, _)| *name == "document.xml") {
                Some("document.xml")
            } else {
                None
            };
            if !parts.iter().any(|(name, _)| *name == "[Content_Types].xml") {
                let mut manifest = String::from(
                    "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
                );
                for (name, _) in parts {
                    if !Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
                    {
                        continue;
                    }
                    let suffix = if Some(*name) == main || name.ends_with("/child.xml") {
                        Some("document.main")
                    } else if name.contains("glossary") {
                        Some("document.glossary")
                    } else if name.contains("header")
                        || name.contains("first")
                        || name.contains("even")
                        || name.contains("default")
                        || name.contains("prior")
                    {
                        Some("header")
                    } else if name.contains("footer") {
                        Some("footer")
                    } else if name.contains("footnotes") {
                        Some("footnotes")
                    } else if name.contains("endnotes") {
                        Some("endnotes")
                    } else if name.contains("comments") {
                        Some("comments")
                    } else if name.contains("settings") {
                        Some("settings")
                    } else {
                        None
                    };
                    if let Some(suffix) = suffix {
                        write!(
                            &mut manifest,
                            "<Override PartName=\"/{name}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{suffix}+xml\"/>"
                        )
                        .expect("fixture manifest override");
                    }
                }
                manifest.push_str("</Types>");
                writer
                    .start_file("[Content_Types].xml", FileOptions::default())
                    .expect("fixture manifest");
                writer
                    .write_all(manifest.as_bytes())
                    .expect("fixture manifest XML");
            }
            if !parts.iter().any(|(name, _)| *name == "_rels/.rels")
                && let Some(main) = main
            {
                let rels = format!(
                    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"main\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{main}\"/></Relationships>"
                );
                writer
                    .start_file("_rels/.rels", FileOptions::default())
                    .expect("fixture root relationships");
                writer
                    .write_all(rels.as_bytes())
                    .expect("fixture root relationship XML");
            }
            for (name, xml) in parts {
                writer
                    .start_file(*name, FileOptions::default().compression_method(method))
                    .expect("fixture part");
                writer.write_all(xml).expect("fixture XML");
            }
            writer.finish().expect("fixture archive");
        }
        bytes
    }

    fn rewrite_zip_method(bytes: &mut [u8], method: u16) {
        for index in 0..bytes.len().saturating_sub(4) {
            match &bytes[index..index + 4] {
                b"PK\x03\x04" => {
                    bytes[index + 8..index + 10].copy_from_slice(&method.to_le_bytes());
                }
                b"PK\x01\x02" => {
                    bytes[index + 10..index + 12].copy_from_slice(&method.to_le_bytes());
                }
                _ => {}
            }
        }
    }

    #[test]
    fn document_admission_observes_queued_deadline_and_cancellation() {
        let lease = lock_document_execution(&control(), IndexWorkStage::TextIndex)
            .expect("initial document lease");
        let docx = docx_archive(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#,
            CompressionMethod::Stored,
        );
        for (path, bytes) in [
            ("guide.docx", docx.as_slice()),
            ("guide.pdf", minimal_pdf().as_slice()),
        ] {
            let waiting =
                IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_millis(100)));
            assert!(
                matches!(
                    extract_document_text_controlled(bytes, path, None, &waiting),
                    Err(DocumentExtractionError::Work(
                        IndexWorkFailure::DeadlineExceeded {
                            stage: IndexWorkStage::TextIndex
                        }
                    ))
                ),
                "{path} must wait for the shared document lease"
            );
        }
        let cancellation = IndexCancellation::new();
        let waiting = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(5)));
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            cancellation.cancel();
        });
        let result = extract_document_text_controlled(&docx, "guide.docx", None, &waiting);
        cancel.join().expect("cancellation thread");
        assert!(matches!(
            result,
            Err(DocumentExtractionError::Work(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::TextIndex
            }))
        ));
        drop(lease);
        assert!(extract_document_text_controlled(&docx, "guide.docx", None, &control()).is_ok());
    }

    fn mark_zip_encrypted(bytes: &mut [u8]) {
        for index in 0..bytes.len().saturating_sub(4) {
            let flags = match &bytes[index..index + 4] {
                b"PK\x03\x04" => &mut bytes[index + 6..index + 8],
                b"PK\x01\x02" => &mut bytes[index + 8..index + 10],
                _ => continue,
            };
            let value = u16::from_le_bytes([flags[0], flags[1]]) | 1;
            flags.copy_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn path_admission_is_case_insensitive_but_not_speculative() {
        assert_eq!(
            document_format_for_path("docs/guide.PDF", None),
            Some(DocumentFormat::Pdf)
        );
        assert_eq!(
            document_format_for_path("docs/guide.docx", Some("pdf")),
            Some(DocumentFormat::Pdf)
        );
        assert_eq!(document_format_for_path("docs/guide.doc", None), None);
        for language in ["text", "rust", "markdown", ""] {
            for path in ["docs/guide.pdf", "docs/guide.docx"] {
                assert_eq!(document_format_for_path(path, Some(language)), None);
            }
        }
    }

    #[test]
    fn mismatched_magic_fails_closed_before_parser_work() {
        let error = extract_document_text_controlled(b"PK\x03\x04", "guide.pdf", None, &control())
            .expect_err("DOCX bytes must not enter the PDF parser");
        assert!(matches!(
            error,
            DocumentExtractionError::MismatchedMagic {
                expected: DocumentFormat::Pdf,
                found: "docx"
            }
        ));
    }

    #[test]
    fn cancellation_is_observed_before_parser_work() {
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        let error = extract_document_text_controlled(b"%PDF-", "guide.pdf", None, &control)
            .expect_err("canceled work must not parse");
        assert!(matches!(
            error,
            DocumentExtractionError::Work(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::TextIndex
            })
        ));
    }

    #[test]
    fn expired_document_deadline_is_observed_before_parser_work() {
        let control = IndexWorkControl::with_deadline(
            IndexCancellation::new(),
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .expect("current instant supports one-second subtraction"),
        );
        let error = extract_document_text_controlled(b"%PDF-", "guide.pdf", None, &control)
            .expect_err("expired work must not parse");
        assert!(matches!(
            error,
            DocumentExtractionError::Work(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::TextIndex
            })
        ));
    }

    #[test]
    fn document_input_bytes_are_bounded_before_parser_work() {
        let mut bytes = vec![b'x'; MAX_DOCUMENT_COMPRESSED_BYTES + 1];
        bytes[..5].copy_from_slice(b"%PDF-");
        let error = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect_err("oversized input must be rejected before parsing");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::InputBytes,
                observed,
                maximum: MAX_DOCUMENT_COMPRESSED_BYTES
            } if observed == MAX_DOCUMENT_COMPRESSED_BYTES + 1
        ));
    }

    pub(super) fn minimal_pdf() -> Vec<u8> {
        let objects = [
            b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".as_slice(),
            b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".as_slice(),
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".as_slice(),
            b"4 0 obj\n<< /Length 40 >>\nstream\nBT /F1 12 Tf 72 720 Td (Hello PDF) Tj ET\nendstream\nendobj\n".as_slice(),
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object);
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn multi_page_pdf() -> Vec<u8> {
        let objects = [
            b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".as_slice(),
            b"2 0 obj\n<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>\nendobj\n".as_slice(),
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".as_slice(),
            b"4 0 obj\n<< /Length 39 >>\nstream\nBT /F1 12 Tf 72 720 Td (Page One) Tj ET\nendstream\nendobj\n".as_slice(),
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".as_slice(),
            b"6 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 7 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".as_slice(),
            b"7 0 obj\n<< /Length 39 >>\nstream\nBT /F1 12 Tf 72 720 Td (Page Two) Tj ET\nendstream\nendobj\n".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object);
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn encrypted_pdf() -> Vec<u8> {
        let objects = [
            b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".as_slice(),
            b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".as_slice(),
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".as_slice(),
            b"4 0 obj\n<< /Length 40 >>\nstream\nBT /F1 12 Tf 72 720 Td (Secret PDF) Tj ET\nendstream\nendobj\n".as_slice(),
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".as_slice(),
            b"6 0 obj\n<< /Filter /Standard /V 1 /R 2 /Length 40 /O <0000000000000000000000000000000000000000000000000000000000000000> /U <0000000000000000000000000000000000000000000000000000000000000000> /P -4 >>\nendobj\n".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object);
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Encrypt 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn pdf_extracts_page_text_with_exact_page_locator() {
        let facts =
            extract_document_text_controlled(&minimal_pdf(), "guide.pdf", Some("pdf"), &control())
                .expect("valid PDF");
        assert!(facts.text.contains("Hello PDF"));
        assert!(matches!(
            facts
                .facts
                .iter()
                .find(|fact| fact.text.contains("Hello PDF")),
            Some(DocumentFact {
                locator: DocumentLocator::Pdf { page: 1, .. },
                ..
            })
        ));
    }

    #[test]
    fn pdf_page_locators_are_page_local_for_multi_page_documents() {
        let facts = extract_document_text_controlled(
            &multi_page_pdf(),
            "guide.pdf",
            Some("pdf"),
            &control(),
        )
        .expect("valid multi-page PDF");
        assert_eq!(facts.facts.len(), 2);
        assert!(matches!(
            facts.facts[1].locator,
            DocumentLocator::Pdf {
                page: 2,
                text_start: 0,
                text_end: 8
            }
        ));
    }

    #[test]
    fn pdf_missing_or_unsupported_page_stream_is_not_silently_omitted() {
        for missing in [true, false] {
            let mut document = lopdf::Document::load_mem(&multi_page_pdf()).expect("fixture PDF");
            if missing {
                document.objects.remove(&(7, 0));
            } else {
                document
                    .objects
                    .get_mut(&(7, 0))
                    .expect("second page stream")
                    .as_stream_mut()
                    .expect("stream object")
                    .dict
                    .set("Filter", "UnsupportedDecode");
            }
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            assert!(
                matches!(
                    extract_document_text_controlled(&bytes, "guide.pdf", None, &control()),
                    Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Pdf,
                        ..
                    })
                ),
                "missing={missing} must fail instead of publishing incomplete text"
            );
        }
    }

    #[test]
    fn empty_pdf_publishes_complete_text_without_facts() {
        let mut document = lopdf::Document::new();
        let mut pages = lopdf::Dictionary::new();
        pages.set("Type", "Pages");
        pages.set("Kids", Vec::<lopdf::Object>::new());
        pages.set("Count", 0);
        let pages = document.add_object(pages);
        let mut catalog = lopdf::Dictionary::new();
        catalog.set("Type", "Catalog");
        catalog.set("Pages", pages);
        let catalog = document.add_object(catalog);
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("empty fixture");
        let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("empty page tree must pass the embedded guest and host");
        assert_eq!(result.completeness, DocumentCompleteness::Complete);
        assert!(result.text.is_empty());
        assert!(result.facts.is_empty());
        assert!(
            result
                .symbol_graph("guide.pdf", Some("pdf"))
                .symbols
                .is_empty()
        );
    }

    #[test]
    fn pdf_indirect_page_tree_fields_preserve_exact_facts() {
        let original = multi_page_pdf();
        let expected = extract_document_text_controlled(&original, "guide.pdf", None, &control())
            .expect("direct page tree");
        let mut document = lopdf::Document::load_mem(&original).expect("fixture PDF");
        let children = document
            .get_dictionary((2, 0))
            .expect("page tree")
            .get(b"Kids")
            .expect("children")
            .clone();
        let children = document.add_object(children);
        let alias = document.add_object(lopdf::Object::Reference(children));
        let count = document.add_object(2);
        let node = document.get_dictionary_mut((2, 0)).expect("page tree");
        node.set("Kids", alias);
        node.set("Count", count);
        for (id, name) in [((2, 0), "Pages"), ((3, 0), "Page"), ((6, 0), "Page")] {
            let name = document.add_object(lopdf::Object::Name(name.as_bytes().to_vec()));
            document
                .get_dictionary_mut(id)
                .expect("page-tree node")
                .set("Type", name);
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let actual = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("indirect page-tree fields");
        assert_eq!(actual, expected);

        let cycle = document.new_object_id();
        document
            .objects
            .insert(cycle, lopdf::Object::Reference(cycle));
        let missing = document.new_object_id();
        for invalid in [lopdf::Object::Null, 0.into(), cycle.into(), missing.into()] {
            document
                .get_dictionary_mut((2, 0))
                .expect("page tree")
                .set("Kids", invalid);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            assert!(
                matches!(
                    result,
                    Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Pdf,
                        ..
                    })
                ),
                "{result:?}"
            );
        }
    }

    #[test]
    fn pdf_missing_page_tree_child_never_publishes_a_complete_prefix() {
        for declared_count in [1, 2] {
            let mut document = lopdf::Document::load_mem(&multi_page_pdf()).expect("fixture PDF");
            document.objects.remove(&(6, 0));
            document
                .get_object_mut((2, 0))
                .expect("page tree")
                .as_dict_mut()
                .expect("page tree dictionary")
                .set("Count", declared_count);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            assert!(
                matches!(
                    result,
                    Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Pdf,
                        ..
                    })
                ),
                "declared_count={declared_count}: {result:?}"
            );
        }
    }

    #[test]
    fn pdf_form_content_keeps_text_and_page_evidence() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let mut form = lopdf::Dictionary::new();
        form.set("Type", "XObject");
        let subtype = document.add_object(lopdf::Object::Name(b"Form".to_vec()));
        form.set("Subtype", subtype);
        form.set(
            "Matrix",
            vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 600.into()],
        );
        form.set("BBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        let resources = document
            .get_dictionary((3, 0))
            .expect("page")
            .get(b"Resources")
            .expect("resources")
            .clone();
        form.set("Resources", resources);
        let form_id = document.add_object(lopdf::Stream::new(
            form,
            b"BT /F1 12 Tf 72 0 Td (Form Text Marker) Tj ET".to_vec(),
        ));
        let mut xobjects = lopdf::Dictionary::new();
        xobjects.set("Fm1", form_id);
        document
            .get_object_mut((3, 0))
            .expect("page")
            .as_dict_mut()
            .expect("page dictionary")
            .get_mut(b"Resources")
            .expect("resources")
            .as_dict_mut()
            .expect("resource dictionary")
            .set("XObject", xobjects);
        let stream = document
            .get_object_mut((4, 0))
            .expect("page stream")
            .as_stream_mut()
            .expect("stream");
        let mut content = stream.content.clone();
        content
            .extend_from_slice(b"\nq 1 0 0 1 0 100 cm /Fm1 Do Q\nq 1 0 0 1 0 -100 cm /Fm1 Do Q\n");
        stream.set_content(content);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("valid Form content");
        assert!(facts.text.contains("Hello PDF"));
        assert_eq!(facts.facts.len(), 3, "{}", facts.text);
        assert_eq!(
            facts
                .facts
                .iter()
                .filter(|fact| fact.text == "Form Text Marker")
                .count(),
            2,
            "{}",
            facts.text
        );
        assert!(
            facts
                .facts
                .iter()
                .any(|fact| fact.text.contains("Form Text Marker")
                    && matches!(fact.locator, DocumentLocator::Pdf { page: 1, .. }))
        );
    }

    #[test]
    fn pdf_rotation_and_quote_operators_preserve_text_locators() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        for (rotation, content) in [
            (0, b"BT /F1 12 Tf 20 TL 72 500 Td (First) Tj 108 0 Td (Second) Tj -108 -20 Td (Next) Tj ET".as_slice()),
            (90, b"BT /F1 12 Tf 0 1 -1 0 112 72 Tm (First) Tj 0 1 -1 0 112 180 Tm (Second) Tj 0 1 -1 0 132 72 Tm (Next) Tj ET"),
            (0, b"BT /F1 12 Tf 20 TL 72 500 Td (First) Tj 108 0 Td (Second) Tj -108 0 Td (Next) ' ET"),
            (0, b"BT /F1 12 Tf 20 TL 72 500 Td (First) Tj 108 0 Td (Second) Tj -108 0 Td 0 0 (Next) \" ET"),
        ] {
            document.get_object_mut((2, 0)).expect("page tree").as_dict_mut()
                .expect("page tree dictionary").set("Rotate", rotation);
            document.get_object_mut((4, 0)).expect("page stream").as_stream_mut()
                .expect("stream").set_content(content.to_vec());
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
                .expect("valid positioned text");
            assert_eq!(facts.text, "First Second\nNext", "rotation={rotation}");
            assert_eq!(facts.facts.len(), 2);
            for (index, fact) in facts.facts.iter().enumerate() {
                assert!(matches!(fact.locator, DocumentLocator::Pdf { page: 1, .. }));
                assert_eq!(fact.line_start, index + 1);
                assert_eq!(fact.line_end, index + 1);
            }
        }
        for rotation in [90, 180, 270] {
            document
                .get_object_mut((2, 0))
                .expect("page tree")
                .as_dict_mut()
                .expect("page tree dictionary")
                .set("Rotate", rotation);
            document.get_object_mut((4, 0)).expect("page stream").as_stream_mut()
                .expect("stream").set_content(b"BT /F1 12 Tf 72 500 Td (First) Tj 108 0 Td (Second) Tj -108 -100 Td (Next) Tj ET".to_vec());
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
                .expect("ordinary rotated text");
            assert_eq!(facts.text, "First Second\nNext");
            assert_eq!(facts.facts.len(), 2);
            for (index, fact) in facts.facts.iter().enumerate() {
                assert!(matches!(fact.locator, DocumentLocator::Pdf { page: 1, .. }));
                assert_eq!((fact.line_start, fact.line_end), (index + 1, index + 1));
            }
        }
    }

    #[test]
    fn pdf_text_state_and_missing_width_preserve_block_text() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let descriptor = lopdf::dictionary! {
            "Type" => "FontDescriptor", "FontName" => "Fixture", "Flags" => 32,
            "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 1000.into()],
            "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200, "CapHeight" => 700,
            "StemV" => 80, "MissingWidth" => 600
        };
        document.objects.insert(
            (5, 0),
            lopdf::dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Fixture",
                "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor,
                "FirstChar" => 65, "LastChar" => 65, "Widths" => vec![600.into()]
            }
            .into(),
        );
        document.get_object_mut((4, 0)).expect("content").as_stream_mut().expect("stream")
            .set_content(b"BT /F1 12 Tf 20 TL 72 500 Td q 100 -100 Td (A) Tj Q T* (StateB) Tj 1 0 0 1 115.2 480 Tm (C) Tj ET".to_vec());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("scoped text with descriptor widths");
        assert_eq!(facts.text, "A\nStateBC");
        assert_eq!(facts.facts.len(), 2);
        assert_eq!(facts.facts[1].text, "StateBC");
        assert_eq!((facts.facts[1].line_start, facts.facts[1].line_end), (2, 2));
        assert!(matches!(
            facts.facts[1].locator,
            DocumentLocator::Pdf { page: 1, .. }
        ));
    }

    #[test]
    fn pdf_font_matrix_and_color_aliases_preserve_text() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let glyph = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"600 0 d0".to_vec(),
        ));
        document.objects.insert((5, 0), lopdf::dictionary! {
            "Type" => "Font", "Subtype" => "Type3",
            "FontBBox" => vec![0.into(), 0.into(), 600.into(), 600.into()],
            "FontMatrix" => vec![0.002.into(), 0.into(), 0.into(), 0.002.into(), 0.into(), 0.into()],
            "CharProcs" => lopdf::dictionary! { "A" => glyph, "B" => glyph },
            "Encoding" => lopdf::dictionary! { "Differences" => vec![65.into(), "A".into(), "B".into()] },
            "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![600.into(), 600.into()]
        }.into());
        document
            .get_dictionary_mut((3, 0))
            .expect("page")
            .get_mut(b"Resources")
            .expect("resources")
            .as_dict_mut()
            .expect("dictionary")
            .set("ColorSpace", lopdf::dictionary! { "CS1" => "DeviceCMYK",
                "IndexedAlias" => vec!["Indexed".into(), "DeviceRGB".into(), 1.into(),
                    lopdf::Object::String(vec![0, 0, 0, 255, 255, 255], lopdf::StringFormat::Hexadecimal)] });
        for color in [
            "",
            "/CS1 cs 0 0 0 1 sc /CS1 CS 0 0 0 1 SC",
            "/IndexedAlias cs 1 sc /IndexedAlias CS 1 SC",
        ] {
            document
                .get_object_mut((4, 0))
                .expect("content")
                .as_stream_mut()
                .expect("stream")
                .set_content(
                    format!("{color} BT /F1 12 Tf 72 500 Td (A) Tj 14.4 0 Td (B) Tj ET")
                        .into_bytes(),
                );
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
                .expect("Type 3 text and visual-only color aliases");
            assert_eq!(facts.text, "AB");
            assert_eq!(facts.facts.len(), 1);
            assert!(matches!(
                facts.facts[0].locator,
                DocumentLocator::Pdf {
                    page: 1,
                    text_start: 0,
                    text_end: 2
                }
            ));
        }
        document
            .get_dictionary_mut((5, 0))
            .expect("font")
            .set("FontMatrix", vec![lopdf::Object::Real(0.002)]);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        assert!(matches!(
            extract_document_text_controlled(&bytes, "guide.pdf", None, &control()),
            Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Pdf,
                ..
            })
        ));
    }

    #[test]
    fn pdf_implicit_encoding_preserves_exact_text_and_refuses_undefined_glyphs() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        document
            .get_dictionary_mut((5, 0))
            .expect("font")
            .set("Encoding", lopdf::Dictionary::new());
        document
            .get_object_mut((4, 0))
            .expect("content")
            .as_stream_mut()
            .expect("stream")
            .set_content(b"BT /F1 12 Tf 72 500 Td <27> Tj ET".to_vec());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("implicit standard font encoding");
        assert_eq!(facts.text, "\u{2019}");
        assert_eq!(facts.facts.len(), 1);
        assert!(matches!(
            facts.facts[0].locator,
            DocumentLocator::Pdf {
                page: 1,
                text_start: 0,
                text_end: 3
            }
        ));
        document.get_dictionary_mut((5, 0)).expect("font").set(
            "Encoding",
            lopdf::dictionary! { "Differences" => vec![39.into(), ".notdef".into()] },
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        assert!(matches!(
            extract_document_text_controlled(&bytes, "guide.pdf", None, &control()),
            Err(DocumentExtractionError::UnsupportedPdfInput)
        ));
    }

    #[test]
    fn pdf_partial_unicode_uses_builtin_font_or_refuses() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let cmap = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange
1 beginbfchar <41> <005A> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end"
                .to_vec(),
        ));
        document
            .get_object_mut((4, 0))
            .expect("content")
            .as_stream_mut()
            .expect("stream")
            .set_content(b"BT /F1 12 Tf 72 500 Td (AB) Tj ET".to_vec());
        for (base, expected) in [("Symbol", Some("Z\u{0392}")), ("Fixture", None)] {
            document.objects.insert(
                (5, 0),
                lopdf::dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => base,
                    "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![600.into(), 600.into()],
                    "ToUnicode" => cmap
                }
                .into(),
            );
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            if let Some(expected) = expected {
                assert_eq!(result.expect("known built-in encoding").text, expected);
            } else {
                assert!(
                    matches!(result, Err(DocumentExtractionError::UnsupportedPdfInput)),
                    "{result:?}"
                );
            }
        }
    }

    #[test]
    fn pdf_calibrated_color_dictionaries_preserve_text_or_refuse() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let parameters = document.add_object(lopdf::dictionary! {
            "WhitePoint" => vec![1.into(), 1.into(), 1.into()]
        });
        let wrong_type = document.add_object(42);
        document.get_object_mut((4, 0)).expect("content").as_stream_mut().expect("stream")
            .set_content(b"BT /F1 12 Tf 72 500 Td (Prefix) Tj ET /Calibrated cs /Calibrated CS BT /F1 12 Tf 72 480 Td (Visible) Tj ET".to_vec());
        for space in ["CalGray", "CalRGB", "Lab"] {
            for (reference, valid) in [(parameters, true), (wrong_type, false), ((999, 0), false)] {
                document.get_dictionary_mut((3, 0)).expect("page")
                    .get_mut(b"Resources").expect("resources").as_dict_mut().expect("dictionary")
                    .set("ColorSpace", lopdf::dictionary! {
                        "Calibrated" => vec![lopdf::Object::Name(space.as_bytes().to_vec()), reference.into()]
                    });
                let mut bytes = Vec::new();
                document.save_to(&mut bytes).expect("fixture serialization");
                let result =
                    extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
                if valid {
                    let facts = result.expect("indirect calibrated dictionary");
                    assert_eq!(
                        facts.text.split_whitespace().collect::<Vec<_>>(),
                        ["Prefix", "Visible"]
                    );
                    assert_eq!(facts.facts.len(), 2);
                    for (fact, (text_start, text_end)) in facts.facts.iter().zip([(0, 6), (7, 14)])
                    {
                        assert_eq!(
                            fact.locator,
                            DocumentLocator::Pdf {
                                page: 1,
                                text_start,
                                text_end
                            }
                        );
                    }
                } else {
                    assert!(
                        matches!(result, Err(DocumentExtractionError::Malformed { .. })),
                        "{space}: {result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn pdf_extended_graphics_state_selects_text_font() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let state_type = document.add_object(lopdf::Object::Name(b"ExtGState".to_vec()));
        let mut state = lopdf::Dictionary::new();
        state.set("Type", state_type);
        state.set("Font", vec![lopdf::Object::Reference((5, 0)), 12.into()]);
        let mut states = lopdf::Dictionary::new();
        states.set("GS", state);
        document
            .get_object_mut((3, 0))
            .expect("page")
            .as_dict_mut()
            .expect("page dictionary")
            .get_mut(b"Resources")
            .expect("resources")
            .as_dict_mut()
            .expect("resource dictionary")
            .set("ExtGState", states);
        document
            .get_object_mut((4, 0))
            .expect("page stream")
            .as_stream_mut()
            .expect("stream")
            .set_content(b"/GS gs BT 10 Tw 2 Tc 72 500 Td (Graphics ) Tj (Font) Tj ET".to_vec());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("ExtGState font is usable without Tf");
        assert_eq!(facts.text.trim(), "Graphics Font");
        assert_eq!(facts.facts.len(), 1);
        assert!(matches!(
            facts.facts[0].locator,
            DocumentLocator::Pdf { page: 1, .. }
        ));
    }

    #[test]
    fn pdf_actual_text_refuses_partial_text_publication() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        document.get_object_mut((4, 0)).expect("page stream").as_stream_mut()
            .expect("stream").set_content(b"BT /F1 12 Tf 72 500 Td (Prefix) Tj /Span << /ActualText (replacement) >> BDC (glyph) Tj EMC ET".to_vec());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
        assert!(
            matches!(result, Err(DocumentExtractionError::UnsupportedPdfInput)),
            "{result:?}"
        );
    }

    #[test]
    fn pdf_structure_replacements_refuse_partial_text_publication() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        document
            .get_object_mut((4, 0))
            .expect("page stream")
            .as_stream_mut()
            .expect("stream")
            .set_content(
                b"BT /F1 12 Tf 72 500 Td (Prefix) Tj /Span << /MCID 0 >> BDC (glyph) Tj EMC ET"
                    .to_vec(),
            );
        let root = document.new_object_id();
        let mut element = lopdf::Dictionary::new();
        element.set("Type", "StructElem");
        element.set("S", "Span");
        element.set("P", root);
        element.set("Pg", (3, 0));
        element.set("K", 0);
        let element = document.add_object(element);
        let mut parents = lopdf::Dictionary::new();
        parents.set(
            "Nums",
            vec![0.into(), lopdf::Object::Array(vec![element.into()])],
        );
        let parents = document.add_object(parents);
        let mut structure = lopdf::Dictionary::new();
        structure.set("Type", "StructTreeRoot");
        structure.set("K", element);
        structure.set("ParentTree", parents);
        document.objects.insert(root, structure.into());
        document
            .get_dictionary_mut((3, 0))
            .expect("page")
            .set("StructParents", 0);
        document
            .get_dictionary_mut((1, 0))
            .expect("catalog")
            .set("StructTreeRoot", root);
        for replacement in [false, true] {
            if replacement {
                document
                    .get_dictionary_mut(element)
                    .expect("structure element")
                    .set("ActualText", lopdf::Object::string_literal("replacement"));
            }
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            if replacement {
                assert!(
                    matches!(result, Err(DocumentExtractionError::UnsupportedPdfInput)),
                    "{result:?}"
                );
            } else {
                let extracted = result.expect("ordinary tagged content remains supported");
                assert_eq!(extracted.text.trim(), "Prefixglyph");
                assert!(matches!(
                    extracted.facts[0].locator,
                    DocumentLocator::Pdf { page: 1, .. }
                ));
            }
        }
    }

    #[test]
    fn pdf_reversed_chars_refuses_partial_text_publication() {
        for content in [
            b"BT /F1 12 Tf 72 500 Td (Prefix) Tj /ReversedChars BMC (desrever) Tj EMC ET".as_slice(),
            b"BT /F1 12 Tf 72 500 Td (Prefix) Tj /ReversedChars << /MCID 0 >> BDC (desrever) Tj EMC ET",
        ] {
            let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
            document.get_object_mut((4, 0)).expect("page stream").as_stream_mut()
                .expect("stream").set_content(content.to_vec());
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            assert!(matches!(result, Err(DocumentExtractionError::UnsupportedPdfInput)), "{result:?}");
        }
    }

    fn pdf_with_xobject(xobject: lopdf::Stream) -> Vec<u8> {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let object = document.add_object(xobject);
        let mut resources = lopdf::Dictionary::new();
        resources.set("Object1", object);
        document
            .get_object_mut((3, 0))
            .expect("page")
            .as_dict_mut()
            .expect("page dictionary")
            .get_mut(b"Resources")
            .expect("resources")
            .as_dict_mut()
            .expect("resource dictionary")
            .set("XObject", resources);
        let stream = document
            .get_object_mut((4, 0))
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        let mut content = stream.content.clone();
        content.extend_from_slice(b"\n/Object1 Do\n");
        stream.set_content(content);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        bytes
    }

    #[test]
    fn recursive_pdf_form_stops_without_publishing_page_text() {
        let mut form = lopdf::Dictionary::new();
        form.set("Type", "XObject");
        form.set("Subtype", "Form");
        form.set("BBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        let bytes = pdf_with_xobject(lopdf::Stream::new(form, b"/Object1 Do".to_vec()));
        let mut document = lopdf::Document::load_mem(&bytes).expect("fixture PDF");
        let resources = document
            .get_dictionary((3, 0))
            .expect("page dictionary")
            .get(b"Resources")
            .expect("page resources")
            .clone();
        let id = resources
            .as_dict()
            .expect("resource dictionary")
            .get(b"XObject")
            .expect("XObject resources")
            .as_dict()
            .expect("XObject dictionary")
            .get(b"Object1")
            .expect("Form reference")
            .as_reference()
            .expect("indirect Form");
        document
            .get_object_mut(id)
            .expect("Form object")
            .as_stream_mut()
            .expect("Form stream")
            .dict
            .set("Resources", resources);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
        assert!(
            matches!(
                result,
                Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::MemoryBytes | DocumentLimit::ExecutionFuel,
                    ..
                } | DocumentExtractionError::Work(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded { .. }
                ))
            ),
            "{result:?}"
        );
    }

    #[test]
    fn pdf_closed_paths_preserve_text_and_missing_current_points_refuse() {
        for (path, valid) in [
            ("10 20 m 30 40 l h 50 60 70 80 v S", true),
            ("10 20 30 40 re 50 60 70 80 v S", true),
            ("50 60 70 80 v", false),
            ("10 20 m s 50 60 70 80 v", false),
            ("10 20 m f* 50 60 70 80 v", false),
            ("10 20 m B 50 60 70 80 v", false),
            ("10 20 m B* 50 60 70 80 v", false),
            ("10 20 m b 50 60 70 80 v", false),
            ("10 20 m b* 50 60 70 80 v", false),
            ("10 20 m S 50 60 70 80 v", false),
            ("10 20 m f 50 60 70 80 v", false),
            ("10 20 m F 50 60 70 80 v", false),
            ("10 20 m n 50 60 70 80 v", false),
            (
                "10 20 30 40 re s 10 20 30 40 re f* 10 20 30 40 re B 10 20 30 40 re B* 10 20 30 40 re b 10 20 30 40 re b*",
                true,
            ),
        ] {
            let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
            let stream = document
                .get_object_mut((4, 0))
                .expect("page content")
                .as_stream_mut()
                .expect("content stream");
            let mut content = format!("{path}\n").into_bytes();
            content.extend_from_slice(&stream.content);
            stream.set_content(content);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            if valid {
                let facts = result.expect("closed path and displayed PDF text");
                assert_eq!(facts.text, "Hello PDF");
                assert_eq!(facts.facts.len(), 1);
                assert!(matches!(
                    facts.facts[0].locator,
                    DocumentLocator::Pdf {
                        page: 1,
                        text_start: 0,
                        text_end: 9
                    }
                ));
            } else {
                assert!(
                    matches!(result, Err(DocumentExtractionError::Malformed { .. })),
                    "{result:?}"
                );
            }
        }
    }

    #[test]
    fn pdf_postscript_xobjects_preserve_displayed_text_and_exact_locator() {
        for subtype in ["PS", "Form", "Unknown"] {
            let mut object = lopdf::Dictionary::new();
            object.set("Type", "XObject");
            object.set("Subtype", subtype);
            if subtype != "PS" {
                object.set("Subtype2", "PS");
            }
            let bytes = pdf_with_xobject(lopdf::Stream::new(
                object,
                b"/Helvetica findfont 12 scalefont setfont (Print only) show".to_vec(),
            ));
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            if subtype == "Unknown" {
                assert!(
                    matches!(result, Err(DocumentExtractionError::Malformed { .. })),
                    "{result:?}"
                );
            } else {
                let facts = result.expect("displayed PDF text");
                assert_eq!(facts.text, "Hello PDF");
                assert_eq!(facts.facts.len(), 1);
                assert!(matches!(
                    facts.facts[0].locator,
                    DocumentLocator::Pdf {
                        page: 1,
                        text_start: 0,
                        text_end: 9
                    }
                ));
            }
        }
    }

    #[test]
    fn pdf_image_pixels_do_not_replace_or_invent_page_text() {
        let mut image = lopdf::Dictionary::new();
        image.set("Type", "XObject");
        image.set("Subtype", "Image");
        image.set("Width", 1);
        image.set("Height", 1);
        image.set("ColorSpace", "DeviceRGB");
        image.set("BitsPerComponent", 8);
        let bytes = pdf_with_xobject(lopdf::Stream::new(image, vec![255, 0, 0]));
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("text and image PDF");
        assert_eq!(facts.text, "Hello PDF");
        let mut document = lopdf::Document::load_mem(&bytes).expect("image fixture");
        let subtype = document.add_object(lopdf::Object::Name(b"Image".to_vec()));
        for object in document.objects.values_mut() {
            if let lopdf::Object::Stream(stream) = object
                && stream
                    .dict
                    .get(b"Subtype")
                    .and_then(lopdf::Object::as_name)
                    .ok()
                    == Some(b"Image".as_slice())
            {
                stream.dict.set("Subtype", subtype);
                stream.dict.set("Filter", "DCTDecode");
            }
        }
        let mut indirect = Vec::new();
        document
            .save_to(&mut indirect)
            .expect("fixture serialization");
        let actual = extract_document_text_controlled(&indirect, "guide.pdf", None, &control())
            .expect("indirect image subtype keeps opaque pixels outside decoding");
        assert_eq!(actual, facts);
    }

    #[test]
    fn pdf_form_font_names_do_not_reuse_page_font_encodings() {
        let mut encoding = lopdf::Dictionary::new();
        encoding.set("Type", "Encoding");
        encoding.set("BaseEncoding", "WinAnsiEncoding");
        encoding.set(
            "Differences",
            vec![65.into(), lopdf::Object::Name(b"Z".to_vec())],
        );
        let mut font = lopdf::Dictionary::new();
        font.set("Type", "Font");
        font.set("Subtype", "Type1");
        font.set("BaseFont", "Helvetica");
        font.set("Encoding", encoding);
        let mut fonts = lopdf::Dictionary::new();
        fonts.set("F1", font);
        let mut resources = lopdf::Dictionary::new();
        resources.set("Font", fonts);
        let mut form = lopdf::Dictionary::new();
        form.set("Type", "XObject");
        form.set("Subtype", "Form");
        form.set("BBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        form.set("Resources", resources);
        let bytes = pdf_with_xobject(lopdf::Stream::new(
            form,
            b"BT /F1 12 Tf 72 700 Td (A) Tj ET".to_vec(),
        ));
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("scoped Form font");
        assert!(facts.text.contains("Hello PDF"));
        assert!(facts.text.contains('Z'), "{}", facts.text);
        assert!(!facts.text.contains('A'), "{}", facts.text);
    }

    #[test]
    fn pdf_cid_text_requires_every_character_to_decode() {
        for (encoding, codes, expected, default_width) in [
            ("Identity-H", "0001", Some("Z")),
            (
                "Identity-H",
                "0001> Tj 3 0 Td <0001> Tj 8 0 Td <0001",
                Some("ZZ Z"),
            ),
            ("Identity-H", "00010002", None),
            ("Identity-H", "000100", None),
            ("Identity-V", "0001", None),
            ("custom", "0001", None),
        ]
        .into_iter()
        .map(|(encoding, codes, expected)| (encoding, codes, expected, None))
        .chain([(
            "Identity-H",
            "0001> Tj 18.006 0 Td <0001",
            Some("ZZ"),
            Some(1500.5_f32),
        )]) {
            let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
            let cmap = document.add_object(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture def /CMapType 2 def
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfchar <0001> <005A> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end"
                    .to_vec(),
            ));
            let mut system = lopdf::Dictionary::new();
            system.set("Registry", lopdf::Object::string_literal("Adobe"));
            system.set("Ordering", lopdf::Object::string_literal("Identity"));
            system.set("Supplement", 0);
            let mut descendant = lopdf::Dictionary::new();
            descendant.set("Type", "Font");
            descendant.set("Subtype", "CIDFontType2");
            descendant.set("BaseFont", "Fixture");
            if let Some(width) = default_width {
                document.version = "2.0".to_owned();
                descendant.set("DW", lopdf::Object::Real(width));
            } else {
                descendant.set("W", vec![lopdf::Object::Integer(1), 1.into(), 200.into()]);
            }
            descendant.set("CIDSystemInfo", system);
            let descriptor = document.add_object(lopdf::dictionary! {
                "Type" => "FontDescriptor", "FontName" => "Fixture", "Flags" => 4,
                "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 1000.into()],
                "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200,
                "CapHeight" => 700, "StemV" => 80
            });
            descendant.set("FontDescriptor", descriptor);
            let descendant = document.add_object(descendant);
            let mut font = lopdf::Dictionary::new();
            font.set("Type", "Font");
            font.set("Subtype", "Type0");
            font.set("BaseFont", "Fixture");
            if encoding == "custom" {
                let encoding = document.add_object(lopdf::Stream::new(
                    lopdf::Dictionary::new(),
                    b"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def
/CMapName /FixtureEncoding def /CMapType 1 def /WMode 0 def
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 begincidrange <0000> <FFFF> 0 endcidrange
endcmap CMapName currentdict /CMap defineresource pop end end"
                        .to_vec(),
                ));
                font.set("Encoding", encoding);
            } else {
                font.set("Encoding", encoding);
            }
            font.set(
                "DescendantFonts",
                vec![lopdf::Object::Reference(descendant)],
            );
            font.set("ToUnicode", cmap);
            document.objects.insert((5, 0), font.into());
            document
                .get_object_mut((4, 0))
                .expect("content")
                .as_stream_mut()
                .expect("stream")
                .set_content(format!("BT /F1 12 Tf 72 720 Td <{codes}> Tj ET").into_bytes());
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("fixture serialization");
            let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
            if let Some(expected) = expected {
                assert_eq!(result.expect("mapped horizontal CID").text, expected);
            } else if encoding != "Identity-H" || codes == "00010002" {
                assert!(
                    matches!(result, Err(DocumentExtractionError::UnsupportedPdfInput)),
                    "{result:?}"
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Pdf,
                            ..
                        })
                    ),
                    "{codes}: {result:?}"
                );
            }
        }
    }

    #[test]
    fn pdf_late_malformed_page_never_publishes_a_complete_prefix() {
        let mut document = lopdf::Document::load_mem(&multi_page_pdf()).expect("fixture PDF");
        document.objects.insert(
            (7, 0),
            lopdf::Stream::new(lopdf::Dictionary::new(), b"BT (unterminated".to_vec()).into(),
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        assert!(matches!(
            extract_document_text_controlled(&bytes, "guide.pdf", None, &control()),
            Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Pdf,
                ..
            })
        ));
    }

    #[test]
    fn pdf_decompression_bomb_stops_at_the_first_host_resource_limit() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let mut stream = lopdf::Stream::new(
            lopdf::Dictionary::new(),
            vec![b' '; MAX_DOCUMENT_EXPANDED_BYTES + 1],
        );
        stream.compress().expect("fixture compression");
        document.objects.insert((4, 0), stream.into());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        assert!(bytes.len() < MAX_DOCUMENT_COMPRESSED_BYTES);
        let result = extract_document_text_controlled(&bytes, "guide.pdf", None, &control());
        assert!(
            matches!(
                &result,
                Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::MemoryBytes
                        | DocumentLimit::ExpandedBytes
                        | DocumentLimit::ExecutionFuel,
                    ..
                } | DocumentExtractionError::Work(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded { .. }
                ))
            ),
            "actual refusal: {result:?}"
        );
    }

    #[test]
    fn encrypted_or_password_protected_pdf_is_rejected_without_credentials() {
        let error = extract_document_text_controlled(
            &encrypted_pdf(),
            "secret.pdf",
            Some("pdf"),
            &control(),
        )
        .expect_err("password-protected PDFs must never be decrypted by indexing");
        assert!(matches!(error, DocumentExtractionError::EncryptedPdf));
    }

    #[test]
    fn unsafe_or_duplicate_docx_parts_are_rejected() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file("../word/document.xml", FileOptions::default())
                .expect("fixture entry");
            writer.write_all(b"<w:document/>").expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("path traversal must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::InvalidDocxPackage { .. }
        ));
    }

    #[test]
    fn stored_and_deflated_docx_parts_are_admitted() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>bounded</w:t></w:r></w:p></w:body></w:document>"#;
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let facts = extract_document_text_controlled(
                &docx_archive(xml, method),
                "guide.docx",
                None,
                &control(),
            )
            .expect("admitted DOCX compression");
            assert_eq!(facts.text, "bounded");
        }
    }

    #[test]
    fn unsupported_docx_compression_is_rejected_before_text_read() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let mut bytes = docx_archive(xml, CompressionMethod::Stored);
        rewrite_zip_method(&mut bytes, 12);
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("unsupported compression must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::UnsupportedDocxInput { .. }
        ));
    }

    #[test]
    fn encrypted_docx_is_rejected_before_text_publication() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let mut bytes = docx_archive(xml, CompressionMethod::Stored);
        mark_zip_encrypted(&mut bytes);
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("encrypted packages must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::UnsupportedDocxInput { .. }
        ));
    }

    #[test]
    fn duplicate_docx_parts_are_rejected_before_document_read() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            for value in [b"first".as_slice(), b"second".as_slice()] {
                writer
                    .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                    .expect("fixture entry");
                writer.write_all(value).expect("fixture XML");
            }
            writer.finish().expect("fixture archive");
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("duplicate package parts must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::InvalidDocxPackage { .. }
        ));
    }

    #[test]
    fn unreferenced_embedded_parts_do_not_trigger_recursive_parsing() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Visible</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/embeddings/oleObject1.bin", b"OLE"),
            ("word/embeddings/nested.docx", b"PK\x03\x04"),
        ]);
        let facts = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect("unreferenced embedded parts remain inert");
        assert_eq!(facts.text, "Visible");
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn non_utf8_docx_part_metadata_is_rejected() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file("word/document.xml", FileOptions::default())
                .expect("fixture entry");
            writer
                .write_all(b"<w:document xmlns:w=\"urn:w\"><w:body/></w:document>")
                .expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
        let name = b"word/document.xml";
        let positions = bytes
            .windows(name.len())
            .enumerate()
            .filter_map(|(index, candidate)| (candidate == name).then_some(index))
            .collect::<Vec<_>>();
        for index in positions {
            bytes[index] = 0xff;
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("non-UTF-8 package metadata must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::InvalidDocxPackage { .. }
        ));
    }

    #[test]
    fn docx_entry_count_is_bounded_before_archive_reads() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            for index in 0..=MAX_DOCUMENT_ENTRIES {
                writer
                    .start_file(format!("parts/{index}.xml"), FileOptions::default())
                    .expect("fixture entry");
            }
            writer.finish().expect("fixture archive");
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("entry count must be bounded before archive reads");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::EntryCount,
                observed,
                maximum: MAX_DOCUMENT_ENTRIES
            } if observed == MAX_DOCUMENT_ENTRIES + 1
        ));
    }

    #[test]
    fn docx_part_name_fanout_is_bounded_before_locator_cloning() {
        let long_part = format!("word/{}.xml", "x".repeat(MAX_DOCX_PART_NAME_BYTES));
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, b"<w:document/>"),
            (&long_part, b"<w:hdr/>"),
        ]);
        let error = extract_document_text_controlled(&archive, "long-name.docx", None, &control())
            .expect_err("large part names cannot multiply through thousands of locators");
        assert!(matches!(
            error,
            DocumentExtractionError::InvalidDocxPackage { message }
                if message.contains("part name exceeds")
        ));
    }

    #[test]
    fn docx_expanded_size_is_bounded_before_decompression() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(
                    DOCX_DOCUMENT_PART,
                    FileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .expect("fixture entry");
            writer
                .write_all(&vec![b'x'; MAX_DOCUMENT_EXPANDED_BYTES + 1])
                .expect("fixture payload");
            writer.finish().expect("fixture archive");
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("expanded ZIP size must be bounded before decompression");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ExpandedBytes,
                observed,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES
            } if observed == MAX_DOCUMENT_EXPANDED_BYTES + 1
        ));
    }

    #[test]
    fn docx_actual_expansion_is_bounded_despite_forged_catalog_size() {
        let mut bytes = docx_archive(
            &vec![b'x'; MAX_DOCUMENT_EXPANDED_BYTES + 8192],
            CompressionMethod::Deflated,
        );
        for index in 0..bytes.len().saturating_sub(4) {
            let size_offset = match &bytes[index..index + 4] {
                b"PK\x03\x04" => index + 22,
                b"PK\x01\x02" => index + 24,
                _ => continue,
            };
            bytes[size_offset..size_offset + 4].copy_from_slice(&1_u32.to_le_bytes());
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("actual decompression must be bounded independently of ZIP metadata");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ExpandedBytes,
                observed,
                maximum: MAX_DOCUMENT_EXPANDED_BYTES,
            } if observed > MAX_DOCUMENT_EXPANDED_BYTES
        ));
    }

    #[test]
    fn docx_repeat_reads_charge_actual_decompression_work() {
        let filler = vec![b'x'; MAX_DOCUMENT_EXPANDED_BYTES / 3 + 1];
        let bytes = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#),
            ("word/repeated.bin", &filler),
        ]);
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("fixture ZIP");
        let mut budget = DocxReadBudget::default();
        for _ in 0..5 {
            read_docx_package_part(
                &mut archive,
                "word/repeated.bin",
                &mut budget,
                &control(),
                IndexWorkStage::TextIndex,
            )
            .expect("bounded repeat read");
        }
        assert!(matches!(
            read_docx_package_part(
                &mut archive,
                "word/repeated.bin",
                &mut budget,
                &control(),
                IndexWorkStage::TextIndex,
            ),
            Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::ParserWorkBytes,
                ..
            }),
        ));
    }

    #[test]
    fn docx_namespace_identity_is_independent_of_prefix() {
        let canonical = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p></w:body></w:document>"#;
        let expected = parse_docx(
            canonical.as_bytes(),
            &control(),
            IndexWorkStage::SymbolParsing,
        )
        .expect("canonical WordprocessingML");
        for xml in [
            canonical.replace("w:", "x:").replace("xmlns:w", "xmlns:x"),
            canonical.replace("w:", "").replace("xmlns:w", "xmlns"),
            canonical.replace(
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "http://purl.oclc.org/ooxml/wordprocessingml/main",
            ),
        ] {
            assert_eq!(
                parse_docx(xml.as_bytes(), &control(), IndexWorkStage::SymbolParsing)
                    .expect("equivalent namespace identity"),
                expected,
            );
        }
        let foreign_text = canonical.replace(
            "<w:t>A</w:t>",
            "<foreign:t xmlns:foreign=\"urn:foreign\">A</foreign:t>",
        );
        assert!(matches!(
            parse_docx(
                foreign_text.as_bytes(),
                &control(),
                IndexWorkStage::SymbolParsing
            ),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. }),
        ));
        for xml in [
            canonical.replace(
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "urn:foreign",
            ),
            canonical.replace(
                "<w:t>A</w:t>",
                "<w:t><foreign:t xmlns:foreign=\"urn:foreign\">A</foreign:t></w:t>",
            ),
            canonical.replace("<w:t>A</w:t>", "<unknown:t>A</unknown:t>"),
            canonical.replace("</w:r></w:p>", "</w:p></w:r>"),
        ] {
            assert!(matches!(
                parse_docx(xml.as_bytes(), &control(), IndexWorkStage::SymbolParsing),
                Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    ..
                }),
            ));
        }
    }

    #[test]
    fn docx_ruby_annotations_are_typed_unsupported() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Prefix</w:t><w:ruby><w:rt><w:r><w:t>Reading</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>Base</w:t></w:r></w:rubyBase></w:ruby></w:r></w:p></w:body></w:document>"#;
        assert!(matches!(
            parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. })
        ));
        let deleted = xml
            .replace("<w:ruby>", "<w:del><w:ruby>")
            .replace("</w:ruby>", "</w:ruby></w:del>");
        assert_eq!(
            parse_docx(deleted.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("deleted ruby stays excluded")
                .text,
            "Prefix"
        );
    }

    #[test]
    fn docx_alternate_format_chunk_is_local_incomplete_coverage() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Prefix</w:t></w:r></w:p><w:altChunk r:id="html"/></w:body></w:document>"#;
        let direct = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("unexamined import retains verified surrounding text");
        assert_eq!(direct.text, "Prefix");
        assert_eq!(
            direct.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::UnexaminedStory],
            }
        );
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="html" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/aFChunk" Target="import.html"/></Relationships>"#;
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="html" ContentType="text/html"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let imported = b"<!doctype html><html><body><p>Imported</p></body></html>";
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            (DOCX_DOCUMENT_PART, xml.as_bytes()),
            ("word/_rels/document.xml.rels", rels),
            ("word/import.html", imported),
        ]);
        let packaged = extract_document_text_controlled(&archive, "import.docx", None, &control())
            .expect("safe imported part is local incomplete coverage");
        assert_eq!(packaged.completeness, direct.completeness);
        let sdt = xml.replace(
            "<w:altChunk r:id=\"html\"/>",
            "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder><w:showingPlcHdr w:val=\"false\"/></w:sdtPr><w:sdtContent><w:tbl><w:tr><w:tc><w:altChunk r:id=\"html\"/></w:tc></w:tr></w:tbl></w:sdtContent></w:sdt>",
        );
        let sdt_archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            (DOCX_DOCUMENT_PART, sdt.as_bytes()),
            ("word/_rels/document.xml.rels", rels),
            ("word/import.html", imported),
        ]);
        let sdt_facts =
            extract_document_text_controlled(&sdt_archive, "import.docx", None, &control())
                .expect("actual alternate-format content must not load a glossary placeholder");
        assert_eq!(sdt_facts.text, "Prefix");
        assert_eq!(sdt_facts.completeness, direct.completeness);
        let rels = std::str::from_utf8(rels).expect("ASCII fixture");
        for broken in [
            rels.replace("relationships/aFChunk", "relationships/header"),
            rels.replace("Target=\"import.html\"", "Target=\"../escape.html\""),
            rels.replace(
                "Target=\"import.html\"",
                "Target=\"https://example.invalid/import.html\" TargetMode=\"External\"",
            ),
        ] {
            let archive = docx_archive_with_parts(&[
                ("[Content_Types].xml", manifest),
                (DOCX_DOCUMENT_PART, xml.as_bytes()),
                ("word/_rels/document.xml.rels", broken.as_bytes()),
                ("word/import.html", imported),
            ]);
            assert!(matches!(
                extract_document_text_controlled(&archive, "broken.docx", None, &control()),
                Err(DocumentExtractionError::InvalidDocxPackage { .. }),
            ));
        }
        let deleted = xml.replace(
            "<w:altChunk r:id=\"html\"/>",
            "<w:del><w:altChunk r:id=\"html\"/></w:del>",
        );
        assert_eq!(
            parse_docx(deleted.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("deleted content stays excluded")
                .text,
            "Prefix"
        );
    }

    #[test]
    fn docx_subdocuments_refuse_incomplete_text() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Prefix</w:t></w:r><w:subDoc r:id="child"/></w:p></w:body></w:document>"#;
        for reference in [
            r#"<w:subDoc r:id="child"/>"#,
            r#"<w:subDoc r:id="child"></w:subDoc>"#,
        ] {
            let xml = xml.replace(r#"<w:subDoc r:id="child"/>"#, reference);
            let bytes = docx_archive(xml.as_bytes(), CompressionMethod::Deflated);
            let result = extract_document_text_controlled(&bytes, "guide.docx", None, &control());
            assert!(
                matches!(
                    result,
                    Err(DocumentExtractionError::InvalidDocxPackage { .. })
                ),
                "{result:?}"
            );
            let deleted = xml.replace(reference, &format!("<w:del>{reference}</w:del>"));
            assert_eq!(
                parse_docx(deleted.as_bytes(), &control(), IndexWorkStage::TextIndex)
                    .expect("deleted subdocument remains excluded")
                    .text,
                "Prefix"
            );
        }
    }

    #[test]
    fn docx_utf16_xml_is_decoded_before_wordprocessing_parsing() {
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Text</w:t></w:r></w:p></w:body></w:document>"#;
        for little_endian in [true, false] {
            for bom in [true, false] {
                let mut bytes = Vec::new();
                for unit in bom.then_some(0xfeff).into_iter().chain(xml.encode_utf16()) {
                    bytes.extend_from_slice(&if little_endian {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    });
                }
                assert_eq!(
                    parse_docx(&bytes, &control(), IndexWorkStage::TextIndex)
                        .expect("valid UTF-16 DOCX XML")
                        .text,
                    "Text"
                );
            }
        }
        assert!(matches!(
            parse_docx(
                xml.replace("UTF-16", "ISO-8859-1").as_bytes(),
                &control(),
                IndexWorkStage::TextIndex
            ),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. })
        ));
        for encoding in ["UTF-8", "US-ASCII"] {
            assert!(
                parse_docx(
                    xml.replace("UTF-16", encoding).as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn docx_xml_declarations_must_match_story_and_package_bytes() {
        let story = r#"<?xml version="1.0" encoding="UTF-16"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Text</w:t></w:r></w:p></w:body></w:document>"#;
        assert!(matches!(
            parse_docx(story.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        for (little_endian, wrong_encoding) in
            [(true, "UTF-16BE"), (false, "UTF-16LE"), (true, "UTF-8")]
        {
            let mut encoded = vec![];
            for unit in story.replace("UTF-16", wrong_encoding).encode_utf16() {
                encoded.extend_from_slice(&if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert!(matches!(
                parse_docx(&encoded, &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Text</w:t></w:r></w:p></w:body></w:document>"#;
        let manifest = br#"<?xml version="1.0" encoding="ISO-8859-1"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            (DOCX_DOCUMENT_PART, main),
        ]);
        assert!(matches!(
            extract_document_text_controlled(&archive, "metadata.docx", None, &control()),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. })
        ));
        let root_rels = br#"<?xml version="1.0" encoding="UTF-16"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="main" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let archive =
            docx_archive_with_parts(&[("_rels/.rels", root_rels), (DOCX_DOCUMENT_PART, main)]);
        assert!(matches!(
            extract_document_text_controlled(&archive, "metadata.docx", None, &control()),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        let late_story = format!(" <!--before-->{story}");
        assert!(matches!(
            parse_docx(late_story.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        let late_manifest = format!(" <!--before-->{}", String::from_utf8_lossy(manifest));
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", late_manifest.as_bytes()),
            (DOCX_DOCUMENT_PART, main),
        ]);
        assert!(
            extract_document_text_controlled(&archive, "metadata.docx", None, &control()).is_err()
        );
        let settings_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="settings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
        let late_settings = br#" <!--before--><?xml version="1.0" encoding="UTF-16"?><w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", settings_rels),
            ("word/settings.xml", late_settings),
        ]);
        assert!(matches!(
            extract_document_text_controlled(&archive, "settings.docx", None, &control()),
            Err(DocumentExtractionError::Malformed { .. })
        ));
    }

    #[test]
    fn docx_direct_hidden_runs_do_not_emit_text_symbols_or_references() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:vanish/></w:rPr><w:t>Secret</w:t><w:sym w:font="Wingdings" w:char="F03A"/><w:footnoteReference w:id="1"/></w:r><w:r><w:rPr><w:vanish w:val="false"/></w:rPr><w:sym w:font="Symbol" w:char="F061"/></w:r><w:r><w:rPr><w:webHidden/></w:rPr><w:t>Visible</w:t></w:r></w:p></w:body></w:document>"#;
        let facts = parse_docx(xml, &control(), IndexWorkStage::TextIndex)
            .expect("directly hidden runs do not contribute rendered payload");
        assert_eq!(facts.text, "αVisible");
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        assert!(matches!(
            &facts.symbols[0].locator,
            DocumentLocator::Docx { run: 2, .. }
        ));
        let invalid = String::from_utf8_lossy(xml).replace("w:val=\"false\"", "w:val=\"maybe\"");
        assert!(matches!(
            parse_docx(invalid.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        for (value, visible) in [
            ("", false),
            (" w:val=\"true\"", false),
            (" w:val=\"1\"", false),
            (" w:val=\"on\"", false),
            (" w:val=\"false\"", true),
            (" w:val=\"0\"", true),
            (" w:val=\"off\"", true),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:rPr><w:vanish{value}/></w:rPr><w:t>Text</w:t></w:r></w:p></w:body></w:document>"
            );
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("valid on/off run visibility");
            assert_eq!(facts.text == "Text", visible, "{value}");
        }
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="note" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#;
        let notes = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, xml),
            ("word/_rels/document.xml.rels", rels),
            ("word/footnotes.xml", notes),
        ]);
        let facts =
            extract_document_text_controlled(&archive, "hidden-note.docx", None, &control())
                .expect("a hidden note reference does not reach the note story");
        assert_eq!(facts.text, "αVisible");
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn docx_hidden_outer_run_does_not_publish_nested_text_box() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:vanish/></w:rPr><w:drawing><w:txbxContent><w:p><w:r><w:t>Hidden</w:t><w:sym w:font="Wingdings" w:char="F03A"/><w:footnoteReference w:id="1"/></w:r></w:p></w:txbxContent></w:drawing></w:r><w:r><w:t>Visible</w:t></w:r></w:p><w:p><w:r><w:t>Following</w:t></w:r></w:p></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="note" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#;
        let notes = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/footnotes.xml", notes),
        ]);
        let facts = extract_document_text_controlled(&archive, "hidden-box.docx", None, &control())
            .expect("a hidden outer run hides its text box and note references");
        assert_eq!(facts.text, "Visible\nFollowing");
        assert!(facts.symbols.is_empty());
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        assert!(matches!(
            &facts.facts[1].locator,
            DocumentLocator::Docx { paragraph: 3, .. }
        ));
    }

    #[test]
    fn docx_hidden_field_markers_keep_cached_result_state() {
        let template = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:vanish/></w:rPr><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:rPr><w:vanish/></w:rPr><w:instrText>PAGE</w:instrText><w:t>INSTRUCTION</w:t></w:r><w:r><w:rPr><w:vanish/></w:rPr><w:fldChar w:fldCharType="separate"/></w:r>CACHE<w:r><w:rPr><w:vanish/></w:rPr><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        for (cached, expected, partial) in [("<w:r><w:t>7</w:t></w:r>", "7", false), ("", "", true)]
        {
            let xml = template.replace("CACHE", cached);
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("hidden field markers preserve field structure");
            assert_eq!(facts.text, expected);
            assert_eq!(
                matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnevaluatedField)),
                partial
            );
        }
    }

    #[test]
    fn docx_style_dependent_visibility_is_partial() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:rStyle w:val="Secret"/></w:rPr><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="styles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
        let styles = br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="character" w:styleId="Secret"><w:rPr><w:vanish/></w:rPr></w:style></w:styles>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/styles.xml", styles),
        ]);
        let facts = extract_document_text_controlled(&archive, "styled.docx", None, &control())
            .expect("unresolved style visibility is not a package error");
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnresolvedVisibility))
        );
    }

    #[test]
    fn docx_spec_vanish_without_direct_vanish_is_renderer_dependent() {
        for (properties, text, partial) in [
            ("<w:specVanish/>", "Text", true),
            ("<w:specVanish/><w:vanish/>", "", false),
            ("<w:vanish/><w:specVanish/>", "", false),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:rPr>{properties}</w:rPr><w:t>Text</w:t></w:r></w:p></w:body></w:document>"
            );
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("specVanish is not a package error");
            assert_eq!(facts.text, text);
            assert_eq!(
                matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnresolvedVisibility)),
                partial
            );
        }
    }

    #[test]
    fn docx_foreign_text_is_unsupported_without_hiding_word_text_boxes() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:body><w:p><m:oMath><m:r><m:t>CONTENT</m:t></m:r></m:oMath></w:p></w:body></w:document>"#;
        for content in ["x", "<![CDATA[x]]>", "&#120;"] {
            assert!(matches!(
                parse_docx(
                    xml.replace("CONTENT", content).as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex
                ),
                Err(DocumentExtractionError::UnsupportedDocxInput { .. })
            ));
        }
        assert!(matches!(
            parse_docx(
                xml.replace("CONTENT", "&unknown;").as_bytes(),
                &control(),
                IndexWorkStage::TextIndex
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        assert!(
            parse_docx(
                xml.replace("CONTENT", " ").as_bytes(),
                &control(),
                IndexWorkStage::TextIndex
            )
            .is_ok()
        );
        for content in [
            "<w:sym w:font=\"Symbol\" w:char=\"F061\"/>",
            "<w:t>Secret</w:t>",
            "<w:txbxContent><w:p><w:r><w:t>Secret</w:t></w:r></w:p></w:txbxContent>",
        ] {
            let opaque = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:x=\"urn:opaque\"><w:body><w:p><w:r><x:opaque>{content}</x:opaque></w:r></w:p></w:body></w:document>"
            );
            assert!(matches!(
                parse_docx(opaque.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::UnsupportedDocxInput { .. })
            ));
        }
        for (word, drawing, inline) in [
            (
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "http://schemas.openxmlformats.org/drawingml/2006/main",
                "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
            ),
            (
                "http://purl.oclc.org/ooxml/wordprocessingml/main",
                "http://purl.oclc.org/ooxml/drawingml/main",
                "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
            ),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"{word}\" xmlns:a=\"{drawing}\" xmlns:wp=\"{inline}\"><w:body><w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><w:txbxContent><w:p><w:r><w:t>Box</w:t><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:txbxContent></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"
            );
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("Word text inside recognized drawing wrappers");
            assert_eq!(facts.text, "Boxα\n");
            assert_eq!(facts.symbols.len(), 1);
            assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        }
    }

    #[test]
    fn docx_deleted_revisions_do_not_publish_run_content() {
        for revision in ["del", "moveFrom"] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Before</w:t></w:r><w:{revision}><w:r><w:delText>Removed</w:delText><w:fldChar w:fldCharType=\"begin\"/><w:sym w:font=\"Wingdings\" w:char=\"F020\"/><w:noBreakHyphen/><w:softHyphen/><w:tab/><w:ptab/><w:br/><w:cr/><w:lastRenderedPageBreak/></w:r><w:del><w:r><w:t>Nested</w:t></w:r></w:del></w:{revision}><w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"
            );
            let parsed = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("tracked revision content is valid XML");
            assert!(matches!(
                parse_docx(
                    xml.replace("Removed", "&unknown;").as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex
                ),
                Err(DocumentExtractionError::Malformed { .. })
            ));
            assert_eq!(parsed.text, "BeforeAfter", "{revision}");
            assert_eq!(parsed.facts.len(), 2);
            assert_eq!(parsed.facts[1].line_start, 1);
            assert_eq!(parsed.facts[1].line_end, 1);
            assert_eq!(
                parsed.facts[1].locator,
                DocumentLocator::Docx {
                    part: DOCX_DOCUMENT_PART.to_owned(),
                    paragraph: 1,
                    run: 4,
                    text_start: 0,
                    text_end: 5,
                }
            );
        }
    }

    #[test]
    fn docx_dynamic_text_blocks_refuse_without_evaluation() {
        let template = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><w:body><w:p><w:fldSimple w:instr="PAGE"><w:r><w:t>7</w:t></w:r></w:fldSimple>BLOCK</w:p></w:body></w:document>"#;
        assert_eq!(
            parse_docx(
                template.replace("BLOCK", "").as_bytes(),
                &control(),
                IndexWorkStage::TextIndex
            )
            .expect("cached field result remains literal text")
            .text,
            "7"
        );
        for name in [
            "pgNum",
            "dayShort",
            "dayLong",
            "monthShort",
            "monthLong",
            "yearShort",
            "yearLong",
            "footnoteReference",
            "endnoteReference",
        ] {
            let attributes = if matches!(name, "footnoteReference" | "endnoteReference") {
                " w:id=\"1\""
            } else {
                ""
            };
            for element in [
                format!("<w:{name}{attributes}/>"),
                format!("<w:{name}{attributes}></w:{name}>"),
            ] {
                let run = format!("<w:r>{element}</w:r>");
                let facts = parse_docx(
                    template.replace("BLOCK", &run).as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex,
                )
                .expect("dynamic marker retains surrounding text with partial coverage");
                assert!(facts.text.contains('\u{fffc}'), "{name}");
                assert!(
                    matches!(facts.completeness, DocumentCompleteness::Partial { .. }),
                    "{name}"
                );
                for discarded in [
                    format!("<w:del>{run}</w:del>"),
                    format!("<w:moveFrom>{run}</w:moveFrom>"),
                    format!(
                        r#"<mc:AlternateContent><mc:Choice Requires="future">{run}</mc:Choice><mc:Fallback/></mc:AlternateContent>"#
                    ),
                ] {
                    assert_eq!(
                        parse_docx(
                            template.replace("BLOCK", &discarded).as_bytes(),
                            &control(),
                            IndexWorkStage::TextIndex
                        )
                        .expect("discarded dynamic text does not require evaluation")
                        .text,
                        "7",
                        "{name}"
                    );
                }
            }
        }
    }

    #[test]
    fn docx_field_carriers_retain_only_literal_and_cached_text() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t xml:space="preserve">Page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE &amp; <![CDATA[ignored]]></w:instrText><w:t>NOT RENDERED</w:t></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText xml:space="preserve"> Literal</w:instrText></w:r><w:r><w:delInstrText>Deleted code</w:delInstrText><w:delText><![CDATA[Deleted text]]></w:delText></w:r></w:p></w:body></w:document>"#;
        let parsed = extract_document_text_controlled(
            &docx_archive(xml, CompressionMethod::Deflated),
            "fields.docx",
            None,
            &control(),
        )
        .expect("field instructions and deleted carriers are valid bounded XML");
        assert_eq!(parsed.text, "Page 7 Literal");
        assert_eq!(parsed.completeness, DocumentCompleteness::Complete);
        let escaped_markers = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="b&#x65;gin"/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType="separ&#x61;te"/><w:t>7</w:t><w:fldChar w:fldCharType="e&#x6e;d"/></w:r></w:p></w:body></w:document>"#;
        let escaped = parse_docx(escaped_markers, &control(), IndexWorkStage::TextIndex)
            .expect("field marker values may contain XML character references");
        assert_eq!(escaped.text, "7");
        assert_eq!(escaped.completeness, DocumentCompleteness::Complete);
        let spaced_field = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>DATE</w:instrText><w:fldChar w:fldCharType="separate"/><w:t xml:space="preserve"> </w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let spaced = parse_docx(spaced_field, &control(), IndexWorkStage::TextIndex)
            .expect("retained whitespace is a cached field result");
        assert_eq!(spaced.text, " ");
        assert_eq!(spaced.completeness, DocumentCompleteness::Complete);
        for field in [
            "<w:fldSimple w:instr=\"PAGE\"/>",
            "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> DATE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>",
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Before</w:t></w:r>{field}<w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"
            );
            let parsed = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("live field without cached text retains surrounding text");
            assert_eq!(parsed.text, "BeforeAfter");
            assert!(
                matches!(parsed.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnevaluatedField)),
                "{field}"
            );
        }
        assert_eq!(parsed.facts.len(), 3);
        for (fact, (run, text)) in
            parsed
                .facts
                .iter()
                .zip([(1, "Page "), (5, "7"), (7, " Literal")])
        {
            assert_eq!(fact.text, text);
            assert_eq!(
                fact.locator,
                DocumentLocator::Docx {
                    part: DOCX_DOCUMENT_PART.to_owned(),
                    paragraph: 1,
                    run,
                    text_start: 0,
                    text_end: text.len(),
                }
            );
        }
        let nested = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>OUTER</w:instrText><w:drawing><w:txbxContent><w:p><w:r><w:instrText>Box</w:instrText></w:r></w:p></w:txbxContent></w:drawing><w:fldChar w:fldCharType="begin"/><w:instrText>INNER</w:instrText><w:fldChar w:fldCharType="separate"/><w:instrText>Outer code remains ignored</w:instrText><w:fldChar w:fldCharType="end"/><w:fldChar w:fldCharType="separate"/><w:t>Result</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let nested = parse_docx(nested, &control(), IndexWorkStage::TextIndex)
            .expect("text boxes inside field instructions do not publish code text");
        assert_eq!(nested.text, "Result");
        assert_eq!(nested.facts.len(), 1);
        assert_eq!(nested.completeness, DocumentCompleteness::Complete);
        let simple_in_instruction = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>IF</w:instrText></w:r><w:fldSimple w:instr="PAGE"/><w:r><w:fldChar w:fldCharType="separate"/><w:t>Result</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let simple = parse_docx(simple_in_instruction, &control(), IndexWorkStage::TextIndex)
            .expect("simple fields inside an outer instruction cannot render");
        assert_eq!(simple.text, "Result");
        assert_eq!(simple.completeness, DocumentCompleteness::Complete);
        let symbol_field = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:sym w:font="Wingdings" w:char="F03A"/><w:fldChar w:fldCharType="separate"/><w:sym w:font="Symbol" w:char="F061"/><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let symbol_field = parse_docx(symbol_field, &control(), IndexWorkStage::TextIndex)
            .expect("only the cached field result is rendered");
        assert_eq!(symbol_field.text, "α");
        assert_eq!(symbol_field.symbols.len(), 1);
        assert_eq!(symbol_field.completeness, DocumentCompleteness::Complete);
        let instruction_refs = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>FIELD</w:instrText><w:footnoteReference w:id="2"/><w:endnoteReference w:id="3"/><w:commentReference w:id="4"/><w:commentRangeStart w:id="4"/><w:commentRangeEnd w:id="4"/><w:pgNum/><w:tab/><w:fldChar w:fldCharType="separate"/><w:t>Visible</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let mut references = DocxStoryReferences::default();
        let instruction_refs = parse_docx_part(
            instruction_refs,
            DOCX_DOCUMENT_PART,
            "document",
            None,
            None,
            &mut references,
            &control(),
            IndexWorkStage::TextIndex,
        )
        .expect("field instructions cannot publish referenced stories or dynamic text");
        assert_eq!(instruction_refs.text, "Visible");
        assert!(instruction_refs.symbols.is_empty());
        assert!(references.items.is_empty());
        assert_eq!(
            instruction_refs.completeness,
            DocumentCompleteness::Complete
        );
        let text = std::str::from_utf8(xml).expect("UTF-8 fixture");
        for invalid in [
            text.replace("PAGE &amp; <![CDATA[ignored]]>", "<w:t>nested</w:t>"),
            text.replace("PAGE &amp; <![CDATA[ignored]]>", "&unknown;"),
            text.replace("<w:p>", "<w:p><w:instrText>outside run</w:instrText>"),
        ] {
            assert!(matches!(
                parse_docx(invalid.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        let deep = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r>{}</w:r></w:p></w:body></w:document>",
            "<w:fldChar w:fldCharType=\"begin\"/>".repeat(MAX_DOCX_XML_DEPTH + 1),
        );
        assert!(matches!(
            parse_docx(deep.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::NestingDepth,
                ..
            })
        ));
    }

    #[test]
    fn docx_field_result_in_text_box_is_cached_only_when_content_is_retained() {
        for (content, retained) in [
            ("<w:t>7</w:t>", true),
            ("<w:sym w:font=\"Symbol\" w:char=\"F061\"/>", true),
            ("", false),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType=\"separate\"/><w:drawing><w:txbxContent><w:p><w:r>{content}</w:r></w:p></w:txbxContent></w:drawing><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:body></w:document>"
            );
            let parsed = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("a text box can contain a field's cached result");
            assert_eq!(
                parsed.completeness == DocumentCompleteness::Complete,
                retained,
                "{content}"
            );
            assert_eq!(parsed.facts.is_empty(), !retained, "{content}");
        }
    }

    #[test]
    fn docx_root_ignorable_policy_preserves_visible_text_and_refuses_other_policies() {
        use std::fmt::Write as _;
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future" mc:Ignorable="future"><w:body><future:wrapper><w:p><w:r><w:t>Ignored</w:t></w:r></w:p></future:wrapper><w:p><w:r><w:t>Visible</w:t></w:r></w:p></w:body></w:document>"#;
        for source in [
            xml.to_owned(),
            xml.replace("<future:wrapper><w:p><w:r><w:t>Ignored</w:t></w:r></w:p></future:wrapper>", ""),
            xml.replace("mc:Ignorable=\"future\"", &format!("mc:Ignorable=\"{}\"", "future ".repeat(128))),
            xml.replace("<future:wrapper>", "<mc:AlternateContent><mc:Choice Requires=\"future\" mc:ProcessContent=\"future:wrapper\"><future:wrapper>").replace("</future:wrapper>", "</future:wrapper></mc:Choice><mc:Fallback/></mc:AlternateContent>"),
            xml.replace(
                "<future:wrapper>",
                "<alias:wrapper xmlns:alias=\"urn:future\">",
            )
            .replace("</future:wrapper>", "</alias:wrapper>"),
            xml.replace(
                "mc:Ignorable=\"future\"",
                "mc:Ignorable=\"future future w\"",
            ),
            xml.replace(
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "http://purl.oclc.org/ooxml/wordprocessingml/main",
            ),
            xml.replace("mc:", "compat:")
                .replace("xmlns:mc=", "xmlns:compat="),
        ] {
            let parsed = parse_docx(source.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("ignore only unknown root-policy namespaces");
            assert_eq!(parsed.text, "Visible");
            assert_eq!(parsed.facts.len(), 1);
            assert!(matches!(
                parsed.facts[0].locator,
                DocumentLocator::Docx {
                    paragraph: 1,
                    run: 1,
                    text_start: 0,
                    text_end: 7,
                    ..
                }
            ));
        }
        let deleted = xml
            .replace("<future:wrapper>", "<w:del mc:Ignorable=\"future\">")
            .replace("</future:wrapper>", "</w:del>");
        let parsed = parse_docx(deleted.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("deleted policies do not affect live text");
        assert_eq!(parsed.text, "Visible");
        assert!(matches!(
            parsed.facts[0].locator,
            DocumentLocator::Docx {
                paragraph: 2,
                run: 1,
                text_start: 0,
                text_end: 7,
                ..
            }
        ));
        for source in [
            xml.replace("mc:Ignorable=\"future\"", "mc:Ignorable=\"\""),
            xml.replace("mc:Ignorable=\"future\"", "mc:Ignorable=\"w\""),
            xml.replace(
                "<future:wrapper>",
                "<future:wrapper xmlns:future=\"urn:different\">",
            ),
            xml.replace("mc:Ignorable=\"future\"", "Ignorable=\"future\""),
        ] {
            assert!(matches!(
                parse_docx(source.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::UnsupportedDocxInput { .. })
            ));
        }
        for source in [
            xml.replace(
                "mc:Ignorable=\"future\"",
                "mc:ProcessContent=\"future:wrapper\"",
            ),
            xml.replace("<future:wrapper>", "<future:wrapper mc:ProcessContent=\"future:child\">"),
            xml.replace("<future:wrapper>", "<future:wrapper mc:MustUnderstand=\"future\">"),
            xml.replace("<future:wrapper>", "<future:wrapper mc:Ignorable=\"future\">"),
            xml.replace("<w:body>", "<w:body><mc:AlternateContent><mc:Choice Requires=\"w\" mc:ProcessContent=\"future:wrapper\">").replace("</w:body>", "</mc:Choice></mc:AlternateContent></w:body>"),
            xml.replace("mc:Ignorable=\"future\"", "mc:MustUnderstand=\"future\""),
            xml.replace(" mc:Ignorable=\"future\"", "")
                .replace("<w:body>", "<w:body mc:Ignorable=\"future\">"),
        ] {
            assert!(matches!(
                parse_docx(source.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::UnsupportedDocxInput { .. })
            ));
        }
        for policy in ["missing", "future:wrapper"] {
            assert!(matches!(
                parse_docx(
                    xml.replace(
                        "mc:Ignorable=\"future\"",
                        &format!("mc:Ignorable=\"{policy}\"")
                    )
                    .as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex
                ),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        for count in [
            MAX_DOCX_IGNORABLE_NAMESPACES,
            MAX_DOCX_IGNORABLE_NAMESPACES + 1,
        ] {
            let mut declarations = String::new();
            let mut prefixes = String::new();
            for index in 0..count {
                write!(declarations, "xmlns:n{index}=\"urn:{index}\" ")
                    .expect("namespace declaration");
                write!(prefixes, "n{index} ").expect("namespace prefix");
            }
            let source = xml
                .replace(
                    "<future:wrapper><w:p><w:r><w:t>Ignored</w:t></w:r></w:p></future:wrapper>",
                    "",
                )
                .replace(
                    "mc:Ignorable=\"future\"",
                    &format!("{declarations} mc:Ignorable=\"{prefixes}\""),
                );
            let bytes = docx_archive(source.as_bytes(), CompressionMethod::Deflated);
            let result = extract_document_text_controlled(&bytes, "guide.docx", None, &control());
            if count == MAX_DOCX_IGNORABLE_NAMESPACES {
                assert_eq!(result.expect("bounded distinct namespaces").text, "Visible");
            } else {
                assert!(matches!(
                    result,
                    Err(DocumentExtractionError::UnsupportedDocxInput { .. })
                ));
            }
        }
    }

    #[test]
    fn docx_compatibility_selects_one_understood_branch() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><w:body><mc:AlternateContent><mc:Choice Requires="future"><w:p><w:r><w:t>Unsupported</w:t></w:r></w:p></mc:Choice><mc:Choice Requires="w"><w:p><w:r><w:t>Chosen</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>Fallback</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent></w:body></w:document>"#;
        let parsed = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("uncertain earlier choice is not guessed");
        assert!(parsed.text.is_empty());
        assert!(
            matches!(parsed.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let fallback = xml.replace("Requires=\"w\"", "Requires=\"future\"");
        let parsed = parse_docx(fallback.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("fallback when no choice is understood");
        assert!(parsed.text.is_empty());
        assert!(
            matches!(parsed.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let nested = xml.replace("<w:t>Chosen</w:t>", "<mc:AlternateContent><mc:Choice Requires=\"future\"><w:instrText>Discarded</w:instrText></mc:Choice><mc:Fallback><w:t>Nested</w:t></mc:Fallback></mc:AlternateContent>");
        let parsed = parse_docx(nested.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("nested alternatives preserve the containing run");
        assert!(parsed.text.is_empty());
        for first in [
            xml.replace("Requires=\"future\"", "Requires=\"w\""),
            xml.replace(
                "xmlns:future=\"urn:future\"",
                "xmlns:future=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"",
            ),
        ] {
            let parsed = parse_docx(first.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("first understood branch and namespace aliases");
            assert_eq!(parsed.text, "Unsupported");
            assert_eq!(parsed.facts.len(), 1);
        }
        for invalid in [
            xml.replace("Requires=\"future\"", "Requires=\"\""),
            xml.replace("<mc:AlternateContent>", "<mc:AlternateContent><w:p/>"),
            xml.replace("</mc:AlternateContent>", "<mc:Fallback/></mc:AlternateContent>"),
            xml.replace("<mc:AlternateContent>", "<mc:AlternateContent><mc:Fallback/>"),
            xml.replace("<mc:AlternateContent>", "<mc:AlternateContent><mc:Choice Requires=\"future\"><w:r><w:t>&unknown;</w:t></w:r></mc:Choice>"),
        ] {
            assert!(matches!(parse_docx(invalid.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::Malformed { .. })));
        }
    }

    #[test]
    fn docx_selected_compatibility_run_properties_hide_payload() {
        for properties in [
            "<mc:AlternateContent><mc:Choice Requires=\"w\"><w:rPr><w:vanish/></w:rPr></mc:Choice><mc:Fallback><w:rPr/></mc:Fallback></mc:AlternateContent>",
            "<w:rPr><mc:AlternateContent><mc:Choice Requires=\"w\"><w:vanish/></mc:Choice><mc:Fallback/></mc:AlternateContent></w:rPr>",
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><w:body><w:p><w:r>{properties}<w:t>Secret</w:t><w:sym w:font=\"Symbol\" w:char=\"F061\"/><w:footnoteReference w:id=\"1\"/></w:r><w:r><w:t>Visible</w:t></w:r></w:p></w:body></w:document>"
            );
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("selected compatibility formatting hides its run");
            assert_eq!(facts.text, "Visible");
            assert!(facts.symbols.is_empty());
            assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        }
    }

    #[test]
    fn docx_unknown_compatibility_choice_never_claims_complete_symbol_coverage() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></mc:Choice><mc:Choice Requires="w"><w:p><w:r><w:sym w:font="Symbol" w:char="F063"/></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:sym w:font="Symbol" w:char="F062"/></w:r></w:p></mc:Fallback></mc:AlternateContent></w:body></w:document>"#;
        let facts = parse_docx(xml, &control(), IndexWorkStage::TextIndex)
            .expect("fallback remains bounded evidence");
        assert!(facts.text.is_empty());
        assert!(facts.symbols.is_empty());
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
    }

    #[test]
    fn docx_nested_text_boxes_preserve_run_order_and_locators() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t><w:drawing><w:txbxContent><w:p><w:r><w:t>Inside</w:t></w:r></w:p></w:txbxContent></w:drawing><w:t>After</w:t></w:r></w:p><w:p><w:r><w:t>Following</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = docx_archive(xml, CompressionMethod::Stored);
        let parsed = extract_document_text_controlled(&bytes, "text-box.docx", None, &control())
            .expect("nested text container is valid WordprocessingML");
        assert_eq!(parsed.text, "Before\nInside\nAfter\nFollowing");
        assert_eq!(parsed.completeness, DocumentCompleteness::Complete);
        assert_eq!(parsed.facts.len(), 4);
        for (fact, (text, paragraph, start, end, line)) in parsed.facts.iter().zip([
            ("Before", 1, 0, 6, 1),
            ("Inside", 2, 0, 6, 2),
            ("After", 1, 6, 11, 3),
            ("Following", 3, 0, 9, 4),
        ]) {
            assert_eq!(fact.text, text);
            assert_eq!((fact.line_start, fact.line_end), (line, line));
            assert_eq!(
                fact.locator,
                DocumentLocator::Docx {
                    part: DOCX_DOCUMENT_PART.to_owned(),
                    paragraph,
                    run: 1,
                    text_start: start,
                    text_end: end,
                }
            );
        }
    }

    #[test]
    fn docx_separate_text_boxes_have_distinct_symbol_locators() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:drawing><w:txbxContent><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:txbxContent></w:drawing><w:drawing><w:txbxContent><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:txbxContent></w:drawing></w:r></w:p></w:body></w:document>"#;
        let facts = parse_docx(xml, &control(), IndexWorkStage::TextIndex)
            .expect("both text boxes have rendered symbols");
        assert_eq!(facts.symbols.len(), 2);
        assert_ne!(facts.symbols[0].locator, facts.symbols[1].locator);
    }

    #[test]
    fn docx_note_item_locators_stay_distinct_across_incremental_parses() {
        let xml = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let parse = |id: &str| {
            parse_docx_part(
                xml,
                "word/footnotes.xml",
                "footnotes",
                Some(&HashSet::from([id.to_owned()])),
                None,
                &mut DocxStoryReferences::default(),
                &control(),
                IndexWorkStage::TextIndex,
            )
        };
        let first = parse("1").expect("first note");
        let second = parse("2").expect("second note");
        assert_ne!(first.symbols[0].locator, second.symbols[0].locator);
    }

    #[test]
    fn docx_story_ids_match_decimal_value_and_reject_numeric_duplicates() {
        for (kind, reference, root, item) in [
            ("footnotes", "footnoteReference", "footnotes", "footnote"),
            ("endnotes", "endnoteReference", "endnotes", "endnote"),
            ("comments", "commentReference", "comments", "comment"),
        ] {
            let marker = if kind == "comments" {
                "<w:commentRangeStart w:id=\"01\"/><w:r><w:commentReference w:id=\"+01\"/></w:r>"
                    .to_owned()
            } else {
                format!("<w:r><w:{reference} w:id=\"+01\"/></w:r>")
            };
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p>{marker}</w:p></w:body></w:document>"
            );
            let rels = format!(
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"story\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{kind}.xml\"/></Relationships>"
            );
            let story = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:{item} w:id=\"1\"><w:p><w:r><w:t>Matched</w:t></w:r></w:p></w:{item}></w:{root}>"
            );
            let part = format!("word/{kind}.xml");
            let package = |story: &str| {
                docx_archive_with_parts(&[
                    (DOCX_DOCUMENT_PART, main.as_bytes()),
                    ("word/_rels/document.xml.rels", rels.as_bytes()),
                    (&part, story.as_bytes()),
                ])
            };
            let facts = extract_document_text_controlled(
                &package(&story),
                "numeric-ids.docx",
                None,
                &control(),
            )
            .expect("decimal-equivalent story reference resolves");
            assert!(facts.text.contains("Matched"), "missing {kind} item");
            let duplicate = story.replace(
                &format!("</w:{root}>"),
                &format!("<w:{item} w:id=\"001\"/></w:{root}>"),
            );
            assert!(matches!(
                extract_document_text_controlled(
                    &package(&duplicate),
                    "duplicate-ids.docx",
                    None,
                    &control(),
                ),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        for (raw, canonical) in [("+0001", "1"), ("-00", "0"), (" -001 ", "-1")] {
            assert_eq!(
                canonical_docx_story_id(raw).expect("valid XML decimal story ID"),
                canonical
            );
        }
        for raw in ["", "+", "1.0", "1e0", "1 0"] {
            assert!(matches!(
                canonical_docx_story_id(raw),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
    }

    #[test]
    fn docx_compatibility_wrapped_story_items_follow_selected_ids() {
        for (root, item) in [
            ("footnotes", "footnote"),
            ("endnotes", "endnote"),
            ("comments", "comment"),
        ] {
            let xml = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:AlternateContent><mc:Choice Requires=\"w\"><w:{item} w:id=\"1\"><w:p><w:r><w:t>Wrapped</w:t></w:r></w:p></w:{item}></mc:Choice><mc:Fallback><w:{item} w:id=\"1\"><w:p><w:r><w:t>Unselected</w:t></w:r></w:p></w:{item}></mc:Fallback></mc:AlternateContent><w:{item} w:id=\"2\"><w:p><w:r><w:t>Ordinary</w:t></w:r></w:p></w:{item}></w:{root}>"
            );
            let parse = |id: &str| {
                parse_docx_part(
                    xml.as_bytes(),
                    "word/story.xml",
                    root,
                    Some(&HashSet::from([id.to_owned()])),
                    None,
                    &mut DocxStoryReferences::default(),
                    &control(),
                    IndexWorkStage::TextIndex,
                )
            };
            let selected = parse("1").expect("wrapped selected story item is found");
            assert_eq!(selected.text, "Wrapped", "{root}");
            let ordinary = parse("2").expect("unreferenced wrapped item is skipped");
            assert_eq!(ordinary.text, "Ordinary", "{root}");
        }
    }

    #[test]
    fn docx_wrapped_continuation_note_does_not_claim_later_text() {
        for (root, item) in [("footnotes", "footnote"), ("endnotes", "endnote")] {
            let xml = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:AlternateContent><mc:Choice Requires=\"w\"><w:{item} w:id=\"1\" w:type=\"continuationSeparator\"><w:p/></w:{item}><w:{item} w:id=\"2\"><w:p><w:r><w:t>Normal</w:t></w:r></w:p></w:{item}></mc:Choice><mc:Fallback/></mc:AlternateContent></w:{root}>"
            );
            let facts = parse_docx_part(
                xml.as_bytes(),
                "word/notes.xml",
                root,
                Some(&HashSet::from(["2".to_owned()])),
                Some(&HashSet::from(["1".to_owned()])),
                &mut DocxStoryReferences::default(),
                &control(),
                IndexWorkStage::TextIndex,
            )
            .expect("empty continuation note cannot taint a later note");
            assert_eq!(facts.text, "Normal");
            assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        }
    }

    #[test]
    fn docx_note_special_items_are_included_once_and_conditional_items_are_partial() {
        for (root, item) in [("footnotes", "footnote"), ("endnotes", "endnote")] {
            let xml = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:{item} w:id=\"0\" w:type=\"separator\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}><w:{item} w:id=\"1\" w:type=\"continuationSeparator\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}><w:{item} w:id=\"-1\" w:type=\"continuationNotice\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}><w:{item} w:id=\"2\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}><w:{item} w:id=\"3\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}><w:{item} w:id=\"99\"><w:p><w:r><w:sym w:font=\"Wingdings\" w:char=\"F03A\"/></w:r></w:p></w:{item}></w:{root}>"
            );
            let special_ids = HashSet::from(["0".to_owned(), "1".to_owned(), "-1".to_owned()]);
            let parse = |id: &str, selected_special_ids: Option<&HashSet<String>>| {
                parse_docx_part(
                    xml.as_bytes(),
                    "word/notes.xml",
                    root,
                    Some(&HashSet::from([id.to_owned()])),
                    selected_special_ids,
                    &mut DocxStoryReferences::default(),
                    &control(),
                    IndexWorkStage::TextIndex,
                )
            };
            let first = parse("2", Some(&special_ids)).expect("referenced note and special items");
            assert!(matches!(
                parse("0", Some(&special_ids)),
                Err(DocumentExtractionError::InvalidDocxPackage { .. })
            ));
            assert!(matches!(
                parse("2", Some(&HashSet::from(["777".to_owned()]))),
                Err(DocumentExtractionError::InvalidDocxPackage { .. })
            ));
            assert_eq!(first.symbols.len(), 4);
            assert!(
                first
                    .symbols
                    .iter()
                    .all(|symbol| symbol.unicode == Some('α'))
            );
            assert!(
                matches!(first.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::ConditionalStory))
            );
            let later = parse("3", None).expect("new note without duplicate special items");
            assert_eq!(later.symbols.len(), 1);
            assert_eq!(later.completeness, DocumentCompleteness::Complete);
            let no_text = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:{item} w:id=\"1\" w:type=\"continuationSeparator\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{item}><w:{item} w:id=\"2\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:{item}></w:{root}>"
            );
            let no_text = parse_docx_part(
                no_text.as_bytes(),
                "word/notes.xml",
                root,
                Some(&HashSet::from(["2".to_owned()])),
                Some(&HashSet::from(["1".to_owned()])),
                &mut DocxStoryReferences::default(),
                &control(),
                IndexWorkStage::TextIndex,
            )
            .expect("non-text continuation marker does not hide a symbol");
            assert_eq!(no_text.symbols.len(), 1);
            assert_eq!(no_text.completeness, DocumentCompleteness::Complete);
            let conditional_comment = format!(
                "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:{item} w:id=\"1\" w:type=\"continuationSeparator\"><w:p><w:commentRangeStart w:id=\"4\"/></w:p></w:{item}><w:{item} w:id=\"2\"><w:p><w:r><w:t>Normal</w:t></w:r></w:p></w:{item}></w:{root}>"
            );
            let mut references = DocxStoryReferences::default();
            let conditional_comment = parse_docx_part(
                conditional_comment.as_bytes(),
                "word/notes.xml",
                root,
                Some(&HashSet::from(["2".to_owned()])),
                Some(&HashSet::from(["1".to_owned()])),
                &mut references,
                &control(),
                IndexWorkStage::TextIndex,
            )
            .expect("comment reference in a continuation item remains conditional");
            assert_eq!(conditional_comment.text, "Normal");
            assert_eq!(
                references.items,
                vec![(DocxStoryKind::Comments, "4".to_owned())]
            );
            assert!(matches!(
                conditional_comment.completeness,
                DocumentCompleteness::Partial { ref gaps }
                    if gaps.contains(&DocumentCoverageGap::ConditionalStory)
            ));
            for (id, item_type, special) in [("2", "", false), ("0", " w:type=\"separator\"", true)]
            {
                let duplicate = format!(
                    "<w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:{item} w:id=\"{id}\"{item_type}><w:p><w:r><w:t>First</w:t></w:r></w:p></w:{item}><w:{item} w:id=\"{id}\"{item_type}><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:{item}></w:{root}>"
                );
                let selected_ids = HashSet::from(["2".to_owned()]);
                let special_ids = HashSet::from(["0".to_owned()]);
                assert!(matches!(
                    parse_docx_part(
                        duplicate.as_bytes(),
                        "word/notes.xml",
                        root,
                        Some(&selected_ids),
                        special.then_some(&special_ids),
                        &mut DocxStoryReferences::default(),
                        &control(),
                        IndexWorkStage::TextIndex,
                    ),
                    Err(DocumentExtractionError::Malformed { .. })
                ));
            }
        }
        let comments = br#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="4"><w:p><w:r><w:t>First</w:t></w:r></w:p></w:comment><w:comment w:id="4"><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:comment></w:comments>"#;
        assert!(matches!(
            parse_docx_part(
                comments,
                "word/comments.xml",
                "comments",
                Some(&HashSet::from(["4".to_owned()])),
                None,
                &mut DocxStoryReferences::default(),
                &control(),
                IndexWorkStage::TextIndex,
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
    }

    #[test]
    fn docx_utf16_normalization_checks_peak_memory_before_allocation() {
        let mut xml = vec![0_u8; MAX_DOCUMENT_EXPANDED_BYTES];
        xml[..2].copy_from_slice(&[0xff, 0xfe]);
        for pair in xml[2..].as_chunks_mut::<2>().0 {
            pair.copy_from_slice(&0x0800_u16.to_le_bytes());
        }
        assert!(matches!(
            decode_docx_xml(xml, &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::MemoryBytes,
                ..
            })
        ));
    }

    #[test]
    fn docx_namespace_storage_is_charged_before_parser_allocation() {
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:unused=\"{}\"><w:body/></w:document>",
            "x".repeat(24 * 1024 * 1024),
        );
        let bytes = docx_archive(xml.as_bytes(), CompressionMethod::Deflated);
        let facts = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect("safe package memory ceiling is document-local incomplete coverage");
        assert_eq!(
            facts.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::ResourceLimit(
                    DocumentLimit::MemoryBytes
                )],
            }
        );
    }

    #[test]
    fn docx_empty_elements_preserve_paragraph_and_run_ordinals() {
        let xml = b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p/><w:p><w:r/><w:r><w:t>A</w:t><w:tab/><w:t/><w:t>B</w:t></w:r></w:p></w:body></w:document>";
        let facts = parse_docx(xml, &control(), IndexWorkStage::SymbolParsing)
            .expect("empty elements are valid document structure");
        assert_eq!(facts.text, "A\tB");
        assert_eq!(facts.facts.len(), 1);
        assert!(matches!(
            facts.facts[0].locator,
            DocumentLocator::Docx {
                paragraph: 2,
                run: 2,
                text_start: 0,
                text_end: 3,
                ..
            }
        ));
        let expanded = String::from_utf8(xml.to_vec())
            .expect("UTF-8 fixture")
            .replace("<w:p/>", "<w:p></w:p>")
            .replace("<w:r/>", "<w:r></w:r>")
            .replace("<w:t/>", "<w:t></w:t>")
            .replace("<w:tab/>", "<w:tab></w:tab>");
        let expanded_facts = parse_docx(
            expanded.as_bytes(),
            &control(),
            IndexWorkStage::SymbolParsing,
        )
        .expect("equivalent explicit empty elements");
        assert_eq!(facts, expanded_facts);
    }

    #[test]
    fn docx_xml_nesting_is_bounded() {
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}<w:body/>{}</w:document>",
            "<w:container>".repeat(MAX_DOCX_XML_DEPTH),
            "</w:container>".repeat(MAX_DOCX_XML_DEPTH),
        );
        let error = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::SymbolParsing)
            .expect_err("XML nesting must be bounded");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::NestingDepth,
                observed,
                maximum: MAX_DOCX_XML_DEPTH,
            } if observed == MAX_DOCX_XML_DEPTH + 1
        ));
    }

    #[test]
    fn docx_aggregate_output_is_bounded_before_reading_remaining_xml() {
        let run = "x".repeat(MAX_DOCUMENT_OUTPUT_BYTES / 2 + 1);
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>{run}</w:t></w:r><w:r><w:t>{run}</w:t></w:r><malformed"
        );
        let error = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::SymbolParsing)
            .expect_err("aggregate output must fail before the later malformed XML");
        assert!(matches!(
            error,
            DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::OutputBytes,
                observed,
                maximum: MAX_DOCUMENT_OUTPUT_BYTES,
            } if observed == MAX_DOCUMENT_OUTPUT_BYTES + 2
        ));
    }

    #[test]
    fn direct_xml_extracts_body_and_table_locators() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> world</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body>
            </w:document>"#;
        let bytes = docx_archive(xml, CompressionMethod::Stored);
        let facts = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect("valid DOCX");
        assert_eq!(facts.text, "Hello world\nCell");
        assert_eq!(facts.facts.len(), 3);
        assert!(matches!(
            &facts.facts[2].locator,
            DocumentLocator::Docx {
                part,
                paragraph: 2,
                run: 1,
                text_start: 0,
                text_end: 4
            } if part == DOCX_DOCUMENT_PART
        ));
    }

    #[test]
    fn direct_xml_rejects_truncated_document_part() {
        let bytes = docx_archive(
            b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>truncated",
            CompressionMethod::Stored,
        );
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("truncated XML must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                ..
            }
        ));
    }

    #[test]
    fn docx_text_whitespace_follows_inherited_xml_space() {
        for (attributes, leaves, expected) in [
            ("", "<w:t> A </w:t><w:t> B </w:t>", "AB"),
            (
                "",
                "<w:t xml:space=\"preserve\"> A </w:t><w:t> B </w:t>",
                " A B",
            ),
            (
                "xml:space=\"preserve\"",
                "<w:t> A </w:t><w:t xml:space=\"default\"> B </w:t><w:t> C </w:t>",
                " A B C ",
            ),
            (
                "",
                "<w:t> \tA&#x20;<![CDATA[ B ]]>&amp; C&#x20;</w:t>",
                "A  B & C",
            ),
            (
                "",
                "<w:t> &#xA0;A&#xA0; </w:t><w:t> \t </w:t>",
                "\u{a0}A\u{a0}",
            ),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" {attributes}><w:body><w:p><w:r>{leaves}</w:r></w:p></w:body></w:document>"
            );
            let facts = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
                .expect("Word text whitespace");
            assert_eq!(facts.text, expected);
            assert_eq!(facts.facts.len(), 1);
            assert_eq!(facts.facts[0].text, expected);
            assert!(
                matches!(facts.facts[0].locator, DocumentLocator::Docx { paragraph: 1, run: 1, text_start: 0, text_end, .. } if text_end == expected.len())
            );
        }
    }

    #[test]
    fn docx_invalid_whitespace_mode_refuses_publication() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Prefix</w:t><w:t xml:space="invalid">suffix</w:t></w:r></w:p></w:body></w:document>"#;
        assert!(matches!(
            extract_document_text_controlled(
                &docx_archive(xml, CompressionMethod::Deflated),
                "guide.docx",
                None,
                &control()
            ),
            Err(DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                ..
            })
        ));
    }

    #[test]
    fn direct_xml_preserves_entities_tabs_and_breaks() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A &amp; B</w:t><w:tab/><w:br/><w:t>C</w:t><w:noBreakHyphen/><w:t>D</w:t><w:softHyphen/><w:t>E</w:t><w:ptab w:alignment="left" w:relativeTo="margin" w:leader="none"/><w:t>F</w:t><w:lastRenderedPageBreak/><w:t>G</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = docx_archive(xml, CompressionMethod::Stored);
        let facts = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect("valid DOCX");
        assert_eq!(facts.text, "A & B\t\nC\u{2011}D\u{00ad}E\tF\nG");
        assert_eq!(facts.facts[0].text, "A & B\t\nC\u{2011}D\u{00ad}E\tF\nG");
        assert_eq!(facts.facts[0].line_start, 1);
        assert_eq!(facts.facts[0].line_end, 3);
        assert_eq!(
            facts.facts[0].locator.to_string(),
            "docx:part=word/document.xml;paragraph=1;run=1;text-span=0..19"
        );
    }

    #[test]
    fn docx_font_specific_symbols_preserve_identity_without_inventing_text() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t><w:sym w:font="Wingdings" w:char="F03A"/><w:t>After</w:t></w:r></w:p></w:body></w:document>"#;
        let facts = extract_document_text_controlled(
            &docx_archive(xml, CompressionMethod::Deflated),
            "symbol.docx",
            None,
            &control(),
        )
        .expect("rendered symbol must not block source indexing");
        assert!(facts.text.contains("Before"));
        assert!(facts.text.contains("After"));
        assert!(!facts.text.contains("F03A"));
        assert_eq!(
            facts.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::UnknownSymbolMapping],
            }
        );
        let graph = facts.symbol_graph("symbol.docx", None);
        assert_eq!(graph.language.as_deref(), Some("docx"));
        assert!(
            graph
                .symbols
                .iter()
                .all(|symbol| symbol.language.as_deref() == Some("docx"))
        );
        assert_eq!(
            facts.symbol_graph("symbol.docx", Some("DOCX")).language,
            graph.language
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol
                .signature
                .contains("font=Some(\"Wingdings\");code=F03A")
                && symbol.signature.contains("docx:part=word/document.xml")
        }));
    }

    #[test]
    fn docx_symbols_with_absent_identity_remain_partial() {
        for (attributes, font, code) in [
            ("w:char=\"F03A\"", None, Some(0xF03A)),
            ("w:font=\"Wingdings\"", Some("Wingdings"), None),
            ("", None, None),
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Before</w:t><w:sym {attributes}/><w:t>After</w:t></w:r></w:p></w:body></w:document>"
            );
            let facts = extract_document_text_controlled(
                &docx_archive(xml.as_bytes(), CompressionMethod::Deflated),
                "symbol.docx",
                None,
                &control(),
            )
            .expect("omitted symbol attributes are valid but unresolved");
            assert!(facts.text.contains("Before\u{fffc}After"));
            assert_eq!(facts.symbols.len(), 1);
            assert_eq!(facts.symbols[0].font.as_deref(), font);
            assert_eq!(facts.symbols[0].code, code);
            assert_eq!(facts.symbols[0].unicode, None);
            let symbol = facts
                .symbol_graph("symbol.docx", None)
                .symbols
                .into_iter()
                .find(|symbol| symbol.name == "document-symbol-1")
                .expect("symbol remains queryable");
            let code = code.map_or_else(|| "missing".to_owned(), |code| format!("{code:04X}"));
            assert!(
                symbol
                    .signature
                    .contains(&format!("font={font:?};code={code}"))
            );
            assert_eq!(
                facts.completeness,
                DocumentCompleteness::Partial {
                    gaps: vec![DocumentCoverageGap::UnknownSymbolMapping],
                }
            );
        }
    }

    #[test]
    fn docx_run_payload_requires_logical_direct_child() {
        for payload in [
            "<w:rPr><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:rPr>",
            "<w:rPr><w:t>Secret</w:t></w:rPr>",
            "<w:wrapper><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:wrapper>",
            "<w:rPr><w:tab/></w:rPr>",
            "<w:rPr><w:footnoteReference w:id=\"1\"/></w:rPr>",
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r>{payload}</w:r></w:p></w:body></w:document>"
            );
            assert!(matches!(
                parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::Malformed { message, .. })
                    if message.contains("not a direct child")
            ));
        }
        let selected = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><mc:AlternateContent><mc:Choice Requires="w"><w:sym w:font="Symbol" w:char="F061"/></mc:Choice><mc:Fallback/></mc:AlternateContent></w:r></w:p></w:body></w:document>"#;
        let facts = parse_docx(selected, &control(), IndexWorkStage::TextIndex)
            .expect("selected compatibility branch preserves direct run payload");
        assert_eq!(facts.text, "α");
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn docx_run_metadata_cannot_publish_text_box_or_story_symbols() {
        for payload in [
            "<w:txbxContent><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:txbxContent>",
            "<w:rPr><w:txbxContent><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:txbxContent></w:rPr>",
            "<w:rPr><a:graphic><a:graphicData><w:txbxContent><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:txbxContent></a:graphicData></a:graphic></w:rPr>",
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><w:body><w:p><w:r>{payload}</w:r></w:p></w:body></w:document>"
            );
            assert!(matches!(
                parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex),
                Err(DocumentExtractionError::Malformed { message, .. })
                    if message.contains("no rendered drawing owner")
            ));
        }
        let comments = br#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="4"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:comment></w:comments>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        for (payload, kind, target, part, story) in [
            (
                "<w:commentRangeStart w:id=\"4\"/>",
                "comments",
                "comments.xml",
                "word/comments.xml",
                comments.as_slice(),
            ),
            (
                "<w:rPr><w:commentRangeStart w:id=\"4\"/></w:rPr>",
                "comments",
                "comments.xml",
                "word/comments.xml",
                comments.as_slice(),
            ),
            (
                "<w:rPr><w:sectPr><w:headerReference r:id=\"header\"/></w:sectPr></w:rPr>",
                "header",
                "header.xml",
                "word/header.xml",
                header.as_slice(),
            ),
        ] {
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body><w:p><w:r>{payload}</w:r></w:p></w:body></w:document>"
            );
            let rels = format!(
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"header\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/></Relationships>"
            );
            let archive = docx_archive_with_parts(&[
                (DOCX_DOCUMENT_PART, main.as_bytes()),
                ("word/_rels/document.xml.rels", rels.as_bytes()),
                (part, story),
            ]);
            assert!(matches!(
                extract_document_text_controlled(&archive, "metadata.docx", None, &control()),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="header" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header.xml"/></Relationships>"#;
        for nested in [
            "<w:r><w:headerReference r:id=\"header\"/></w:r>",
            "<w:wrapper><w:headerReference r:id=\"header\"/></w:wrapper>",
            "<w:r><w:titlePg/></w:r>",
        ] {
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body><w:p><w:pPr><w:sectPr>{nested}</w:sectPr></w:pPr></w:p></w:body></w:document>"
            );
            let archive = docx_archive_with_parts(&[
                (DOCX_DOCUMENT_PART, main.as_bytes()),
                ("word/_rels/document.xml.rels", rels),
                ("word/header.xml", header),
            ]);
            assert!(matches!(
                extract_document_text_controlled(&archive, "section.docx", None, &control()),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        let selected = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:sectPr><mc:AlternateContent><mc:Choice Requires="w"><w:headerReference r:id="header"/><w:titlePg w:val="false"/></mc:Choice><mc:Fallback/></mc:AlternateContent></w:sectPr></w:body></w:document>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, selected),
            ("word/_rels/document.xml.rels", rels),
            ("word/header.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "section.docx", None, &control())
            .expect("selected section metadata remains direct after compatibility wrappers");
        assert_eq!(facts.symbols.len(), 1);
    }

    #[test]
    fn docx_font_identity_has_a_bounded_retained_size() {
        let package = |font: &str| {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:sym w:font=\"{font}\" w:char=\"F03A\"/></w:r></w:p></w:body></w:document>"
            );
            docx_archive(xml.as_bytes(), CompressionMethod::Deflated)
        };
        let retained = extract_document_text_controlled(
            &package(&"A".repeat(MAX_DOCX_FONT_NAME_BYTES)),
            "font.docx",
            None,
            &control(),
        )
        .expect("bounded exact font identity");
        assert_eq!(retained.symbols.len(), 1);
        let over_limit = extract_document_text_controlled(
            &package(&"A".repeat(MAX_DOCX_FONT_NAME_BYTES + 1)),
            "font.docx",
            None,
            &control(),
        )
        .expect("oversized font becomes local incomplete coverage");
        assert_eq!(
            over_limit.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::ResourceLimit(
                    DocumentLimit::MemoryBytes
                )],
            }
        );
        assert!(over_limit.symbols.is_empty());
    }

    #[test]
    fn docx_rendered_story_parts_retain_symbols_and_skip_unreferenced_content() {
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
        let package_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rMain" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let document_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml" TargetMode="Internal"/><Relationship Id="rFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rFootnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rEndnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="rSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Main</w:t><w:sym w:font="Wingdings" w:char="F03A"/><w:drawing><w:txbxContent><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:txbxContent></w:drawing></w:r></w:p><w:p><w:pPr><w:framePr/></w:pPr><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p><w:p><w:r><w:footnoteReference w:id="2"/><w:endnoteReference w:id="3"/><w:commentReference w:id="4"/></w:r></w:p><w:sectPr><w:headerReference r:id="rHeader" w:type="default"/><w:footerReference r:id="rFooter" w:type="default"/></w:sectPr></w:body></w:document>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Header</w:t><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:hdr>"#;
        let footer = br#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Footer</w:t><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:ftr>"#;
        let footnotes = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="0" w:type="separator"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:footnote><w:footnote w:id="1" w:type="continuationSeparator"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Footnote</w:t><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:footnote><w:footnote w:id="99"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let endnotes = br#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="0" w:type="separator"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:endnote><w:endnote w:id="3"><w:p><w:r><w:t>Endnote</w:t><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:endnote></w:endnotes>"#;
        let comments = br#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="4"><w:p><w:r><w:t>Comment</w:t><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:comment></w:comments>"#;
        let settings = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnotePr><w:footnote w:id="+00"/><w:footnote w:id="01"/></w:footnotePr><w:endnotePr><w:endnote w:id="0"/></w:endnotePr></w:settings>"#;
        let glossary = br#"<w:glossaryDocument xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docParts><w:docPart><w:docPartBody><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:docPartBody></w:docPart></w:docParts></w:glossaryDocument>"#;
        let archive = |settings: &[u8]| {
            docx_archive_with_parts(&[
                ("[Content_Types].xml", manifest),
                ("_rels/.rels", package_rels),
                (DOCX_DOCUMENT_PART, main),
                ("word/_rels/document.xml.rels", document_rels),
                ("word/header1.xml", header),
                ("word/footer1.xml", footer),
                ("word/footnotes.xml", footnotes),
                ("word/endnotes.xml", endnotes),
                ("word/comments.xml", comments),
                ("word/settings.xml", settings),
                ("word/glossary/document.xml", glossary),
            ])
        };
        let facts =
            extract_document_text_controlled(&archive(settings), "stories.docx", None, &control())
                .expect("every referenced rendered story is admitted");
        assert_eq!(facts.symbols.len(), 11);
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::ConditionalStory))
        );
        for (part, expected) in [("word/footnotes.xml", 3), ("word/endnotes.xml", 2)] {
            assert_eq!(facts.symbols.iter().filter(|symbol| {
                matches!(&symbol.locator, DocumentLocator::Docx { part: found, .. } if found == part)
            }).count(), expected, "missing or repeated note separator in {part}");
        }
        for part in [
            "word/document.xml",
            "word/header1.xml",
            "word/footer1.xml",
            "word/footnotes.xml",
            "word/endnotes.xml",
            "word/comments.xml",
        ] {
            assert!(facts.symbols.iter().any(|symbol| {
                matches!(&symbol.locator, DocumentLocator::Docx { part: found, .. } if found == part)
            }), "missing symbol in {part}");
        }
        assert!(facts.symbols.iter().all(|symbol| {
            !matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part.contains("glossary"))
        }));
        assert!(facts.text.contains('α'));
        assert!(facts.text.contains("Header"));
        assert!(facts.text.contains("Comment"));
        let unlisted = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnotePr/><w:endnotePr/></w:settings>"#;
        let unlisted = extract_document_text_controlled(
            &archive(unlisted),
            "unlisted-separators.docx",
            None,
            &control(),
        )
        .expect("unlisted special notes are not loaded");
        assert_eq!(unlisted.symbols.len(), 8);
        assert!(
            matches!(unlisted.completeness, DocumentCompleteness::Partial { ref gaps } if !gaps.contains(&DocumentCoverageGap::ConditionalStory))
        );
        for part in ["word/footnotes.xml", "word/endnotes.xml"] {
            assert_eq!(unlisted.symbols.iter().filter(|symbol| {
                matches!(&symbol.locator, DocumentLocator::Docx { part: found, .. } if found == part)
            }).count(), 1, "unlisted note separator was emitted from {part}");
        }
        let compatible = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent><mc:Choice Requires="w"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        let compatible = extract_document_text_controlled(
            &archive(compatible),
            "compatible-settings.docx",
            None,
            &control(),
        )
        .expect("selected compatibility settings load their special note");
        assert_eq!(compatible.symbols.len(), 9);
        assert!(
            !matches!(compatible.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let nested = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent><mc:Choice Requires="w"><mc:AlternateContent><mc:Choice Requires="w"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        let nested = extract_document_text_controlled(
            &archive(nested),
            "nested-settings.docx",
            None,
            &control(),
        )
        .expect("nested selected settings load their special note");
        assert_eq!(nested.symbols.len(), 9);
        assert!(
            !matches!(nested.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let uncertain = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><mc:AlternateContent><mc:Choice Requires="future"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        let uncertain = extract_document_text_controlled(
            &archive(uncertain),
            "uncertain-settings.docx",
            None,
            &control(),
        )
        .expect("unknown compatibility settings remain file-local incomplete");
        assert!(
            matches!(uncertain.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let nested_uncertain = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><mc:AlternateContent><mc:Choice Requires="w"><mc:AlternateContent><mc:Choice Requires="future"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        let nested_uncertain = extract_document_text_controlled(
            &archive(nested_uncertain),
            "nested-uncertain-settings.docx",
            None,
            &control(),
        )
        .expect("nested unknown branch remains incomplete");
        assert!(
            matches!(nested_uncertain.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let undeclared = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent><mc:Choice Requires="missing"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        assert!(matches!(
            extract_document_text_controlled(
                &archive(undeclared),
                "undeclared-settings.docx",
                None,
                &control()
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        let invalid_token = std::str::from_utf8(undeclared)
            .expect("UTF-8 settings fixture")
            .replace("Requires=\"missing\"", "Requires=\"w:future\"");
        assert!(matches!(
            extract_document_text_controlled(
                &archive(invalid_token.as_bytes()),
                "invalid-prefix-settings.docx",
                None,
                &control()
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        let mixed = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><mc:AlternateContent><mc:Choice Requires="future missing"><w:footnotePr><w:footnote w:id="0"/></w:footnotePr></mc:Choice><mc:Fallback/></mc:AlternateContent></w:settings>"#;
        for xml in [
            std::str::from_utf8(mixed)
                .expect("UTF-8 settings fixture")
                .to_owned(),
            std::str::from_utf8(mixed)
                .expect("UTF-8 settings fixture")
                .replace("future missing", "missing future"),
        ] {
            assert!(matches!(
                extract_document_text_controlled(
                    &archive(xml.as_bytes()),
                    "mixed-prefixes.docx",
                    None,
                    &control()
                ),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
        for property in ["footnotePr", "endnotePr"] {
            let closing = format!("</w:{property}>");
            let duplicate_properties = std::str::from_utf8(settings)
                .expect("UTF-8 settings fixture")
                .replace(&closing, &format!("{closing}<w:{property}/>"));
            assert!(matches!(
                extract_document_text_controlled(
                    &archive(duplicate_properties.as_bytes()),
                    "duplicate-note-properties.docx",
                    None,
                    &control(),
                ),
                Err(DocumentExtractionError::Malformed { .. })
            ));
        }
    }

    #[test]
    fn docx_story_relationships_refuse_external_escape_missing_and_wrong_type() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Keep old publication</w:t></w:r></w:p><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:hdr>"#;
        let escaped_external = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="https://example.invalid/a" TargetMode="Ext&#x65;rnal"/></Relationships>"#;
        let parsed =
            parse_docx_relationships(escaped_external, &control(), IndexWorkStage::TextIndex)
                .expect("escaped TargetMode retains external classification");
        assert_eq!(
            parsed.get("h").map(|(_, _, external)| *external),
            Some(true)
        );
        for (relationship, include_header) in [
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="https://example.invalid/a" TargetMode="External""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="https://example.invalid/a" TargetMode="Ext&#x65;rnal""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="../../outside.xml""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="missing.xml""#,
                false,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="header1.xml""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml" TargetMode="Unexpected""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml" TargetMode="Unexp&#x65;cted""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header%2Fone.xml""#,
                true,
            ),
            (
                r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header%GGone.xml""#,
                true,
            ),
        ] {
            let rels = format!(
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"h\" {relationship}/></Relationships>"
            );
            let mut parts = vec![
                (DOCX_DOCUMENT_PART, main.as_slice()),
                ("word/_rels/document.xml.rels", rels.as_bytes()),
            ];
            if include_header {
                parts.push(("word/header1.xml", header.as_slice()));
            }
            let archive = docx_archive_with_parts(&parts);
            assert!(
                matches!(
                    extract_document_text_controlled(&archive, "unsafe.docx", None, &control()),
                    Err(DocumentExtractionError::InvalidDocxPackage { .. })
                ),
                "{relationship}"
            );
        }
    }

    #[test]
    fn docx_safe_percent_encoded_story_target_retains_symbols() {
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header%20one.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header%20one.xml" TargetMode="Int&#x65;rnal"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/header%20one.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "encoded.docx", None, &control())
            .expect("safe encoded target resolves to its package part");
        assert!(facts.symbols.iter().any(|symbol| {
            matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part == "word/header%20one.xml")
        }));
    }

    #[test]
    fn docx_relative_dot_and_unreserved_uri_escapes_resolve_inside_package() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="./header%31.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "normalized.docx", None, &control())
            .expect("relative target is normalized within the package");
        assert!(facts.symbols.iter().any(|symbol| {
            matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part == "word/header1.xml")
        }));
    }

    #[test]
    fn docx_package_metadata_tables_are_bounded_before_map_fanout() {
        let oversized = format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
            " ".repeat(MAX_DOCX_METADATA_BYTES)
        );
        assert!(matches!(
            parse_docx_relationships(oversized.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::InvalidDocxPackage { .. })
        ));
        let mut records = String::new();
        for index in 0..=MAX_DOCX_METADATA_RECORDS {
            write!(records, "<Relationship Id=\"r{index}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header.xml\"/>")
                .expect("fixture relationship");
        }
        let xml = format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{records}</Relationships>"
        );
        assert!(matches!(
            parse_docx_relationships(xml.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::InvalidDocxPackage { .. })
        ));
    }

    #[test]
    fn docx_referenced_unexamined_subdocument_is_partial() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Visible</w:t></w:r><w:subDoc r:id="child"/></w:p></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="child" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/subDocument" Target="child.xml"/></Relationships>"#;
        let child = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:body></w:document>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/child.xml", child),
        ]);
        let facts =
            extract_document_text_controlled(&archive, "subdocument.docx", None, &control())
                .expect("in-package subdocument is valid but not recursively interpreted");
        assert!(facts.text.contains("Visible"));
        assert!(facts.symbols.is_empty());
        assert_eq!(
            facts.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::UnexaminedStory],
            }
        );
        let sdt_main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:subDoc r:id="child"/></w:p></w:sdtContent></w:sdt></w:body></w:document>"#;
        let mut references = DocxStoryReferences::default();
        let sdt = parse_docx_part(
            sdt_main,
            DOCX_DOCUMENT_PART,
            "document",
            None,
            None,
            &mut references,
            &control(),
            IndexWorkStage::TextIndex,
        )
        .expect("subdocument anchor is actual SDT content");
        assert_eq!(
            references.linked_parts,
            vec![(DocxStoryKind::Subdocument, "child".to_owned())]
        );
        assert!(!references.glossary_placeholder);
        assert_eq!(sdt.completeness, facts.completeness);
    }

    #[test]
    fn docx_glossary_placeholder_is_partial_only_when_rendered() {
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/glossary/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml"/></Types>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="glossary" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/glossaryDocument" Target="glossary/document.xml"/></Relationships>"#;
        let glossary = br#"<w:glossaryDocument xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docParts><w:docPart><w:docPartPr><w:name w:val="Hint"/><w:types><w:type w:val="bbPlcHdr"/></w:types></w:docPartPr><w:docPartBody><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:docPartBody></w:docPart></w:docParts></w:glossaryDocument>"#;
        for (content, incomplete) in [
            ("<w:p><w:r><w:t>Literal</w:t></w:r></w:p>", false),
            (
                "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>",
                true,
            ),
            (
                "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt>",
                false,
            ),
            (
                "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder><w:showingPlcHdr/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt>",
                true,
            ),
            (
                "<w:sdt><mc:AlternateContent><mc:Choice Requires=\"w\"><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder><w:showingPlcHdr/></w:sdtPr></mc:Choice><mc:Fallback/></mc:AlternateContent><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt>",
                true,
            ),
            (
                "<w:sdt><w:sdtPr><w:placeholder><mc:AlternateContent><mc:Choice Requires=\"w\"><w:docPart w:val=\"Hint\"/></mc:Choice><mc:Fallback/></mc:AlternateContent></w:placeholder><w:showingPlcHdr/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt>",
                true,
            ),
            (
                "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder><mc:AlternateContent><mc:Choice Requires=\"w\"><w:showingPlcHdr/></mc:Choice><mc:Fallback/></mc:AlternateContent></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt>",
                true,
            ),
            (
                "<w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder></w:sdtPr><mc:AlternateContent><mc:Choice Requires=\"w\"><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></mc:Choice><mc:Fallback/></mc:AlternateContent></w:sdt>",
                false,
            ),
        ] {
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><w:body>{content}</w:body></w:document>"
            );
            let archive = docx_archive_with_parts(&[
                ("[Content_Types].xml", manifest),
                (DOCX_DOCUMENT_PART, main.as_bytes()),
                ("word/_rels/document.xml.rels", rels),
                ("word/glossary/document.xml", glossary),
            ]);
            let facts =
                extract_document_text_controlled(&archive, "glossary.docx", None, &control())
                    .expect("valid in-package glossary relationship");
            assert_eq!(
                matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory)),
                incomplete,
                "{content}"
            );
            assert!(
                facts.symbols.is_empty(),
                "unexamined glossary symbol was invented"
            );
        }
        let hidden = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder><w:showingPlcHdr w:val="false"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Cached</w:t></w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, hidden)]),
            "hidden-placeholder.docx",
            None,
            &control(),
        )
        .expect("a false placeholder flag does not require a glossary relationship");
        assert_eq!(facts.text, "Cached");
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        let preserved_space = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:t xml:space="preserve"> </w:t></w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, preserved_space)]),
            "preserved-space.docx",
            None,
            &control(),
        )
        .expect("retained whitespace replaces a glossary placeholder");
        assert_eq!(facts.text, " ");
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        let hidden_only = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:rPr><w:vanish/></w:rPr><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[
                ("[Content_Types].xml", manifest),
                (DOCX_DOCUMENT_PART, hidden_only),
                ("word/_rels/document.xml.rels", rels),
                ("word/glossary/document.xml", glossary),
            ]),
            "hidden-placeholder.docx",
            None,
            &control(),
        )
        .expect("hidden content does not satisfy a visible glossary placeholder");
        assert!(facts.symbols.is_empty());
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory) && !gaps.contains(&DocumentCoverageGap::UnknownSymbolMapping))
        );
        for control_name in [
            "noBreakHyphen",
            "softHyphen",
            "cr",
            "ptab",
            "lastRenderedPageBreak",
            "pgNum",
        ] {
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:{control_name}/></w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"
            );
            let facts = extract_document_text_controlled(
                &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, main.as_bytes())]),
                "visible-control.docx",
                None,
                &control(),
            )
            .expect("visible controls do not load an unrendered glossary placeholder");
            assert!(!facts.text.is_empty(), "{control_name}");
            assert!(
                !matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory)),
                "{control_name}"
            );
        }
        for content in [
            "<w:object xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\"><v:shape/><o:OLEObject/></w:object>",
            "<w:object><w:control/></w:object>",
        ] {
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val=\"Hint\"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r>{content}</w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"
            );
            let facts = extract_document_text_controlled(
                &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, main.as_bytes())]),
                "embedded-control.docx",
                None,
                &control(),
            )
            .expect("rendered objects do not load an unrendered glossary placeholder");
            assert!(
                !matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory)),
                "{content}"
            );
        }
        let content_part = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:contentPart r:id="xml"/></w:r></w:p></w:sdtContent></w:sdt></w:body></w:document>"#;
        let content_part_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#;
        let content_part_manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[
                ("[Content_Types].xml", content_part_manifest),
                (DOCX_DOCUMENT_PART, content_part),
                ("word/_rels/document.xml.rels", content_part_rels),
                ("customXml/item1.xml", b"<equation>opaque</equation>"),
            ]),
            "alternate-xml-control.docx",
            None,
            &control(),
        )
        .expect("unsupported content part does not trigger glossary lookup");
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        for relationship in [
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#.as_slice(),
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="https://example.invalid/item.xml" TargetMode="External"/></Relationships>"#,
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="../customXml/item1.xml"/></Relationships>"#,
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../../../outside.xml"/></Relationships>"#,
        ] {
            assert!(matches!(
                extract_document_text_controlled(
                    &docx_archive_with_parts(&[
                        ("[Content_Types].xml", content_part_manifest),
                        (DOCX_DOCUMENT_PART, content_part),
                        ("word/_rels/document.xml.rels", relationship),
                        ("customXml/item1.xml", b"<equation>opaque</equation>"),
                    ]),
                    "invalid-content-part.docx",
                    None,
                    &control(),
                ),
                Err(DocumentExtractionError::InvalidDocxPackage { .. })
            ));
        }
        let no_id = std::str::from_utf8(content_part)
            .expect("fixture is UTF-8")
            .replace(" r:id=\"xml\"", "");
        assert!(matches!(
            extract_document_text_controlled(
                &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, no_id.as_bytes())]),
                "missing-content-part-id.docx",
                None,
                &control(),
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
        let note_manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#;
        let note_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="f" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="e" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="c" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:footnoteReference w:id="2"/><w:endnoteReference w:id="3"/><w:commentReference w:id="4"/></w:r></w:p></w:body></w:document>"#;
        let footnotes = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="2"><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:footnoteRef/></w:r></w:p></w:sdtContent></w:sdt></w:footnote></w:footnotes>"#;
        let endnotes = br#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="3"><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:endnoteRef/></w:r></w:p></w:sdtContent></w:sdt></w:endnote></w:endnotes>"#;
        let comments = br#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="4"><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p><w:r><w:annotationRef/></w:r></w:p></w:sdtContent></w:sdt></w:comment></w:comments>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[
                ("[Content_Types].xml", note_manifest),
                (DOCX_DOCUMENT_PART, main),
                ("word/_rels/document.xml.rels", note_rels),
                ("word/footnotes.xml", footnotes),
                ("word/endnotes.xml", endnotes),
                ("word/comments.xml", comments),
            ]),
            "story-reference-mark.docx",
            None,
            &control(),
        )
        .expect("story reference marks replace placeholders without a glossary relationship");
        assert_eq!(facts.text.chars().filter(|ch| *ch == '\u{fffc}').count(), 6);
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnevaluatedField) && !gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
        let header_manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/glossary/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml"/></Types>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt></w:hdr>"#;
        let main_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="glossary" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/glossaryDocument" Target="glossary/document.xml"/></Relationships>"#;
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", header_manifest),
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", main_rels),
            ("word/header1.xml", header),
            ("word/glossary/document.xml", glossary),
        ]);
        let facts =
            extract_document_text_controlled(&archive, "header-glossary.docx", None, &control())
                .expect("header placeholder uses the main document glossary relationship");
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::UnexaminedStory))
        );
    }

    #[test]
    fn docx_glossary_placeholder_inside_field_instruction_is_not_rendered() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>IF</w:instrText></w:r><w:sdt><w:sdtPr><w:placeholder><w:docPart w:val="Hint"/></w:placeholder></w:sdtPr><w:sdtContent/></w:sdt><w:r><w:fldChar w:fldCharType="separate"/><w:t>Cached</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let facts = extract_document_text_controlled(
            &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, main)]),
            "instruction-placeholder.docx",
            None,
            &control(),
        )
        .expect("non-rendered field instruction cannot require a glossary story");
        assert_eq!(facts.text, "Cached");
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn docx_special_note_settings_share_the_fact_ceiling() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Visible</w:t></w:r></w:p></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="settings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
        for count in [MAX_DOCUMENT_FACTS, MAX_DOCUMENT_FACTS + 1] {
            let mut footnotes = String::new();
            for id in 0..MAX_DOCUMENT_FACTS / 2 {
                write!(footnotes, "<w:footnote w:id=\"{id}\"/>").expect("footnote fixture");
            }
            let mut endnotes = String::new();
            for id in MAX_DOCUMENT_FACTS / 2..count {
                write!(endnotes, "<w:endnote w:id=\"{id}\"/>").expect("endnote fixture");
            }
            let settings = format!(
                "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:footnotePr>{footnotes}</w:footnotePr><w:endnotePr>{endnotes}</w:endnotePr></w:settings>"
            );
            let facts = extract_document_text_controlled(
                &docx_archive_with_parts(&[
                    (DOCX_DOCUMENT_PART, main),
                    ("word/_rels/document.xml.rels", rels),
                    ("word/settings.xml", settings.as_bytes()),
                ]),
                "special-notes.docx",
                None,
                &control(),
            )
            .expect("bounded settings keep verified main text");
            assert_eq!(facts.text, "Visible");
            assert_eq!(
                matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::ResourceLimit(DocumentLimit::FactCount))),
                count > MAX_DOCUMENT_FACTS
            );
        }
    }

    #[test]
    fn docx_story_content_type_must_match_relationship_kind() {
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", header),
        ]);
        assert!(matches!(
            extract_document_text_controlled(&archive, "wrong-type.docx", None, &control()),
            Err(DocumentExtractionError::InvalidDocxPackage { .. }),
        ));
    }

    #[test]
    fn docx_content_type_default_covers_main_and_story_override_takes_precedence() {
        let manifest = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let archive = |manifest: &str| {
            docx_archive_with_parts(&[
                ("[Content_Types].xml", manifest.as_bytes()),
                (DOCX_DOCUMENT_PART, main),
                ("word/_rels/document.xml.rels", rels),
                ("word/header1.xml", header),
            ])
        };
        let facts =
            extract_document_text_controlled(&archive(manifest), "defaults.docx", None, &control())
                .expect("default-covered main and overridden header are valid");
        assert_eq!(facts.symbols.len(), 2);
        let wrong = manifest.replace("wordprocessingml.header+xml", "wordprocessingml.footer+xml");
        assert!(matches!(
            extract_document_text_controlled(&archive(&wrong), "defaults.docx", None, &control()),
            Err(DocumentExtractionError::InvalidDocxPackage { .. })
        ));
    }

    #[test]
    fn docx_utf16_package_metadata_and_stories_preserve_symbols() {
        let manifest = r#"<?xml version="1.0" encoding="UTF-16"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
        let root_rels = r#"<?xml version="1.0" encoding="UTF-16"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="main" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let main = r#"<?xml version="1.0" encoding="UTF-16"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-16"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
        let header = r#"<?xml version="1.0" encoding="UTF-16"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        for little_endian in [true, false] {
            let encode = |xml: &str| {
                let mut bytes = if little_endian {
                    vec![0xff, 0xfe]
                } else {
                    vec![0xfe, 0xff]
                };
                for unit in xml.encode_utf16() {
                    bytes.extend_from_slice(&if little_endian {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    });
                }
                bytes
            };
            let manifest = encode(manifest);
            let root_rels = encode(root_rels);
            let main = encode(main);
            let rels = encode(rels);
            let header = encode(header);
            let archive = docx_archive_with_parts(&[
                ("[Content_Types].xml", &manifest),
                ("_rels/.rels", &root_rels),
                (DOCX_DOCUMENT_PART, &main),
                ("word/_rels/document.xml.rels", &rels),
                ("word/header1.xml", &header),
            ]);
            let facts = extract_document_text_controlled(&archive, "utf16.docx", None, &control())
                .expect("UTF-16 package parts are valid");
            assert_eq!(facts.symbols.len(), 2);
        }
    }

    #[test]
    fn docx_header_note_uses_main_document_relationship() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let main_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="n" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:footnoteReference w:id="2"/></w:r></w:p></w:hdr>"#;
        let footnotes = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="2"><w:p><w:r><w:sym w:font="Wingdings" w:char="F03A"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", main_rels),
            ("word/header1.xml", header),
            ("word/footnotes.xml", footnotes),
        ]);
        let facts = extract_document_text_controlled(&archive, "nested.docx", None, &control())
            .expect("reachable nested story is valid");
        assert!(facts.symbols.iter().any(|symbol| {
            matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part == "word/footnotes.xml")
        }));
    }

    #[test]
    fn docx_header_note_fanout_batches_ids_before_large_note_part() {
        let mut main = String::from(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>",
        );
        let mut rels = String::from(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"notes\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>",
        );
        let mut notes = String::from(
            "<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><!--",
        );
        notes.push_str(&"x".repeat(4 * 1024 * 1024));
        notes.push_str("-->");
        let mut parts = Vec::new();
        for index in 0..18 {
            write!(&mut main, "<w:p><w:pPr><w:sectPr><w:headerReference r:id=\"h{index}\"/></w:sectPr></w:pPr></w:p>").expect("section fixture");
            write!(&mut rels, "<Relationship Id=\"h{index}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header{index}.xml\"/>").expect("relationship fixture");
            write!(&mut notes, "<w:footnote w:id=\"{index}\"><w:p><w:r><w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p></w:footnote>").expect("note fixture");
            parts.push((format!("word/header{index}.xml"), format!("<w:hdr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:p><w:r><w:footnoteReference w:id=\"{index}\"/></w:r></w:p></w:hdr>")));
        }
        main.push_str("</w:body></w:document>");
        rels.push_str("</Relationships>");
        notes.push_str("</w:footnotes>");
        parts.push((DOCX_DOCUMENT_PART.to_owned(), main));
        parts.push(("word/_rels/document.xml.rels".to_owned(), rels));
        parts.push(("word/footnotes.xml".to_owned(), notes));
        let borrowed = parts
            .iter()
            .map(|(name, xml)| (name.as_str(), xml.as_bytes()))
            .collect::<Vec<_>>();
        let archive = docx_archive_with_parts(&borrowed);
        let facts = extract_document_text_controlled(&archive, "batched.docx", None, &control())
            .expect("all referenced notes fit the unique package bounds");
        assert_eq!(facts.symbols.len(), 18);
        assert!(
            matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps }
            if gaps.contains(&DocumentCoverageGap::UnevaluatedField)
                && !gaps.iter().any(|gap| matches!(gap, DocumentCoverageGap::ResourceLimit(_))))
        );
    }

    #[test]
    fn docx_inactive_header_variants_do_not_publish_symbols() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="default" r:id="d"/><w:headerReference w:type="first" r:id="f"/><w:headerReference w:type="even" r:id="e"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="d" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="default.xml"/><Relationship Id="f" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="first.xml"/><Relationship Id="e" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="even.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/default.xml", header),
            ("word/first.xml", header),
            ("word/even.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "inactive.docx", None, &control())
            .expect("inactive variants are safe to skip");
        assert_eq!(facts.symbols.len(), 1);
        assert!(
            matches!(&facts.symbols[0].locator, DocumentLocator::Docx { part, .. } if part == "word/default.xml")
        );
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn docx_first_and_default_headers_need_pagination_coverage() {
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="d" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="default.xml"/><Relationship Id="f" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="first.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        for (prior, title, first, symbols, conditional) in [
            (
                "",
                "<w:titlePg/>",
                "<w:headerReference w:type=\"first\" r:id=\"f\"/>",
                2,
                true,
            ),
            ("", "<w:titlePg/>", "", 1, true),
            (
                "<w:p><w:pPr><w:sectPr><w:headerReference w:type=\"default\" r:id=\"d\"/></w:sectPr></w:pPr></w:p>",
                "<w:titlePg/>",
                "",
                1,
                true,
            ),
            (
                "",
                "<w:titlePg w:val=\"false\"/>",
                "<w:headerReference w:type=\"first\" r:id=\"f\"/>",
                1,
                false,
            ),
        ] {
            let default = if prior.is_empty() {
                "<w:headerReference w:type=\"default\" r:id=\"d\"/>"
            } else {
                ""
            };
            let main = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{prior}<w:sectPr>{default}{first}{title}</w:sectPr></w:body></w:document>"
            );
            let archive = docx_archive_with_parts(&[
                (DOCX_DOCUMENT_PART, main.as_bytes()),
                ("word/_rels/document.xml.rels", rels),
                ("word/default.xml", header),
                ("word/first.xml", header),
            ]);
            let facts =
                extract_document_text_controlled(&archive, "headers.docx", None, &control())
                    .expect("reachable header variants are valid");
            assert_eq!(facts.symbols.len(), symbols);
            assert_eq!(
                matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::ConditionalStory)),
                conditional
            );
        }
    }

    #[test]
    fn docx_duplicate_section_story_variant_is_malformed() {
        for kind in ["header", "footer"] {
            for variant in ["default", "first", "even"] {
                let main = format!(
                    "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body><w:sectPr><w:{kind}Reference w:type=\"{variant}\" r:id=\"a\"/><w:{kind}Reference w:type=\"{variant}\" r:id=\"b\"/></w:sectPr></w:body></w:document>"
                );
                assert!(
                    matches!(
                        extract_document_text_controlled(
                            &docx_archive_with_parts(&[(DOCX_DOCUMENT_PART, main.as_bytes())]),
                            "duplicate-section-variant.docx",
                            None,
                            &control(),
                        ),
                        Err(DocumentExtractionError::Malformed { .. })
                    ),
                    "{kind} {variant}"
                );
            }
        }
    }

    #[test]
    fn docx_historical_section_properties_do_not_select_old_headers() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="default" r:id="current"/><w:sectPrChange><w:sectPr><w:headerReference w:type="default" r:id="old"/></w:sectPr></w:sectPrChange></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="current" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/header.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "revision.docx", None, &control())
            .expect("tracked old section must not replace current header");
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
    }

    #[test]
    fn docx_later_title_page_inherits_prior_first_header() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:pPr><w:sectPr><w:headerReference w:type="first" r:id="prior"/></w:sectPr></w:pPr></w:p><w:sectPr><w:titlePg/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="prior" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="prior.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/prior.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "inherit.docx", None, &control())
            .expect("later title page inherits prior first header");
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.completeness, DocumentCompleteness::Complete);
        let conflicting = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="first" r:id="prior"/><w:titlePg w:val="false"/><w:titlePg w:val="true"/></w:sectPr></w:body></w:document>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, conflicting),
            ("word/_rels/document.xml.rels", rels),
            ("word/prior.xml", header),
        ]);
        assert!(matches!(
            extract_document_text_controlled(
                &archive,
                "duplicate-title-page.docx",
                None,
                &control()
            ),
            Err(DocumentExtractionError::Malformed { .. })
        ));
    }

    #[test]
    fn docx_root_relationship_selects_nonconventional_main_part() {
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/header.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
        let root_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="main" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="./%64ocument.xml"/></Relationships>"#;
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p><w:sectPr><w:headerReference r:id="h"/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let archive = docx_archive_with_parts(&[
            ("[Content_Types].xml", manifest),
            ("_rels/.rels", root_rels),
            ("document.xml", main),
            ("_rels/document.xml.rels", rels),
            ("header.xml", header),
        ]);
        let facts = extract_document_text_controlled(&archive, "root.docx", None, &control())
            .expect("OPC root relationship selects the main part");
        assert_eq!(facts.symbols.len(), 2);
        assert!(facts.symbols.iter().any(|symbol| {
            matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part == "document.xml")
        }));
        assert!(facts.symbols.iter().any(|symbol| {
            matches!(&symbol.locator, DocumentLocator::Docx { part, .. } if part == "header.xml")
        }));
    }

    #[test]
    fn docx_missing_opc_metadata_never_publishes_complete_text() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture part");
            writer
                .write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#)
                .expect("fixture XML");
            writer.finish().expect("fixture package");
        }
        assert!(matches!(
            extract_document_text_controlled(&bytes, "truncated.docx", None, &control()),
            Err(DocumentExtractionError::InvalidDocxPackage { .. }),
        ));
    }

    #[test]
    fn docx_enabled_header_variants_preserve_symbols_with_conditional_coverage() {
        let main = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="first" r:id="f"/><w:headerReference w:type="even" r:id="e"/><w:titlePg/></w:sectPr></w:body></w:document>"#;
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="f" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="first.xml"/><Relationship Id="e" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="even.xml"/><Relationship Id="s" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
        let header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:sym w:font="Symbol" w:char="F061"/></w:r></w:p></w:hdr>"#;
        let settings = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:evenAndOddHeaders/></w:settings>"#;
        let archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/first.xml", header),
            ("word/even.xml", header),
            ("word/settings.xml", settings),
        ]);
        let facts = extract_document_text_controlled(&archive, "enabled.docx", None, &control())
            .expect("enabled variants are reachable");
        assert_eq!(facts.symbols.len(), 2);
        assert_eq!(
            facts.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::ConditionalStory],
            }
        );
        let first_only = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="first" r:id="f"/><w:titlePg/></w:sectPr></w:body></w:document>"#;
        let first_only_archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, first_only),
            ("word/_rels/document.xml.rels", rels),
            ("word/first.xml", header),
            ("word/even.xml", header),
            ("word/settings.xml", settings),
        ]);
        let first_only_facts = extract_document_text_controlled(
            &first_only_archive,
            "first-only.docx",
            None,
            &control(),
        )
        .expect("first-page header does not depend on page parity");
        assert_eq!(first_only_facts.symbols.len(), 1);
        assert_eq!(
            first_only_facts.completeness,
            DocumentCompleteness::Complete
        );
        let default_only = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sectPr><w:headerReference w:type="default" r:id="f"/></w:sectPr></w:body></w:document>"#;
        let default_only_archive = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, default_only),
            ("word/_rels/document.xml.rels", rels),
            ("word/first.xml", header),
            ("word/settings.xml", settings),
        ]);
        let default_only_facts = extract_document_text_controlled(
            &default_only_archive,
            "default-only.docx",
            None,
            &control(),
        )
        .expect("default header remains parity-dependent");
        assert_eq!(default_only_facts.symbols.len(), 1);
        assert_eq!(
            default_only_facts.completeness,
            DocumentCompleteness::Partial {
                gaps: vec![DocumentCoverageGap::ConditionalStory],
            }
        );
        let undeclared_settings = br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><x:evenAndOddHeaders/></w:settings>"#;
        let malformed = docx_archive_with_parts(&[
            (DOCX_DOCUMENT_PART, main),
            ("word/_rels/document.xml.rels", rels),
            ("word/first.xml", header),
            ("word/even.xml", header),
            ("word/settings.xml", undeclared_settings),
        ]);
        assert!(matches!(
            extract_document_text_controlled(&malformed, "undeclared-settings.docx", None, &control()),
            Err(DocumentExtractionError::Malformed { message, .. })
                if message.contains("undeclared namespace prefix")
        ));
        for settings in [
            br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:evenAndOddHeaders x:val="false"/></w:settings>"#.as_slice(),
            br#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" x:ignored="true"><w:evenAndOddHeaders/></w:settings>"#.as_slice(),
        ] {
            let malformed = docx_archive_with_parts(&[
                (DOCX_DOCUMENT_PART, main),
                ("word/_rels/document.xml.rels", rels),
                ("word/first.xml", header),
                ("word/even.xml", header),
                ("word/settings.xml", settings),
            ]);
            assert!(matches!(
                extract_document_text_controlled(&malformed, "undeclared-settings.docx", None, &control()),
                Err(DocumentExtractionError::Malformed { message, .. })
                    if message.contains("undeclared namespace prefix")
            ));
        }
    }

    #[test]
    fn docx_post_admission_fact_limit_is_local_incomplete_coverage() {
        for count in [
            MAX_DOCUMENT_FACTS - 1,
            MAX_DOCUMENT_FACTS,
            MAX_DOCUMENT_FACTS + 1,
        ] {
            let xml = format!(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r>{}</w:r></w:p></w:body></w:document>",
                "<w:sym w:font=\"Wingdings\" w:char=\"F03A\"/>".repeat(count),
            );
            let archive = docx_archive(xml.as_bytes(), CompressionMethod::Deflated);
            let facts =
                extract_document_text_controlled(&archive, "limited.docx", None, &control())
                    .expect("safe post-admission fact ceiling is file-local");
            if count < MAX_DOCUMENT_FACTS {
                assert_eq!(facts.symbols.len(), count);
                assert_eq!(facts.facts.len(), 1);
                assert!(
                    !matches!(facts.completeness, DocumentCompleteness::Partial { ref gaps } if gaps.contains(&DocumentCoverageGap::ResourceLimit(DocumentLimit::FactCount)))
                );
            } else {
                assert!(facts.symbols.is_empty());
                assert_eq!(
                    facts.completeness,
                    DocumentCompleteness::Partial {
                        gaps: vec![DocumentCoverageGap::ResourceLimit(DocumentLimit::FactCount)],
                    }
                );
            }
        }
    }

    #[test]
    fn direct_xml_rejects_external_doctype_declarations() {
        let xml = br#"<!DOCTYPE w:document SYSTEM "https://example.invalid/document.dtd"><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let bytes = docx_archive(xml, CompressionMethod::Stored);
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("external declarations must never enter the parser boundary");
        assert!(matches!(
            error,
            DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                ..
            }
        ));
    }

    #[test]
    fn direct_xml_rejects_non_whitespace_outside_document_root() {
        let xml = br#"prefix<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
        let bytes = docx_archive(xml, CompressionMethod::Stored);
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("non-whitespace outside the root must fail closed");
        assert!(matches!(
            error,
            DocumentExtractionError::Malformed {
                format: DocumentFormat::Docx,
                ..
            }
        ));
    }
}
