//! Purpose: Define `ProjectAtlas` symbol graph domain types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU32;
use thiserror::Error;

/// Kind of symbol stored in the `ProjectAtlas` graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    /// A free function or language-level function declaration.
    Function,
    /// A method declaration associated with a type or class.
    Method,
    /// A class declaration.
    Class,
    /// A Rust-style struct or record declaration.
    Struct,
    /// An enum declaration.
    Enum,
    /// A trait declaration.
    Trait,
    /// An interface declaration.
    Interface,
    /// A module, namespace, package, or source unit.
    Module,
    /// A type alias or type declaration.
    Type,
    /// A constant, static, field, or variable declaration worth indexing.
    Value,
    /// An import, use, include, using, or package dependency edge source.
    Import,
    /// A package manifest entry such as a Cargo package.
    Package,
    /// A workspace manifest entry such as a Cargo workspace.
    Workspace,
    /// A dependency declared in a manifest.
    Dependency,
    /// A symbol that did not map cleanly to a richer kind.
    Unknown,
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl SymbolKind {
    /// Return the stable persisted spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Type => "type",
            Self::Value => "value",
            Self::Import => "import",
            Self::Package => "package",
            Self::Workspace => "workspace",
            Self::Dependency => "dependency",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a persisted symbol kind.
    #[must_use]
    pub fn from_db(value: &str) -> Self {
        match value {
            "function" => Self::Function,
            "method" => Self::Method,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "interface" => Self::Interface,
            "module" => Self::Module,
            "type" => Self::Type,
            "value" => Self::Value,
            "import" => Self::Import,
            "package" => Self::Package,
            "workspace" => Self::Workspace,
            "dependency" => Self::Dependency,
            _ => Self::Unknown,
        }
    }
}

/// Kind of graph relation stored for symbols.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    /// One symbol contains another symbol.
    Contains,
    /// A source imports or includes another module.
    Imports,
    /// A source symbol calls a target symbol or expression.
    Calls,
    /// A package or manifest depends on another package.
    DependsOn,
}

impl fmt::Display for RelationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl RelationKind {
    /// Return the stable persisted spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Imports => "imports",
            Self::Calls => "calls",
            Self::DependsOn => "depends-on",
        }
    }

    /// Parse a persisted relation kind.
    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "contains" => Some(Self::Contains),
            "imports" => Some(Self::Imports),
            "calls" => Some(Self::Calls),
            "depends-on" => Some(Self::DependsOn),
            _ => None,
        }
    }
}

/// Parser strategy used to produce a graph entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParserKind {
    /// A tree-sitter grammar produced the result.
    TreeSitter,
    /// A manifest parser produced the result.
    Manifest,
    /// A deterministic structural adapter produced the result.
    Structural,
    /// A conservative regex fallback produced the result.
    Fallback,
}

impl fmt::Display for ParserKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ParserKind {
    /// Return the stable persisted spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TreeSitter => "tree-sitter",
            Self::Manifest => "manifest",
            Self::Structural => "structural",
            Self::Fallback => "fallback",
        }
    }

    /// Parse a persisted parser kind.
    #[must_use]
    pub fn from_db(value: &str) -> Self {
        match value {
            "tree-sitter" => Self::TreeSitter,
            "manifest" => Self::Manifest,
            "structural" => Self::Structural,
            _ => Self::Fallback,
        }
    }
}

/// A code or manifest symbol indexed by `ProjectAtlas`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodeSymbol {
    /// Repository-relative file path.
    pub path: String,
    /// Detected language or file family.
    pub language: Option<String>,
    /// Symbol name.
    pub name: String,
    /// Symbol kind.
    pub kind: SymbolKind,
    /// Compact declaration signature or source row.
    pub signature: String,
    /// Whether the declaration is exported or publicly visible.
    pub exported: bool,
    /// Extracted doc comment or docstring associated with the symbol.
    pub documentation: Option<String>,
    /// One-based start line.
    pub line_start: usize,
    /// One-based end line.
    pub line_end: usize,
    /// Optional containing symbol name.
    pub parent: Option<String>,
    /// Parser strategy that produced this symbol.
    pub parser: ParserKind,
    /// Optional detail, usually the original parser node kind.
    pub detail: Option<String>,
}

