//! Bounded parser facts for Markdown and the Markdown subset of MDX.

use crate::check_parser_iteration;
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, SymbolGraph, SymbolKind, SymbolSourceSelector,
};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::ops::Range;

/// Maximum UTF-8 bytes admitted to one Markdown parse.
pub const MAX_MARKDOWN_BYTES: usize = 2_000_000;
/// Maximum headings retained from one Markdown document.
pub const MAX_MARKDOWN_HEADINGS: usize = 512;
/// Maximum explicit document-reference candidates retained from one document.
pub const MAX_DOCUMENT_LINK_CANDIDATES: usize = 1_024;
/// Maximum UTF-8 bytes retained for one heading or link label.
pub const MAX_MARKDOWN_LABEL_BYTES: usize = 240;
/// Maximum UTF-8 bytes retained for one repository-relative selector.
pub const MAX_DOCUMENT_SELECTOR_BYTES: usize = 512;
/// Maximum aggregate UTF-8 evidence bytes retained from one document.
pub const MAX_MARKDOWN_EVIDENCE_BYTES: usize = 262_144;

/// Parser implementation that emitted Markdown facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownParserProvenance {
    /// The workspace-pinned `pulldown-cmark` parser.
    PulldownCmark,
}

/// Completeness of one bounded Markdown extraction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownFactCompleteness {
    /// All supported syntax was examined without reaching a fact limit.
    Complete,
    /// A hard limit or explicitly unsupported structure prevented complete coverage.
    Partial,
}

/// Hard bound reached while extracting Markdown facts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MarkdownFactLimit {
    /// The document exceeded the admitted parser byte ceiling.
    InputBytes,
    /// The retained heading count reached its ceiling.
    HeadingCount,
    /// The retained explicit-reference count reached its ceiling.
    CandidateCount,
    /// A compact label exceeded its per-label byte ceiling.
    LabelBytes,
    /// A repository-relative selector exceeded its byte ceiling.
    SelectorBytes,
    /// Aggregate retained evidence reached its per-document ceiling.
    EvidenceBytes,
}

/// Unsupported Markdown/MDX structure observed by the parser.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MarkdownUnsupportedStructure {
    /// An image target was present; images never document source identities.
    Image,
    /// Raw HTML or JSX-like structure was present and was not interpreted as Markdown.
    RawHtmlOrMdx,
    /// A parser destination was dynamic or templated rather than a static identity.
    DynamicDestination,
}

/// Exact source selector for one parser fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownSourceSelector {
    /// Inclusive UTF-8 byte offset.
    pub byte_start: usize,
    /// Exclusive UTF-8 byte offset.
    pub byte_end: usize,
    /// Inclusive one-based start line.
    pub line_start: usize,
    /// Zero-based Unicode-scalar start column.
    pub column_start: usize,
    /// Inclusive one-based end line.
    pub line_end: usize,
    /// Exclusive zero-based Unicode-scalar end column.
    pub column_end: usize,
}

/// One bounded Markdown heading fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownHeadingFact {
    /// Heading depth from one through six.
    pub level: u8,
    /// Compact bounded heading label.
    pub text: String,
    /// Deterministic lowercase selector slug.
    pub slug: String,
    /// One-based occurrence among headings with the same slug.
    pub occurrence: usize,
    /// Exact source range for this heading.
    pub source: MarkdownSourceSelector,
}

/// Syntax that supplied one explicit document-reference candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentLinkSource {
    /// A destination emitted by the Markdown parser, including resolved reference links.
    MarkdownDestination,
    /// A complete inline-code span containing only one repository-relative selector.
    InlineCode,
}

/// One explicit static repository-local reference awaiting filesystem resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentLinkCandidate {
    /// Parser syntax that supplied the candidate.
    pub source_kind: DocumentLinkSource,
    /// Bounded repository-relative selector, including any query or fragment evidence.
    pub selector: String,
    /// Compact parser-visible label when the syntax supplies one.
    pub label: Option<String>,
    /// Stable enclosing heading selector, or the document file when absent.
    pub enclosing_heading: Option<String>,
    /// Exact source range for the complete link or code span.
    pub source: MarkdownSourceSelector,
}

