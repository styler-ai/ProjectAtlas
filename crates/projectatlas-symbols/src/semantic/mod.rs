//! Language-owned semantic extraction and canonical-reference normalization.

mod cargo;
pub(super) mod ecmascript;
pub(super) mod embedded_source;
mod python;
mod rust;

use crate::resolution_keys::{ImportReference, ImportSyntax};
use projectatlas_core::language::{EmbeddedHostKind, SemanticProviderOwner, language_capability};
use projectatlas_core::symbols::{ParserKind, SymbolGraph};

/// Select an independently supported direct or admitted embedded provider.
pub(super) fn provider_for_graph(graph: &SymbolGraph) -> Option<SemanticProviderOwner> {
    language_capability(graph.language.as_deref()?)?.effective_semantic_provider()
}

/// Return whether this graph may publish an ordinary source-module identity.
pub(super) fn emits_source_module_keys(graph: &SymbolGraph) -> bool {
    let Some(capability) = graph.language.as_deref().and_then(language_capability) else {
        return false;
    };
    match capability.semantic_provider {
        SemanticProviderOwner::Rust
        | SemanticProviderOwner::EcmaScript
        | SemanticProviderOwner::Python => return true,
        SemanticProviderOwner::Cargo => return false,
        SemanticProviderOwner::Unavailable => {}
    }
    let Some(embedded) = capability.embedded_language else {
        return false;
    };
    match embedded.host_kind {
        EmbeddedHostKind::HtmlLike => false,
        EmbeddedHostKind::Component | EmbeddedHostKind::Template => graph
            .symbols
            .iter()
            .any(|symbol| symbol.parser == ParserKind::TreeSitter && symbol.exported),
    }
}

/// Parse one provider-owned import statement.
pub(super) fn parse_imports(
    provider: SemanticProviderOwner,
    import_text: &str,
) -> Vec<ImportReference> {
    match provider {
        SemanticProviderOwner::Rust => rust::parse_import(import_text),
        SemanticProviderOwner::EcmaScript => ecmascript::parse_import(import_text),
        SemanticProviderOwner::Python => python::parse_import(import_text),
        SemanticProviderOwner::Cargo | SemanticProviderOwner::Unavailable => Vec::new(),
    }
}

/// Return whether a declaration participates in project-wide export resolution.
pub(super) fn is_export_candidate(
    provider: SemanticProviderOwner,
    graph: &SymbolGraph,
    symbol_index: usize,
) -> bool {
    match provider {
        SemanticProviderOwner::Rust => rust::is_export_candidate(graph, symbol_index),
        SemanticProviderOwner::EcmaScript => ecmascript::is_export_candidate(graph, symbol_index),
        SemanticProviderOwner::Python => python::is_export_candidate(graph, symbol_index),
        SemanticProviderOwner::Cargo => {
            cargo::is_export_candidate(graph.symbols[symbol_index].kind)
        }
        SemanticProviderOwner::Unavailable => false,
    }
}

/// Derive provider-owned canonical scopes for one normalized import.
pub(super) fn import_scopes(
    provider: SemanticProviderOwner,
    caller_path: &str,
    reference: &ImportReference,
) -> Vec<String> {
    match provider {
        SemanticProviderOwner::Rust => rust::import_scopes(caller_path, reference),
        SemanticProviderOwner::EcmaScript => ecmascript::import_scopes(caller_path, reference),
        SemanticProviderOwner::Python => python::import_scopes(caller_path, reference),
        SemanticProviderOwner::Cargo | SemanticProviderOwner::Unavailable => Vec::new(),
    }
}

/// Return whether ordinary source import and call dependencies belong to this provider.
pub(super) const fn supports_source_dependencies(provider: SemanticProviderOwner) -> bool {
    matches!(
        provider,
        SemanticProviderOwner::Rust
            | SemanticProviderOwner::EcmaScript
            | SemanticProviderOwner::Python
    )
}

/// Return whether manifest package dependencies belong to this provider.
pub(super) const fn supports_package_dependencies(provider: SemanticProviderOwner) -> bool {
    matches!(provider, SemanticProviderOwner::Cargo)
}

/// Parse the legacy public import helper without using it for provider selection.
pub(super) fn parse_display_import(import_text: &str) -> Vec<ImportReference> {
    let import_text = import_text.trim();
    if import_text.starts_with("use ") {
        parse_imports(SemanticProviderOwner::Rust, import_text)
    } else if import_text.starts_with("import ") && import_text.contains(" from ") {
        parse_imports(SemanticProviderOwner::EcmaScript, import_text)
    } else if import_text.starts_with("from ") || import_text.starts_with("import ") {
        parse_imports(SemanticProviderOwner::Python, import_text)
    } else {
        Vec::new()
    }
}

/// Build a normalized named-import reference when every identity is present.
fn named_reference(
    syntax: ImportSyntax,
    module: &str,
    item: &str,
    alias_marker: &str,
) -> Option<ImportReference> {
    let item = item.trim();
    let (imported, alias) = split_alias(item, alias_marker)?;
    let imported = imported.trim();
    let module = module.trim();
    if !is_compact_module(module) || !is_compact_name(imported) || imported == "*" {
        return None;
    }
    let local = alias.unwrap_or(imported).trim();
    is_compact_name(local).then(|| ImportReference::new(syntax, module, Some(imported), local))
}

/// Build a normalized module-import reference when both identities are present.
fn module_reference(syntax: ImportSyntax, module: &str, local: &str) -> Option<ImportReference> {
    let module = module.trim();
    let local = local.trim();
    (is_compact_module(module) && is_compact_name(local))
        .then(|| ImportReference::new(syntax, module, None, local))
}

/// Split an imported identity from its optional caller-local alias.
fn split_alias<'a>(value: &'a str, marker: &str) -> Option<(&'a str, Option<&'a str>)> {
    let Some((left, right)) = value.split_once(marker) else {
        return Some((value, None));
    };
    if left.trim().is_empty() || right.trim().is_empty() || right.contains(marker) {
        return None;
    }
    Some((left.trim(), Some(right.trim())))
}

/// Return whether a compact declaration or caller-local identity is safe to project.
fn is_compact_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_whitespace)
        && !value.chars().any(|character| {
            matches!(
                character,
                '{' | '}' | '(' | ')' | '[' | ']' | ',' | ';' | '"' | '\''
            )
        })
}

/// Return whether a compact module identity is safe to resolve further.
fn is_compact_module(value: &str) -> bool {
    is_compact_name(value) && !value.contains("//")
}

/// Extract one complete single- or double-quoted value.
fn quoted_text(text: &str) -> Option<&str> {
    let text = text.trim();
    let quote = text
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''))?;
    let rest = &text[quote.len_utf8()..];
    let end = rest.find(quote)?;
    if !rest[end + quote.len_utf8()..].trim().is_empty() {
        return None;
    }
    Some(&rest[..end])
}