/// A directed relation between symbols or source-level references.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SymbolRelation {
    /// Repository-relative file path.
    pub path: String,
    /// Source symbol name or module sentinel.
    pub source_name: String,
    /// Target symbol, import path, or dependency name.
    pub target_name: String,
    /// Relation kind.
    pub kind: RelationKind,
    /// One-based line where the relation appears.
    pub line: usize,
    /// Compact source context for the relation.
    pub context: String,
    /// Parser strategy that produced this relation.
    pub parser: ParserKind,
}

/// Symbol graph extracted from one file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SymbolGraph {
    /// Repository-relative file path.
    pub path: String,
    /// Detected language or file family.
    pub language: Option<String>,
    /// Primary parser strategy used for the file.
    pub parser: ParserKind,
    /// Extracted declaration and manifest symbols.
    pub symbols: Vec<CodeSymbol>,
    /// Extracted import, dependency, containment, and call relations.
    pub relations: Vec<SymbolRelation>,
}

/// Failure to represent an expanded symbol graph in its compact worker form.
#[derive(Debug, Error)]
pub enum CompactSymbolGraphError {
    /// The graph contained more distinct strings than a 32-bit internal ID can address.
    #[error("compact symbol graph exceeds the 32-bit interned-text identity space")]
    TooManyInternedStrings,
    /// A source line cannot be represented by the compact 32-bit row layout.
    #[error("compact symbol graph {field} value {value} exceeds u32")]
    LineOutOfRange {
        /// Row field that exceeded the compact representation.
        field: &'static str,
        /// Original expanded value.
        value: usize,
    },
}

/// Compact worker-owned symbol graph with interned text and contiguous typed rows.
///
/// This is an internal physical layout shared across workspace crates. It does
/// not change the source- and serialization-compatible [`SymbolGraph`] model.
#[derive(Debug)]
pub struct CompactSymbolGraph {
    /// Interned UTF-8 values addressed by non-zero 32-bit row identities.
    texts: Vec<Box<str>>,
    /// Interned repository-relative graph path.
    path: CompactTextId,
    /// Interned detected language or file family.
    language: Option<CompactTextId>,
    /// Primary parser strategy for the file.
    parser: ParserKind,
    /// Contiguous declaration and manifest rows.
    symbols: Vec<CompactCodeSymbol>,
    /// Contiguous import, dependency, containment, and call rows.
    relations: Vec<CompactSymbolRelation>,
}

/// Borrowed symbol row resolved from a [`CompactSymbolGraph`].
#[derive(Clone, Copy, Debug)]
pub struct CompactCodeSymbolRef<'a> {
    /// Graph that owns the intern pool and row storage.
    graph: &'a CompactSymbolGraph,
    /// Borrowed contiguous symbol row.
    row: &'a CompactCodeSymbol,
}

impl<'a> CompactCodeSymbolRef<'a> {
    /// Return the repository-relative path.
    #[must_use]
    pub fn path(self) -> &'a str {
        self.graph.resolve(self.row.path)
    }

    /// Return the detected language or file family.
    #[must_use]
    pub fn language(self) -> Option<&'a str> {
        self.row.language.map(|id| self.graph.resolve(id))
    }

    /// Return the symbol name.
    #[must_use]
    pub fn name(self) -> &'a str {
        self.graph.resolve(self.row.name)
    }

    /// Return the typed symbol kind.
    #[must_use]
    pub const fn kind(self) -> SymbolKind {
        self.row.kind
    }

    /// Return the declaration signature or source row.
    #[must_use]
    pub fn signature(self) -> &'a str {
        self.graph.resolve(self.row.signature)
    }

    /// Return whether the declaration is exported.
    #[must_use]
    pub const fn exported(self) -> bool {
        self.row.exported
    }

    /// Return associated documentation.
    #[must_use]
    pub fn documentation(self) -> Option<&'a str> {
        self.row.documentation.map(|id| self.graph.resolve(id))
    }

    /// Return the one-based start line.
    #[must_use]
    pub const fn line_start(self) -> u32 {
        self.row.line_start
    }

    /// Return the one-based end line.
    #[must_use]
    pub const fn line_end(self) -> u32 {
        self.row.line_end
    }

    /// Return the containing symbol name.
    #[must_use]
    pub fn parent(self) -> Option<&'a str> {
        self.row.parent.map(|id| self.graph.resolve(id))
    }

    /// Return the parser strategy that produced this symbol.
    #[must_use]
    pub const fn parser(self) -> ParserKind {
        self.row.parser
    }

    /// Return parser-specific detail.
    #[must_use]
    pub fn detail(self) -> Option<&'a str> {
        self.row.detail.map(|id| self.graph.resolve(id))
    }
}