/// Coverage state for one Markdown extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownFactCoverage {
    /// Whether supported structure was completely examined.
    pub completeness: MarkdownFactCompleteness,
    /// Deduplicated hard limits reached during extraction.
    pub limits: Vec<MarkdownFactLimit>,
    /// Deduplicated unsupported structure observed during extraction.
    pub unsupported: Vec<MarkdownUnsupportedStructure>,
}

/// Bounded parser facts derived from one Markdown or MDX document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownFacts {
    /// Parser provenance for every fact in this batch.
    pub provenance: MarkdownParserProvenance,
    /// Deterministic headings in source order.
    pub headings: Vec<MarkdownHeadingFact>,
    /// Explicit static local-reference candidates in source order.
    pub link_candidates: Vec<DocumentLinkCandidate>,
    /// Completeness, limit, and unsupported-structure state.
    pub coverage: MarkdownFactCoverage,
}

impl MarkdownFacts {
    /// Project the heading facts into the existing symbol graph contract.
    #[must_use]
    pub fn symbol_graph(&self, path: &str, language: Option<&str>) -> SymbolGraph {
        let symbols = self
            .headings
            .iter()
            .filter_map(|heading| Some((heading, crate::compact_symbol_identity(&heading.text)?)))
            .map(|(heading, name)| CodeSymbol {
                path: path.to_owned(),
                language: language.map(str::to_owned),
                name,
                kind: SymbolKind::Heading,
                signature: heading_signature(&heading.slug, heading.occurrence),
                exported: false,
                documentation: None,
                line_start: heading.source.line_start,
                line_end: heading.source.line_end,
                source_selector: Some(SymbolSourceSelector {
                    byte_start: heading.source.byte_start,
                    byte_end: heading.source.byte_end,
                    column_start: heading.source.column_start,
                    column_end: heading.source.column_end,
                }),
                parent: None,
                parser: ParserKind::Structural,
                detail: Some(format!(
                    "level={};slug={};occurrence={};bytes={}..{}",
                    heading.level,
                    heading.slug,
                    heading.occurrence,
                    heading.source.byte_start,
                    heading.source.byte_end
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

/// Extract bounded headings and explicit local-reference candidates.
#[must_use]
pub fn extract_markdown_facts(content: &str) -> MarkdownFacts {
    match extract_markdown_facts_checked(content, &mut || Ok::<(), Infallible>(())) {
        Ok(facts) => facts,
        Err(unreachable) => match unreachable {},
    }
}

/// Extract Markdown facts while observing the shared indexing cancellation boundary.
///
/// # Errors
///
/// Returns a typed cancellation or deadline failure without returning partial work.
pub fn extract_markdown_facts_controlled(
    content: &str,
    control: &IndexWorkControl,
) -> Result<MarkdownFacts, IndexWorkFailure> {
    extract_markdown_facts_checked(content, &mut || {
        control.check(IndexWorkStage::SymbolParsing)
    })
}

/// Extract Markdown facts while observing the caller's indexing checkpoint.
pub(crate) fn extract_markdown_facts_checked<E>(
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<MarkdownFacts, E> {
    check()?;
    let mut extraction = MarkdownExtraction::new(content);
    if content.len() > MAX_MARKDOWN_BYTES {
        extraction.limit(MarkdownFactLimit::InputBytes);
        return Ok(extraction.finish());
    }

    let parser = Parser::new_ext(content, Options::all()).into_offset_iter();
    for (iteration, (event, range)) in parser.enumerate() {
        check_parser_iteration(iteration, check)?;
        extraction.consume(event, range);
    }
    check()?;
    Ok(extraction.finish())
}

/// Mutable bounded state for one parser pass.
struct MarkdownExtraction<'a> {
    /// Original source used for exact range normalization.
    content: &'a str,
    /// Sorted byte offsets for one-based line lookup.
    line_starts: Vec<usize>,
    /// Retained heading facts in source order.
    headings: Vec<MarkdownHeadingFact>,
    /// Retained explicit references in source order.
    link_candidates: Vec<DocumentLinkCandidate>,
    /// Per-slug occurrence counters for duplicate heading identity.
    slug_occurrences: BTreeMap<String, usize>,
    /// Deduplicated limits reached during extraction.
    limits: Vec<MarkdownFactLimit>,
    /// Deduplicated unsupported structure observed during extraction.
    unsupported: Vec<MarkdownUnsupportedStructure>,
    /// Aggregate retained evidence bytes.
    retained_evidence_bytes: usize,
    /// Heading currently receiving inline parser text.
    heading: Option<HeadingBuilder>,
    /// Stable selector for the current enclosing heading section.
    enclosing_heading: Option<String>,
    /// Accepted link currently receiving parser-visible label text.
    link: Option<LinkBuilder>,
    /// Current link nesting depth, including rejected links.
    link_depth: usize,
    /// Current image nesting depth used to suppress alt-text code candidates.
    image_depth: usize,
}

impl<'a> MarkdownExtraction<'a> {
    /// Initialize bounded state and exact line offsets.
    fn new(content: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            content
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            content,
            line_starts,
            headings: Vec::new(),
            link_candidates: Vec::new(),
            slug_occurrences: BTreeMap::new(),
            limits: Vec::new(),
            unsupported: Vec::new(),
            retained_evidence_bytes: 0,
            heading: None,
            enclosing_heading: None,
            link: None,
            link_depth: 0,
            image_depth: 0,
        }
    }

    /// Consume one parser event and its exact source range.
    fn consume(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.enclosing_heading = None;
                self.heading = Some(HeadingBuilder::new(level, range.start));
            }
            Event::End(TagEnd::Heading(_)) => self.finish_heading(range.end),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.link_depth = self.link_depth.saturating_add(1);
                self.link = if dest_url.len() > MAX_DOCUMENT_SELECTOR_BYTES {
                    self.limit(MarkdownFactLimit::SelectorBytes);
                    None
                } else {
                    admit_selector(dest_url.as_ref(), DocumentLinkSource::MarkdownDestination)
                        .map(|selector| LinkBuilder::new(selector, range.start))
                };
                if looks_dynamic(dest_url.as_ref()) {
                    self.unsupported(MarkdownUnsupportedStructure::DynamicDestination);
                }
            }
            Event::End(TagEnd::Link) => {
                self.finish_link(range.end);
                self.link_depth = self.link_depth.saturating_sub(1);
            }
            Event::Start(Tag::Image { .. }) => {
                self.image_depth = self.image_depth.saturating_add(1);
                self.unsupported(MarkdownUnsupportedStructure::Image);
            }
            Event::End(TagEnd::Image) => {
                self.image_depth = self.image_depth.saturating_sub(1);
            }
            Event::Text(text) => {
                if let Some(heading) = self.heading.as_mut() {
                    heading.label.push(text.as_ref());
                }
                if let Some(link) = self.link.as_mut() {
                    link.label.push(text.as_ref());
                }
            }
            Event::Code(code) => {
                if let Some(heading) = self.heading.as_mut() {
                    heading.label.push(code.as_ref());
                }
                if let Some(link) = self.link.as_mut() {
                    link.label.push(code.as_ref());
                }
                if self.link_depth == 0 && self.image_depth == 0 {
                    self.push_code_candidate(code.as_ref(), range);
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                self.unsupported(MarkdownUnsupportedStructure::RawHtmlOrMdx);
            }
            _ => {}
        }
    }

    /// Complete and retain the active heading when within bounds.
    fn finish_heading(&mut self, byte_end: usize) {
        let Some(heading) = self.heading.take() else {
            return;
        };
        if heading.label.truncated {
            self.limit(MarkdownFactLimit::LabelBytes);
        }
        let text = heading.label.finish();
        if text.is_empty() {
            return;
        }
        if self.headings.len() >= MAX_MARKDOWN_HEADINGS {
            self.limit(MarkdownFactLimit::HeadingCount);
            return;
        }
        let slug = heading_slug(&text);
        let evidence_bytes = text.len().saturating_add(slug.len());
        if !self.retain(evidence_bytes) {
            return;
        }
        let occurrence = self.slug_occurrences.entry(slug.clone()).or_default();
        *occurrence = occurrence.saturating_add(1);
        let signature = heading_signature(&slug, *occurrence);
        self.headings.push(MarkdownHeadingFact {
            level: heading_level(heading.level),
            text,
            slug,
            occurrence: *occurrence,
            source: self.source_selector(heading.byte_start, byte_end),
        });
        self.enclosing_heading = Some(signature);
    }

    /// Complete and retain the active parser-emitted link when within bounds.
    fn finish_link(&mut self, byte_end: usize) {
        let Some(link) = self.link.take() else {
            return;
        };
        if link.label.truncated {
            self.limit(MarkdownFactLimit::LabelBytes);
        }
        let label = link.label.finish();
        self.push_candidate(
            DocumentLinkSource::MarkdownDestination,
            link.selector,
            (!label.is_empty()).then_some(label),
            link.byte_start,
            byte_end,
        );
    }

    /// Admit a complete inline-code span only when it is one path selector.
    fn push_code_candidate(&mut self, code: &str, range: Range<usize>) {
        if code != code.trim() {
            return;
        }
        if code.len() > MAX_DOCUMENT_SELECTOR_BYTES {
            self.limit(MarkdownFactLimit::SelectorBytes);
            return;
        }
        let Some(selector) = admit_selector(code, DocumentLinkSource::InlineCode) else {
            return;
        };
        self.push_candidate(
            DocumentLinkSource::InlineCode,
            selector,
            None,
            range.start,
            range.end,
        );
    }

    /// Retain one already-admitted explicit candidate within count and byte bounds.
    fn push_candidate(
        &mut self,
        source_kind: DocumentLinkSource,
        selector: String,
        label: Option<String>,
        byte_start: usize,
        byte_end: usize,
    ) {
        if selector.len() > MAX_DOCUMENT_SELECTOR_BYTES {
            self.limit(MarkdownFactLimit::SelectorBytes);
            return;
        }
        if self.link_candidates.len() >= MAX_DOCUMENT_LINK_CANDIDATES {
            self.limit(MarkdownFactLimit::CandidateCount);
            return;
        }
        let evidence_bytes = selector
            .len()
            .saturating_add(label.as_ref().map_or(0, String::len))
            .saturating_add(self.enclosing_heading.as_ref().map_or(0, String::len));
        if !self.retain(evidence_bytes) {
            return;
        }
        self.link_candidates.push(DocumentLinkCandidate {
            source_kind,
            selector,
            label,
            enclosing_heading: self.enclosing_heading.clone(),
            source: self.source_selector(byte_start, byte_end),
        });
    }

    /// Reserve aggregate evidence bytes without exceeding the hard ceiling.
    fn retain(&mut self, bytes: usize) -> bool {
        let Some(next) = self.retained_evidence_bytes.checked_add(bytes) else {
            self.limit(MarkdownFactLimit::EvidenceBytes);
            return false;
        };
        if next > MAX_MARKDOWN_EVIDENCE_BYTES {
            self.limit(MarkdownFactLimit::EvidenceBytes);
            return false;
        }
        self.retained_evidence_bytes = next;
        true
    }

    /// Convert parser byte offsets into the public exact source selector.
    fn source_selector(&self, byte_start: usize, byte_end: usize) -> MarkdownSourceSelector {
        let byte_end = self.trim_trailing_line_endings(byte_start, byte_end);
        MarkdownSourceSelector {
            byte_start,
            byte_end,
            line_start: self.line_at(byte_start),
            column_start: 0,
            line_end: self.line_at(byte_end.saturating_sub(1).max(byte_start)),
            column_end: 0,
        }
    }

    /// Remove parser-owned trailing line endings from one heading or link range.
    fn trim_trailing_line_endings(&self, byte_start: usize, mut byte_end: usize) -> usize {
        while byte_end > byte_start
            && self
                .content
                .as_bytes()
                .get(byte_end - 1)
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            byte_end -= 1;
        }
        byte_end
    }

    /// Return the one-based line containing a byte offset.
    fn line_at(&self, byte: usize) -> usize {
        self.line_starts
            .partition_point(|start| *start <= byte)
            .max(1)
    }

    /// Record one reached limit once in stable enum order.
    fn limit(&mut self, limit: MarkdownFactLimit) {
        insert_sorted_unique(&mut self.limits, limit);
    }

    /// Record one unsupported structure once in stable enum order.
    fn unsupported(&mut self, unsupported: MarkdownUnsupportedStructure) {
        insert_sorted_unique(&mut self.unsupported, unsupported);
    }

    /// Finalize immutable public facts and derive completeness.
    fn finish(mut self) -> MarkdownFacts {
        let mut offsets = Vec::with_capacity(
            self.headings
                .len()
                .saturating_add(self.link_candidates.len())
                .saturating_mul(2),
        );
        for source in self.headings.iter().map(|heading| &heading.source).chain(
            self.link_candidates
                .iter()
                .map(|candidate| &candidate.source),
        ) {
            offsets.push(source.byte_start);
            offsets.push(source.byte_end);
        }
        let positions = source_positions(self.content, offsets);
        for source in self
            .headings
            .iter_mut()
            .map(|heading| &mut heading.source)
            .chain(
                self.link_candidates
                    .iter_mut()
                    .map(|candidate| &mut candidate.source),
            )
        {
            apply_source_positions(source, &positions);
        }
        let completeness = if self.limits.is_empty() && self.unsupported.is_empty() {
            MarkdownFactCompleteness::Complete
        } else {
            MarkdownFactCompleteness::Partial
        };
        MarkdownFacts {
            provenance: MarkdownParserProvenance::PulldownCmark,
            headings: self.headings,
            link_candidates: self.link_candidates,
            coverage: MarkdownFactCoverage {
                completeness,
                limits: self.limits,
                unsupported: self.unsupported,
            },
        }
    }
}

/// Map sorted fact byte boundaries to exact source lines and Unicode-scalar columns in one pass.
fn source_positions(content: &str, mut offsets: Vec<usize>) -> Vec<(usize, usize, usize)> {
    offsets.sort_unstable();
    offsets.dedup();
    let mut positions = Vec::with_capacity(offsets.len());
    let mut next_offset = 0;
    let mut byte = 0;
    let mut line = 1;
    let mut column = 0;
    for character in content.chars() {
        while offsets
            .get(next_offset)
            .is_some_and(|offset| *offset == byte)
        {
            positions.push((byte, line, column));
            next_offset += 1;
        }
        byte += character.len_utf8();
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    while offsets
        .get(next_offset)
        .is_some_and(|offset| *offset == byte)
    {
        positions.push((byte, line, column));
        next_offset += 1;
    }
    positions
}

/// Apply exact positions for one parser source selector.
fn apply_source_positions(
    source: &mut MarkdownSourceSelector,
    positions: &[(usize, usize, usize)],
) {
    if let Ok(index) = positions.binary_search_by_key(&source.byte_start, |position| position.0) {
        source.line_start = positions[index].1;
        source.column_start = positions[index].2;
    }
    if let Ok(index) = positions.binary_search_by_key(&source.byte_end, |position| position.0) {
        source.line_end = positions[index].1;
        source.column_end = positions[index].2;
    }
}

/// Active heading assembled from bounded inline text events.
struct HeadingBuilder {
    /// Parser heading depth.
    level: HeadingLevel,
    /// Inclusive source start byte.
    byte_start: usize,
    /// Bounded compact label accumulator.
    label: BoundedLabel,
}

impl HeadingBuilder {
    /// Start one heading at its parser source range.
    fn new(level: HeadingLevel, byte_start: usize) -> Self {
        Self {
            level,
            byte_start,
            label: BoundedLabel::default(),
        }
    }
}

/// Active accepted Markdown link assembled from parser events.
struct LinkBuilder {
    /// Static repository-relative selector.
    selector: String,
    /// Inclusive source start byte.
    byte_start: usize,
    /// Bounded compact visible label.
    label: BoundedLabel,
}

impl LinkBuilder {
    /// Start one accepted link at its parser source range.
    fn new(selector: String, byte_start: usize) -> Self {
        Self {
            selector,
            byte_start,
            label: BoundedLabel::default(),
        }
    }
}

/// UTF-8-safe compact text accumulator with a hard byte ceiling.
#[derive(Default)]
struct BoundedLabel {
    /// Retained compact text.
    text: String,
    /// Whether the next retained scalar needs one separating space.
    pending_space: bool,
    /// Whether at least one scalar could not be retained.
    truncated: bool,
}

impl BoundedLabel {
    /// Append one parser text fragment while compacting whitespace.
    fn push(&mut self, value: &str) {
        for character in value.chars() {
            if character.is_whitespace() {
                self.pending_space = !self.text.is_empty();
                continue;
            }
            let separator_bytes = usize::from(self.pending_space && !self.text.is_empty());
            if self
                .text
                .len()
                .saturating_add(separator_bytes)
                .saturating_add(character.len_utf8())
                > MAX_MARKDOWN_LABEL_BYTES
            {
                self.truncated = true;
                continue;
            }
            if separator_bytes == 1 {
                self.text.push(' ');
            }
            self.pending_space = false;
            self.text.push(character);
        }
    }

    /// Return the bounded compact text.
    fn finish(self) -> String {
        self.text
    }
}

/// Validate one parser or inline-code selector without filesystem guessing.
fn admit_selector(value: &str, source: DocumentLinkSource) -> Option<String> {
    if value.is_empty()
        || value.starts_with(['/', '\\', '#', '?'])
        || value.ends_with('/')
        || value.contains(['\\', '\0', '\r', '\n'])
        || value.contains("//")
        || looks_dynamic(value)
        || (source == DocumentLinkSource::InlineCode && value.chars().any(char::is_whitespace))
    {
        return None;
    }
    let path_end = value.find(['?', '#']).unwrap_or(value.len());
    let path = &value[..path_end];
    if path.is_empty() || path.ends_with(['/', '\\']) {
        return None;
    }
    let first_segment = path.split('/').next().unwrap_or_default();
    if first_segment.contains(':') {
        return None;
    }
    let identity = strip_line_selector(path);
    let final_segment = identity.rsplit('/').next().unwrap_or_default();
    if final_segment.is_empty() || matches!(final_segment, "." | "..") {
        return None;
    }
    let path_like = source == DocumentLinkSource::MarkdownDestination
        || identity.contains('/')
        || final_segment.starts_with('.')
        || final_segment
            .rsplit_once('.')
            .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty());
    path_like.then(|| value.to_owned())
}

/// Remove an optional supported line selector when testing file-shaped syntax.
fn strip_line_selector(path: &str) -> &str {
    let Some((identity, selector)) = path.rsplit_once(':') else {
        return path;
    };
    let selector = selector.strip_prefix('L').unwrap_or(selector);
    let line_selector = selector.split_once('-').map_or_else(
        || !selector.is_empty() && selector.chars().all(|character| character.is_ascii_digit()),
        |(start, end)| {
            !start.is_empty()
                && !end.is_empty()
                && start.chars().all(|character| character.is_ascii_digit())
                && end
                    .strip_prefix('L')
                    .unwrap_or(end)
                    .chars()
                    .all(|character| character.is_ascii_digit())
        },
    );
    if line_selector { identity } else { path }
}

/// Return whether a selector contains dynamic or templated syntax.
fn looks_dynamic(value: &str) -> bool {
    value.contains(['{', '}', '<', '>', '|', '*']) || value.contains('$')
}

/// Convert the parser heading enum into the stable numeric depth.
fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Build the stable selector shared by heading symbols and enclosing link facts.
fn heading_signature(slug: &str, occurrence: usize) -> String {
    if occurrence == 1 {
        slug.to_owned()
    } else {
        format!("{slug}-{}", occurrence - 1)
    }
}

/// Build a deterministic bounded Unicode-aware heading slug.
fn heading_slug(text: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            separator = false;
            slug.extend(character.to_lowercase());
        } else if character.is_whitespace() || character == '-' {
            separator = !slug.is_empty();
        }
    }
    if slug.is_empty() {
        "section".to_owned()
    } else {
        slug
    }
}

