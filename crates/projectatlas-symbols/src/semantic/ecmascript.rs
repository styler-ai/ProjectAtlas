//! ECMAScript import, export, and repository-relative module semantics.

use super::ecmascript_identifier::{binding_identifier_value, identifier_name_value};
use super::ecmascript_literal::module_string_value;
use super::{ImportReference, ImportSyntax, SourceImportProof};
use crate::configured_modules::ConfiguredModuleSource;
use projectatlas_core::symbols::SymbolGraph;
use tree_sitter::Node;

/// Read a grammar-owned module export name, whose role permits reserved words.
pub(super) fn source_export_name(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?;
    if node.kind() == "string" {
        module_string_value(text).map(std::borrow::Cow::into_owned)
    } else {
        identifier_name_value(text).map(std::borrow::Cow::into_owned)
    }
}

/// Read the independently static module literal from its actual grammar field.
pub(super) fn source_module(node: Node<'_>, content: &str) -> Option<String> {
    let source = node.child_by_field_name("source")?;
    if source.has_error() || source.kind() != "string" {
        return None;
    }
    module_string_value(source.utf8_text(content.as_bytes()).ok()?)
        .filter(|value| !value.is_empty())
        .map(std::borrow::Cow::into_owned)
}

/// Collect independently valid import leaves from the existing ECMAScript AST.
pub(super) fn source_bindings<E>(
    node: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SourceImportProof, E> {
    check()?;
    let Some(module) = source_module(node, content) else {
        return Ok(SourceImportProof::default());
    };
    let mut cursor = node.walk();
    let mut clause = None;
    for child in node.children(&mut cursor) {
        check()?;
        if !child.is_named() && matches!(child.kind(), "type" | "typeof") {
            return Ok(SourceImportProof::default());
        }
        if child.kind() == "import_clause" {
            clause = Some(child);
        }
    }
    if node.has_error() {
        // Recovery may absorb or reinterpret a later specifier inside ERROR.
        // Use only the bounded captured-source normalizer in this case, never
        // combine its roles with potentially reinterpreted recovery AST roles.
        let clause =
            crate::import_source::canonical_ecmascript_import_clause(node, content, check)?;
        let references =
            clause.map_or_else(Vec::new, |clause| parse_import_clause(&clause, &module));
        return Ok(SourceImportProof {
            module: Some(module),
            references,
        });
    }
    let Some(clause) = clause else {
        return Ok(SourceImportProof {
            module: Some(module),
            references: Vec::new(),
        });
    };
    let mut references = Vec::new();
    let mut default = None;
    let mut default_valid = true;
    let mut before_comma = true;
    let mut cursor = clause.walk();
    for child in clause.children(&mut cursor) {
        check()?;
        match child.kind() {
            "comment" => {}
            "," => before_comma = false,
            "identifier" if before_comma => {
                if default.replace(child).is_some() {
                    default_valid = false;
                }
            }
            "namespace_import" if !child.has_error() => {
                let mut names = child.walk();
                if let Some(alias) = child
                    .named_children(&mut names)
                    .find(|name| name.kind() == "identifier")
                    .and_then(|name| name.utf8_text(content.as_bytes()).ok())
                    .and_then(binding_identifier_value)
                {
                    references.push(ImportReference::new(
                        ImportSyntax::EcmaScript,
                        &module,
                        None,
                        &alias,
                    ));
                }
            }
            "named_imports" => {
                let mut items = child.walk();
                for item in child.named_children(&mut items) {
                    check()?;
                    if item.kind() != "import_specifier" || item.has_error() {
                        continue;
                    }
                    let mut tokens = item.walk();
                    if item
                        .children(&mut tokens)
                        .any(|token| !token.is_named() && matches!(token.kind(), "type" | "typeof"))
                    {
                        continue;
                    }
                    let Some(name) = item.child_by_field_name("name") else {
                        continue;
                    };
                    let alias = item.child_by_field_name("alias").unwrap_or(name);
                    let Some(local) = alias
                        .utf8_text(content.as_bytes())
                        .ok()
                        .and_then(binding_identifier_value)
                    else {
                        continue;
                    };
                    let Some(raw_name) = name.utf8_text(content.as_bytes()).ok() else {
                        continue;
                    };
                    let imported = if name.kind() == "string" {
                        module_string_value(raw_name)
                    } else {
                        identifier_name_value(raw_name)
                    };
                    if let Some(imported) = imported {
                        references.push(ImportReference::new(
                            ImportSyntax::EcmaScript,
                            &module,
                            Some(&imported),
                            &local,
                        ));
                    }
                }
            }
            _ if before_comma => default_valid = false,
            _ => {}
        }
    }
    if default_valid
        && let Some(local) = default
            .and_then(|name| name.utf8_text(content.as_bytes()).ok())
            .and_then(binding_identifier_value)
    {
        references.insert(
            0,
            ImportReference::new(ImportSyntax::EcmaScript, &module, Some("default"), &local),
        );
    }
    Ok(SourceImportProof {
        module: Some(module),
        references,
    })
}

/// Parse independent default, named and namespace ECMAScript import bindings.
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
    let Some(module) = module_string_value(from) else {
        return Vec::new();
    };
    if module.is_empty() {
        return Vec::new();
    }
    parse_import_clause(bindings, &module)
}