/// Borrowed relation row resolved from a [`CompactSymbolGraph`].
#[derive(Clone, Copy, Debug)]
pub struct CompactSymbolRelationRef<'a> {
    /// Graph that owns the intern pool and row storage.
    graph: &'a CompactSymbolGraph,
    /// Borrowed contiguous relation row.
    row: &'a CompactSymbolRelation,
}

impl<'a> CompactSymbolRelationRef<'a> {
    /// Return the repository-relative path.
    #[must_use]
    pub fn path(self) -> &'a str {
        self.graph.resolve(self.row.path)
    }

    /// Return the source symbol name or module sentinel.
    #[must_use]
    pub fn source_name(self) -> &'a str {
        self.graph.resolve(self.row.source_name)
    }

    /// Return the target symbol, import path, or dependency name.
    #[must_use]
    pub fn target_name(self) -> &'a str {
        self.graph.resolve(self.row.target_name)
    }

    /// Return the typed relation identity.
    #[must_use]
    pub const fn kind(self) -> RelationKind {
        self.row.kind
    }

    /// Return the one-based occurrence line.
    #[must_use]
    pub const fn line(self) -> u32 {
        self.row.line
    }

    /// Return compact source context for the relation.
    #[must_use]
    pub fn context(self) -> &'a str {
        self.graph.resolve(self.row.context)
    }

    /// Return the parser strategy that produced this relation.
    #[must_use]
    pub const fn parser(self) -> ParserKind {
        self.row.parser
    }
}

impl CompactSymbolGraph {
    /// Return the graph's repository-relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        self.resolve(self.path)
    }

    /// Return the graph's detected language or file family.
    #[must_use]
    pub fn language(&self) -> Option<&str> {
        self.language.map(|id| self.resolve(id))
    }

    /// Return the graph's primary parser strategy.
    #[must_use]
    pub const fn parser(&self) -> ParserKind {
        self.parser
    }

    /// Return the number of compact symbol rows.
    #[must_use]
    pub const fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Return the number of compact relation rows.
    #[must_use]
    pub const fn relation_count(&self) -> usize {
        self.relations.len()
    }

    /// Return the number of distinct interned strings and paths.
    #[must_use]
    pub const fn interned_text_count(&self) -> usize {
        self.texts.len()
    }

    /// Iterate contiguous symbol rows without expanding owned strings.
    pub fn symbols(&self) -> impl ExactSizeIterator<Item = CompactCodeSymbolRef<'_>> + '_ {
        self.symbols
            .iter()
            .map(|row| CompactCodeSymbolRef { graph: self, row })
    }

    /// Iterate contiguous relation rows without expanding owned strings.
    pub fn relations(&self) -> impl ExactSizeIterator<Item = CompactSymbolRelationRef<'_>> + '_ {
        self.relations
            .iter()
            .map(|row| CompactSymbolRelationRef { graph: self, row })
    }

    /// Expand the compatibility model at an explicit caller boundary.
    #[must_use]
    pub fn to_symbol_graph(&self) -> SymbolGraph {
        SymbolGraph {
            path: self.path().to_string(),
            language: self.language().map(ToString::to_string),
            parser: self.parser,
            symbols: self
                .symbols()
                .map(|symbol| CodeSymbol {
                    path: symbol.path().to_string(),
                    language: symbol.language().map(ToString::to_string),
                    name: symbol.name().to_string(),
                    kind: symbol.kind(),
                    signature: symbol.signature().to_string(),
                    exported: symbol.exported(),
                    documentation: symbol.documentation().map(ToString::to_string),
                    line_start: symbol.line_start() as usize,
                    line_end: symbol.line_end() as usize,
                    parent: symbol.parent().map(ToString::to_string),
                    parser: symbol.parser(),
                    detail: symbol.detail().map(ToString::to_string),
                })
                .collect(),
            relations: self
                .relations()
                .map(|relation| SymbolRelation {
                    path: relation.path().to_string(),
                    source_name: relation.source_name().to_string(),
                    target_name: relation.target_name().to_string(),
                    kind: relation.kind(),
                    line: relation.line() as usize,
                    context: relation.context().to_string(),
                    parser: relation.parser(),
                })
                .collect(),
        }
    }

    /// Resolve one validated compact text identity.
    fn resolve(&self, id: CompactTextId) -> &str {
        &self.texts[id.index()]
    }
}

