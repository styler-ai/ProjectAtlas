//! Purpose: Provide shared `ProjectAtlas` query services for CLI and MCP adapters.

mod import_aliases;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use import_aliases::{ImportAliasMap, load_import_alias_map};
use projectatlas_core::outline::estimate_tokens;
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolKind, SymbolRelation,
};
use projectatlas_core::{IndexedNode, NodeKind, repo_path_to_native, validated_repo_file_key};
use projectatlas_db::{AtlasStore, DbError, IndexedFileText};
use regex::RegexBuilder;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Maximum caller references retained for one summarized symbol.
const CALLERS_PER_SYMBOL_LIMIT: usize = 20;
/// Relation query limit multiplier used for called-by lookup.
const CALLER_RELATION_LIMIT_PER_TARGET: usize = 20;
/// Maximum package/module symbols read for file-level metadata.
const FILE_METADATA_SYMBOL_LIMIT: usize = 20;
/// Status emitted when live source was read successfully.
const SOURCE_STATUS_LIVE: &str = "live-source";
/// Status emitted when indexed metadata had to stand in for live source.
const SOURCE_STATUS_INDEXED: &str = "indexed-metadata";

/// Service-layer failures.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Database operation failed.
    #[error("{0}")]
    Db(#[from] DbError),
    /// User input or stored metadata was invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Filesystem operation failed.
    #[error("io error for {path:?}: {source}")]
    Io {
        /// Path involved in the IO failure.
        path: PathBuf,
        /// Source IO error.
        source: std::io::Error,
    },
    /// Serialization failed while building a telemetry baseline.
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Convenient result alias for service operations.
pub type ServiceResult<T> = Result<T, ServiceError>;

/// Structured deterministic intelligence for one indexed file.
#[derive(Debug, Serialize)]
pub struct FileSummaryReport {
    /// Repository-relative file path.
    pub file_path: String,
    /// Detected language or file family.
    pub language: String,
    /// Source line count when the file can be read.
    pub line_count: usize,
    /// Whether source-derived fields came from live source or indexed metadata.
    pub source_status: String,
    /// Error text when live source could not be read.
    pub source_error: String,
    /// Parser family that produced the stored observed summary.
    pub parser_kind: String,
    /// Summary quality status: `ok`, `fallback`, or `missing`.
    pub summary_status: String,
    /// Current purpose one-liner, if approved or suggested.
    pub purpose: String,
    /// Purpose lifecycle status.
    pub purpose_status: String,
    /// Purpose source.
    pub purpose_source: String,
    /// Observed one-line summary from scan and deep index facts.
    pub observed_summary: String,
    /// Package, module, or manifest name when indexed.
    pub package: String,
    /// File or primary symbol documentation when indexed.
    pub docstring: String,
    /// Total indexed symbols.
    pub symbol_count: usize,
    /// Maximum rows returned per repeated section.
    pub limit: usize,
    /// Total indexed functions before limiting.
    pub total_functions: usize,
    /// Total indexed methods before limiting.
    pub total_methods: usize,
    /// Total indexed classes before limiting.
    pub total_classes: usize,
    /// Total indexed type-like declarations before limiting.
    pub total_types: usize,
    /// Total call relationships before limiting.
    pub total_calls: usize,
    /// Total import relationships before limiting.
    pub total_imports: usize,
    /// Total manifest dependency relationships before limiting.
    pub total_dependencies: usize,
    /// Total exported/public symbols before limiting.
    pub total_exports: usize,
    /// Whether any repeated section was truncated.
    pub truncated: bool,
    /// Indexed functions.
    pub functions: Vec<FileSymbolSummary>,
    /// Indexed methods.
    pub methods: Vec<FileSymbolSummary>,
    /// Indexed classes or class-like types.
    pub classes: Vec<FileSymbolSummary>,
    /// Indexed structs, enums, traits, interfaces, and type aliases.
    pub types: Vec<FileSymbolSummary>,
    /// Imported modules and include-like dependencies.
    pub imports: Vec<String>,
    /// Manifest package dependencies.
    pub dependencies: Vec<String>,
    /// Exported or publicly visible declarations.
    pub exports: Vec<String>,
    /// Call relationships discovered inside this file.
    pub calls: Vec<FileCallSummary>,
}

/// Compact file-summary symbol row.
#[derive(Debug, Serialize)]
pub struct FileSymbolSummary {
    /// Symbol name.
    pub name: String,
    /// Symbol kind.
    pub kind: String,
    /// One-based start line.
    pub line: usize,
    /// One-based end line.
    pub end_line: usize,
    /// Declaration signature.
    pub signature: String,
    /// Whether the symbol is exported or publicly visible.
    pub exported: bool,
    /// Extracted doc comment or docstring.
    pub documentation: String,
    /// Optional parent symbol.
    pub parent: String,
    /// Symbols that call this symbol across the indexed graph.
    pub called_by: Vec<String>,
}

/// Compact file-summary call row.
#[derive(Debug, Serialize)]
pub struct FileCallSummary {
    /// Calling symbol name.
    pub source: String,
    /// Called symbol name.
    pub target: String,
    /// One-based call line.
    pub line: usize,
    /// Compact call-site context.
    pub context: String,
}

/// Result row for indexed text search.
#[derive(Debug, Serialize)]
pub struct SearchMatch {
    /// Repository-relative path.
    pub path: String,
    /// One-based line number.
    pub line: usize,
    /// Context before the matching line.
    pub context_before: Vec<String>,
    /// Matching line text.
    pub text: String,
    /// Context after the matching line.
    pub context_after: Vec<String>,
}

/// Search report returned by CLI and MCP adapters.
#[derive(Debug, Serialize)]
pub struct SearchReport {
    /// Search pattern.
    pub query: String,
    /// Search mode: `literal`, `regex`, or `fuzzy`.
    pub mode: String,
    /// Source used for broad repository search.
    pub source: String,
    /// Pagination start index.
    pub start_index: usize,
    /// Matches observed before pagination and bounded early stop.
    pub total: usize,
    /// Alias for `total` that makes bounded search semantics explicit.
    pub observed_total: usize,
    /// Whether `total`/`observed_total` is known to be the exhaustive match count.
    pub total_is_complete: bool,
    /// Returned matches after pagination.
    pub returned: usize,
    /// Indexed files opened while serving the query.
    pub searched_files: usize,
    /// Source bytes read while serving the query.
    pub searched_bytes: usize,
    /// Whether the search stopped after satisfying the requested page.
    pub truncated: bool,
    /// Search matches.
    pub results: Vec<SearchMatch>,
}

/// Exact code slice returned after orientation.
#[derive(Debug, Serialize)]
pub struct CodeSlice {
    /// Repository-relative path.
    pub path: String,
    /// One-based start line.
    pub start_line: usize,
    /// One-based end line.
    pub end_line: usize,
    /// Total source line count.
    pub line_count: usize,
    /// Estimated tokens for the slice.
    pub estimated_tokens: usize,
    /// Slice content.
    pub content: String,
}

/// Optional selectors for disambiguating a symbol slice.
#[derive(Debug, Default)]
pub struct SymbolSliceSelector<'a> {
    /// Symbol name to locate.
    pub name: &'a str,
    /// Optional parent symbol, such as a class or struct name.
    pub parent: Option<&'a str>,
    /// Optional symbol kind, such as `function`, `method`, or `struct`.
    pub kind: Option<&'a str>,
    /// Optional line that must fall inside the selected symbol range.
    pub line: Option<usize>,
}

impl From<&SymbolRelation> for FileCallSummary {
    fn from(relation: &SymbolRelation) -> Self {
        Self {
            source: relation.source_name.clone(),
            target: relation.target_name.clone(),
            line: relation.line,
            context: relation.context.clone(),
        }
    }
}

