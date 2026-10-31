//! Grammar-owned static import source normalization.

use tree_sitter::Node;

use crate::{MAX_SNIPPET_CHARS, PARSER_CONTROL_CHECK_INTERVAL};

/// Retain literal bytes while removing only grammar-proven import trivia.
pub(super) fn canonical_import_source<E>(
    node: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<String>, E> {
    if node.has_error() {
        // Keep malformed source evidence for the existing identity/rejection
        // boundary; removing its trivia could turn recovery syntax into a binding.
        return Ok(node
            .utf8_text(content.as_bytes())
            .ok()
            .filter(|text| text.chars().count() <= MAX_SNIPPET_CHARS)
            .map(ToString::to_string));
    }
    canonical_import_recovery_source(node, content, check)
}

/// Normalize grammar-proven trivia without repairing malformed punctuation.
///
/// Recovery consumers independently validate binding roles and cooked names;
/// this function does not make an ERROR node syntax-valid.
pub(super) fn canonical_import_recovery_source<E>(
    node: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<String>, E> {
    canonical_import_source_until(node, content, node.end_byte(), check)
}

/// Bound a recovery clause by the actual unaliased `from` grammar token.
pub(super) fn canonical_ecmascript_import_clause<E>(
    node: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<String>, E> {
    let Some(source) = node.child_by_field_name("source") else {
        return Ok(None);
    };
    let mut separator = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check()?;
        if !child.is_named()
            && !child.is_missing()
            && child.kind() == "from"
            && child.end_byte() <= source.start_byte()
        {
            separator = Some(child.start_byte());
        }
    }
    let Some(end) = separator else {
        return Ok(None);
    };
    Ok(
        canonical_import_source_until(node, content, end, check)?.and_then(|source| {
            source
                .strip_prefix("import")
                .map(str::trim)
                .map(ToString::to_string)
        }),
    )
}

/// Normalize one grammar-owned source prefix without copying its later literals.
fn canonical_import_source_until<E>(
    node: Node<'_>,
    content: &str,
    end: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<String>, E> {
    let mut output = ImportSourceText::default();
    let mut copied_until = node.start_byte();
    let mut cursor = node.walk();
    let mut visited = 0_usize;
    loop {
        if visited.is_multiple_of(PARSER_CONTROL_CHECK_INTERVAL) {
            check()?;
        }
        visited = visited.saturating_add(1);
        let current = cursor.node();
        if current.start_byte() >= end {
            let Some(suffix) = content.get(copied_until..end) else {
                return Ok(None);
            };
            return Ok(output.source(suffix, check)?.then_some(output.text));
        }
        let trivia = matches!(
            current.kind(),
            "comment" | "line_comment" | "block_comment" | "line_continuation"
        );
        let literal = matches!(
            current.kind(),
            "string" | "string_literal" | "raw_string_literal"
        );
        if trivia || literal {
            let Some(prefix) = content.get(copied_until..current.start_byte()) else {
                return Ok(None);
            };
            if !output.source(prefix, check)? {
                return Ok(None);
            }
            if literal {
                let Some(text) = content.get(current.byte_range()) else {
                    return Ok(None);
                };
                if !output.literal(text) {
                    return Ok(None);
                }
            } else {
                output.trivia();
            }
            copied_until = current.end_byte();
        } else if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                let Some(suffix) = content.get(copied_until..end) else {
                    return Ok(None);
                };
                return Ok(output.source(suffix, check)?.then_some(output.text));
            }
        }
    }
}

/// One bounded canonical statement with whitespace pending between source tokens.
#[derive(Default)]
struct ImportSourceText {
    /// Canonical text, never truncated into another identifier or module path.
    text: String,
    /// Unicode scalar count used by the existing parser-output contract.
    characters: usize,
    /// Trivia separates tokens but never adds leading or trailing whitespace.
    pending_space: bool,
}

impl ImportSourceText {
    /// Compact whitespace outside literal nodes while preserving token spelling.
    fn source<E>(
        &mut self,
        text: &str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        for (index, character) in text.chars().enumerate() {
            if index > 0 && index.is_multiple_of(PARSER_CONTROL_CHECK_INTERVAL) {
                check()?;
            }
            if character.is_whitespace() {
                self.trivia();
            } else if !self.character(character) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Copy the complete bounded literal without whitespace or escape rewriting.
    fn literal(&mut self, text: &str) -> bool {
        if !self.flush_space() {
            return false;
        }
        for character in text.chars() {
            if !self.push(character) {
                return false;
            }
        }
        true
    }

    /// Preserve separation where a comment or source whitespace occurred.
    fn trivia(&mut self) {
        self.pending_space = !self.text.is_empty();
    }

    /// Add a non-whitespace source character after its pending token separator.
    fn character(&mut self, character: char) -> bool {
        self.flush_space() && self.push(character)
    }

    /// Materialize one pending separator only before subsequent source content.
    fn flush_space(&mut self) -> bool {
        if self.pending_space {
            self.pending_space = false;
            self.push(' ')
        } else {
            true
        }
    }

    /// Refuse an oversized result without emitting a truncated semantic identity.
    fn push(&mut self, character: char) -> bool {
        if self.characters >= MAX_SNIPPET_CHARS {
            return false;
        }
        self.text.push(character);
        self.characters += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{extract_symbol_graph, parse_import_references};
    use projectatlas_core::symbols::RelationKind;

    #[test]
    fn comment_trivia_preserves_static_import_bindings() {
        for (path, language, source, module) in [
            (
                "src/main.rs",
                "rust",
                "use crate::worker::{run/* token */as/* token */launch, /* separator */ stop};",
                "crate::worker",
            ),
            (
                "main.py",
                "python",
                "from worker import (run as launch, # separator\n stop)\n",
                "worker",
            ),
            (
                "main.js",
                "javascript",
                "import { run as launch, /* separator */ stop } from './worker.js';",
                "./worker.js",
            ),
            (
                "main.ts",
                "typescript",
                "import { run as launch, // separator\n stop } from './worker.js';",
                "./worker.js",
            ),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let references = graph
                .relations
                .iter()
                .filter(|relation| relation.kind == RelationKind::Imports)
                .flat_map(|relation| parse_import_references(&relation.target_name))
                .collect::<Vec<_>>();
            assert!(
                references.iter().any(|reference| {
                    reference.module() == module
                        && reference.imported() == Some("run")
                        && reference.local() == "launch"
                }),
                "lost alias for {language}: {references:?}"
            );
            assert!(
                references.iter().any(|reference| {
                    reference.module() == module && reference.imported() == Some("stop")
                }),
                "lost supported sibling for {language}: {references:?}"
            );
        }
    }

    #[test]
    fn explicit_python_continuation_preserves_import_alias() {
        let graph = extract_symbol_graph(
            "main.py",
            Some("python"),
            "from worker import run \\\n    as launch\n",
        );
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports
                && parse_import_references(&relation.target_name)
                    .iter()
                    .any(|reference| reference.local() == "launch")
        }));
    }

    #[test]
    fn nested_rust_use_bindings_keep_distinct_modules_and_self_namespaces() {
        let source =
            "use crate::worker::{run as launch, nested::{self as tools, tail as last}, *};";
        let graph = extract_symbol_graph("src/main.rs", Some("rust"), source);
        let references = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .flat_map(|relation| parse_import_references(&relation.target_name))
            .collect::<Vec<_>>();
        for (module, imported, local) in [
            ("crate::worker", Some("run"), "launch"),
            ("crate::worker::nested", Some("tail"), "last"),
            ("crate::worker::nested", None, "tools"),
        ] {
            assert!(
                references.iter().any(|reference| {
                    reference.module() == module
                        && reference.imported() == imported
                        && reference.local() == local
                }),
                "lost nested binding {module}::{imported:?} as {local}: {references:?}"
            );
        }
        assert!(references.iter().all(|reference| reference.local() != "*"));
    }

    #[test]
    fn rust_build_script_provider_preserves_nested_bindings() {
        let graph = extract_symbol_graph(
            "build.rs",
            Some("rust-build-script"),
            "use crate::worker::{run as launch, nested::{tail as last}};",
        );
        let references = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .flat_map(|relation| parse_import_references(&relation.target_name))
            .collect::<Vec<_>>();
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker" && reference.local() == "launch"
        }));
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker::nested" && reference.local() == "last"
        }));
    }

    #[test]
    fn flattened_rust_bindings_retain_their_leaf_lines_and_absolute_anchors() {
        let source =
            "use crate::worker::{\nrun as launch,\nnested::{\nself as tools,\ntail as last\n}\n};";
        let graph = extract_symbol_graph("src/main.rs", Some("rust"), source);
        for (local, expected_line) in [("launch", 2), ("tools", 4), ("last", 5)] {
            assert!(graph.relations.iter().any(|relation| {
                relation.line == expected_line
                    && parse_import_references(&relation.target_name)
                        .iter()
                        .any(|reference| reference.local() == local)
            }));
        }
        let absolute = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "use ::{foo, bar::item as alias};",
        );
        let imports = absolute
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .collect::<Vec<_>>();
        assert_eq!(imports.len(), 2);
        assert!(
            imports
                .iter()
                .all(|relation| relation.target_name.starts_with("use ::"))
        );
    }

    #[test]
    fn rust_path_trivia_and_outer_groups_preserve_aliases() {
        let graph = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "use {crate /* root */ :: worker :: { self as workers, run as launch }};",
        );
        let references = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .flat_map(|relation| parse_import_references(&relation.target_name))
            .collect::<Vec<_>>();
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker"
                && reference.imported().is_none()
                && reference.local() == "workers"
        }));
        assert!(references.iter().any(|reference| {
            reference.module() == "crate::worker"
                && reference.imported() == Some("run")
                && reference.local() == "launch"
        }));
    }

    #[test]
    fn module_literal_whitespace_and_comment_markers_are_source_data() {
        for module in [
            "./dep  space.js",
            "./literal/*marker*/dep.js",
            "./trailing-space.js ",
        ] {
            let source = format!("import {{ run as launch }} from '{module}';");
            let graph = extract_symbol_graph("main.js", Some("javascript"), &source);
            let relation = graph
                .relations
                .iter()
                .find(|relation| relation.kind == RelationKind::Imports);
            assert!(relation.is_some(), "missing import fact for {module}");
            let references = relation
                .into_iter()
                .flat_map(|relation| parse_import_references(&relation.target_name))
                .collect::<Vec<_>>();
            assert!(
                references
                    .iter()
                    .any(|reference| reference.module() == module && reference.local() == "launch"),
                "module literal changed or rejected: {module}: {references:?}"
            );
        }
    }

    #[test]
    fn escaped_ecmascript_literals_use_their_static_string_value() {
        for (literal, expected) in [
            (r"'./odd\'name.js'", "./odd'name.js"),
            (r#""./odd\"name.js""#, "./odd\"name.js"),
            (r"'./back\\slash.js'", "./back\\slash.js"),
            (r"'./de\x70.js'", "./dep.js"),
            (r"'./de\u0070.js'", "./dep.js"),
            (r"'./\u{1F680}.js'", "./🚀.js"),
            (r"'./\uD83D\uDE80.js'", "./🚀.js"),
            ("'./de\\\np.js'", "./dep.js"),
        ] {
            let source = format!("import {{ run as launch }} from {literal};");
            let graph = extract_symbol_graph("main.js", Some("javascript"), &source);
            let references = graph
                .relations
                .iter()
                .filter(|relation| relation.kind == RelationKind::Imports)
                .flat_map(|relation| parse_import_references(&relation.target_name))
                .collect::<Vec<_>>();
            assert!(
                references.iter().any(|reference| {
                    reference.module() == expected && reference.local() == "launch"
                }),
                "escaped module lost its string value: {literal}: {references:?}"
            );
        }
    }

    #[test]
    fn long_comment_does_not_consume_canonical_import_bound() {
        let comment = "comment ".repeat(1_000);
        let source =
            format!("import {{ run as launch, /* {comment} */ stop }} from './worker.js';");
        let graph = extract_symbol_graph("main.js", Some("javascript"), &source);
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports
                && relation.target_name.chars().count() <= super::super::MAX_SNIPPET_CHARS
                && parse_import_references(&relation.target_name)
                    .iter()
                    .any(|reference| reference.local() == "launch")
        }));
    }

    #[test]
    fn canonical_import_keeps_original_bounded_context() {
        let source = "from worker import (run as launch, # original separator\n stop)\n";
        let graph = extract_symbol_graph("main.py", Some("python"), source);
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports
                && !relation.target_name.contains('#')
                && relation.context.contains("# original separator\n")
                && relation.line == 1
        }));
    }

    #[test]
    fn malformed_imports_do_not_gain_bindings_when_trivia_is_removed() {
        for (language, source) in [
            ("python", "from worker import (run as, # comment\n stop)"),
            (
                "javascript",
                "import { run as /* comment */ } from './worker.js';",
            ),
            ("rust", "use crate::worker::{run as /* comment */};"),
        ] {
            let graph = extract_symbol_graph("source", Some(language), source);
            assert!(
                graph
                    .relations
                    .iter()
                    .filter(|relation| relation.kind == RelationKind::Imports)
                    .all(|relation| parse_import_references(&relation.target_name).is_empty()),
                "malformed {language} import gained a semantic binding"
            );
        }
    }

    #[test]
    fn import_trivia_walk_observes_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        let language = crate::tree_sitter_language("javascript")
            .ok_or_else(|| std::io::Error::other("missing built-in ECMAScript grammar"))?;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language)?;
        let source = format!(
            "import {{ {}run as launch }} from './worker.js';",
            "/* separator */ ".repeat(200)
        );
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| std::io::Error::other("test import was not parsed"))?;
        let node = tree
            .root_node()
            .named_child(0)
            .ok_or_else(|| std::io::Error::other("missing test import declaration"))?;
        let mut checkpoints = 0_usize;
        let result = super::canonical_import_source(node, &source, &mut || {
            checkpoints += 1;
            if checkpoints == 2 {
                Err("cancelled during import traversal")
            } else {
                Ok(())
            }
        });
        if result != Err("cancelled during import traversal") || checkpoints != 2 {
            return Err(std::io::Error::other(
                "import traversal did not stop at its cancellation checkpoint",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn long_whitespace_span_observes_cancellation_inside_the_span() {
        let mut output = super::ImportSourceText::default();
        let result = output.source(&" ".repeat(10_000), &mut || Err("cancelled in whitespace"));
        assert_eq!(result, Err("cancelled in whitespace"));
        assert!(output.text.is_empty());
    }
}