impl TryFrom<SymbolGraph> for CompactSymbolGraph {
    type Error = CompactSymbolGraphError;

    fn try_from(graph: SymbolGraph) -> Result<Self, Self::Error> {
        let mut texts = CompactTextPoolBuilder::default();
        let path = texts.intern(graph.path)?;
        let language = texts.intern_optional(graph.language)?;
        let symbols = graph
            .symbols
            .into_iter()
            .map(|symbol| CompactCodeSymbol::new(symbol, &mut texts))
            .collect::<Result<Vec<_>, _>>()?;
        let relations = graph
            .relations
            .into_iter()
            .map(|relation| CompactSymbolRelation::new(relation, &mut texts))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            texts: texts.finish(),
            path,
            language,
            parser: graph.parser,
            symbols,
            relations,
        })
    }
}

/// Non-zero one-based identity into a compact graph's intern pool.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CompactTextId(NonZeroU32);

impl CompactTextId {
    /// Return the zero-based vector index encoded by this identity.
    fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// Allocation-free symbol row whose text fields address the owning intern pool.
#[derive(Debug)]
struct CompactCodeSymbol {
    /// Interned repository-relative path.
    path: CompactTextId,
    /// Interned detected language or file family.
    language: Option<CompactTextId>,
    /// Interned symbol name.
    name: CompactTextId,
    /// Interned declaration signature or source row.
    signature: CompactTextId,
    /// Interned documentation, when present.
    documentation: Option<CompactTextId>,
    /// Interned containing symbol name, when present.
    parent: Option<CompactTextId>,
    /// Interned parser-specific detail, when present.
    detail: Option<CompactTextId>,
    /// One-based start line bounded to the accepted source-file size.
    line_start: u32,
    /// One-based end line bounded to the accepted source-file size.
    line_end: u32,
    /// Typed symbol identity.
    kind: SymbolKind,
    /// Typed parser identity.
    parser: ParserKind,
    /// Whether the declaration is exported.
    exported: bool,
}

impl CompactCodeSymbol {
    /// Move one expanded symbol into interned compact storage.
    fn new(
        symbol: CodeSymbol,
        texts: &mut CompactTextPoolBuilder,
    ) -> Result<Self, CompactSymbolGraphError> {
        Ok(Self {
            path: texts.intern(symbol.path)?,
            language: texts.intern_optional(symbol.language)?,
            name: texts.intern(symbol.name)?,
            signature: texts.intern(symbol.signature)?,
            documentation: texts.intern_optional(symbol.documentation)?,
            parent: texts.intern_optional(symbol.parent)?,
            detail: texts.intern_optional(symbol.detail)?,
            line_start: compact_line("symbol.line_start", symbol.line_start)?,
            line_end: compact_line("symbol.line_end", symbol.line_end)?,
            kind: symbol.kind,
            parser: symbol.parser,
            exported: symbol.exported,
        })
    }
}

/// Allocation-free relation row whose text fields address the owning intern pool.
#[derive(Debug)]
struct CompactSymbolRelation {
    /// Interned repository-relative path.
    path: CompactTextId,
    /// Interned source symbol name or module sentinel.
    source_name: CompactTextId,
    /// Interned target symbol, import path, or dependency name.
    target_name: CompactTextId,
    /// Interned compact source context.
    context: CompactTextId,
    /// One-based occurrence line bounded to the accepted source-file size.
    line: u32,
    /// Typed relation identity.
    kind: RelationKind,
    /// Typed parser identity.
    parser: ParserKind,
}

impl CompactSymbolRelation {
    /// Move one expanded relation into interned compact storage.
    fn new(
        relation: SymbolRelation,
        texts: &mut CompactTextPoolBuilder,
    ) -> Result<Self, CompactSymbolGraphError> {
        Ok(Self {
            path: texts.intern(relation.path)?,
            source_name: texts.intern(relation.source_name)?,
            target_name: texts.intern(relation.target_name)?,
            context: texts.intern(relation.context)?,
            line: compact_line("relation.line", relation.line)?,
            kind: relation.kind,
            parser: relation.parser,
        })
    }
}

/// Build-only text interner that moves its keys into the final indexed pool.
#[derive(Default)]
struct CompactTextPoolBuilder {
    /// Unique owned text mapped to its insertion-ordered identity.
    ids: HashMap<Box<str>, CompactTextId>,
}

impl CompactTextPoolBuilder {
    /// Move or deduplicate one required UTF-8 value.
    fn intern(&mut self, value: String) -> Result<CompactTextId, CompactSymbolGraphError> {
        if let Some(id) = self.ids.get(value.as_str()) {
            return Ok(*id);
        }
        let next = self
            .ids
            .len()
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(NonZeroU32::new)
            .ok_or(CompactSymbolGraphError::TooManyInternedStrings)?;
        let id = CompactTextId(next);
        self.ids.insert(value.into_boxed_str(), id);
        Ok(id)
    }

