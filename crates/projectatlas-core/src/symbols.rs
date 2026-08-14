//! Purpose: Define `ProjectAtlas` symbol graph domain types.

use serde::{Deserialize, Serialize};
use std::fmt;

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
    /// A documentation heading with an exact source selector.
    Heading,
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
        formatter.write_str(match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Heading => "heading",
            Self::Type => "type",
            Self::Value => "value",
            Self::Import => "import",
            Self::Package => "package",
            Self::Workspace => "workspace",
            Self::Dependency => "dependency",
            Self::Unknown => "unknown",
        })
    }
}

impl SymbolKind {
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
            "heading" => Self::Heading,
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
        formatter.write_str(match self {
            Self::Contains => "contains",
            Self::Imports => "imports",
            Self::Calls => "calls",
            Self::DependsOn => "depends-on",
        })
    }
}

impl RelationKind {
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
        formatter.write_str(match self {
            Self::TreeSitter => "tree-sitter",
            Self::Manifest => "manifest",
            Self::Structural => "structural",
            Self::Fallback => "fallback",
        })
    }
}

impl ParserKind {
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

/// Exact parser-supplied byte and column selector for one symbol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SymbolSourceSelector {
    /// Inclusive UTF-8 byte offset.
    pub byte_start: usize,
    /// Exclusive UTF-8 byte offset.
    pub byte_end: usize,
    /// Zero-based Unicode-scalar start column.
    pub column_start: usize,
    /// Exclusive zero-based Unicode-scalar end column.
    pub column_end: usize,
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
    /// Exact byte and column selector when the parser supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_selector: Option<SymbolSourceSelector>,
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
mod tests {
    use super::*;

    #[test]
    fn heading_symbol_kind_round_trips_without_changing_unknown_fallback() {
        assert_eq!(SymbolKind::Heading.to_string(), "heading");
        assert_eq!(SymbolKind::from_db("heading"), SymbolKind::Heading);
        assert!(matches!(
            serde_json::to_string(&SymbolKind::Heading).as_deref(),
            Ok("\"heading\"")
        ));
        assert!(matches!(
            serde_json::from_str::<SymbolKind>("\"heading\""),
            Ok(SymbolKind::Heading)
        ));
        assert_eq!(SymbolKind::from_db("future-kind"), SymbolKind::Unknown);
    }
}
