//! ECMAScript import, export, and repository-relative module semantics.

use super::{
    ImportReference, ImportSyntax, is_compact_name, module_reference, named_reference, quoted_text,
};
use projectatlas_core::symbols::SymbolGraph;

/// Parse accepted named and namespace ECMAScript import forms.
pub(super) fn parse_import(import_text: &str) -> Vec<ImportReference> {
    let Some((bindings, from)) = import_text
        .trim()
        .strip_prefix("import ")
        .and_then(|rest| rest.split_once(" from "))
    else {
        return Vec::new();
    };
    let from = from.trim();
    let from = from.strip_suffix(';').unwrap_or(from).trim();
    let Some(module) = quoted_text(from) else {
        return Vec::new();
    };
    let bindings = bindings.trim();
    if let Some(rest) = bindings.strip_prefix('*') {
        let Some(alias) = rest.trim().strip_prefix("as ").map(str::trim) else {
            return Vec::new();
        };
        if !is_compact_name(alias) {
            return Vec::new();
        }
        return module_reference(ImportSyntax::EcmaScript, module, alias)
            .into_iter()
            .collect();
    }
    let Some(items) = bindings
        .strip_prefix('{')
        .and_then(|bindings| bindings.strip_suffix('}'))
    else {
        return Vec::new();
    };
    if items.contains(['{', '}', '(', ')', '[', ']']) {
        return Vec::new();
    }
    let items = items.trim().strip_suffix(',').unwrap_or(items.trim());
    let references = items
        .split(',')
        .map(|item| named_reference(ImportSyntax::EcmaScript, module, item, " as "))
        .collect::<Option<Vec<_>>>();
    references
        .filter(|references| !references.is_empty())
        .unwrap_or_default()
}

/// Return whether the parser marked one declaration as exported.
pub(super) fn is_export_candidate(graph: &SymbolGraph, symbol_index: usize) -> bool {
    graph.symbols[symbol_index].exported
}

/// Resolve and normalize scopes referenced by one ECMAScript import.
pub(super) fn import_scopes(caller_path: &str, reference: &ImportReference) -> Vec<String> {
    resolve_relative_import_path(caller_path, reference.module())
        .map(|scope| vec![scope])
        .unwrap_or_default()
}

/// Resolve one repository-relative ECMAScript module specifier.
pub(crate) fn resolve_relative_import_path(caller_path: &str, module_spec: &str) -> Option<String> {
    if !(module_spec.starts_with("./") || module_spec.starts_with("../")) {
        return None;
    }
    let mut components = caller_path
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _file)| {
            parent
                .split('/')
                .filter(|component| !component.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        });
    for component in module_spec.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                components.pop()?;
            }
            value => components.push(value.to_string()),
        }
    }
    Some(super::super::resolution_keys::strip_known_source_extension(
        &components.join("/"),
    ))
}