    /// Move or deduplicate one optional UTF-8 value.
    fn intern_optional(
        &mut self,
        value: Option<String>,
    ) -> Result<Option<CompactTextId>, CompactSymbolGraphError> {
        value.map(|value| self.intern(value)).transpose()
    }

    /// Move owned map keys into an identity-ordered resolution vector.
    fn finish(self) -> Vec<Box<str>> {
        let mut values = self
            .ids
            .into_iter()
            .map(|(text, id)| (id.index(), text))
            .collect::<Vec<_>>();
        values.sort_unstable_by_key(|(index, _)| *index);
        values.into_iter().map(|(_, text)| text).collect()
    }
}

/// Narrow one accepted source line into the compact physical representation.
fn compact_line(field: &'static str, value: usize) -> Result<u32, CompactSymbolGraphError> {
    u32::try_from(value).map_err(|_source| CompactSymbolGraphError::LineOutOfRange { field, value })
}

/// File-level parser metadata persisted even when a graph has no symbols.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceParseMetadata {
    /// Repository-relative file path.
    pub path: String,
    /// Detected language or file family.
    pub language: Option<String>,
    /// Primary parser strategy used for the file.
    pub parser: ParserKind,
    /// Number of declaration or manifest symbols emitted for this file.
    pub symbol_count: usize,
    /// Number of relations emitted for this file.
    pub relation_count: usize,
}

impl SourceParseMetadata {
    /// Build persisted parser metadata from a graph.
    #[must_use]
    pub fn from_graph(graph: &SymbolGraph) -> Self {
        Self {
            path: graph.path.clone(),
            language: graph.language.clone(),
            parser: graph.parser,
            symbol_count: graph.symbols.len(),
            relation_count: graph.relations.len(),
        }
    }
}