/// Build structured file intelligence from the durable index.
///
/// # Errors
///
/// Returns an error when the file path is invalid, not indexed, or indexed
/// metadata cannot be read.
pub fn build_file_summary(
    store: &AtlasStore,
    file: &Path,
    limit: usize,
) -> ServiceResult<FileSummaryReport> {
    let file_key = validated_indexed_file_key(store, file)?;
    let effective_limit = limit.max(1);
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| ServiceError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    let metadata_symbols = store.load_symbols_by_kinds(
        &file_key,
        &metadata_symbol_kinds(),
        FILE_METADATA_SYMBOL_LIMIT,
    )?;
    let source_read =
        indexed_native_path(store, &file_key).and_then(|path| read_file_content(&path));
    let (file_content, source_status, source_error) = match source_read {
        Ok(content) => (Some(content), SOURCE_STATUS_LIVE.to_string(), String::new()),
        Err(error) => (None, SOURCE_STATUS_INDEXED.to_string(), error.to_string()),
    };
    let line_count = file_content.as_deref().map_or_else(
        || store.max_symbol_end_line_for_path(&file_key),
        |content| Ok(line_count_from_content(content)),
    )?;
    let docstring = file_content
        .as_deref()
        .and_then(file_level_docstring)
        .unwrap_or_else(|| file_docstring(&metadata_symbols));
    let function_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Function], effective_limit)?;
    let method_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Method], effective_limit)?;
    let class_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Class], effective_limit)?;
    let type_kinds = type_symbol_kinds();
    let type_symbols = store.load_symbols_by_kinds(&file_key, &type_kinds, effective_limit)?;
    let summarized_symbols = summarized_symbol_set(
        &function_symbols,
        &method_symbols,
        &class_symbols,
        &type_symbols,
    );
    let summarized_names = symbol_names(&summarized_symbols);
    let symbol_name_counts = store.symbol_name_counts(&summarized_names)?;
    let alias_scope_symbols = store.load_symbols_by_names(&summarized_names)?;
    let alias_counts = symbol_alias_counts(&alias_scope_symbols);
    let import_aliases = load_import_alias_map(store, &summarized_symbols, &alias_counts)?;
    let caller_targets = caller_target_names(&summarized_symbols, &import_aliases);
    let caller_relations =
        store.load_call_relations_to_targets(&caller_targets, CALLER_RELATION_LIMIT_PER_TARGET)?;
    let called_by = called_by_map(
        &summarized_symbols,
        &caller_relations,
        &symbol_name_counts,
        &alias_counts,
        &import_aliases,
    );
    let functions = summarize_symbols(&function_symbols, &called_by);
    let methods = summarize_symbols(&method_symbols, &called_by);
    let classes = summarize_symbols(&class_symbols, &called_by);
    let types = summarize_symbols(&type_symbols, &called_by);
    let imports = store.load_distinct_relation_targets_by_kind(
        &file_key,
        RelationKind::Imports,
        effective_limit,
    )?;
    let dependencies = store.load_distinct_relation_targets_by_kind(
        &file_key,
        RelationKind::DependsOn,
        effective_limit,
    )?;
    let exports = store.load_exported_symbol_names_for_path(&file_key, effective_limit)?;
    let calls = store
        .load_symbol_relations_by_kind(&file_key, RelationKind::Calls, effective_limit)?
        .iter()
        .map(FileCallSummary::from)
        .collect::<Vec<_>>();
    let total_functions = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Function])?;
    let total_methods = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Method])?;
    let total_classes = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Class])?;
    let total_types = store.count_symbols_by_kinds(&file_key, &type_kinds)?;
    let total_calls = store.count_symbol_relations_by_kind(&file_key, RelationKind::Calls)?;
    let total_imports =
        store.count_distinct_relation_targets_by_kind(&file_key, RelationKind::Imports)?;
    let total_dependencies =
        store.count_distinct_relation_targets_by_kind(&file_key, RelationKind::DependsOn)?;
    let total_exports = store.exported_symbol_count_for_path(&file_key)?;
    let symbol_count = store.symbol_count_for_path(&file_key)?;
    let symbol_parser_kinds = store.symbol_parser_kinds_for_path(&file_key)?;
    let truncated = [
        total_functions,
        total_methods,
        total_classes,
        total_types,
        total_calls,
        total_imports,
        total_dependencies,
        total_exports,
    ]
    .iter()
    .any(|total| *total > effective_limit);

    let observed_summary = indexed.summary.unwrap_or_default();
    let parser_kind =
        summary_parser_kind(&observed_summary, symbol_count, &symbol_parser_kinds).to_string();
    let summary_status =
        summary_status(&observed_summary, symbol_count, &symbol_parser_kinds).to_string();

    Ok(FileSummaryReport {
        file_path: file_key,
        language: indexed
            .node
            .language
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        line_count,
        purpose: indexed.purpose.purpose.clone().unwrap_or_default(),
        purpose_status: indexed.purpose.status.to_string(),
        purpose_source: indexed.purpose.source.to_string(),
        observed_summary,
        package: package_name(&metadata_symbols),
        docstring,
        symbol_count,
        source_status,
        source_error,
        parser_kind,
        summary_status,
        limit: effective_limit,
        total_functions,
        total_methods,
        total_classes,
        total_types,
        total_calls,
        total_imports,
        total_dependencies,
        total_exports,
        truncated,
        functions,
        methods,
        classes,
        types,
        imports,
        dependencies,
        exports,
        calls,
    })
}

/// Serialize the exact file summary payload for token telemetry.
///
/// # Errors
///
/// Returns an error when the summary payload cannot be serialized.
pub fn file_summary_baseline_text(report: &FileSummaryReport) -> ServiceResult<String> {
    Ok(serde_json::to_string(report)?)
}

/// Return the parser family implied by stored summary and parser metadata.
fn summary_parser_kind(
    summary: &str,
    symbol_count: usize,
    parser_kinds: &[ParserKind],
) -> &'static str {
    if symbol_count > 0 {
        return symbol_parser_kind(parser_kinds);
    }
    if is_symbol_graph_empty_summary(summary) {
        "symbol-graph"
    } else if summary.is_empty() {
        "missing"
    } else if is_scanner_fallback_summary(summary) {
        "scanner-metadata"
    } else {
        "structural"
    }
}

/// Return a summary quality status for agent consumers.
fn summary_status(summary: &str, symbol_count: usize, parser_kinds: &[ParserKind]) -> &'static str {
    if summary.is_empty() {
        "missing"
    } else if is_scanner_fallback_summary(summary)
        || fallback_only_symbols(symbol_count, parser_kinds)
    {
        "fallback"
    } else {
        "ok"
    }
}

/// Return the parser family for a non-empty symbol graph.
fn symbol_parser_kind(parser_kinds: &[ParserKind]) -> &'static str {
    let has_tree_sitter = parser_kinds.contains(&ParserKind::TreeSitter);
    let has_manifest = parser_kinds.contains(&ParserKind::Manifest);
    let has_structural = parser_kinds.contains(&ParserKind::Structural);
    let has_fallback = parser_kinds.contains(&ParserKind::Fallback);
    let family_count = usize::from(has_tree_sitter)
        .saturating_add(usize::from(has_manifest))
        .saturating_add(usize::from(has_structural))
        .saturating_add(usize::from(has_fallback));
    match (
        family_count,
        has_tree_sitter,
        has_manifest,
        has_structural,
        has_fallback,
    ) {
        (1, true, false, false, false) => "tree-sitter-symbol-graph",
        (1, false, true, false, false) => "manifest-symbol-graph",
        (1, false, false, true, false) => "structural-symbol-graph",
        (1, false, false, false, true) => "fallback-symbol-graph",
        _ => "mixed-symbol-graph",
    }
}

