//! Bounded, in-process extraction for the supported repository document formats.

use crate::check_parser_iteration;
use projectatlas_core::symbols::{CodeSymbol, ParserKind, SymbolGraph, SymbolKind};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use quick_xml::NsReader;
use quick_xml::events::{BytesRef, Event};
use quick_xml::name::{QName, ResolveResult};
use std::collections::HashSet;
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
/// Maximum evidence facts retained from one document.
pub const MAX_DOCUMENT_FACTS: usize = limits::FACT_LIMIT;
/// The only DOCX package part admitted to the parser.
pub const DOCX_DOCUMENT_PART: &str = "word/document.xml";

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentCompleteness {
    /// Every supported content item in the admitted format was extracted.
    Complete,
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
    /// A text span in the admitted DOCX document part.
    Docx {
        /// Package part containing the source text.
        part: &'static str,
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

/// Complete bounded text and provenance extracted from one document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentFacts {
    /// Format admitted by magic and language/extension checks.
    pub format: DocumentFormat,
    /// Extracted text with preserved paragraph and run layout for the text index.
    pub text: String,
    /// Exact text facts used to build graph evidence.
    pub facts: Vec<DocumentFact>,
    /// Whether the supported part was fully examined.
    pub completeness: DocumentCompleteness,
    /// Parser provenance for audit and graph evidence.
    pub provenance: DocumentParserProvenance,
}

impl DocumentFacts {
    /// Project exact document facts into the existing sparse symbol graph.
    #[must_use]
    pub fn symbol_graph(&self, path: &str, language: Option<&str>) -> SymbolGraph {
        let symbols = self
            .facts
            .iter()
            .enumerate()
            .map(|(index, fact)| CodeSymbol {
                path: path.to_owned(),
                language: language.map(str::to_owned),
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
        SymbolGraph {
            path: path.to_owned(),
            language: language.map(str::to_owned),
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
    Ok(extract_document_controlled_with_stage(
        bytes,
        path,
        language,
        control,
        IndexWorkStage::SymbolParsing,
    )?
    .symbol_graph(path, language))
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
        let lower_name = name.to_ascii_lowercase();
        if lower_name.starts_with("word/embeddings/")
            || Path::new(&lower_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"))
            || Path::new(&lower_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            return Err(DocumentExtractionError::InvalidDocxPackage {
                message: format!("embedded document part {name} is unsupported"),
            });
        }
    }
    let mut xml = Vec::new();
    {
        let mut document_part = archive.by_name(DOCX_DOCUMENT_PART).map_err(|_error| {
            DocumentExtractionError::InvalidDocxPackage {
                message: format!("required part {DOCX_DOCUMENT_PART} is missing"),
            }
        })?;
        let mut chunk = [0_u8; 8192];
        loop {
            control.check(stage)?;
            let read = document_part.read(&mut chunk).map_err(|error| {
                DocumentExtractionError::InvalidDocxPackage {
                    message: error.to_string(),
                }
            })?;
            if read == 0 {
                break;
            }
            let observed = xml.len().saturating_add(read);
            if observed > MAX_DOCUMENT_EXPANDED_BYTES {
                return Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::ExpandedBytes,
                    observed,
                    maximum: MAX_DOCUMENT_EXPANDED_BYTES,
                });
            }
            xml.extend_from_slice(&chunk[..read]);
        }
    }
    drop(archive);
    drop(names);
    control.check(stage)?;
    // Retain XML, parser staging, and namespace resolver copies, plus geometric string
    // growth in the current run/output/facts, and the bounded fact vector.
    // Archive metadata has already been dropped before these allocations overlap.
    check_memory_budget(
        bytes
            .len()
            .saturating_add(xml.capacity().saturating_mul(3))
            .saturating_add(MAX_DOCUMENT_OUTPUT_BYTES.saturating_mul(4))
            .saturating_add(MAX_DOCX_XML_DEPTH.saturating_mul(
                std::mem::size_of::<(DocxTextContext, usize)>()
                    + MAX_DOCX_XML_DEPTH * std::mem::size_of::<DocxFieldPhase>()
                    + std::mem::size_of::<DocxAlternative>(),
            ))
            .saturating_add(
                MAX_DOCUMENT_FACTS
                    .saturating_mul(std::mem::size_of::<DocumentFact>())
                    .saturating_mul(2),
            ),
    )?;
    parse_docx(&xml, control, stage)
}

/// Raw run text retained without XML parser whitespace trimming.
#[derive(Default)]
struct RawDocxRun {
    /// Exact decoded run text.
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
    Result,
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

/// Selection state for one bounded Markup Compatibility alternative.
struct DocxAlternative {
    /// XML depth of the enclosing `AlternateContent` element.
    depth: usize,
    /// Whether an earlier branch was selected.
    selected: bool,
    /// Whether the required first Choice has appeared.
    choice_seen: bool,
    /// Whether the final optional Fallback has appeared.
    fallback_seen: bool,
}

/// Parse body paragraphs and table paragraphs from the admitted XML part.
fn parse_docx(
    xml: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    if xml.starts_with(&[0xff, 0xfe])
        || xml.starts_with(&[0xfe, 0xff])
        || xml.starts_with(&[0, b'<', 0, b'?'])
        || xml.starts_with(&[b'<', 0, b'?', 0])
    {
        return Err(DocumentExtractionError::UnsupportedDocxInput {
            message: "DOCX XML encoding is not supported; UTF-8 is required".to_owned(),
        });
    }
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut output = String::new();
    let mut output_line = 1usize;
    let mut facts = Vec::new();
    let mut paragraph_number = 0usize;
    let mut paragraph = DocxTextContext::default();
    let mut text_boxes = Vec::new();
    let mut text_carrier = None;
    let mut alternatives: Vec<DocxAlternative> = Vec::new();
    let mut skipped_branch_depth = None;
    let mut deleted_depth = None;
    let mut foreign_depth = None;
    let mut element_depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;
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
        let compatibility = matches!(&namespace, ResolveResult::Bound(namespace)
            if namespace.as_ref() == "http://schemas.openxmlformats.org/markup-compatibility/2006");
        let wordprocessing = match namespace {
            ResolveResult::Bound(namespace) => wordprocessing_namespace(namespace.as_ref()),
            ResolveResult::Unbound => false,
            ResolveResult::Unknown(prefix) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: format!("DOCX XML contained an undeclared namespace prefix: {prefix}"),
                });
            }
        };
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
                    if root_seen || !wordprocessing || name.as_ref() != "document" {
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
                if skipped_branch_depth.is_some() {
                    continue;
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
                        let requires = event
                            .try_get_attribute("Requires")
                            .map_err(|error| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: error.to_string(),
                            })?
                            .ok_or_else(|| DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX compatibility choice requires namespace prefixes"
                                    .to_owned(),
                            })?;
                        let requires =
                            quick_xml::escape::unescape(&requires.value).map_err(|error| {
                                DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: error.to_string(),
                                }
                            })?;
                        if requires.split_whitespace().next().is_none() {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX compatibility choice requires namespace prefixes"
                                    .to_owned(),
                            });
                        }
                        let mut supported = true;
                        for (index, prefix) in requires.split_whitespace().enumerate() {
                            check_parser_iteration(index, &mut || control.check(stage))?;
                            let qualified = format!("{prefix}:choice");
                            let (namespace, _) =
                                reader.resolver().resolve_element(QName(&qualified));
                            if !matches!(namespace, ResolveResult::Bound(namespace)
                                if wordprocessing_namespace(namespace.as_ref()))
                            {
                                supported = false;
                                break;
                            }
                        }
                        supported
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
                    if !alternative.selected && supported {
                        alternative.selected = true;
                    } else {
                        skipped_branch_depth = Some(element_depth);
                    }
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
                        selected: false,
                        choice_seen: false,
                        fallback_seen: false,
                    });
                    continue;
                }
                if !wordprocessing && !compatibility && foreign_depth.is_none() {
                    foreign_depth = Some(element_depth);
                }
                match if wordprocessing { name.as_ref() } else { "" } {
                    "altChunk" if deleted_depth.is_none() => {
                        return Err(DocumentExtractionError::UnsupportedDocxInput {
                            message:
                                "alternate-format DOCX chunks require unsupported part decoding"
                                    .to_owned(),
                        });
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
                    "del" | "moveFrom" if deleted_depth.is_none() => {
                        deleted_depth = Some(element_depth);
                    }
                    "txbxContent" => {
                        if let Some(run) = paragraph.run.as_mut() {
                            publish_docx_run_fragment(
                                run,
                                paragraph.number,
                                paragraph.run_number,
                                &mut output,
                                &mut output_line,
                                &mut facts,
                            )?;
                        }
                        text_boxes.push((std::mem::take(&mut paragraph), output.len()));
                    }
                    "p" if !paragraph.open => {
                        paragraph.open = true;
                        paragraph_number += 1;
                        paragraph.number = paragraph_number;
                        paragraph.run_number = 0;
                        if !output.is_empty() && deleted_depth.is_none() {
                            push_output_byte(&mut output, b'\n')?;
                            output_line += 1;
                        }
                    }
                    "r" if paragraph.open && paragraph.run.is_none() => {
                        paragraph.run_number += 1;
                        paragraph.run = Some(RawDocxRun::default());
                    }
                    "t" | "instrText" | "delText" | "delInstrText" if paragraph.run.is_some() => {
                        let ignored = deleted_depth.is_some()
                            || matches!(name.as_ref(), "delText" | "delInstrText")
                            || (name.as_ref() == "instrText"
                                && paragraph.fields.contains(&DocxFieldPhase::Instruction));
                        text_carrier = Some(if ignored {
                            DocxTextCarrier::Ignored
                        } else {
                            DocxTextCarrier::Rendered
                        });
                    }
                    "fldChar" if paragraph.run.is_some() && deleted_depth.is_some() => {}
                    "fldChar" if paragraph.run.is_some() => {
                        let mut field_type = None;
                        for attribute in event.attributes() {
                            let attribute =
                                attribute.map_err(|error| DocumentExtractionError::Malformed {
                                    format: DocumentFormat::Docx,
                                    message: error.to_string(),
                                })?;
                            let (namespace, local) =
                                reader.resolver().resolve_attribute(attribute.key);
                            if local.as_ref() == "fldCharType"
                                && matches!(namespace, ResolveResult::Bound(namespace)
                                    if wordprocessing_namespace(namespace.as_ref()))
                            {
                                field_type = Some(attribute.value.into_owned());
                            }
                        }
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
                                    *phase = DocxFieldPhase::Result;
                                }
                            }
                            Some("end") if !paragraph.fields.is_empty() => {
                                paragraph.fields.pop();
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
                        if deleted_depth.is_none() =>
                    {
                        if let Some(run) = paragraph.run.as_mut() {
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
                    "sym" if paragraph.run.is_some() && deleted_depth.is_none() => {
                        return Err(DocumentExtractionError::UnsupportedDocxInput {
                            message: "font-specific symbols require unsupported font decoding"
                                .to_owned(),
                        });
                    }
                    "p" | "r" | "t" | "instrText" | "delText" | "delInstrText" | "fldChar" => {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX paragraph, run, or text nesting is invalid".to_owned(),
                        });
                    }
                    _ => {}
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
                if foreign_depth == Some(element_depth) {
                    foreign_depth = None;
                }
                if let Some(depth) = skipped_branch_depth {
                    if element_depth == depth {
                        skipped_branch_depth = None;
                    }
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
                let name = event.local_name();
                if deleted_depth == Some(element_depth) {
                    deleted_depth = None;
                }
                match if wordprocessing { name.as_ref() } else { "" } {
                    "t" | "instrText" | "delText" | "delInstrText" => text_carrier = None,
                    "r" => {
                        if let Some(mut run) = paragraph.run.take() {
                            publish_docx_run_fragment(
                                &mut run,
                                paragraph.number,
                                paragraph.run_number,
                                &mut output,
                                &mut output_line,
                                &mut facts,
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
                        let Some((outer, previous_bytes)) = text_boxes.pop() else {
                            return Err(DocumentExtractionError::Malformed {
                                format: DocumentFormat::Docx,
                                message: "DOCX text box had no matching container".to_owned(),
                            });
                        };
                        paragraph = outer;
                        if output.len() > previous_bytes && !output.ends_with('\n') {
                            push_output_byte(&mut output, b'\n')?;
                            output_line += 1;
                        }
                    }
                    "p" => paragraph.open = false,
                    _ => {}
                }
                element_depth -= 1;
                if element_depth == 0 {
                    root_closed = true;
                }
            }
            Event::Decl(declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let encoding =
                        encoding.map_err(|error| DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: error.to_string(),
                        })?;
                    if !encoding.eq_ignore_ascii_case("UTF-8")
                        && !encoding.eq_ignore_ascii_case("US-ASCII")
                    {
                        return Err(DocumentExtractionError::UnsupportedDocxInput {
                            message: "DOCX XML encoding is not supported; UTF-8 is required"
                                .to_owned(),
                        });
                    }
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
        || skipped_branch_depth.is_some()
        || deleted_depth.is_some()
        || foreign_depth.is_some()
        || paragraph.run.is_some()
        || text_carrier.is_some()
        || !paragraph.fields.is_empty()
    {
        return Err(DocumentExtractionError::Malformed {
            format: DocumentFormat::Docx,
            message: "DOCX XML ended before all elements were closed".to_owned(),
        });
    }
    Ok(DocumentFacts {
        format: DocumentFormat::Docx,
        text: output,
        facts,
        completeness: DocumentCompleteness::Complete,
        provenance: DocumentParserProvenance::QuickXml,
    })
}

/// Emit a run fragment before leaving its container, preserving its original byte locator.
fn publish_docx_run_fragment(
    run: &mut RawDocxRun,
    paragraph: usize,
    run_number: usize,
    output: &mut String,
    output_line: &mut usize,
    facts: &mut Vec<DocumentFact>,
) -> Result<(), DocumentExtractionError> {
    if run.text.is_empty() {
        return Ok(());
    }
    if facts.len() >= MAX_DOCUMENT_FACTS {
        return Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::FactCount,
            observed: facts.len() + 1,
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
            part: DOCX_DOCUMENT_PART,
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
    use std::io::Write;
    use std::time::{Duration, Instant};
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::FileOptions;

    fn control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), None)
    }

    fn docx_archive(xml: &[u8], method: CompressionMethod) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(
                    DOCX_DOCUMENT_PART,
                    FileOptions::default().compression_method(method),
                )
                .expect("fixture entry");
            writer.write_all(xml).expect("fixture XML");
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
        form.set("Subtype", "Form");
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
    fn pdf_extended_graphics_state_selects_text_font() {
        let mut document = lopdf::Document::load_mem(&minimal_pdf()).expect("fixture PDF");
        let mut state = lopdf::Dictionary::new();
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
        for (encoding, codes, expected) in [
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
        ] {
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
            descendant.set("W", vec![lopdf::Object::Integer(1), 1.into(), 200.into()]);
            descendant.set("CIDSystemInfo", system);
            descendant.set("FontDescriptor", lopdf::Dictionary::new());
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
    fn embedded_document_parts_are_rejected_without_recursive_parsing() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer
                .write_all(b"<w:document xmlns:w=\"urn:w\"><w:body/></w:document>")
                .expect("fixture XML");
            writer
                .start_file("word/embeddings/nested.docx", FileOptions::default())
                .expect("embedded fixture entry");
            writer.write_all(b"PK\x03\x04").expect("embedded fixture");
            writer.finish().expect("fixture archive");
        }
        let error = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect_err("embedded documents must not recurse");
        assert!(matches!(
            error,
            DocumentExtractionError::InvalidDocxPackage { .. }
        ));
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
    fn docx_alternate_format_chunk_refuses_incomplete_publication() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Prefix</w:t></w:r></w:p><w:altChunk r:id="html"/></w:body></w:document>"#;
        assert!(matches!(
            parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. })
        ));
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
    fn docx_unsupported_xml_encoding_is_not_malformed() {
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
                assert!(matches!(
                    parse_docx(&bytes, &control(), IndexWorkStage::TextIndex),
                    Err(DocumentExtractionError::UnsupportedDocxInput { .. })
                ));
            }
        }
        for encoding in ["UTF-16", "ISO-8859-1"] {
            assert!(matches!(
                parse_docx(
                    xml.replace("UTF-16", encoding).as_bytes(),
                    &control(),
                    IndexWorkStage::TextIndex
                ),
                Err(DocumentExtractionError::UnsupportedDocxInput { .. })
            ));
        }
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
        let drawing = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:graphic><a:graphicData><w:txbxContent><w:p><w:r><w:t>Box</w:t></w:r></w:p></w:txbxContent></a:graphicData></a:graphic></w:drawing></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            parse_docx(drawing, &control(), IndexWorkStage::TextIndex)
                .expect("Word text inside opaque drawing wrappers")
                .text,
            "Box\n"
        );
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
                    part: DOCX_DOCUMENT_PART,
                    paragraph: 1,
                    run: 4,
                    text_start: 0,
                    text_end: 5,
                }
            );
        }
    }

    #[test]
    fn docx_field_carriers_retain_only_literal_and_cached_text() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE &amp; <![CDATA[ignored]]></w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText> Literal</w:instrText></w:r><w:r><w:delInstrText>Deleted code</w:delInstrText><w:delText><![CDATA[Deleted text]]></w:delText></w:r></w:p></w:body></w:document>"#;
        let parsed = extract_document_text_controlled(
            &docx_archive(xml, CompressionMethod::Deflated),
            "fields.docx",
            None,
            &control(),
        )
        .expect("field instructions and deleted carriers are valid bounded XML");
        assert_eq!(parsed.text, "Page 7 Literal");
        assert_eq!(parsed.completeness, DocumentCompleteness::Complete);
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
                    part: DOCX_DOCUMENT_PART,
                    paragraph: 1,
                    run,
                    text_start: 0,
                    text_end: text.len(),
                }
            );
        }
        let nested = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>OUTER</w:instrText><w:drawing><w:txbxContent><w:p><w:r><w:instrText>Box</w:instrText></w:r></w:p></w:txbxContent></w:drawing><w:fldChar w:fldCharType="begin"/><w:instrText>INNER</w:instrText><w:fldChar w:fldCharType="separate"/><w:instrText>Outer code remains ignored</w:instrText><w:fldChar w:fldCharType="end"/><w:fldChar w:fldCharType="separate"/><w:t>Result</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
        let nested = parse_docx(nested, &control(), IndexWorkStage::TextIndex)
            .expect("nested fields and text boxes keep independent state");
        assert_eq!(nested.text, "Box\nResult");
        assert_eq!(nested.facts.len(), 2);
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
    fn docx_compatibility_selects_one_understood_branch() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:future="urn:future"><w:body><mc:AlternateContent><mc:Choice Requires="future"><w:p><w:r><w:t>Unsupported</w:t></w:r></w:p></mc:Choice><mc:Choice Requires="w"><w:p><w:r><w:t>Chosen</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>Fallback</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent></w:body></w:document>"#;
        let parsed = parse_docx(xml.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("one understood choice");
        assert_eq!(parsed.text, "Chosen");
        assert_eq!(parsed.facts.len(), 1);
        let fallback = xml.replace("Requires=\"w\"", "Requires=\"future\"");
        let parsed = parse_docx(fallback.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("fallback when no choice is understood");
        assert_eq!(parsed.text, "Fallback");
        assert_eq!(parsed.facts.len(), 1);
        let nested = xml.replace("<w:t>Chosen</w:t>", "<mc:AlternateContent><mc:Choice Requires=\"future\"><w:instrText>Discarded</w:instrText></mc:Choice><mc:Fallback><w:t>Nested</w:t></mc:Fallback></mc:AlternateContent>");
        let parsed = parse_docx(nested.as_bytes(), &control(), IndexWorkStage::TextIndex)
            .expect("nested alternatives preserve the containing run");
        assert_eq!(parsed.text, "Nested");
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
                    part: DOCX_DOCUMENT_PART,
                    paragraph,
                    run: 1,
                    text_start: start,
                    text_end: end,
                }
            );
        }
    }

    #[test]
    fn docx_namespace_storage_is_charged_before_parser_allocation() {
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:unused=\"{}\"><w:body/></w:document>",
            "x".repeat(24 * 1024 * 1024),
        );
        let bytes = docx_archive(xml.as_bytes(), CompressionMethod::Deflated);
        assert!(matches!(
            extract_document_text_controlled(&bytes, "guide.docx", None, &control()),
            Err(DocumentExtractionError::ResourceLimit {
                limit: DocumentLimit::MemoryBytes,
                ..
            }),
        ));
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
              <w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t> world</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body>
            </w:document>"#;
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer.write_all(xml).expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
        let facts = extract_document_text_controlled(&bytes, "guide.docx", None, &control())
            .expect("valid DOCX");
        assert_eq!(facts.text, "Hello world\nCell");
        assert_eq!(facts.facts.len(), 3);
        assert!(matches!(
            facts.facts[2].locator,
            DocumentLocator::Docx {
                part: DOCX_DOCUMENT_PART,
                paragraph: 2,
                run: 1,
                text_start: 0,
                text_end: 4
            }
        ));
    }

    #[test]
    fn direct_xml_rejects_truncated_document_part() {
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer
                .write_all(b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>truncated")
                .expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
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
    fn direct_xml_preserves_entities_tabs_and_breaks() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A &amp; B</w:t><w:tab/><w:br/><w:t>C</w:t><w:noBreakHyphen/><w:t>D</w:t><w:softHyphen/><w:t>E</w:t><w:ptab w:alignment="left" w:relativeTo="margin" w:leader="none"/><w:t>F</w:t><w:lastRenderedPageBreak/><w:t>G</w:t></w:r></w:p></w:body></w:document>"#;
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer.write_all(xml).expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
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
    fn docx_font_specific_symbols_refuse_instead_of_inventing_text() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t><w:sym w:font="Wingdings" w:char="F03A"/><w:t>After</w:t></w:r></w:p></w:body></w:document>"#;
        assert!(matches!(
            extract_document_text_controlled(
                &docx_archive(xml, CompressionMethod::Deflated),
                "symbol.docx",
                None,
                &control()
            ),
            Err(DocumentExtractionError::UnsupportedDocxInput { .. })
        ));
    }

    #[test]
    fn direct_xml_rejects_external_doctype_declarations() {
        let xml = br#"<!DOCTYPE w:document SYSTEM "https://example.invalid/document.dtd"><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer.write_all(xml).expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
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
        let mut bytes = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file(DOCX_DOCUMENT_PART, FileOptions::default())
                .expect("fixture entry");
            writer.write_all(xml).expect("fixture XML");
            writer.finish().expect("fixture archive");
        }
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
