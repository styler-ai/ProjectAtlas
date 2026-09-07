//! Bounded, in-process extraction for the supported repository document formats.

use crate::check_parser_iteration;
use projectatlas_core::symbols::{CodeSymbol, ParserKind, SymbolGraph, SymbolKind};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use quick_xml::Reader;
use quick_xml::events::{BytesRef, Event};
use std::collections::HashSet;
use std::fmt;
use std::io::{Cursor, Read};
use std::path::Path;
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
}

/// Complete bounded text and provenance extracted from one document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentFacts {
    /// Format admitted by magic and language/extension checks.
    pub format: DocumentFormat,
    /// Newline-separated extracted text used by the existing text index.
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
                line_start: index + 1,
                line_end: index + 1,
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
    /// A DOCX entry requires encryption or compression outside the admitted set.
    #[error("unsupported DOCX package input: {message}")]
    UnsupportedDocxInput {
        /// Bounded package diagnostic.
        message: String,
    },
    /// Shared indexing cancellation or deadline stopped extraction.
    #[error(transparent)]
    Work(#[from] IndexWorkFailure),
}

/// Identify a document format using the registry language first and extension second.
#[must_use]
pub fn document_format_for_path(path: &str, language: Option<&str>) -> Option<DocumentFormat> {
    let language = language.unwrap_or_default().to_ascii_lowercase();
    match language.as_str() {
        "pdf" => Some(DocumentFormat::Pdf),
        "docx" => Some(DocumentFormat::Docx),
        _ => Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "pdf" => Some(DocumentFormat::Pdf),
                "docx" => Some(DocumentFormat::Docx),
                _ => None,
            }),
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
    // Retain the XML allocation plus space for parser staging, geometric string
    // growth in the current run/output/facts, and the bounded fact vector.
    // Archive metadata has already been dropped before these allocations overlap.
    check_memory_budget(
        bytes
            .len()
            .saturating_add(xml.capacity().saturating_mul(2))
            .saturating_add(MAX_DOCUMENT_OUTPUT_BYTES.saturating_mul(4))
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
}