/// Return whether the only available symbol graph was created by fallback parsing.
fn fallback_only_symbols(symbol_count: usize, parser_kinds: &[ParserKind]) -> bool {
    symbol_count > 0
        && !parser_kinds.is_empty()
        && parser_kinds
            .iter()
            .all(|parser_kind| *parser_kind == ParserKind::Fallback)
}

/// Return whether a no-declaration source summary came from the symbol graph.
fn is_symbol_graph_empty_summary(summary: &str) -> bool {
    summary
        .trim()
        .ends_with("source file with no declarations found.")
}

/// Return whether a summary is only the filesystem byte-count fallback.
fn is_scanner_fallback_summary(summary: &str) -> bool {
    let trimmed = summary.trim_end_matches('.');
    let Some((_, tail)) = trimmed.rsplit_once(", ") else {
        return false;
    };
    let Some(number) = tail.strip_suffix(" bytes") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

/// Search indexed project files with bounded source reads and `globset` filters.
///
/// # Errors
///
/// Returns an error when the index is unavailable, the regex or glob is
/// invalid, or an indexed file cannot be read.
pub fn search_indexed_files(
    store: &AtlasStore,
    pattern: &str,
    regex: bool,
    fuzzy: bool,
    case_sensitive: bool,
    file_pattern: Option<&str>,
    context_lines: usize,
    start_index: usize,
    limit: usize,
) -> ServiceResult<SearchReport> {
    if regex && fuzzy {
        return Err(ServiceError::InvalidInput(
            "search cannot combine regex and fuzzy modes".to_string(),
        ));
    }
    let path_matcher = build_path_matcher(file_pattern)?;
    let matcher = if regex {
        LineMatcher::Regex(
            RegexBuilder::new(pattern)
                .case_insensitive(!case_sensitive)
                .build()
                .map_err(|source| ServiceError::InvalidInput(source.to_string()))?,
        )
    } else if fuzzy {
        LineMatcher::Fuzzy {
            needle: normalized_search_text(pattern, case_sensitive),
            case_sensitive,
        }
    } else {
        LineMatcher::Literal {
            needle: normalized_search_text(pattern, case_sensitive),
            case_sensitive,
        }
    };
    let mut report = SearchReport {
        query: pattern.to_string(),
        mode: matcher.mode().to_string(),
        source: "sqlite-file-text".to_string(),
        start_index,
        total: 0,
        observed_total: 0,
        total_is_complete: true,
        returned: 0,
        searched_files: 0,
        searched_bytes: 0,
        truncated: false,
        results: Vec::new(),
    };
    if limit == 0 {
        return Ok(report);
    }
    let needed = start_index.saturating_add(limit);
    store.visit_file_texts_for_search(matcher.literal_prefilter(), case_sensitive, |text| {
        if !path_matches(&text.path, path_matcher.as_ref()) {
            return Ok(true);
        }
        report.searched_files += 1;
        report.searched_bytes = report.searched_bytes.saturating_add(text.byte_count);
        let lines = indexed_text_lines(&text);
        append_line_matches(
            &mut report,
            &text.path,
            &lines,
            &matcher,
            context_lines,
            needed,
        );
        if report.results.len() >= limit {
            report.truncated = true;
            return Ok(false);
        }
        Ok(true)
    })?;
    report.returned = report.results.len();
    report.observed_total = report.total;
    report.total_is_complete = !report.truncated;
    Ok(report)
}

/// Filter file nodes through a repository-relative glob.
///
/// # Errors
///
/// Returns an error when `file_pattern` is not a valid repository glob.
pub fn filter_files_by_glob(
    nodes: Vec<IndexedNode>,
    file_pattern: Option<&str>,
) -> ServiceResult<Vec<IndexedNode>> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    Ok(nodes
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter(|node| matcher.is_match(&node.node.path))
        .collect())
}

/// Load ranked file nodes and apply the shared repository-relative glob policy.
///
/// # Errors
///
/// Returns an error when the file pattern is invalid or indexed nodes cannot be
/// loaded.
pub fn load_ranked_file_nodes(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
) -> ServiceResult<Vec<IndexedNode>> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    if !matcher.filters() {
        return Ok(store.load_ranked_nodes(query, NodeKind::File, folder, limit.max(1), 0)?);
    }
    let target = limit.max(1);
    let batch_size = target.saturating_mul(20).clamp(50, 500);
    let mut offset = 0usize;
    let mut selected = Vec::new();
    loop {
        let batch = store.load_ranked_nodes(query, NodeKind::File, folder, batch_size, offset)?;
        if batch.is_empty() {
            break;
        }
        offset = offset.saturating_add(batch.len());
        for node in batch {
            if matcher.is_match(&node.node.path) {
                selected.push(node);
                if selected.len() >= target {
                    return Ok(selected);
                }
            }
        }
    }
    Ok(selected)
}

/// Return whether one repository-relative path matches an optional file glob.
///
/// # Errors
///
/// Returns an error when `file_pattern` is not a valid repository glob.
pub fn file_path_matches_glob(path: &str, file_pattern: Option<&str>) -> ServiceResult<bool> {
    Ok(FilePathMatcher::new(file_pattern)?.is_match(path))
}

/// Reusable repository-relative file path matcher.
pub struct FilePathMatcher {
    /// Compiled optional glob matcher.
    matcher: Option<GlobSet>,
}

impl FilePathMatcher {
    /// Compile a repository-relative glob matcher once for many path checks.
    ///
    /// # Errors
    ///
    /// Returns an error when `file_pattern` is not a valid repository glob.
    pub fn new(file_pattern: Option<&str>) -> ServiceResult<Self> {
        Ok(Self {
            matcher: build_path_matcher(file_pattern)?,
        })
    }

    /// Return whether this matcher has an active filtering glob.
    #[must_use]
    pub fn filters(&self) -> bool {
        self.matcher.is_some()
    }

    /// Return whether `path` matches the compiled repository-relative glob.
    #[must_use]
    pub fn is_match(&self, path: &str) -> bool {
        path_matches(path, self.matcher.as_ref())
    }
}

/// Borrow indexed text content as line slices for context extraction.
fn indexed_text_lines(text: &IndexedFileText) -> Vec<&str> {
    text.content.lines().collect()
}

/// Read an exact line slice from an indexed project file.
///
/// # Errors
///
/// Returns an error when the file is not an indexed project file, line numbers
/// are invalid, or source cannot be read.
pub fn read_indexed_code_slice(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
) -> ServiceResult<CodeSlice> {
    let file_key = validated_indexed_file_key(store, file)?;
    let native_file = indexed_native_path(store, &file_key)?;
    read_code_slice(&native_file, &file_key, start_line, end_line)
}

