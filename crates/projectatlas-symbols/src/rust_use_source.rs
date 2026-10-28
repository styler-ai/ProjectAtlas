//! Canonical flat import facts from the existing Rust use-tree AST.

use tree_sitter::Node;

use crate::MAX_SNIPPET_CHARS;

/// Bounded canonical imports and whether every syntax binding was represented.
pub(super) struct RustUseSources {
    /// Separate module/binding facts in source order.
    pub(super) facts: Vec<RustUseFact>,
    /// False when output or syntax bounds prevented retaining a binding.
    pub(super) complete: bool,
}

/// One canonical binding with its original leaf location.
pub(super) struct RustUseFact {
    /// Existing flat use syntax consumed by provider normalization.
    pub(super) statement: String,
    /// One-based line where this binding starts in the original use tree.
    pub(super) line: usize,
    /// Complete grammar-owned path and leaf role before legacy normalization.
    pub(super) binding: Option<crate::SourceRustUse>,
}

/// Flatten one valid Rust declaration without parsing its text a second time.
pub(super) fn use_sources<E>(
    node: Node<'_>,
    content: &str,
    limit: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<RustUseSources>, E> {
    if node.kind() != "use_declaration" || node.has_error() {
        return Ok(None);
    }
    let Some(argument) = node.child_by_field_name("argument") else {
        return Ok(None);
    };
    let mut sources = RustUseSources {
        facts: Vec::new(),
        complete: true,
    };
    collect_use_sources(argument, "", false, 0, content, limit, &mut sources, check)?;
    let module = crate::rust_binding_source::module_owner(node, content);
    let reexport =
        crate::rust_binding_source::visibility(node) != crate::SourceRustVisibility::Private;
    for fact in &mut sources.facts {
        check()?;
        if let Some(binding) = &mut fact.binding {
            binding.supported = module.is_some()
                && crate::rust_binding_source::visibility(node)
                    != crate::SourceRustVisibility::Unsupported
                && !crate::rust_binding_source::has_unsupported_attributes(node, content);
            binding.module = module
                .as_ref()
                .map_or_else(|| std::sync::Arc::from([]), std::sync::Arc::clone);
            binding.reexport = reexport;
        }
    }
    Ok(Some(sources))
}

/// Traverse use lists with bounded path depth and bounded emitted fact count.
#[allow(clippy::too_many_arguments)]
fn collect_use_sources<E>(
    node: Node<'_>,
    prefix: &str,
    grouped: bool,
    depth: usize,
    content: &str,
    limit: usize,
    sources: &mut RustUseSources,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    check()?;
    if matches!(node.kind(), "line_comment" | "block_comment") {
        return Ok(());
    }
    if depth >= MAX_SNIPPET_CHARS || sources.facts.len() >= limit {
        sources.complete = false;
        return Ok(());
    }
    if node.kind() == "use_list" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_use_sources(
                child,
                prefix,
                true,
                depth + 1,
                content,
                limit,
                sources,
                check,
            )?;
        }
        return Ok(());
    }
    if node.kind() == "scoped_use_list" {
        let path = node.child_by_field_name("path").map_or_else(
            || {
                Some(
                    if node
                        .utf8_text(content.as_bytes())
                        .ok()?
                        .trim_start()
                        .starts_with("::")
                    {
                        "::".to_string()
                    } else {
                        String::new()
                    },
                )
            },
            |path| rust_path(path, content, 0),
        );
        let (Some(path), Some(list)) = (path, node.child_by_field_name("list")) else {
            sources.complete = false;
            return Ok(());
        };
        let Some(prefix) = joined_path(prefix, &path) else {
            sources.complete = false;
            return Ok(());
        };
        return collect_use_sources(
            list,
            &prefix,
            true,
            depth + 1,
            content,
            limit,
            sources,
            check,
        );
    }
    let (path_node, alias) = if node.kind() == "use_as_clause" {
        let path = node.child_by_field_name("path");
        let alias = node
            .child_by_field_name("alias")
            .and_then(|alias| bounded_node_text(alias, content));
        let (Some(path), Some(alias)) = (path, alias) else {
            sources.complete = false;
            return Ok(());
        };
        (path, Some(alias))
    } else {
        (node, None)
    };
    let mut binding = None;
    let statement = if path_node.kind() == "use_wildcard" {
        let mut cursor = path_node.walk();
        let path = path_node
            .named_children(&mut cursor)
            .find(|child| !matches!(child.kind(), "line_comment" | "block_comment"))
            .map_or_else(|| Some(String::new()), |path| rust_path(path, content, 0));
        path.and_then(|path| joined_path(prefix, &path))
            .map(|path| {
                binding = source_binding(&path, crate::SourceRustUseRole::Wildcard, None);
                if path.is_empty() {
                    "use *;".to_string()
                } else {
                    format!("use {path}::*;")
                }
            })
    } else {
        rust_path(path_node, content, 0).and_then(|path| {
            if grouped && path == "self" && !prefix.is_empty() {
                let local = alias.unwrap_or_else(|| prefix.rsplit("::").next().unwrap_or(prefix));
                binding =
                    source_binding(prefix, crate::SourceRustUseRole::SelfNamespace, Some(local));
                return Some(format!("use {prefix}::{{self as {local}}};"));
            }
            let full = joined_path(prefix, &path)?;
            binding = source_binding(
                &full,
                crate::SourceRustUseRole::Item,
                alias.or_else(|| full.rsplit("::").next()),
            );
            let suffix = alias.map_or_else(String::new, |alias| format!(" as {alias}"));
            if grouped && let Some((module, name)) = full.rsplit_once("::") {
                Some(format!("use {module}::{{{name}{suffix}}};"))
            } else {
                Some(format!("use {full}{suffix};"))
            }
        })
    };
    match statement {
        Some(statement) if statement.chars().count() <= MAX_SNIPPET_CHARS => {
            sources.facts.push(RustUseFact {
                statement,
                line: node.start_position().row + 1,
                binding,
            });
        }
        Some(_) | None => sources.complete = false,
    }
    Ok(())
}

