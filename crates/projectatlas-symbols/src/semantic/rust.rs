//! Rust import and export semantics.

use super::{ImportReference, ImportSyntax, module_reference, named_reference, split_alias};
use projectatlas_core::symbols::SymbolGraph;

/// Parse accepted Rust simple, named, and aliased `use` forms.
pub(super) fn parse_import(import_text: &str) -> Vec<ImportReference> {
    let Some(rest) = import_text.trim().strip_prefix("use ") else {
        return Vec::new();
    };
    let rest = rest.trim();
    let rest = rest.strip_suffix(';').unwrap_or(rest).trim();
    if rest.is_empty() || rest.ends_with(" as") {
        return Vec::new();
    }
    if let Some(open) = rest.find('{') {
        let Some(close) = rest.rfind('}').filter(|close| *close > open) else {
            return Vec::new();
        };
        if !rest[close + 1..].trim().is_empty() {
            return Vec::new();
        }
        let module = rest[..open].trim().trim_end_matches("::").trim();
        if module.is_empty() {
            return Vec::new();
        }
        let items = &rest[open + 1..close];
        if items.contains(['{', '}', '(', ')', '[', ']']) {
            return Vec::new();
        }
        let items = items.trim().strip_suffix(',').unwrap_or(items.trim());
        return items
            .split(',')
            .map(|item| named_reference(ImportSyntax::Rust, module, item, " as "))
            .collect::<Option<Vec<_>>>()
            .filter(|references| !references.is_empty())
            .unwrap_or_default();
    }
    let Some((path, alias)) = split_alias(rest, " as ") else {
        return Vec::new();
    };
    let path = path.trim();
    if path.contains(['{', '}', '(', ')', '[', ']', ',']) {
        return Vec::new();
    }
    let components = path.split("::").collect::<Vec<_>>();
    if components.iter().any(|component| component.is_empty()) {
        return Vec::new();
    }
    if components.len() <= 2 {
        return module_reference(ImportSyntax::Rust, path, alias.unwrap_or(path))
            .into_iter()
            .collect();
    }
    let Some((module, imported)) = path.rsplit_once("::") else {
        return Vec::new();
    };
    named_reference(
        ImportSyntax::Rust,
        module,
        &format!("{imported} as {}", alias.unwrap_or(imported)),
        " as ",
    )
    .into_iter()
    .collect()
}

/// Return whether the Rust parser marked one declaration as exported.
pub(super) fn is_export_candidate(graph: &SymbolGraph, symbol_index: usize) -> bool {
    graph.symbols[symbol_index].exported
}

/// Resolve one anchored Rust module path against the caller's repository module.
pub(super) fn import_scopes(caller_path: &str, reference: &ImportReference) -> Vec<String> {
    resolve_anchored_module_path(caller_path, reference.module())
        .map(|scope| vec![scope])
        .unwrap_or_default()
}

/// Resolve only explicit `crate`, `self`, and `super` paths without guessing bare crates.
fn resolve_anchored_module_path(caller_path: &str, module: &str) -> Option<String> {
    let components = caller_module_components(caller_path)?;
    let mut module_components = module.split("::");
    let anchor = module_components.next()?;
    let mut resolved = match anchor {
        "crate" => crate_source_root(&components),
        "self" => components,
        "super" => {
            let mut parent = components;
            parent.pop()?;
            while module_components.clone().next() == Some("super") {
                module_components.next();
                parent.pop()?;
            }
            parent
        }
        _ => return None,
    };
    for component in module_components {
        if component.is_empty() || matches!(component, "." | "..") {
            return None;
        }
        resolved.push(component.to_string());
    }
    (!resolved.is_empty()).then(|| resolved.join("/"))
}

/// Return the caller's canonical repository module components.
fn caller_module_components(caller_path: &str) -> Option<Vec<String>> {
    let (parent, file) = caller_path.rsplit_once('/').unwrap_or(("", caller_path));
    let stem = super::super::resolution_keys::strip_known_source_extension(file);
    if stem.is_empty() {
        return None;
    }
    let mut components = parent
        .split('/')
        .filter(|component| !component.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let is_source_root = components
        .last()
        .is_some_and(|component| component == "src");
    if stem != "mod" && !(is_source_root && matches!(stem.as_str(), "lib" | "main")) {
        components.push(stem);
    }
    Some(components)
}

/// Retain the repository prefix through the nearest canonical Cargo `src` root.
fn crate_source_root(components: &[String]) -> Vec<String> {
    components
        .iter()
        .rposition(|component| component == "src")
        .map_or_else(Vec::new, |index| components[..=index].to_vec())
}