/// Parse body paragraphs and table paragraphs from the admitted XML part.
fn parse_docx(
    xml: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<DocumentFacts, DocumentExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut output = String::new();
    let mut facts = Vec::new();
    let mut paragraph_open = false;
    let mut paragraph_number = 0usize;
    let mut run_number = 0usize;
    let mut run: Option<RawDocxRun> = None;
    let mut in_text = false;
    let mut element_depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut event_index = 0usize;
    loop {
        check_parser_iteration(event_index, &mut || control.check(stage))?;
        event_index = event_index.saturating_add(1);
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = event.name();
                if element_depth == 0 {
                    if root_seen || name.as_ref() != "w:document" {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX XML must contain one w:document root".to_owned(),
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
                match name.as_ref() {
                    "w:p" if !paragraph_open => {
                        paragraph_open = true;
                        paragraph_number += 1;
                        run_number = 0;
                        if !output.is_empty() {
                            push_output_byte(&mut output, b'\n')?;
                        }
                    }
                    "w:r" if paragraph_open && run.is_none() => {
                        run_number += 1;
                        run = Some(RawDocxRun::default());
                    }
                    "w:t" if run.is_some() && !in_text => in_text = true,
                    "w:tab" | "w:br" | "w:cr" => {
                        if let Some(run) = run.as_mut() {
                            append_docx_run_text(
                                run,
                                if name.as_ref() == "w:tab" { "\t" } else { "\n" },
                                output.len(),
                            )?;
                        }
                    }
                    "w:p" | "w:r" | "w:t" => {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "DOCX paragraph, run, or text nesting is invalid".to_owned(),
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                if in_text {
                    let Some(run) = run.as_mut() else {
                        return Err(DocumentExtractionError::Malformed {
                            format: DocumentFormat::Docx,
                            message: "text appeared outside a run".to_owned(),
                        });
                    };
                    append_docx_run_text(run, event.as_ref(), output.len())?;
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
            Ok(Event::CData(event)) => {
                if !in_text {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "CDATA appeared outside a run".to_owned(),
                    });
                }
                let Some(run) = run.as_mut() else {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "CDATA appeared outside a run".to_owned(),
                    });
                };
                append_docx_run_text(run, event.as_ref(), output.len())?;
            }
            Ok(Event::GeneralRef(reference)) => {
                if !in_text {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "entity appeared outside a text run".to_owned(),
                    });
                }
                let Some(run) = run.as_mut() else {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "entity appeared outside a run".to_owned(),
                    });
                };
                let text = decode_docx_reference(&reference)?;
                append_docx_run_text(run, &text, output.len())?;
            }
            Ok(Event::DocType(_)) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: "DOCX XML DOCTYPE and external declarations are unsupported"
                        .to_owned(),
                });
            }
            Ok(Event::End(event)) => {
                if element_depth == 0 {
                    return Err(DocumentExtractionError::Malformed {
                        format: DocumentFormat::Docx,
                        message: "DOCX XML contained an unmatched closing element".to_owned(),
                    });
                }
                let name = event.name();
                match name.as_ref() {
                    "w:t" => in_text = false,
                    "w:r" => {
                        if let Some(run) = run.take()
                            && !run.text.is_empty()
                        {
                            if facts.len() >= MAX_DOCUMENT_FACTS {
                                return Err(DocumentExtractionError::ResourceLimit {
                                    limit: DocumentLimit::FactCount,
                                    observed: facts.len() + 1,
                                    maximum: MAX_DOCUMENT_FACTS,
                                });
                            }
                            let end = run.text.len();
                            let required = output.len().saturating_add(end);
                            if required > MAX_DOCUMENT_OUTPUT_BYTES {
                                return Err(DocumentExtractionError::ResourceLimit {
                                    limit: DocumentLimit::OutputBytes,
                                    observed: required,
                                    maximum: MAX_DOCUMENT_OUTPUT_BYTES,
                                });
                            }
                            output.push_str(&run.text);
                            facts.push(DocumentFact {
                                text: run.text,
                                locator: DocumentLocator::Docx {
                                    part: DOCX_DOCUMENT_PART,
                                    paragraph: paragraph_number,
                                    run: run_number,
                                    text_start: 0,
                                    text_end: end,
                                },
                            });
                        }
                    }
                    "w:p" => paragraph_open = false,
                    _ => {}
                }
                element_depth -= 1;
                if element_depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(DocumentExtractionError::Malformed {
                    format: DocumentFormat::Docx,
                    message: error.to_string(),
                });
            }
            _ => {}
        }
    }
    if !root_seen
        || !root_closed
        || element_depth != 0
        || paragraph_open
        || run.is_some()
        || in_text
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
                text_start: 2,
                text_end: 10
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
            b"BT /F1 12 Tf 72 700 Td (Form Text Marker) Tj ET".to_vec(),
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
        content.extend_from_slice(b"\n/Fm1 Do\n");
        stream.set_content(content);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("fixture serialization");
        let facts = extract_document_text_controlled(&bytes, "guide.pdf", None, &control())
            .expect("valid Form content");
        assert!(facts.text.contains("Hello PDF"));
        assert!(facts.text.contains("Form Text Marker"), "{}", facts.text);
        assert!(
            facts
                .facts
                .iter()
                .any(|fact| fact.text.contains("Form Text Marker")
                    && matches!(fact.locator, DocumentLocator::Pdf { page: 1, .. }))
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
        for (codes, accepted) in [("0001", true), ("00010002", false), ("000100", false)] {
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
            descendant.set("CIDSystemInfo", system);
            descendant.set("FontDescriptor", lopdf::Dictionary::new());
            let descendant = document.add_object(descendant);
            let mut font = lopdf::Dictionary::new();
            font.set("Type", "Font");
            font.set("Subtype", "Type0");
            font.set("BaseFont", "Fixture");
            font.set("Encoding", "Identity-H");
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
            if accepted {
                assert_eq!(result.expect("mapped CID").text, "Z");
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
    fn docx_empty_elements_preserve_paragraph_and_run_ordinals() {
        let xml = b"<w:document><w:body><w:p/><w:p><w:r/><w:r><w:t>A</w:t><w:tab/><w:t/><w:t>B</w:t></w:r></w:p></w:body></w:document>";
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
            "<w:document>{}<w:body/>{}</w:document>",
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
            "<w:document><w:body><w:p><w:r><w:t>{run}</w:t></w:r><w:r><w:t>{run}</w:t></w:r><malformed"
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
                .write_all(b"<w:document><w:body><w:p><w:r><w:t>truncated")
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
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A &amp; B</w:t><w:tab/><w:br/><w:t>C</w:t></w:r></w:p></w:body></w:document>"#;
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
        assert_eq!(facts.text, "A & B\t\nC");
        assert_eq!(facts.facts[0].text, "A & B\t\nC");
        assert_eq!(
            facts.facts[0].locator.to_string(),
            "docx:part=word/document.xml;paragraph=1;run=1;text-span=0..8"
        );
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
