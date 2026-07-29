//! Python import and export semantics.

use super::{ImportReference, ImportSyntax, module_reference, named_reference, split_alias};
use projectatlas_core::symbols::SymbolGraph;

/// Parse accepted Python module and named import forms.
pub(super) fn parse_import(import_text: &str) -> Vec<ImportReference> {
    let import_text = import_text.trim();
    if let Some(rest) = import_text.strip_prefix("from ") {
        let Some((module, imports)) = rest.split_once(" import ") else {
            return Vec::new();
        };
        if !valid_module(module.trim()) || imports.contains(['(', ')', '{', '}', '[', ']']) {
            return Vec::new();
        }
        let imports = imports.trim().strip_suffix(',').unwrap_or(imports.trim());
        return imports
            .split(',')
            .map(|item| named_reference(ImportSyntax::Python, module.trim(), item, " as "))
            .collect::<Option<Vec<_>>>()
            .filter(|references| !references.is_empty())
            .unwrap_or_default();
    }
    let Some(rest) = import_text.strip_prefix("import ") else {
        return Vec::new();
    };
    let modules = rest.trim().strip_suffix(',').unwrap_or(rest.trim());
    modules
        .split(',')
        .map(|item| {
            let (module, alias) = split_alias(item.trim(), " as ")?;
            let module = module.trim();
            if !valid_module(module) || module.starts_with('.') {
                return None;
            }
            let local = alias.unwrap_or_else(|| module.split('.').next().unwrap_or(module));
            module_reference(ImportSyntax::Python, module, local)
        })
        .collect::<Option<Vec<_>>>()
        .filter(|references| !references.is_empty())
        .unwrap_or_default()
}

/// Return whether one top-level Python declaration is publicly referenceable.
pub(super) fn is_export_candidate(graph: &SymbolGraph, symbol_index: usize) -> bool {
    let symbol = &graph.symbols[symbol_index];
    symbol.parent.is_none() && !symbol.name.starts_with('_')
}

/// Resolve one absolute or caller-relative Python module without basename guessing.
pub(super) fn import_scopes(caller_path: &str, reference: &ImportReference) -> Vec<String> {
    resolve_module_path(caller_path, reference.module())
        .map(|scope| vec![scope])
        .unwrap_or_default()
}

/// Resolve Python dot-relative imports against the importing module's package path.
fn resolve_module_path(caller_path: &str, module: &str) -> Option<String> {
    if !valid_module(module) {
        return None;
    }
    let relative_level = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = &module[relative_level..];
    if relative_level == 0 {
        return Some(remainder.replace('.', "/"));
    }
    let (parent, _file) = caller_path.rsplit_once('/')?;
    let mut components = parent
        .split('/')
        .filter(|component| !component.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    for _ in 1..relative_level {
        components.pop()?;
    }
    if !remainder.is_empty() {
        components.extend(remainder.split('.').map(ToString::to_string));
    }
    (!components.is_empty()).then(|| components.join("/"))
}

/// Accept only simple dotted module names with an optional leading relative level.
fn valid_module(module: &str) -> bool {
    let module = module.trim();
    if module.is_empty() || module.chars().any(char::is_whitespace) {
        return false;
    }
    let remainder = module.trim_start_matches('.');
    if remainder.is_empty() {
        return module.starts_with('.');
    }
    remainder
        .split('.')
        .all(|component| !component.is_empty() && valid_identifier(component))
}

/// Accept conservative Python identifiers and abstain on punctuation.
fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}