/// Retain the AST walker's complete path without classifying its final identifier.
fn source_binding(
    path: &str,
    role: crate::SourceRustUseRole,
    local: Option<&str>,
) -> Option<crate::SourceRustUse> {
    Some(crate::SourceRustUse {
        path: crate::SourceRustPath::from_ast_path(path)?,
        role,
        local: local.filter(|local| *local != "_").map(ToString::to_string),
        module: std::sync::Arc::from([]),
        reexport: false,
        supported: true,
    })
}

/// Recover path components from syntax fields instead of compacting punctuation trivia.
pub(super) fn rust_path(node: Node<'_>, content: &str, depth: usize) -> Option<String> {
    if depth >= MAX_SNIPPET_CHARS {
        return None;
    }
    match node.kind() {
        "identifier" | "crate" | "self" | "super" => {
            bounded_node_text(node, content).map(ToString::to_string)
        }
        "scoped_identifier" => {
            let name = rust_path(node.child_by_field_name("name")?, content, depth + 1)?;
            let path = node.child_by_field_name("path").map_or_else(
                || Some(String::new()),
                |path| rust_path(path, content, depth + 1),
            )?;
            let absolute = node
                .utf8_text(content.as_bytes())
                .ok()?
                .trim_start()
                .starts_with("::");
            if path.is_empty() && absolute {
                Some(format!("::{name}"))
            } else {
                joined_path(&path, &name)
            }
        }
        _ => None,
    }
}

/// Borrow one already-parsed, nonempty bounded identifier component.
fn bounded_node_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    node.utf8_text(content.as_bytes())
        .ok()
        .filter(|text| !text.is_empty() && text.chars().count() <= MAX_SNIPPET_CHARS)
}

/// Extend a bounded path without changing an absolute or caller-relative anchor.
fn joined_path(prefix: &str, path: &str) -> Option<String> {
    if !prefix.is_empty() && path.starts_with("::") {
        return None;
    }
    let joined = match (prefix.is_empty(), path.is_empty()) {
        (true, _) => path.to_string(),
        (_, true) => prefix.to_string(),
        (false, false) if prefix == "::" => format!("::{path}"),
        (false, false) => format!("{prefix}::{path}"),
    };
    (joined.chars().count() <= MAX_SNIPPET_CHARS).then_some(joined)
}

#[cfg(test)]
mod tests {
    use crate::{MAX_SNIPPET_CHARS, extract_symbol_graph, parse_import_references};
    use projectatlas_core::symbols::{ParserKind, RelationKind};

    #[test]
    fn oversized_use_branch_keeps_supported_siblings_and_reports_partial_facts() {
        let path = "nested::".repeat(MAX_SNIPPET_CHARS);
        let source = format!("use crate::worker::{{{path}tail as oversized, run as launch}};");
        let graph = extract_symbol_graph("src/main.rs", Some("rust"), &source);
        assert_eq!(graph.parser, ParserKind::Fallback);
        let references = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .flat_map(|relation| parse_import_references(&relation.target_name))
            .collect::<Vec<_>>();
        assert!(
            references
                .iter()
                .any(|reference| reference.local() == "launch")
        );
        assert!(
            references
                .iter()
                .all(|reference| reference.local() != "oversized")
        );
    }

    #[test]
    fn explicit_self_import_keeps_the_module_instead_of_a_self_named_declaration() {
        let references = parse_import_references("use crate::worker::{self, run};");
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker"
                && reference.imported().is_none()
                && reference.local() == "worker"
        }));
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker" && reference.imported() == Some("run")
        }));
    }

    #[test]
    fn use_tree_work_and_output_are_cooperatively_bounded() -> Result<(), Box<dyn std::error::Error>>
    {
        let language = crate::tree_sitter_language("rust")
            .ok_or_else(|| std::io::Error::other("missing built-in Rust grammar"))?;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language)?;
        let source = "use crate::worker::{run as launch, nested::{tail as last}};";
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| std::io::Error::other("test use tree did not parse"))?;
        let node = tree
            .root_node()
            .named_child(0)
            .ok_or_else(|| std::io::Error::other("missing use declaration"))?;
        let limited = super::use_sources(node, source, 1, &mut || Ok::<(), &str>(()))?
            .ok_or_else(|| std::io::Error::other("valid use tree was not admitted"))?;
        if limited.complete || limited.facts.len() != 1 {
            return Err(std::io::Error::other("use output limit was not preserved").into());
        }
        let mut checkpoints = 0_usize;
        let stopped = super::use_sources(node, source, 10, &mut || {
            checkpoints += 1;
            if checkpoints == 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });
        if !matches!(stopped, Err("cancelled")) || checkpoints != 2 {
            return Err(std::io::Error::other("use traversal ignored cancellation").into());
        }
        Ok(())
    }
}
