//! Source-owned ECMAScript grammar parameters shared by parsing and binding.

use tree_sitter::Node;

use super::EcmascriptNameContext;
use super::embedded_source::EmbeddedScriptRole;

/// A callable's name, formals and body need not use the same grammar parameters.
#[derive(Clone, Copy)]
pub(crate) struct CallableNameContexts {
    /// Declaration names inherit outer Await/Yield; expression names own them.
    pub(crate) name: EcmascriptNameContext,
    /// Arrow formals inherit outer Await/Yield restrictions.
    pub(crate) formals: EcmascriptNameContext,
    /// Ordinary and arrow bodies establish their own Await/Yield parameters.
    pub(crate) body: EcmascriptNameContext,
}

/// Select an explicit source goal or the grammar-proven module declarations.
pub(crate) fn source_context<E>(
    root: Node<'_>,
    path: &str,
    role: Option<EmbeddedScriptRole>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<EcmascriptNameContext, E> {
    let module = match role {
        Some(EmbeddedScriptRole::HtmlClassic) => false,
        Some(_) => true,
        None => match path.rsplit_once('.').map(|(_, extension)| extension) {
            Some("mjs" | "mts") => true,
            Some("cjs") => false,
            _ => {
                let mut cursor = root.walk();
                let mut module = false;
                for child in root.named_children(&mut cursor) {
                    check()?;
                    module |= matches!(child.kind(), "import_statement" | "export_statement");
                }
                module
            }
        },
    };
    let mut context = if module {
        EcmascriptNameContext::module()
    } else {
        EcmascriptNameContext::default()
    };
    context = context.with_strict(strict_directive(root, content, check)?);
    Ok(context)
}

/// Select the closed ECMAScript callable grammar family.
pub(crate) fn is_callable(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "function"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}

/// Class expressions and declarations both establish strict code.
pub(crate) fn is_class(node: Node<'_>) -> bool {
    matches!(node.kind(), "class" | "class_declaration")
}

/// Derive all callable grammar contexts once from the pinned parser's nodes.
pub(crate) fn callable_contexts<E>(
    node: Node<'_>,
    outer: EcmascriptNameContext,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<CallableNameContexts, E> {
    let mut cursor = node.walk();
    let mut asynchronous = false;
    let mut generator = matches!(
        node.kind(),
        "generator_function" | "generator_function_declaration"
    );
    for child in node.children(&mut cursor) {
        check()?;
        asynchronous |= child.kind() == "async";
        generator |= child.kind() == "*";
    }
    let arrow = node.kind() == "arrow_function";
    let mut body = outer.callable(asynchronous, generator, arrow);
    if let Some(block) = node.child_by_field_name("body") {
        body = body.with_strict(strict_directive(block, content, check)?);
    }
    let name = if matches!(
        node.kind(),
        "function_expression" | "function" | "generator_function"
    ) {
        body
    } else {
        outer.with_strict(body.is_strict())
    };
    let formals = if arrow {
        arrow_formal_context(outer, body, asynchronous)
    } else {
        body
    };
    Ok(CallableNameContexts {
        name,
        formals,
        body,
    })
}

/// Arrow formals inherit restrictions that their non-async concise body resets.
fn arrow_formal_context(
    outer: EcmascriptNameContext,
    body: EcmascriptNameContext,
    asynchronous: bool,
) -> EcmascriptNameContext {
    EcmascriptNameContext {
        await_keyword: outer.await_keyword || asynchronous,
        yield_keyword: outer.yield_keyword,
        ..body
    }
}

/// Select the context of an actual child, including enclosing computed keys.
pub(crate) fn child_context(
    node: Node<'_>,
    child: Node<'_>,
    outer: EcmascriptNameContext,
    callable: Option<CallableNameContexts>,
) -> EcmascriptNameContext {
    if let Some(callable) = callable {
        if node
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == child.id())
        {
            return if node.kind() == "method_definition" {
                outer
            } else {
                callable.name
            };
        }
        if ["parameters", "parameter"].iter().any(|field| {
            node.child_by_field_name(field)
                .is_some_and(|parameters| parameters.id() == child.id())
        }) {
            return callable.formals;
        }
        return callable.body;
    }
    let mut context = outer;
    if is_class(node) {
        context = context.with_strict(true);
    }
    if node.kind() == "class_static_block" {
        context = context.with_strict(true);
        context.await_keyword = true;
        context.yield_keyword = false;
        context.arguments_forbidden = true;
    }
    if matches!(node.kind(), "field_definition" | "public_field_definition")
        && node
            .child_by_field_name("value")
            .is_some_and(|value| value.id() == child.id())
    {
        context.await_keyword = context.is_module();
        context.yield_keyword = false;
        context.arguments_forbidden = true;
    }
    context
}

/// Detect directive prologues without interpreting escaped string literals.
pub(crate) fn strict_directive<E>(
    body: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    if !matches!(body.kind(), "program" | "statement_block") {
        return Ok(false);
    }
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        check()?;
        if matches!(child.kind(), "comment" | "hash_bang_line") {
            continue;
        }
        if child.kind() != "expression_statement" {
            break;
        }
        let Some(expression) = crate::first_named_child(child) else {
            break;
        };
        if expression.kind() != "string" {
            break;
        }
        if matches!(
            content.get(expression.byte_range()),
            Some("\"use strict\"" | "'use strict'")
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Repair the pinned grammar's contextual-keyword ambiguity in proven Scripts.
///
/// A single lazy same-length buffer keeps original UTF-8 coordinates. The normal
/// parser then owns calls/arguments; every graph text read still uses the original
/// source. Only the exact TypeScript keyword/formals recovery shape is admitted;
/// arbitrary recovery, literals, comments and real operators remain untouched.
pub(crate) fn project_contextual_identifiers<E>(
    root: Node<'_>,
    context: EcmascriptNameContext,
    content: &str,
    program_span: crate::SourceByteSpan,
    projected: &mut Option<String>,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    fn visit<E>(
        node: Node<'_>,
        mut context: EcmascriptNameContext,
        content: &str,
        projected: &mut Option<String>,
        depth: usize,
        mut uncertain: bool,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        check()?;
        if depth >= crate::MAX_TREE_SITTER_AST_DEPTH || node.is_missing() {
            return Ok(false);
        }
        if node.is_error() {
            uncertain = true;
            // A recovered wrapper may hide an ordinary arrow/function boundary.
            // Candidate AST validation must prove the actual parameters afterward.
            context = context.callable(false, false, true);
        }
        if node.child_count() == 0 {
            let contextual = !context.is_module()
                && match node.kind() {
                    "yield" => !context.is_strict() && !context.yield_keyword,
                    "await" => !context.await_keyword,
                    "interface" => {
                        !context.is_strict()
                            && node.parent().is_some_and(|parent| parent.is_error())
                    }
                    _ => false,
                };
            if contextual
                && content.get(node.byte_range()) == Some(node.kind())
                && (!uncertain || original_keyword_recovery(node, check)?)
            {
                if projected.is_none() {
                    check()?;
                    *projected = Some(content.to_owned());
                    check()?;
                }
                if let Some(buffer) = projected
                    && let Some(last) = buffer.get_mut(node.end_byte() - 1..node.end_byte())
                {
                    last.make_ascii_uppercase();
                }
                return Ok(true);
            }
            return Ok(false);
        }
        let callable = if is_callable(node) {
            Some(callable_contexts(node, context, content, check)?)
        } else {
            None
        };
        let mut changed = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            changed |= visit(
                child,
                child_context(node, child, context, callable),
                content,
                projected,
                depth + 1,
                uncertain,
                check,
            )?;
        }
        Ok(changed)
    }
    if context.is_module() {
        return Ok(false);
    }
    let Some(program) = content.get(program_span.start..program_span.end) else {
        return Ok(false);
    };
    if !contains_contextual_keyword(program, check)? {
        return Ok(false);
    }
    visit(root, context, content, projected, 0, false, check)
}

/// Search only this program's bytes, with bounded cooperative scan checkpoints.
fn contains_contextual_keyword<E>(
    source: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    let mut offset = 0;
    while offset < source.len() {
        check()?;
        let mut end = offset.saturating_add(65_536).min(source.len());
        while !source.is_char_boundary(end) {
            end += 1;
        }
        // Eight bytes cover a possible interface token crossing a chunk boundary.
        let mut overlap = end.saturating_add(8).min(source.len());
        while !source.is_char_boundary(overlap) {
            overlap += 1;
        }
        let chunk = &source[offset..overlap];
        if chunk.contains("yield") || chunk.contains("await") || chunk.contains("interface") {
            return Ok(true);
        }
        offset = end;
    }
    check()?;
    Ok(false)
}

/// Require original native call/binding evidence before relaxing an error context.
fn original_keyword_recovery<E>(
    token: Node<'_>,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    if token.parent().is_some_and(|error| {
        error.is_error()
            && error.child_count() == 1
            && error.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "formal_parameters" | "required_parameter" | "optional_parameter"
                )
            })
    }) {
        return Ok(true);
    }
    let mut next = token.next_sibling();
    for _ in 0..crate::MAX_TREE_SITTER_AST_DEPTH {
        check()?;
        let Some(node) = next else {
            return Ok(false);
        };
        if node.is_missing() {
            return Ok(false);
        }
        match node.kind() {
            "comment" | "(" | "await" | "yield" | "interface" => {}
            "formal_parameters" | "parenthesized_expression" => return Ok(!node.has_error()),
            "ERROR" => {
                // A conditional's following arm can be recovered as a type
                // annotation inside this wrapper. Only its leading complete
                // native group proves the call prefix; the candidate parser
                // must independently establish the whole expression/program.
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    check()?;
                    if child.kind() == "comment" {
                        continue;
                    }
                    return Ok(matches!(
                        child.kind(),
                        "formal_parameters" | "parenthesized_expression"
                    ) && !child.has_error());
                }
                return Ok(false);
            }
            _ => return Ok(false),
        }
        next = node.next_sibling();
    }
    Ok(false)
}

