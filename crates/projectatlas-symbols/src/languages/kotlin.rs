//! Kotlin-specific symbol graph augmentation.

use projectatlas_core::symbols::{SymbolGraph, SymbolKind};
use regex::Regex;

use super::{push_contains_relation, symbol_exists, upsert_method_parent};
use crate::push_symbol;

/// Add Kotlin package, type, and method facts missing from generic traversal.
pub(super) fn augment(graph: &mut SymbolGraph, content: &str) {
    let (Ok(package_regex), Ok(type_regex), Ok(function_regex)) = (
        Regex::new(r"^\s*package\s+([A-Za-z_][A-Za-z0-9_.]*)"),
        Regex::new(r"\b(?:class|interface|object)\s+([A-Za-z_][A-Za-z0-9_]*)"),
        Regex::new(r"\bfun\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("),
    ) else {
        return;
    };
    let mut current_type: Option<String> = None;
    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if let Some(capture) = package_regex.captures(trimmed)
            && let Some(package_name) = capture.get(1).map(|value| value.as_str())
            && !symbol_exists(graph, SymbolKind::Module, package_name)
        {
            push_symbol(
                graph,
                package_name,
                SymbolKind::Module,
                line_number,
                line_number,
                None,
                Some("package_header"),
                trimmed,
            );
        }
        if let Some(capture) = type_regex.captures(trimmed)
            && let Some(type_name) = capture.get(1).map(|value| value.as_str())
        {
            if !symbol_exists(graph, SymbolKind::Class, type_name) {
                push_symbol(
                    graph,
                    type_name,
                    SymbolKind::Class,
                    line_number,
                    line_number,
                    None,
                    Some("kotlin-type"),
                    trimmed,
                );
            }
            current_type = Some(type_name.to_string());
        }
        if let Some(capture) = function_regex.captures(trimmed)
            && let Some(function_name) = capture.get(1).map(|value| value.as_str())
            && let Some(parent) = current_type.as_deref()
        {
            upsert_method_parent(graph, line_number, function_name, parent);
            push_contains_relation(graph, parent, function_name, line_number, "kotlin-method");
        }
        if trimmed.contains('}') {
            current_type = None;
        }
    }
}