#[cfg(test)]
mod compact_tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn task_arri_ut_arri_4_21_compacts_typed_symbol_graph_rows()
    -> Result<(), Box<dyn std::error::Error>> {
        let graph = repeated_graph(2_048);
        let expanded_bytes = expanded_retained_bytes(&graph);
        let compact = CompactSymbolGraph::try_from(graph.clone())?;
        let compact_bytes = compact_retained_bytes(&compact);

        require(
            compact.to_symbol_graph() == graph,
            "compact graph did not preserve the compatibility model",
        )?;
        require(
            size_of::<CompactTextId>() == size_of::<u32>(),
            "compact text identity is wider than u32",
        )?;
        require(
            size_of::<Option<CompactTextId>>() == size_of::<u32>(),
            "optional compact text identity lost non-zero niche packing",
        )?;
        require(
            size_of::<CompactCodeSymbol>() <= 40,
            "compact symbol row exceeded its selected bound",
        )?;
        require(
            size_of::<CompactSymbolRelation>() <= 24,
            "compact relation row exceeded its selected bound",
        )?;
        require(
            compact.interned_text_count() < 80,
            "repeated text was not interned",
        )?;
        require(
            compact_bytes * 2 < expanded_bytes,
            "representative compact storage was not at least 50% smaller",
        )?;
        require(
            compact
                .relations()
                .all(|relation| relation.kind() == RelationKind::Calls),
            "typed relation identity changed during compaction",
        )?;
        if let Ok(out_of_range) = usize::try_from(u64::from(u32::MAX) + 1) {
            let mut invalid = repeated_graph(1);
            invalid.symbols[0].line_end = out_of_range;
            require(
                matches!(
                    CompactSymbolGraph::try_from(invalid),
                    Err(CompactSymbolGraphError::LineOutOfRange {
                        field: "symbol.line_end",
                        value,
                    }) if value == out_of_range
                ),
                "out-of-range source lines did not fail compact conversion",
            )?;
        }
        Ok(())
    }

    /// Return a test error instead of asserting inside a fallible test.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    fn repeated_graph(rows: usize) -> SymbolGraph {
        let path = "src/repeated/service.rs";
        let language = "rust";
        let symbols = (0..rows)
            .map(|index| CodeSymbol {
                path: path.to_string(),
                language: Some(language.to_string()),
                name: format!("handler_{}", index % 32),
                kind: SymbolKind::Function,
                signature: "fn(&Request) -> Response".to_string(),
                exported: true,
                documentation: Some("Handle one routed request.".to_string()),
                line_start: index + 1,
                line_end: index + 1,
                parent: Some("Service".to_string()),
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            })
            .collect();
        let relations = (0..rows)
            .map(|index| SymbolRelation {
                path: path.to_string(),
                source_name: format!("handler_{}", index % 32),
                target_name: "dispatch".to_string(),
                kind: RelationKind::Calls,
                line: index + 1,
                context: "dispatch(request)".to_string(),
                parser: ParserKind::TreeSitter,
            })
            .collect();
        SymbolGraph {
            path: path.to_string(),
            language: Some(language.to_string()),
            parser: ParserKind::TreeSitter,
            symbols,
            relations,
        }
    }

    fn expanded_retained_bytes(graph: &SymbolGraph) -> usize {
        let graph_text =
            graph.path.capacity() + graph.language.as_ref().map_or(0, String::capacity);
        let symbols = graph.symbols.capacity() * size_of::<CodeSymbol>()
            + graph
                .symbols
                .iter()
                .map(|symbol| {
                    symbol.path.capacity()
                        + symbol.language.as_ref().map_or(0, String::capacity)
                        + symbol.name.capacity()
                        + symbol.signature.capacity()
                        + symbol.documentation.as_ref().map_or(0, String::capacity)
                        + symbol.parent.as_ref().map_or(0, String::capacity)
                        + symbol.detail.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>();
        let relations = graph.relations.capacity() * size_of::<SymbolRelation>()
            + graph
                .relations
                .iter()
                .map(|relation| {
                    relation.path.capacity()
                        + relation.source_name.capacity()
                        + relation.target_name.capacity()
                        + relation.context.capacity()
                })
                .sum::<usize>();
        graph_text + symbols + relations
    }

    fn compact_retained_bytes(graph: &CompactSymbolGraph) -> usize {
        graph.texts.capacity() * size_of::<Box<str>>()
            + graph.texts.iter().map(|text| text.len()).sum::<usize>()
            + graph.symbols.capacity() * size_of::<CompactCodeSymbol>()
            + graph.relations.capacity() * size_of::<CompactSymbolRelation>()
    }
}
