//! Objective-C-specific symbol graph augmentation.

use projectatlas_core::symbols::{CodeSymbol, SymbolGraph, SymbolKind};
use std::collections::HashMap;

use super::{push_contains_relation, set_method_parent, symbol_exists};
use crate::push_symbol;

/// Add Objective-C class ownership around interface/implementation blocks.
pub(super) fn augment(graph: &mut SymbolGraph, content: &str) {
    augment_blocks(graph, content);
    normalize_duplicates(graph);
}

/// Add symbols and ownership relations from Objective-C block syntax.
fn augment_blocks(graph: &mut SymbolGraph, content: &str) {
    let mut current_class: Option<String> = None;
    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(class_name) = block_name(trimmed, "@interface") {
            if !symbol_exists(graph, SymbolKind::Class, &class_name) {
                push_symbol(
                    graph,
                    &class_name,
                    SymbolKind::Class,
                    line_index + 1,
                    line_index + 1,
                    None,
                    Some("objective-c-interface"),
                    trimmed,
                );
            }
            current_class = Some(class_name);
            continue;
        }
        if let Some(class_name) = block_name(trimmed, "@implementation") {
            if !symbol_exists(graph, SymbolKind::Class, &class_name) {
                push_symbol(
                    graph,
                    &class_name,
                    SymbolKind::Class,
                    line_index + 1,
                    line_index + 1,
                    None,
                    Some("objective-c-implementation"),
                    trimmed,
                );
            }
            current_class = Some(class_name);
            continue;
        }
        if trimmed == "@end" {
            current_class = None;
            continue;
        }
        if let Some(class_name) = current_class.as_deref()
            && let Some(method_name) = method_name(trimmed)
        {
            set_method_parent(graph, line_index + 1, class_name);
            push_contains_relation(
                graph,
                class_name,
                &method_name,
                line_index + 1,
                "objective-c-method",
            );
        }
    }
}

/// Collapse Objective-C interface and implementation duplicates in summaries.
fn normalize_duplicates(graph: &mut SymbolGraph) {
    let mut normalized = Vec::with_capacity(graph.symbols.len());
    let mut class_indices = HashMap::new();
    let mut method_indices = HashMap::new();
    for symbol in graph.symbols.drain(..) {
        match symbol.kind {
            SymbolKind::Class => upsert_class(&mut normalized, &mut class_indices, symbol),
            SymbolKind::Method => upsert_method(&mut normalized, &mut method_indices, symbol),
            _ => normalized.push(symbol),
        }
    }
    graph.symbols = normalized;
}

/// Insert a class symbol while preferring implementation entries.
fn upsert_class(
    symbols: &mut Vec<CodeSymbol>,
    indices: &mut HashMap<(String, String), usize>,
    symbol: CodeSymbol,
) {
    let key = (symbol.path.clone(), symbol.name.clone());
    let Some(existing_index) = indices.get(&key).copied() else {
        indices.insert(key, symbols.len());
        symbols.push(symbol);
        return;
    };
    if class_is_implementation(&symbol) && !class_is_implementation(&symbols[existing_index]) {
        symbols[existing_index] = symbol;
    }
}

/// Insert a method symbol while preferring implementation bodies.
fn upsert_method(
    symbols: &mut Vec<CodeSymbol>,
    indices: &mut HashMap<(String, String, String), usize>,
    symbol: CodeSymbol,
) {
    let key = (
        symbol.path.clone(),
        symbol.parent.clone().unwrap_or_default(),
        symbol.name.clone(),
    );
    let Some(existing_index) = indices.get(&key).copied() else {
        indices.insert(key, symbols.len());
        symbols.push(symbol);
        return;
    };
    if method_has_body(&symbol) && !method_has_body(&symbols[existing_index]) {
        symbols[existing_index] = symbol;
    }
}

/// Return whether an Objective-C class symbol points at implementation syntax.
fn class_is_implementation(symbol: &CodeSymbol) -> bool {
    symbol.detail.as_deref() == Some("objective-c-implementation")
        || symbol.signature.trim_start().starts_with("@implementation")
}

/// Return whether an Objective-C method symbol points at an implementation body.
fn method_has_body(symbol: &CodeSymbol) -> bool {
    symbol.signature.contains('{') || symbol.line_end > symbol.line_start
}

/// Return the class name from an Objective-C block declaration line.
fn block_name(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?.trim();
    rest.split([' ', ':', '('])
        .next()
        .map(ToString::to_string)
        .filter(|name| !name.is_empty())
}

/// Return the method selector head from an Objective-C method declaration line.
fn method_name(line: &str) -> Option<String> {
    if !(line.starts_with("- (") || line.starts_with("+ (")) {
        return None;
    }
    let after_return = line.split_once(')')?.1.trim();
    after_return
        .split([':', ' ', '{', ';'])
        .next()
        .map(ToString::to_string)
        .filter(|name| !name.is_empty())
}