/// Read a symbol body by exact symbol name and optional disambiguators.
///
/// # Errors
///
/// Returns an error when the symbol is absent, ambiguous, filtered out by the
/// selector, or source cannot be read.
pub fn read_symbol_slice(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
) -> ServiceResult<CodeSlice> {
    let file_key = validated_indexed_file_key(store, file)?;
    let requested_kind = selector.kind.map(parse_symbol_kind).transpose()?;
    let mut symbols = store.load_symbols_by_exact_file_and_name(&file_key, selector.name)?;
    if let Some(parent) = selector.parent {
        symbols.retain(|symbol| symbol.parent.as_deref() == Some(parent));
    }
    if let Some(kind) = requested_kind {
        symbols.retain(|symbol| symbol.kind == kind);
    }
    if let Some(line) = selector.line {
        symbols.retain(|symbol| symbol.line_start <= line && line <= symbol.line_end);
    }
    let symbol = match symbols.as_slice() {
        [symbol] => symbol,
        [] => {
            return Err(ServiceError::InvalidInput(format!(
                "symbol {:?} was not found in indexed file {file_key}",
                selector.name
            )));
        }
        _ => {
            return Err(ServiceError::InvalidInput(format!(
                "symbol {:?} is ambiguous in {file_key}; pass symbol_parent, symbol_kind, or symbol_line. candidates: {}",
                selector.name,
                describe_symbol_candidates(&symbols)
            )));
        }
    };
    let native_file = indexed_native_path(store, &file_key)?;
    read_code_slice(
        &native_file,
        &file_key,
        symbol.line_start,
        Some(symbol.line_end),
    )
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
fn validated_file_key(file: &Path) -> ServiceResult<String> {
    validated_repo_file_key(file).map_err(|source| ServiceError::InvalidInput(source.to_string()))
}

/// Validate that a path belongs to the indexed project file set.
fn validated_indexed_file_key(store: &AtlasStore, file: &Path) -> ServiceResult<String> {
    let file_key = validated_file_key(file)?;
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| ServiceError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    if indexed.node.kind != NodeKind::File {
        return Err(ServiceError::InvalidInput(format!(
            "path {file_key:?} is not an indexed file"
        )));
    }
    Ok(file_key)
}

/// Load the project root recorded by the latest scan.
fn indexed_project_root(store: &AtlasStore) -> ServiceResult<PathBuf> {
    store.project_root()?.map(PathBuf::from).ok_or_else(|| {
        ServiceError::InvalidInput(
            "indexed project root is missing; run projectatlas scan <project-root> first"
                .to_string(),
        )
    })
}

/// Build an absolute native path for a previously validated indexed file key.
fn indexed_native_path(store: &AtlasStore, file_key: &str) -> ServiceResult<PathBuf> {
    Ok(indexed_project_root(store)?.join(repo_path_to_native(file_key)))
}

/// Read source text for a selected file.
fn read_file_content(file: &Path) -> ServiceResult<String> {
    fs::read_to_string(file).map_err(|source| ServiceError::Io {
        path: file.to_path_buf(),
        source,
    })
}

/// Build a path matcher from an optional repository glob.
fn build_path_matcher(pattern: Option<&str>) -> ServiceResult<Option<GlobSet>> {
    let Some(pattern) = pattern else {
        return Ok(None);
    };
    let normalized = pattern.trim().replace('\\', "/");
    if normalized.is_empty() || normalized == "*" {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    add_glob(&mut builder, &normalized)?;
    if !normalized.contains('/') {
        add_glob(&mut builder, &format!("**/{normalized}"))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|source| ServiceError::InvalidInput(source.to_string()))
}

/// Add one normalized glob to a builder.
fn add_glob(builder: &mut GlobSetBuilder, pattern: &str) -> ServiceResult<()> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|source| ServiceError::InvalidInput(source.to_string()))?;
    builder.add(glob);
    Ok(())
}

/// Return whether a repository path matches an optional compiled glob.
fn path_matches(path: &str, matcher: Option<&GlobSet>) -> bool {
    matcher.is_none_or(|matcher| matcher.is_match(path))
}

/// Line-level search mode.
enum LineMatcher {
    /// Regex-backed line matching.
    Regex(regex::Regex),
    /// Literal substring matching.
    Literal {
        /// Normalized literal needle.
        needle: String,
        /// Whether matching is case-sensitive.
        case_sensitive: bool,
    },
    /// Fuzzy subsequence matching.
    Fuzzy {
        /// Normalized fuzzy needle.
        needle: String,
        /// Whether matching is case-sensitive.
        case_sensitive: bool,
    },
}

impl LineMatcher {
    /// Return the serialized search mode name.
    fn mode(&self) -> &'static str {
        match self {
            Self::Regex(_) => "regex",
            Self::Literal { .. } => "literal",
            Self::Fuzzy { .. } => "fuzzy",
        }
    }

    /// Return a literal substring prefilter when SQL can safely narrow files.
    fn literal_prefilter(&self) -> Option<&str> {
        match self {
            Self::Literal { needle, .. } => Some(needle.as_str()),
            Self::Regex(_) | Self::Fuzzy { .. } => None,
        }
    }

    /// Return whether this matcher accepts one source line.
    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(line),
            Self::Literal {
                needle,
                case_sensitive,
            } => normalized_search_text(line, *case_sensitive).contains(needle),
            Self::Fuzzy {
                needle,
                case_sensitive,
            } => fuzzy_subsequence_matches(needle, &normalized_search_text(line, *case_sensitive)),
        }
    }
}

/// Append bounded line matches from one source file.
fn append_line_matches(
    report: &mut SearchReport,
    path: &str,
    lines: &[&str],
    matcher: &LineMatcher,
    context_lines: usize,
    needed: usize,
) {
    for (index, line) in lines.iter().enumerate() {
        if !matcher.is_match(line) {
            continue;
        }
        report.total += 1;
        if report.total <= report.start_index {
            continue;
        }
        if report.results.len() >= needed.saturating_sub(report.start_index) {
            report.truncated = true;
            return;
        }
        report.results.push(SearchMatch {
            path: path.to_string(),
            line: index + 1,
            context_before: context_before(lines, index, context_lines),
            text: (*line).to_string(),
            context_after: context_after(lines, index, context_lines),
        });
    }
}

/// Normalize search text for case-sensitive or insensitive matching.
fn normalized_search_text(text: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        text.to_string()
    } else {
        text.to_ascii_lowercase()
    }
}

/// Return whether every needle character appears in candidate order.
fn fuzzy_subsequence_matches(needle: &str, candidate: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut needle = needle.chars();
    let Some(mut expected) = needle.next() else {
        return true;
    };
    for character in candidate.chars() {
        if character == expected {
            let Some(next) = needle.next() else {
                return true;
            };
            expected = next;
        }
    }
    false
}

