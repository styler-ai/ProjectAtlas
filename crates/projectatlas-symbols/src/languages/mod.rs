//! Language-specific symbol graph augmenters.

mod gradle;
mod kotlin;
mod objective_c;
mod zig;

use projectatlas_core::symbols::{RelationKind, SymbolGraph, SymbolKind};

use crate::language_parser_registry::SymbolAugmenter;
use crate::{push_relation, push_symbol};

/// Add language-specific facts that are not exposed by the generic node pass.
pub(crate) fn augment_language_graph(
    graph: &mut SymbolGraph,
    content: &str,
    augmenters: &[SymbolAugmenter],
) {
    for augmenter in augmenters {
        match augmenter {
            SymbolAugmenter::Kotlin => kotlin::augment(graph, content),
            SymbolAugmenter::GradleKotlin => gradle::augment_kotlin(graph, content),
            SymbolAugmenter::ObjectiveC => objective_c::augment(graph, content),
            SymbolAugmenter::Zig => zig::augment(graph, content),
            SymbolAugmenter::GradleGroovy => gradle::augment_groovy(graph, content),
        }
    }
}

/// Add language-specific fallback facts for languages without a native parser.
pub(crate) fn augment_fallback_language_graph(
    graph: &mut SymbolGraph,
    content: &str,
    augmenters: &[SymbolAugmenter],
) {
    for augmenter in augmenters {
        match augmenter {
            SymbolAugmenter::GradleKotlin => gradle::augment_kotlin(graph, content),
            SymbolAugmenter::GradleGroovy => gradle::augment_groovy(graph, content),
            SymbolAugmenter::Kotlin | SymbolAugmenter::ObjectiveC | SymbolAugmenter::Zig => {}
        }
    }
}

/// Return whether a graph already has a symbol with this exact kind and name.
fn symbol_exists(graph: &SymbolGraph, kind: SymbolKind, name: &str) -> bool {
    graph
        .symbols
        .iter()
        .any(|symbol| symbol.kind == kind && symbol.name == name)
}

/// Attach a parent to a method-like symbol already emitted on the same line.
fn set_method_parent(graph: &mut SymbolGraph, line: usize, parent: &str) {
    if let Some(symbol) = graph.symbols.iter_mut().find(|symbol| {
        matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            && symbol.line_start == line
    }) {
        symbol.kind = SymbolKind::Method;
        symbol.parent = Some(parent.to_string());
    }
}

/// Convert or insert a method symbol for a known parent type.
fn upsert_method_parent(graph: &mut SymbolGraph, line: usize, name: &str, parent: &str) {
    if let Some(symbol) = graph.symbols.iter_mut().find(|symbol| {
        matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            && symbol.line_start == line
            && symbol.name == name
    }) {
        symbol.kind = SymbolKind::Method;
        symbol.parent = Some(parent.to_string());
        return;
    }
    if !symbol_exists(graph, SymbolKind::Method, name) {
        push_symbol(
            graph,
            name,
            SymbolKind::Method,
            line,
            line,
            Some(parent.to_string()),
            Some("language-augment-method"),
            name,
        );
    }
}

/// Add a contains relation between a parent symbol and a child symbol.
fn push_contains_relation(
    graph: &mut SymbolGraph,
    parent: &str,
    child: &str,
    line: usize,
    detail: &str,
) {
    push_relation(graph, parent, child, RelationKind::Contains, line, detail);
}