/// Insert one enum state once while retaining its declaration order.
fn insert_sorted_unique<T: Ord>(values: &mut Vec<T>, value: T) {
    if let Err(index) = values.binary_search(&value) {
        values.insert(index, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_exact_unicode_headings_and_static_candidates() {
        let content = "# Über `Atlas`\n\nÜber Atlas\n----------\n\né [readme](README)\n\n[core][core]\n\n[core]: ../src/lib.rs#entry\n\nUse `src/lib.rs:12-20`.\n";
        let facts = extract_markdown_facts(content);

        assert_eq!(facts.provenance, MarkdownParserProvenance::PulldownCmark);
        assert_eq!(
            facts.coverage.completeness,
            MarkdownFactCompleteness::Complete
        );
        assert_eq!(facts.headings.len(), 2);
        assert_eq!(facts.headings[0].text, "Über Atlas");
        assert_eq!(facts.headings[0].slug, "über-atlas");
        assert_eq!(facts.headings[0].occurrence, 1);
        assert_eq!(facts.headings[0].source.line_start, 1);
        assert_eq!(facts.headings[0].source.column_start, 0);
        assert_eq!(facts.headings[0].source.line_end, 1);
        assert_eq!(facts.headings[0].source.column_end, 14);
        assert_eq!(
            &content[facts.headings[0].source.byte_start..facts.headings[0].source.byte_end],
            "# Über `Atlas`"
        );
        assert_eq!(facts.headings[1].slug, "über-atlas");
        assert_eq!(facts.headings[1].occurrence, 2);
        assert_eq!(facts.headings[1].source.line_start, 3);
        assert_eq!(facts.headings[1].source.column_start, 0);
        assert_eq!(facts.headings[1].source.line_end, 4);
        assert_eq!(facts.headings[1].source.column_end, 10);
        assert_eq!(
            facts
                .link_candidates
                .iter()
                .map(|candidate| (candidate.source_kind, candidate.selector.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (DocumentLinkSource::MarkdownDestination, "README"),
                (
                    DocumentLinkSource::MarkdownDestination,
                    "../src/lib.rs#entry"
                ),
                (DocumentLinkSource::InlineCode, "src/lib.rs:12-20"),
            ]
        );
        assert_eq!(facts.link_candidates[0].source.line_start, 6);
        assert_eq!(facts.link_candidates[0].source.column_start, 2);
        assert_eq!(facts.link_candidates[0].source.line_end, 6);
        assert_eq!(facts.link_candidates[0].source.column_end, 18);
        assert!(
            facts
                .link_candidates
                .iter()
                .all(|candidate| candidate.enclosing_heading.as_deref() == Some("über-atlas-1"))
        );

        let graph = facts.symbol_graph("docs/guide.md", Some("markdown"));
        assert_eq!(graph.parser, ParserKind::Structural);
        assert_eq!(graph.symbols.len(), 2);
        assert_eq!(graph.symbols[0].kind, SymbolKind::Heading);
        assert_eq!(graph.symbols[0].line_start, 1);
        assert_eq!(
            graph.symbols[0].source_selector,
            Some(SymbolSourceSelector {
                byte_start: 0,
                byte_end: 15,
                column_start: 0,
                column_end: 14,
            })
        );
        assert_eq!(
            graph
                .symbols
                .iter()
                .map(|symbol| symbol.signature.as_str())
                .collect::<Vec<_>>(),
            vec!["über-atlas", "über-atlas-1"]
        );
        assert!(
            graph.symbols[0]
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("slug=über-atlas;occurrence=1;bytes="))
        );
    }

    #[test]
    fn symbol_graph_reserves_the_derived_scope_namespace() {
        let content = format!(
            "# {}literal\n\n# Visible\n",
            projectatlas_core::graph::QUALIFIED_SYMBOL_SCOPE_PREFIX
        );
        let facts = extract_markdown_facts(&content);
        let graph = facts.symbol_graph("README.md", Some("markdown"));

        assert_eq!(facts.headings.len(), 2);
        assert_eq!(
            graph
                .symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["Visible"]
        );
    }

    #[test]
    fn rejects_non_local_non_static_and_false_positive_candidates() {
        let content = r"
![image](assets/logo.png)
[external](https://example.test/x)
[absolute](/src/lib.rs)
[drive](C:/src/lib.rs)
[unc](//server/share/lib.rs)
[dynamic]({target})
[templated](docs/$name.md)
[fragment](#entry)
[directory](../src/)
`foo()` `cargo test` `README` `../` `https://example.test/x` `src/lib.rs and prose`

```md
# fenced heading
`src/fenced.rs`
```

<section>
# raw HTML heading
</section>
";
        let facts = extract_markdown_facts(content);

        assert!(facts.headings.is_empty());
        assert!(facts.link_candidates.is_empty());
        assert_eq!(
            facts.coverage.completeness,
            MarkdownFactCompleteness::Partial
        );
        assert_eq!(
            facts.coverage.unsupported,
            vec![
                MarkdownUnsupportedStructure::Image,
                MarkdownUnsupportedStructure::RawHtmlOrMdx,
                MarkdownUnsupportedStructure::DynamicDestination,
            ]
        );
    }

    #[test]
    fn keeps_supported_markdown_outside_mdx_structure() {
        let content = "<Component source={target}>\n# Not a heading\n</Component>\n\n# Real heading\n\n[src](src/lib.rs)\n";
        let facts = extract_markdown_facts(content);

        assert_eq!(facts.headings.len(), 1);
        assert_eq!(facts.headings[0].text, "Real heading");
        assert_eq!(facts.link_candidates.len(), 1);
        assert_eq!(facts.link_candidates[0].selector, "src/lib.rs");
        assert_eq!(
            facts.coverage.completeness,
            MarkdownFactCompleteness::Partial
        );
        assert_eq!(
            facts.coverage.unsupported,
            vec![MarkdownUnsupportedStructure::RawHtmlOrMdx]
        );
    }

    #[test]
    fn exposes_hard_limits_without_unbounded_retention() {
        use std::fmt::Write as _;

        let oversized = "x".repeat(MAX_MARKDOWN_BYTES + 1);
        let oversized_facts = extract_markdown_facts(&oversized);
        assert!(oversized_facts.headings.is_empty());
        assert_eq!(
            oversized_facts.coverage.limits,
            vec![MarkdownFactLimit::InputBytes]
        );

        let oversized_selector = format!(
            "[target](src/{}.rs)\n",
            "x".repeat(MAX_DOCUMENT_SELECTOR_BYTES)
        );
        let selector_facts = extract_markdown_facts(&oversized_selector);
        assert!(selector_facts.link_candidates.is_empty());
        assert!(
            selector_facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::SelectorBytes)
        );

        let long_label = format!("# {}\n", "é".repeat(MAX_MARKDOWN_LABEL_BYTES));
        let label_facts = extract_markdown_facts(&long_label);
        assert_eq!(label_facts.headings.len(), 1);
        assert!(label_facts.headings[0].text.len() <= MAX_MARKDOWN_LABEL_BYTES);
        assert_eq!(
            label_facts.coverage.limits,
            vec![MarkdownFactLimit::LabelBytes]
        );

        let mut many_headings = String::new();
        for index in 0..=MAX_MARKDOWN_HEADINGS {
            assert!(writeln!(many_headings, "# Heading {index}").is_ok());
        }
        let heading_facts = extract_markdown_facts(&many_headings);
        assert_eq!(heading_facts.headings.len(), MAX_MARKDOWN_HEADINGS);
        assert!(
            heading_facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::HeadingCount)
        );

        let mut many_candidates = String::new();
        for index in 0..=MAX_DOCUMENT_LINK_CANDIDATES {
            assert!(writeln!(many_candidates, "[target](src/file_{index}.rs)").is_ok());
        }
        let candidate_facts = extract_markdown_facts(&many_candidates);
        assert_eq!(
            candidate_facts.link_candidates.len(),
            MAX_DOCUMENT_LINK_CANDIDATES
        );
        assert!(
            candidate_facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::CandidateCount)
        );

        let mut evidence = MarkdownExtraction::new("");
        assert!(evidence.retain(MAX_MARKDOWN_EVIDENCE_BYTES));
        assert!(!evidence.retain(1));
        assert_eq!(evidence.limits, vec![MarkdownFactLimit::EvidenceBytes]);
    }

    #[test]
    fn markdown_dispatch_is_additive_and_rst_remains_unsupported() {
        let markdown = crate::extract_symbol_graph(
            "docs/guide.mdx",
            Some("markdown"),
            "# Guide\n\n[src](src/lib.rs)\n",
        );
        assert_eq!(markdown.parser, ParserKind::Structural);
        assert_eq!(markdown.symbols.len(), 1);
        assert_eq!(markdown.symbols[0].kind, SymbolKind::Heading);
        assert!(markdown.relations.is_empty());

        let rst = crate::extract_symbol_graph(
            "docs/guide.rst",
            Some("rst"),
            "Guide\n=====\n\n:doc:`src/lib.rs`\n",
        );
        assert_eq!(rst.parser, ParserKind::Fallback);
        assert!(rst.symbols.is_empty());
        assert!(rst.relations.is_empty());
    }

    #[test]
    fn controlled_facts_preserve_results_and_propagate_cancellation() {
        use projectatlas_core::IndexCancellation;

        let content = "# Guide\n\n[src](src/lib.rs)\n";
        let active = IndexWorkControl::new(IndexCancellation::new(), None);
        assert_eq!(
            extract_markdown_facts_controlled(content, &active),
            Ok(extract_markdown_facts(content))
        );

        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let cancelled = IndexWorkControl::new(cancellation, None);
        assert_eq!(
            extract_markdown_facts_controlled(content, &cancelled),
            Err(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SymbolParsing,
            })
        );
    }
}