/// Whether a candidate is ready, needs one operator-restoring parse, or is unsafe.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ContextualProjectionValidation {
    /// Every changed byte is a valid identifier under the actual candidate AST.
    Valid,
    /// An actual reserved operator/binding was restored; the tree must be reparsed.
    Restored,
    /// Original-to-candidate correspondence or grammar context was not proved.
    Rejected,
}

/// Validate every changed token against actual native scopes, never guessed error scopes.
pub(crate) fn validate_contextual_projection<E>(
    root: Node<'_>,
    path: &str,
    role: Option<EmbeddedScriptRole>,
    span: crate::SourceByteSpan,
    original: &str,
    projected: &mut str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<ContextualProjectionValidation, E> {
    fn visit<E>(
        node: Node<'_>,
        context: EcmascriptNameContext,
        original: &str,
        projected: &mut str,
        depth: usize,
        restored: &mut bool,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<usize>, E> {
        check()?;
        if depth >= crate::MAX_TREE_SITTER_AST_DEPTH || node.is_error() || node.is_missing() {
            return Ok(None);
        }
        if node.kind() == "arrow_function" && !arrow_line_terminators_valid(node, original, check)?
        {
            return Ok(None);
        }
        if node.child_count() == 0 {
            let Some(text) = original.get(node.byte_range()) else {
                return Ok(None);
            };
            let Some(candidate) = projected.get(node.byte_range()) else {
                return Ok(None);
            };
            if text == candidate {
                return Ok(Some(0));
            }
            if !matches!(text, "await" | "yield" | "interface")
                || !matches!(
                    node.kind(),
                    "identifier"
                        | "type_identifier"
                        | "property_identifier"
                        | "shorthand_property_identifier"
                        | "shorthand_property_identifier_pattern"
                )
                || text.as_bytes()[..text.len() - 1] != candidate.as_bytes()[..candidate.len() - 1]
                || text.as_bytes()[text.len() - 1].to_ascii_uppercase()
                    != candidate.as_bytes()[candidate.len() - 1]
            {
                return Ok(None);
            }
            let allowed = if node.kind() == "property_identifier" {
                super::ecmascript_identifier::identifier_name_value(text).is_some()
            } else {
                super::ecmascript_identifier::contextual_identifier_value(
                    text,
                    context,
                    super::EcmascriptNameRole::Reference,
                )
                .is_some()
            };
            if !allowed {
                if let Some(last) = projected.get_mut(node.end_byte() - 1..node.end_byte()) {
                    last.make_ascii_lowercase();
                }
                *restored = true;
                return Ok(Some(0));
            }
            return Ok(Some(1));
        }
        let callable = if is_callable(node) {
            Some(callable_contexts(node, context, original, check)?)
        } else {
            None
        };
        let mut count = 0;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let Some(admitted) = visit(
                child,
                child_context(node, child, context, callable),
                original,
                projected,
                depth + 1,
                restored,
                check,
            )?
            else {
                return Ok(None);
            };
            count += admitted;
        }
        Ok(Some(count))
    }
    let context = source_context(root, path, role, original, check)?;
    let mut restored = false;
    let Some(admitted) = visit(root, context, original, projected, 0, &mut restored, check)? else {
        return Ok(ContextualProjectionValidation::Rejected);
    };
    let (Some(before), Some(after)) = (
        original.get(span.start..span.end),
        projected.get(span.start..span.end),
    ) else {
        return Ok(ContextualProjectionValidation::Rejected);
    };
    let mut changed = 0;
    for (index, (before, after)) in before.bytes().zip(after.bytes()).enumerate() {
        crate::check_parser_iteration(index, check)?;
        changed += usize::from(before != after);
    }
    Ok(if changed != admitted {
        ContextualProjectionValidation::Rejected
    } else if restored {
        ContextualProjectionValidation::Restored
    } else {
        ContextualProjectionValidation::Valid
    })
}

/// Enforce arrow no-line-terminator boundaries absent from tolerant AST shape checks.
fn arrow_line_terminators_valid<E>(
    node: Node<'_>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    fn gap<E>(
        content: &str,
        start: usize,
        end: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        let Some(text) = content.get(start..end) else {
            return Ok(false);
        };
        for (index, value) in text.chars().enumerate() {
            crate::check_parser_iteration(index, check)?;
            if matches!(value, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                return Ok(false);
            }
        }
        Ok(true)
    }
    let Some(parameters) = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("parameter"))
    else {
        return Ok(false);
    };
    let mut cursor = node.walk();
    let Some(arrow) = node
        .children(&mut cursor)
        .find(|child| child.kind() == "=>")
    else {
        return Ok(false);
    };
    let head = node
        .child_by_field_name("return_type")
        .unwrap_or(parameters);
    if !gap(content, head.end_byte(), arrow.start_byte(), check)? {
        return Ok(false);
    }
    let mut cursor = node.walk();
    if let Some(asynchronous) = node
        .children(&mut cursor)
        .find(|child| child.kind() == "async")
    {
        let first = node
            .child_by_field_name("type_parameters")
            .unwrap_or(parameters);
        return gap(content, asynchronous.end_byte(), first.start_byte(), check);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        ContextualProjectionValidation, contains_contextual_keyword,
        project_contextual_identifiers, source_context, validate_contextual_projection,
    };
    use std::convert::Infallible;

    #[test]
    fn contextual_projection_is_lazy_and_preserves_non_identifier_syntax() {
        for (content, expected) in [
            (
                "// await yield interface\nconst text='await yield interface';",
                None,
            ),
            ("interface Shape{value:string}", None),
            ("function* generate(){yield 1;}", None),
            ("async function run(){await value;}", None),
            ("export {}; await value;", None),
            ("class C{static{await value;}}", None),
            ("function await(){} await(", None),
            (
                "function yield(){} yield();",
                Some("function yield(){} yielD();"),
            ),
            (
                "'use strict'; function await(){} await();",
                Some("'use strict'; function await(){} awaiT();"),
            ),
            (
                "function await(){} await(await());",
                Some("function await(){} awaiT(awaiT());"),
            ),
            (
                "function interface(){} interface();",
                Some("function interface(){} interfacE();"),
            ),
        ] {
            let Some(grammar) = crate::tree_sitter_language("typescript") else {
                return;
            };
            let mut check = || Ok::<(), Infallible>(());
            let parsed = crate::parse_tree_sitter_language(&grammar, content, &mut check);
            assert!(parsed.is_ok());
            let Ok(Some(tree)) = parsed else { return };
            let context = source_context(tree.root_node(), "main.ts", None, content, &mut check);
            assert!(context.is_ok());
            let Ok(context) = context;
            let mut projected = None;
            let repaired = project_contextual_identifiers(
                tree.root_node(),
                context,
                content,
                crate::SourceByteSpan {
                    start: 0,
                    end: content.len(),
                },
                &mut projected,
                &mut check,
            );
            assert_eq!(repaired, Ok(expected.is_some()), "{content}");
            assert_eq!(projected.as_deref(), expected, "{content}");
            if let Some(projected) = projected {
                assert_eq!(projected.len(), content.len());
            }
        }
    }

    #[test]
    fn contextual_prefilter_preserves_chunk_boundaries_and_observes_cancellation() {
        for token in ["yield", "await", "interface"] {
            for offset in 0..token.len() {
                let content = format!("{}{token}é", " ".repeat(65_536 - offset));
                assert_eq!(
                    contains_contextual_keyword(&content, &mut || Ok::<(), ()>(())),
                    Ok(true)
                );
            }
        }
        let content = " ".repeat(200_000);
        let mut checks = 0;
        let result = contains_contextual_keyword(&content, &mut || {
            checks += 1;
            if checks == 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err("cancelled"));
    }

    #[test]
    fn candidate_context_restores_operators_and_rejects_unmatched_rewrites() {
        for (original, candidate, expected) in [
            (
                "async function caller(){await(work());}",
                "async function caller(){awaiT(work());}",
                ContextualProjectionValidation::Restored,
            ),
            (
                "function* caller(){yield(work());}",
                "function* caller(){yielD(work());}",
                ContextualProjectionValidation::Restored,
            ),
            (
                "export {}; await(work());",
                "export {}; awaiT(work());",
                ContextualProjectionValidation::Restored,
            ),
            (
                "'await';",
                "'awaiT';",
                ContextualProjectionValidation::Rejected,
            ),
            (
                "// await",
                "// awaiT",
                ContextualProjectionValidation::Rejected,
            ),
        ] {
            let Some(grammar) = crate::tree_sitter_language("typescript") else {
                return;
            };
            let mut check = || Ok::<(), Infallible>(());
            let tree = crate::parse_tree_sitter_language(&grammar, candidate, &mut check);
            assert!(matches!(tree, Ok(Some(_))));
            let Ok(Some(tree)) = tree else { return };
            let mut projected = candidate.to_string();
            let result = validate_contextual_projection(
                tree.root_node(),
                "main.ts",
                None,
                crate::SourceByteSpan {
                    start: 0,
                    end: original.len(),
                },
                original,
                &mut projected,
                &mut check,
            );
            assert!(matches!(result, Ok(value) if value == expected));
            if expected == ContextualProjectionValidation::Restored {
                assert_eq!(projected, original);
            }
        }
    }
}
