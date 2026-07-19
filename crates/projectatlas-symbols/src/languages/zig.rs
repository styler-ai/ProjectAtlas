//! Zig-specific symbol graph augmentation.

use projectatlas_core::symbols::{SymbolGraph, SymbolKind};
use regex::Regex;

use super::symbol_exists;
use crate::{check_parser_iteration, push_symbol};

/// Add Zig binding names around anonymous struct declarations.
pub(super) fn augment<E>(
    graph: &mut SymbolGraph,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    check()?;
    let Ok(struct_binding_regex) =
        Regex::new(r"\b(?:pub\s+)?const\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*struct\b")
    else {
        return Ok(());
    };
    for (line_index, line) in content.lines().enumerate() {
        check_parser_iteration(line_index, check)?;
        let line_number = line_index + 1;
        let trimmed = line.trim();
        let Some(capture) = struct_binding_regex.captures(trimmed) else {
            continue;
        };
        let Some(type_name) = capture.get(1).map(|value| value.as_str()) else {
            continue;
        };
        rename_struct_symbol_on_line(graph, line_number, type_name, check)?;
        mark_methods_on_line(graph, line_number, type_name, check)?;
    }
    check()?;
    Ok(())
}

/// Rename an anonymous Zig struct symbol to the binding that owns it.
fn rename_struct_symbol_on_line<E>(
    graph: &mut SymbolGraph,
    line: usize,
    name: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for (iteration, symbol) in graph.symbols.iter_mut().enumerate() {
        check_parser_iteration(iteration, check)?;
        if symbol.kind == SymbolKind::Struct && symbol.line_start == line {
            symbol.name = name.to_string();
            symbol.detail = Some("zig-struct-binding".to_string());
            return Ok(());
        }
    }
    if !symbol_exists(graph, SymbolKind::Struct, name) {
        push_symbol(
            graph,
            name,
            SymbolKind::Struct,
            line,
            line,
            None,
            Some("zig-struct-binding"),
            name,
        );
    }
    Ok(())
}

/// Mark Zig functions inside a struct binding as methods of that binding.
fn mark_methods_on_line<E>(
    graph: &mut SymbolGraph,
    line: usize,
    parent: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for (iteration, symbol) in graph.symbols.iter_mut().enumerate() {
        check_parser_iteration(iteration, check)?;
        if symbol.kind == SymbolKind::Function
            && symbol.line_start == line
            && symbol.signature.contains("self")
        {
            symbol.kind = SymbolKind::Method;
            symbol.parent = Some(parent.to_string());
        }
    }
    Ok(())
}