/// Normalize a bounded clause independently from its already decoded module.
fn parse_import_clause(bindings: &str, module: &str) -> Vec<ImportReference> {
    let bindings = bindings.trim();
    if bindings.starts_with(['*', '{']) {
        return parse_secondary_bindings(bindings, module).unwrap_or_default();
    }
    let Some(commas) = unquoted_import_commas(bindings) else {
        return Vec::new();
    };
    let (default, remainder) = commas.first().map_or((bindings, None), |index| {
        (
            bindings[..*index].trim(),
            Some(bindings[index + 1..].trim()),
        )
    });
    if default.is_empty() || default.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    let mut references = binding_identifier_value(default)
        .map(|default| {
            ImportReference::new(ImportSyntax::EcmaScript, module, Some("default"), &default)
        })
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(remainder) = remainder {
        let Some(secondary) = parse_secondary_bindings(remainder, module) else {
            return Vec::new();
        };
        references.extend(secondary);
    }
    references
}

/// Parse the namespace or named clause following an optional default binding.
fn parse_secondary_bindings(bindings: &str, module: &str) -> Option<Vec<ImportReference>> {
    if let Some(rest) = bindings.strip_prefix('*') {
        let alias = rest.trim().strip_prefix("as ")?.trim();
        return Some(
            binding_identifier_value(alias)
                .map(|alias| ImportReference::new(ImportSyntax::EcmaScript, module, None, &alias))
                .into_iter()
                .collect(),
        );
    }
    let items = bindings.strip_prefix('{')?.strip_suffix('}')?;
    if items.trim().is_empty() {
        return Some(Vec::new());
    }
    let items = items.trim().strip_suffix(',').unwrap_or(items.trim());
    let mut references = Vec::new();
    let mut start = 0;
    for end in unquoted_import_commas(items)?
        .into_iter()
        .chain(std::iter::once(items.len()))
    {
        let item = items.get(start..end)?.trim();
        start = end + 1;
        if item.is_empty() {
            return None;
        }
        let (imported, local) = item
            .rsplit_once(" as ")
            .map_or((item, item), |(imported, local)| {
                (imported.trim(), local.trim())
            });
        let quoted = imported.starts_with(['\'', '"']);
        if (!quoted && imported.chars().any(char::is_whitespace))
            || local.chars().any(char::is_whitespace)
        {
            return None;
        }
        let imported = if quoted {
            module_string_value(imported)
        } else {
            identifier_name_value(imported)
        };
        if let Some(imported) = imported
            && let Some(local) = binding_identifier_value(local)
        {
            references.push(ImportReference::new(
                ImportSyntax::EcmaScript,
                module,
                Some(&imported),
                &local,
            ));
        }
    }
    Some(references)
}

/// Frame comma separators without treating quoted export-name contents as syntax.
///
/// This only frames literals; the existing strict `StringValue` decoder validates
/// their complete escape grammar, while OXC validates unquoted identifiers.
fn unquoted_import_commas(source: &str) -> Option<Vec<usize>> {
    let mut commas = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in source.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == ',' {
            commas.push(index);
        }
    }
    quote.is_none().then_some(commas)
}

/// Return whether the parser marked one declaration as exported.
pub(super) fn is_export_candidate(graph: &SymbolGraph, symbol_index: usize) -> bool {
    graph.symbols[symbol_index].exported
}

/// Resolve and normalize scopes referenced by one ECMAScript import.
pub(super) fn import_scopes(
    caller_path: &str,
    reference: &ImportReference,
    configured_source: Option<&ConfiguredModuleSource<'_>>,
) -> Vec<String> {
    module_scopes(caller_path, reference.module(), configured_source)
}

/// Resolve a static module independently from whether any local binding survived.
pub(super) fn module_scopes(
    caller_path: &str,
    module: &str,
    configured_source: Option<&ConfiguredModuleSource<'_>>,
) -> Vec<String> {
    if let Some(scope) = resolve_relative_import_path(caller_path, module) {
        return vec![scope];
    }
    configured_source.map_or_else(Vec::new, |configured| configured.scopes_for_import(module))
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

#[cfg(test)]
mod tests {
    use super::parse_import;
    use crate::{ImportReference, ImportSyntax};

    #[test]
    fn mixed_imports_preserve_distinct_default_named_and_namespace_bindings() {
        for (statement, expected) in [
            (
                "import primary, { run as launch, stop } from './dep';",
                vec![
                    (Some("default"), "primary"),
                    (Some("run"), "launch"),
                    (Some("stop"), "stop"),
                ],
            ),
            (
                "import primary, * as workers from './dep';",
                vec![(Some("default"), "primary"), (None, "workers")],
            ),
            (
                "import primary from './dep';",
                vec![(Some("default"), "primary")],
            ),
            (
                "import primary, {} from './dep';",
                vec![(Some("default"), "primary")],
            ),
        ] {
            let expected = expected
                .into_iter()
                .map(|(imported, local)| {
                    ImportReference::new(ImportSyntax::EcmaScript, "./dep", imported, local)
                })
                .collect::<Vec<_>>();
            assert_eq!(parse_import(statement), expected, "{statement}");
        }
    }

    #[test]
    fn malformed_mixed_clauses_do_not_salvage_a_default_binding() {
        for statement in [
            "import primary, from './dep';",
            "import primary, other from './dep';",
            "import primary, * workers from './dep';",
            "import primary, { run as } from './dep';",
            "import primary, { run } unexpected from './dep';",
            "import primary, { run } from variable;",
            "import primary, { run } from '';",
            "import type primary, { run } from './dep';",
            "import default from './dep';",
        ] {
            assert!(parse_import(statement).is_empty(), "{statement}");
        }
    }
}