/// Return context lines before a match.
fn context_before(lines: &[&str], index: usize, context_lines: usize) -> Vec<String> {
    let start = index.saturating_sub(context_lines);
    lines[start..index]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// Return context lines after a match.
fn context_after(lines: &[&str], index: usize, context_lines: usize) -> Vec<String> {
    let start = index.saturating_add(1);
    let end = lines.len().min(start.saturating_add(context_lines));
    lines[start..end]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// Read an exact line slice from a previously validated file.
fn read_code_slice(
    native_file: &Path,
    file_key: &str,
    start_line: usize,
    end_line: Option<usize>,
) -> ServiceResult<CodeSlice> {
    if start_line == 0 {
        return Err(ServiceError::InvalidInput(
            "start-line must be one or greater".to_string(),
        ));
    }
    let content = read_file_content(native_file)?;
    let lines = content.lines().collect::<Vec<_>>();
    let line_count = lines.len();
    let end_line = end_line.unwrap_or(start_line);
    if end_line < start_line {
        return Err(ServiceError::InvalidInput(
            "end-line must be greater than or equal to start-line".to_string(),
        ));
    }
    if start_line > line_count {
        return Err(ServiceError::InvalidInput(format!(
            "start-line {start_line} exceeds file line count {line_count}"
        )));
    }
    let end_index = end_line.min(line_count);
    let content = lines[start_line - 1..end_index].join("\n");
    Ok(CodeSlice {
        path: file_key.to_string(),
        start_line,
        end_line: end_index,
        line_count,
        estimated_tokens: estimate_tokens(&content),
        content,
    })
}

/// Parse a user-facing symbol kind selector.
fn parse_symbol_kind(kind: &str) -> ServiceResult<SymbolKind> {
    let normalized = kind.trim().to_ascii_lowercase();
    let parsed = SymbolKind::from_db(&normalized);
    if parsed == SymbolKind::Unknown && normalized != "unknown" {
        return Err(ServiceError::InvalidInput(format!(
            "unsupported symbol kind {kind:?}"
        )));
    }
    Ok(parsed)
}

/// Describe symbol candidates for ambiguity errors.
fn describe_symbol_candidates(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "{} parent={} kind={} lines={}-{}",
                symbol.name,
                symbol.parent.as_deref().unwrap_or(""),
                symbol.kind,
                symbol.line_start,
                symbol.line_end
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Count source lines in loaded content.
fn line_count_from_content(content: &str) -> usize {
    content.lines().count()
}

/// Return distinct symbol names for caller lookup.
fn symbol_names(symbols: &[CodeSymbol]) -> Vec<String> {
    let mut names = symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Return symbol kinds that can provide file-level metadata.
fn metadata_symbol_kinds() -> [SymbolKind; 3] {
    [
        SymbolKind::Package,
        SymbolKind::Workspace,
        SymbolKind::Module,
    ]
}

/// Return symbol kinds grouped in the `types` summary section.
fn type_symbol_kinds() -> [SymbolKind; 7] {
    [
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Trait,
        SymbolKind::Interface,
        SymbolKind::Type,
        SymbolKind::Package,
        SymbolKind::Workspace,
    ]
}

/// Combine displayed symbol rows for caller lookup without changing section order.
fn summarized_symbol_set(
    functions: &[CodeSymbol],
    methods: &[CodeSymbol],
    classes: &[CodeSymbol],
    types: &[CodeSymbol],
) -> Vec<CodeSymbol> {
    functions
        .iter()
        .chain(methods)
        .chain(classes)
        .chain(types)
        .cloned()
        .collect()
}

/// Return exact call target names that can safely resolve to displayed symbols.
fn caller_target_names(symbols: &[CodeSymbol], import_aliases: &ImportAliasMap) -> Vec<String> {
    let mut targets = HashSet::new();
    for symbol in symbols {
        targets.insert(symbol.name.clone());
        for alias in symbol_target_aliases(symbol) {
            targets.insert(alias);
        }
    }
    for alias in import_aliases.values().flatten() {
        targets.insert(alias.target_name.clone());
    }
    let mut values = targets.into_iter().collect::<Vec<_>>();
    values.sort();
    values
}

/// Build reverse call lookup for displayed symbols across the indexed graph.
fn called_by_map(
    symbols: &[CodeSymbol],
    relations: &[SymbolRelation],
    name_counts: &HashMap<String, usize>,
    alias_counts: &HashMap<String, usize>,
    import_aliases: &ImportAliasMap,
) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for symbol in symbols {
        let symbol_key = symbol_summary_key(symbol);
        for relation in relations.iter().filter(|relation| {
            relation_matches_symbol(relation, symbol, name_counts, alias_counts, import_aliases)
        }) {
            let caller = caller_reference(relation);
            let callers = map.entry(symbol_key.clone()).or_default();
            if !callers.iter().any(|existing| existing == &caller) {
                callers.push(caller);
            }
        }
    }
    for callers in map.values_mut() {
        callers.sort();
        callers.truncate(CALLERS_PER_SYMBOL_LIMIT);
    }
    map
}

/// Return whether a relation can be deterministically attached to a symbol.
fn relation_matches_symbol(
    relation: &SymbolRelation,
    symbol: &CodeSymbol,
    name_counts: &HashMap<String, usize>,
    alias_counts: &HashMap<String, usize>,
    import_aliases: &ImportAliasMap,
) -> bool {
    if relation.kind != RelationKind::Calls {
        return false;
    }
    let target = relation.target_name.trim();
    if target == symbol.name
        && (relation.path == symbol.path
            || name_counts.get(&symbol.name).copied().unwrap_or(0) <= 1)
    {
        return true;
    }
    if symbol_target_aliases(symbol)
        .iter()
        .any(|alias| alias == target && alias_counts.get(alias).copied().unwrap_or(0) <= 1)
    {
        return true;
    }
    import_aliases
        .get(&symbol_summary_key(symbol))
        .is_some_and(|aliases| {
            aliases.iter().any(|alias| {
                alias.caller_path == relation.path && alias.target_name == relation.target_name
            })
        })
}

/// Count target aliases across displayed symbols.
fn symbol_alias_counts(symbols: &[CodeSymbol]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for alias in symbols.iter().flat_map(symbol_target_aliases) {
        *counts.entry(alias).or_insert(0) += 1;
    }
    counts
}

/// Return exact qualified target strings that identify a symbol by file path.
fn symbol_target_aliases(symbol: &CodeSymbol) -> Vec<String> {
    let mut aliases = HashSet::new();
    let modules = module_aliases_for_path(&symbol.path);
    for module in &modules {
        aliases.insert(format!("{module}::{}", symbol.name));
        aliases.insert(format!("{module}.{}", symbol.name));
        aliases.insert(format!("crate::{module}::{}", symbol.name));
        aliases.insert(format!("crate.{module}.{}", symbol.name));
    }
    if modules.is_empty() {
        aliases.insert(format!("crate::{}", symbol.name));
        aliases.insert(format!("self::{}", symbol.name));
    }
    let mut values = aliases.into_iter().collect::<Vec<_>>();
    values.sort();
    values
}

/// Return module aliases inferred from a normalized repository path.
fn module_aliases_for_path(path: &str) -> Vec<String> {
    let mut aliases = HashSet::new();
    for stem in source_stems_for_path(path) {
        let mut components = stem
            .split('/')
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();
        if components
            .first()
            .is_some_and(|component| *component == "src")
        {
            components.remove(0);
        }
        if components.last().is_some_and(|component| {
            matches!(*component, "lib" | "main" | "mod" | "index" | "__init__")
        }) {
            components.pop();
        }
        if components.is_empty() {
            continue;
        }
        aliases.insert(components.join("::"));
        aliases.insert(components.join("."));
        if let Some(last) = components.last() {
            aliases.insert((*last).to_string());
        }
    }
    let mut values = aliases.into_iter().collect::<Vec<_>>();
    values.sort();
    values
}

/// Return source path stems, including package-entry aliases.
fn source_stems_for_path(path: &str) -> Vec<String> {
    let stem = strip_known_source_extension(path);
    let mut stems = vec![stem.clone()];
    if let Some((parent, entry_name)) = stem.rsplit_once('/')
        && matches!(entry_name, "index" | "__init__" | "mod")
    {
        stems.push(parent.to_string());
    }
    stems.sort();
    stems.dedup();
    stems
}

/// Strip common source extensions while preserving dotted directory names.
fn strip_known_source_extension(path: &str) -> String {
    for extension in [
        ".d.ts", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".rs",
    ] {
        if let Some(stem) = path.strip_suffix(extension) {
            return stem.to_string();
        }
    }
    path.rsplit_once('.')
        .map_or_else(|| path.to_string(), |(stem, _extension)| stem.to_string())
}

/// Build a stable identity key for a summarized symbol row.
fn symbol_summary_key(symbol: &CodeSymbol) -> String {
    format!("{}\0{}\0{}", symbol.path, symbol.name, symbol.line_start)
}

/// Return a compact caller reference.
fn caller_reference(relation: &SymbolRelation) -> String {
    format!("{}::{}", relation.path, relation.source_name)
}

/// Summarize already-selected symbols.
fn summarize_symbols(
    symbols: &[CodeSymbol],
    called_by: &HashMap<String, Vec<String>>,
) -> Vec<FileSymbolSummary> {
    let mut rows = symbols
        .iter()
        .map(|symbol| FileSymbolSummary {
            name: symbol.name.clone(),
            kind: symbol.kind.to_string(),
            line: symbol.line_start,
            end_line: symbol.line_end,
            signature: symbol.signature.clone(),
            exported: symbol.exported,
            documentation: symbol.documentation.clone().unwrap_or_default(),
            parent: symbol.parent.clone().unwrap_or_default(),
            called_by: called_by
                .get(&symbol_summary_key(symbol))
                .cloned()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.name.cmp(&right.name))
    });
    rows
}

/// Return a best-effort package or module name from indexed symbols.
fn package_name(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .find(|symbol| matches!(symbol.kind, SymbolKind::Package | SymbolKind::Workspace))
        .or_else(|| {
            symbols.iter().find(|symbol| {
                symbol.kind == SymbolKind::Module
                    && matches!(
                        symbol.detail.as_deref(),
                        Some(
                            "package_declaration"
                                | "package_clause"
                                | "package_header"
                                | "namespace_declaration"
                                | "file_scoped_namespace_declaration"
                                | "module_declaration"
                        )
                    )
            })
        })
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

/// Return file-level documentation from the best indexed symbol source.
fn file_docstring(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .find(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Package | SymbolKind::Workspace | SymbolKind::Module
            ) && symbol
                .documentation
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        })
        .and_then(|symbol| symbol.documentation.clone())
        .unwrap_or_default()
}

/// Extract file-level documentation from source text.
fn file_level_docstring(content: &str) -> Option<String> {
    leading_string_docstring(content).or_else(|| leading_doc_comments(content))
}

/// Extract a Python-style file docstring at the beginning of a file.
fn leading_string_docstring(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    for quote in ["\"\"\"", "'''"] {
        if let Some(rest) = trimmed.strip_prefix(quote)
            && let Some(end) = rest.find(quote)
        {
            return compact_doc_text(&rest[..end]);
        }
    }
    None
}

/// Extract leading file-level doc comments.
fn leading_doc_comments(content: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_block = false;
    let mut module_style: Option<bool> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && lines.is_empty() {
            continue;
        }
        if in_block {
            if let Some(end) = trimmed.find("*/") {
                lines.push(trimmed[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(trimmed.trim_start_matches('*').trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("//!") {
            if module_style.is_some_and(|module| !module) {
                break;
            }
            module_style = Some(true);
            lines.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("///") {
            if module_style.is_some_and(|module| module) {
                break;
            }
            module_style = Some(false);
            lines.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("/*!") {
            if module_style.is_some_and(|module| !module) {
                break;
            }
            module_style = Some(true);
            in_block = true;
            if let Some(end) = value.find("*/") {
                lines.push(value[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(value.trim_start_matches('*').trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("/**") {
            if module_style.is_some_and(|module| module) {
                break;
            }
            module_style = Some(false);
            in_block = true;
            if let Some(end) = value.find("*/") {
                lines.push(value[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(value.trim_start_matches('*').trim().to_string());
        } else {
            break;
        }
    }
    compact_doc_text(&lines.join(" "))
}

/// Normalize documentation text to one compact line.
fn compact_doc_text(raw: &str) -> Option<String> {
    let text = raw
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() { None } else { Some(text) }
}

/// Return sorted exported symbol names.
#[cfg(test)]
fn exported_symbol_names(symbols: &[CodeSymbol]) -> Vec<String> {
    let mut names = symbols
        .iter()
        .filter(|symbol| symbol.exported)
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::symbols::{ParserKind, SymbolGraph};
    use projectatlas_core::{Node, Purpose, PurposeSource, PurposeStatus, normalized_parent};
    use std::error::Error;
    use std::io;

    #[test]
    fn metadata_helpers_are_stable() -> Result<(), Box<dyn Error>> {
        let mut package = test_symbol("Cargo.toml", SymbolKind::Package, "projectatlas");
        package.documentation = Some("ProjectAtlas package manifest.".to_string());
        let mut alpha = test_symbol("src/lib.rs", SymbolKind::Function, "alpha");
        alpha.exported = true;
        alpha.documentation = Some("Alpha entry point.".to_string());
        let mut beta = test_symbol("src/lib.rs", SymbolKind::Function, "beta");
        beta.exported = true;
        let private = test_symbol("src/lib.rs", SymbolKind::Function, "private");
        let symbols = vec![beta, package, private, alpha];

        require_eq(
            &package_name(&symbols),
            &"projectatlas".to_string(),
            "package name",
        )?;
        require_eq(
            &file_docstring(&symbols),
            &"ProjectAtlas package manifest.".to_string(),
            "file docstring",
        )?;
        require_eq(
            &exported_symbol_names(&symbols),
            &vec!["alpha".to_string(), "beta".to_string()],
            "exported symbols",
        )?;
        require_eq(
            &file_level_docstring("//! Module level docs.\nfn main() {}"),
            &Some("Module level docs.".to_string()),
            "rust module docs",
        )?;
        require_eq(
            &file_level_docstring("\"\"\"Python module docs.\"\"\"\nclass Atlas: pass"),
            &Some("Python module docs.".to_string()),
            "python module docs",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_marks_fallback_symbol_graph_as_fallback() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("component.vue"),
            "<script setup></script>",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/component.vue", "hash-vue")])?;
        store.set_purpose(
            "src/component.vue",
            "Provide Vue component behavior",
            PurposeSource::Agent,
        )?;
        store.set_node_summary("src/component.vue", "vue component with bindings selected.")?;
        let mut fallback_symbol = test_symbol("src/component.vue", SymbolKind::Value, "selected");
        fallback_symbol.parser = ParserKind::Fallback;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/component.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Fallback,
            symbols: vec![fallback_symbol],
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("src/component.vue"), 10)?;
        require_eq(
            &report.parser_kind,
            &"fallback-symbol-graph".to_string(),
            "fallback parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"fallback".to_string(),
            "fallback summary status",
        )
    }

    #[test]
    fn file_summary_marks_structural_symbol_graph_as_ok() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("component.vue"),
            "<script setup>const selected = ref(false)</script>",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/component.vue", "hash-vue")])?;
        store.set_node_summary("src/component.vue", "vue component with bindings selected.")?;
        let mut structural_symbol = test_symbol("src/component.vue", SymbolKind::Value, "selected");
        structural_symbol.parser = ParserKind::Structural;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/component.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![structural_symbol],
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("src/component.vue"), 10)?;
        require_eq(
            &report.parser_kind,
            &"structural-symbol-graph".to_string(),
            "structural parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"ok".to_string(),
            "structural summary status",
        )
    }

    #[test]
    fn module_aliases_include_package_entries_and_compound_extensions() -> Result<(), Box<dyn Error>>
    {
        require_eq(
            &module_aliases_for_path("src/packages/foo/index.ts"),
            &vec![
                "foo".to_string(),
                "packages.foo".to_string(),
                "packages::foo".to_string(),
            ],
            "typescript package entry aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/types/api.d.ts"),
            &vec![
                "api".to_string(),
                "types.api".to_string(),
                "types::api".to_string(),
            ],
            "typescript definition aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/package/__init__.py"),
            &vec!["package".to_string()],
            "python package entry aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/lib.rs"),
            &Vec::<String>::new(),
            "rust root lib aliases",
        )
    }

    #[test]
    fn file_summary_includes_cross_file_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("lib.rs"),
            "/// Shared helper.\npub fn helper() {}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/lib.rs", "hash-lib"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.set_purpose(
            "src/lib.rs",
            "Provide shared library behavior",
            PurposeSource::Agent,
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![{
                let mut symbol = test_symbol("src/lib.rs", SymbolKind::Function, "helper");
                symbol.exported = true;
                symbol.line_start = 2;
                symbol.line_end = 2;
                symbol
            }],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "crate::helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        let report = build_file_summary(&store, Path::new("src/lib.rs"), 10)?;
        require_eq(
            &report.purpose_status,
            &PurposeStatus::Approved.to_string(),
            "purpose status",
        )?;
        let helper = report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("helper summary missing"))?;
        require_eq(
            &helper.called_by,
            &vec!["src/main.rs::main".to_string()],
            "cross-file called-by",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_rejects_ambiguous_called_by_matches() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(root.join("src").join("a.rs"), "pub fn helper() {}\n")?;
        fs::write(root.join("src").join("b.rs"), "pub fn helper() {}\n")?;
        fs::write(
            root.join("src").join("main.rs"),
            "mod a;\nmod b;\nfn main() { b::helper(); }\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/a.rs", SymbolKind::Function, "helper")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/b.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/b.rs", SymbolKind::Function, "helper")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "b::helper".to_string(),
                kind: RelationKind::Calls,
                line: 3,
                context: "b::helper();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        let a_report = build_file_summary(&store, Path::new("src/a.rs"), 10)?;
        let a_helper = a_report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("a::helper summary missing"))?;
        require_eq(&a_helper.called_by, &Vec::<String>::new(), "a called-by")?;

        let b_report = build_file_summary(&store, Path::new("src/b.rs"), 10)?;
        let b_helper = b_report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("b::helper summary missing"))?;
        require_eq(
            &b_helper.called_by,
            &vec!["src/main.rs::main".to_string()],
            "b called-by",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_rejects_ambiguous_module_alias_called_by_matches() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src").join("foo"))?;
        fs::create_dir_all(root.join("src").join("bar"))?;
        fs::write(root.join("src/foo/service.rs"), "pub fn run() {}\n")?;
        fs::write(root.join("src/bar/service.rs"), "pub fn run() {}\n")?;
        fs::write(root.join("src/main.rs"), "fn main() { service::run(); }\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.rs", "hash-foo"),
            test_node("src/bar/service.rs", "hash-bar"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/foo/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/foo/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/bar/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/bar/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "service::run".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "service::run();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        for path in ["src/foo/service.rs", "src/bar/service.rs"] {
            let report = build_file_summary(&store, Path::new(path), 10)?;
            let run = report
                .functions
                .iter()
                .find(|symbol| symbol.name == "run")
                .ok_or_else(|| io::Error::other("run summary missing"))?;
            require_eq(
                &run.called_by,
                &Vec::<String>::new(),
                "ambiguous module alias called-by",
            )?;
        }
        Ok(())
    }

    #[test]
    fn file_summary_resolves_rust_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/foo"))?;
        fs::write(root.join("src/foo/service.rs"), "pub fn run() {}\n")?;
        fs::write(
            root.join("src/main.rs"),
            "use crate::foo::service as foo_service;\nfn main() { foo_service::run(); }\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.rs", "hash-service"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/foo/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/foo/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "use crate::foo::service as foo_service;".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "use crate::foo::service as foo_service;".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "main".to_string(),
                    target_name: "foo_service::run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "foo_service::run();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/foo/service.rs"), 10)?,
            "run",
            "src/main.rs::main",
        )
    }

    #[test]
    fn file_summary_resolves_typescript_named_import_alias_called_by() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/service.ts"), "export function run() {}\n")?;
        fs::write(
            root.join("src/main.ts"),
            "import { run as serviceRun } from \"./service\";\nserviceRun();\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/service.ts", "hash-service"),
            test_node("src/main.ts", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/service.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/service.ts", SymbolKind::Function, "run")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.ts", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import { run as serviceRun } from \"./service\";".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import { run as serviceRun } from \"./service\";".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "main".to_string(),
                    target_name: "serviceRun".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "serviceRun();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/service.ts"), 10)?,
            "run",
            "src/main.ts::main",
        )
    }

    #[test]
    fn file_summary_resolves_python_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/package"))?;
        fs::write(root.join("src/package/module.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "import package.module as service\nservice.run()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/package/module.py", "hash-module"),
            test_node("src/main.py", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/package/module.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/package/module.py",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.py", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import package.module as service".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import package.module as service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "service.run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "service.run()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/package/module.py"), 10)?,
            "run",
            "src/main.py::main",
        )
    }

    #[test]
    fn file_summary_resolves_python_no_alias_import_when_name_is_ambiguous()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/package"))?;
        fs::write(root.join("src/package/module.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "from package.module import run\nrun()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/package/module.py", "hash-module"),
            test_node("src/main.py", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/package/module.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/package/module.py",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("src/main.py", SymbolKind::Function, "main"),
                test_symbol("src/main.py", SymbolKind::Import, "run"),
            ],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from package.module import run".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "from package.module import run".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "run()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/package/module.py"), 10)?,
            "run",
            "src/main.py::main",
        )
    }

    #[test]
    fn file_summary_rejects_ambiguous_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/foo"))?;
        fs::create_dir_all(root.join("src/bar"))?;
        fs::write(root.join("src/foo/service.py"), "def run():\n    pass\n")?;
        fs::write(root.join("src/bar/service.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "from foo.service import run as call_service\nfrom bar.service import run as call_service\ncall_service()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.py", "hash-foo"),
            test_node("src/bar/service.py", "hash-bar"),
            test_node("src/main.py", "hash-main"),
        ])?;
        for path in ["src/foo/service.py", "src/bar/service.py"] {
            store.replace_symbol_graph(&SymbolGraph {
                path: path.to_string(),
                language: Some("python".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![test_symbol(path, SymbolKind::Function, "run")],
                relations: Vec::new(),
            })?;
        }
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.py", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from foo.service import run as call_service".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "from foo.service import run as call_service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from bar.service import run as call_service".to_string(),
                    kind: RelationKind::Imports,
                    line: 2,
                    context: "from bar.service import run as call_service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "call_service".to_string(),
                    kind: RelationKind::Calls,
                    line: 3,
                    context: "call_service()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        for path in ["src/foo/service.py", "src/bar/service.py"] {
            let report = build_file_summary(&store, Path::new(path), 10)?;
            let run = report
                .functions
                .iter()
                .find(|symbol| symbol.name == "run")
                .ok_or_else(|| io::Error::other("run summary missing"))?;
            require_eq(
                &run.called_by,
                &Vec::<String>::new(),
                "ambiguous import alias called-by",
            )?;
        }
        Ok(())
    }

    #[test]
    fn file_summary_marks_indexed_metadata_fallback() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/missing.rs", "hash-missing")])?;

        let report = build_file_summary(&store, Path::new("src/missing.rs"), 10)?;
        require_eq(
            &report.source_status,
            &SOURCE_STATUS_INDEXED.to_string(),
            "source status",
        )?;
        if report.source_error.is_empty() {
            return Err(io::Error::other("source fallback error was empty").into());
        }
        Ok(())
    }

    #[test]
    fn search_uses_globset_and_stops_after_requested_page() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join("src").join("a.rs"), "needle one\n")?;
        fs::write(root.join("src").join("b.rs"), "needle two\n")?;
        fs::write(root.join("docs").join("readme.md"), "needle docs\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("docs/readme.md", "hash-docs"),
        ])?;
        index_test_file_texts(
            &mut store,
            root,
            &[
                test_node("src/a.rs", "hash-a"),
                test_node("src/b.rs", "hash-b"),
                test_node("docs/readme.md", "hash-docs"),
            ],
        )?;

        let report =
            search_indexed_files(&store, "needle", false, false, false, Some("*.rs"), 0, 0, 1)?;
        require_eq(&report.returned, &1, "returned rows")?;
        require_eq(&report.searched_files, &1, "bounded searched files")?;
        require_eq(&report.truncated, &true, "truncated flag")?;
        require_eq(&report.observed_total, &report.total, "observed total")?;
        require_eq(
            &report.total_is_complete,
            &false,
            "truncated search completeness",
        )?;

        let report = search_indexed_files(
            &store,
            "needle",
            false,
            false,
            false,
            Some("src\\*.rs"),
            0,
            0,
            10,
        )?;
        require_eq(&report.returned, &2, "windows glob returned rows")?;
        require_eq(&report.total_is_complete, &true, "complete search total")?;
        if report
            .results
            .iter()
            .any(|row| row.path == "docs/readme.md")
        {
            return Err(io::Error::other("globset filter included docs/readme.md").into());
        }
        Ok(())
    }

    #[test]
    fn file_glob_filter_matches_repository_paths() -> Result<(), Box<dyn Error>> {
        let nodes = vec![
            test_indexed_node("src/a.rs", "hash-a"),
            test_indexed_node("src/nested/b.rs", "hash-b"),
            test_indexed_node("docs/readme.md", "hash-docs"),
        ];

        let filtered = filter_files_by_glob(nodes.clone(), Some("*.rs"))?;
        require_eq(&filtered.len(), &2, "rs glob count")?;
        let matcher = FilePathMatcher::new(Some("*.rs"))?;
        require_eq(&matcher.filters(), &true, "compiled glob filters")?;
        require_eq(&matcher.is_match("src/a.rs"), &true, "compiled nested rs")?;
        require_eq(&matcher.is_match("a.rs"), &true, "compiled basename rs")?;
        require_eq(
            &matcher.is_match("docs/readme.md"),
            &false,
            "compiled markdown miss",
        )?;

        let nested = filter_files_by_glob(nodes, Some("src\\nested\\*.rs"))?;
        require_eq(&nested.len(), &1, "windows glob count")?;
        require_eq(
            &nested[0].node.path,
            &"src/nested/b.rs".to_string(),
            "windows glob path",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_file_nodes_uses_shared_glob_policy() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/nested/b.rs", "hash-b"),
            test_node("docs/readme.md", "hash-docs"),
        ])?;
        for path in ["src/a.rs", "src/nested/b.rs", "docs/readme.md"] {
            store.set_purpose(path, "needle orientation target", PurposeSource::Agent)?;
            store.set_node_summary(path, "needle indexed summary")?;
        }

        let selected = load_ranked_file_nodes(&store, "needle", None, Some("*.rs"), 10)?;
        require_eq(&selected.len(), &2, "ranked rs glob count")?;
        if selected
            .iter()
            .any(|node| node.node.path == "docs/readme.md")
        {
            return Err(io::Error::other("ranked glob included docs/readme.md").into());
        }

        let nested = load_ranked_file_nodes(&store, "needle", None, Some("src/nested/*.rs"), 10)?;
        require_eq(&nested.len(), &1, "ranked nested glob count")?;
        require_eq(
            &nested[0].node.path,
            &"src/nested/b.rs".to_string(),
            "ranked nested glob path",
        )?;
        Ok(())
    }

    #[test]
    fn fuzzy_search_matches_approximate_line_terms() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src").join("main.rs"),
            "fn build_project_atlas() {}\nfn unrelated() {}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        let nodes = [test_node("src/main.rs", "hash-main")];
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;

        let report =
            search_indexed_files(&store, "bpa", false, true, false, Some("*.rs"), 0, 0, 10)?;
        require_eq(&report.mode, &"fuzzy".to_string(), "search mode")?;
        require_eq(&report.returned, &1, "fuzzy returned rows")?;
        require_eq(
            &report.results[0].text,
            &"fn build_project_atlas() {}".to_string(),
            "fuzzy match text",
        )?;

        let invalid = search_indexed_files(&store, "bpa", true, true, false, None, 0, 0, 10);
        if invalid.is_ok() {
            return Err(io::Error::other("regex+fuzzy search was accepted").into());
        }
        Ok(())
    }

    #[test]
    fn symbol_slice_reports_ambiguity_and_accepts_parent_selector() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src").join("lib.rs"),
            "struct A;\nimpl A {\n    fn run(&self) {\n        a();\n    }\n}\nstruct B;\nimpl B {\n    fn run(&self) {\n        b();\n    }\n}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/lib.rs", "hash-lib")])?;
        let mut a_run = test_symbol("src/lib.rs", SymbolKind::Method, "run");
        a_run.parent = Some("A".to_string());
        a_run.line_start = 3;
        a_run.line_end = 5;
        let mut b_run = test_symbol("src/lib.rs", SymbolKind::Method, "run");
        b_run.parent = Some("B".to_string());
        b_run.line_start = 9;
        b_run.line_end = 11;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![a_run, b_run],
            relations: Vec::new(),
        })?;

        let ambiguous = read_symbol_slice(
            &store,
            Path::new("src/lib.rs"),
            &SymbolSliceSelector {
                name: "run",
                ..SymbolSliceSelector::default()
            },
        );
        if !matches!(ambiguous, Err(ServiceError::InvalidInput(message)) if message.contains("ambiguous") && message.contains("parent=A") && message.contains("parent=B"))
        {
            return Err(
                io::Error::other("ambiguous symbol slice did not report candidates").into(),
            );
        }

        let slice = read_symbol_slice(
            &store,
            Path::new("src/lib.rs"),
            &SymbolSliceSelector {
                name: "run",
                parent: Some("B"),
                ..SymbolSliceSelector::default()
            },
        )?;
        if !slice.content.contains("b();") || slice.content.contains("a();") {
            return Err(io::Error::other("parent selector returned wrong symbol slice").into());
        }
        Ok(())
    }

    /// Build a representative file node.
    fn test_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent(path),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some(hash.to_string()),
        }
    }

    /// Build a representative indexed file node.
    fn test_indexed_node(path: &str, hash: &str) -> IndexedNode {
        IndexedNode {
            node: test_node(path, hash),
            purpose: Purpose {
                path: path.to_string(),
                purpose: Some(format!("Purpose for {path}")),
                source: PurposeSource::Agent,
                status: PurposeStatus::Approved,
            },
            summary: Some(format!("Summary for {path}")),
        }
    }

    /// Persist fixture text rows for search service tests.
    fn index_test_file_texts(
        store: &mut AtlasStore,
        root: &Path,
        nodes: &[Node],
    ) -> Result<(), Box<dyn Error>> {
        let mut paths = Vec::new();
        let mut texts = Vec::new();
        for node in nodes {
            paths.push(node.path.clone());
            let native = root.join(repo_path_to_native(&node.path));
            let content = fs::read_to_string(native)?;
            texts.push(IndexedFileText {
                path: node.path.clone(),
                content_hash: node.content_hash.clone(),
                byte_count: content.len(),
                line_count: content.lines().count(),
                content,
            });
        }
        store.replace_file_texts_for_paths(&paths, &texts)?;
        Ok(())
    }

    /// Build a compact test symbol.
    fn test_symbol(path: &str, kind: SymbolKind, name: &str) -> CodeSymbol {
        CodeSymbol {
            path: path.to_string(),
            language: Some("rust".to_string()),
            name: name.to_string(),
            kind,
            signature: name.to_string(),
            exported: false,
            documentation: None,
            line_start: 1,
            line_end: 1,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: None,
        }
    }

    fn assert_single_called_by(
        report: &FileSummaryReport,
        symbol_name: &str,
        caller: &str,
    ) -> Result<(), Box<dyn Error>> {
        let symbol = report
            .functions
            .iter()
            .find(|symbol| symbol.name == symbol_name)
            .ok_or_else(|| io::Error::other(format!("{symbol_name} summary missing")))?;
        require_eq(
            &symbol.called_by,
            &vec![caller.to_string()],
            "import alias called-by",
        )
    }

    /// Require two test values to be equal without panicking.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: std::fmt::Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }
}
