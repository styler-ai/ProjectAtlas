//! Purpose: Extract tree-sitter-backed `ProjectAtlas` symbol graphs.

mod languages;

use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use regex::Regex;
use std::borrow::Cow;
use toml::Value as TomlValue;
use tree_sitter::{Language, Node, Parser};

/// Maximum symbols kept from one file to bound large generated sources.
const MAX_SYMBOLS_PER_FILE: usize = 4_000;
/// Maximum relations kept from one file to bound call-heavy sources.
const MAX_RELATIONS_PER_FILE: usize = 8_000;
/// Maximum text length stored for signatures and relation context.
const MAX_SNIPPET_CHARS: usize = 240;
/// Maximum text length stored for extracted documentation.
const MAX_DOC_CHARS: usize = 500;

/// Extract a symbol graph from source or manifest content.
#[must_use]
pub fn extract_symbol_graph(path: &str, language: Option<&str>, content: &str) -> SymbolGraph {
    if is_cargo_manifest(path, language) {
        return extract_cargo_manifest_graph(path, language, content);
    }
    let parse_content = content_without_leading_purpose_header(content);
    if let Some(graph) = extract_tree_sitter_graph(path, language, parse_content.as_ref()) {
        return graph;
    }
    extract_fallback_graph(path, language, parse_content.as_ref())
}

/// Return whether the language has a specialized tree-sitter parser.
#[must_use]
pub fn has_specialized_parser(language: &str) -> bool {
    tree_sitter_language(language).is_some()
}

/// Return all specialized parser language identifiers.
#[must_use]
pub fn specialized_languages() -> &'static [&'static str] {
    &[
        "rust",
        "rust-build-script",
        "python",
        "javascript",
        "typescript",
        "tsx",
        "java",
        "kotlin",
        "csharp",
        "go",
        "objective-c",
        "zig",
        "c",
        "cpp",
        "h",
        "hpp",
    ]
}

/// Return whether a file is a Cargo manifest or lockfile.
fn is_cargo_manifest(path: &str, language: Option<&str>) -> bool {
    path.ends_with("Cargo.toml")
        || path.ends_with("Cargo.lock")
        || matches!(language, Some("cargo-manifest" | "cargo-lock"))
}

/// Extract Cargo package, workspace, and dependency entries.
fn extract_cargo_manifest_graph(path: &str, language: Option<&str>, content: &str) -> SymbolGraph {
    let mut graph = empty_graph(path, language, ParserKind::Manifest);
    if path.ends_with("Cargo.lock") || matches!(language, Some("cargo-lock")) {
        extract_cargo_lock_packages(&mut graph, content);
        return graph;
    }
    extract_cargo_toml_entries(&mut graph, content);
    graph
}

/// Extract package names from Cargo.lock.
fn extract_cargo_lock_packages(graph: &mut SymbolGraph, content: &str) {
    let Ok(lockfile) = content.parse::<TomlValue>() else {
        return;
    };
    let Some(packages) = lockfile.get("package").and_then(TomlValue::as_array) else {
        return;
    };
    for package in packages {
        let Some(name) = package
            .as_table()
            .and_then(|table| table.get("name"))
            .and_then(TomlValue::as_str)
        else {
            continue;
        };
        let line = cargo_lock_name_line(content, name).unwrap_or(1);
        push_symbol(
            graph,
            name,
            SymbolKind::Dependency,
            line,
            line,
            None,
            Some("cargo-lock-package"),
            &format!("lock package {name}"),
        );
    }
}

/// Return the one-based source line for a package name in a Cargo.lock file.
fn cargo_lock_name_line(content: &str, package_name: &str) -> Option<usize> {
    let mut in_package = false;
    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line == "[[package]]" {
            in_package = true;
            continue;
        }
        if line.starts_with('[') {
            in_package = false;
        }
        if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
            && value.trim().trim_matches('"') == package_name
        {
            return Some(index + 1);
        }
    }
    None
}

/// Extract package, workspace, and dependencies from Cargo.toml.
fn extract_cargo_toml_entries(graph: &mut SymbolGraph, content: &str) {
    let Ok(manifest) = content.parse::<TomlValue>() else {
        return;
    };
    let Some(root) = manifest.as_table() else {
        return;
    };
    let line_index = CargoTomlLineIndex::new(content);
    if root.contains_key("workspace") {
        let line = line_index.section_line("workspace").unwrap_or(1);
        push_symbol(
            graph,
            "workspace",
            SymbolKind::Workspace,
            line,
            line,
            None,
            Some("cargo-workspace"),
            line_index.line_text(line).unwrap_or("[workspace]"),
        );
    }
    if let Some(package) = root.get("package").and_then(TomlValue::as_table)
        && let Some(name) = package.get("name").and_then(TomlValue::as_str)
    {
        let line = line_index.key_line("package", "name").unwrap_or(1);
        push_symbol(
            graph,
            name,
            SymbolKind::Package,
            line,
            line,
            None,
            Some("cargo-package"),
            line_index.line_text(line).unwrap_or(name),
        );
    }
    collect_cargo_dependencies(graph, &line_index, &[], root);
}

/// Recursively collect dependency tables from parsed Cargo TOML.
fn collect_cargo_dependencies(
    graph: &mut SymbolGraph,
    line_index: &CargoTomlLineIndex,
    path: &[String],
    table: &toml::map::Map<String, TomlValue>,
) {
    let section = path.join(".");
    if is_dependency_table_path(path) {
        for (name, value) in table {
            let line = line_index
                .key_line(&section, name)
                .or_else(|| line_index.section_line(&section))
                .unwrap_or(1);
            let detail = line_index
                .line_text(line)
                .map_or_else(|| name.as_str(), str::trim);
            let dependency_name = manifest_dependency_name(name, value);
            push_symbol(
                graph,
                &dependency_name,
                SymbolKind::Dependency,
                line,
                line,
                Some(section.clone()),
                Some("cargo-dependency"),
                detail,
            );
            push_relation(
                graph,
                "cargo",
                &dependency_name,
                RelationKind::DependsOn,
                line,
                detail,
            );
        }
        return;
    }
    for (key, value) in table {
        let Some(child) = value.as_table() else {
            continue;
        };
        let mut child_path = path.to_owned();
        child_path.push(key.clone());
        collect_cargo_dependencies(graph, line_index, &child_path, child);
    }
}

/// Return whether a parsed TOML table path declares dependencies.
fn is_dependency_table_path(path: &[String]) -> bool {
    path.last().is_some_and(|last| {
        last == "dependencies" || last == "dev-dependencies" || last == "build-dependencies"
    })
}

/// Return the Cargo dependency package name for normal or renamed dependencies.
fn manifest_dependency_name(key: &str, value: &TomlValue) -> String {
    value
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(TomlValue::as_str)
        .unwrap_or(key)
        .to_string()
}

/// Source-line lookup for parsed Cargo TOML entries.
struct CargoTomlLineIndex<'a> {
    /// Original lines.
    lines: Vec<&'a str>,
    /// Section declaration lines keyed by dotted path.
    sections: std::collections::HashMap<String, usize>,
    /// Key declaration lines keyed by dotted section and key name.
    keys: std::collections::HashMap<(String, String), usize>,
}

impl<'a> CargoTomlLineIndex<'a> {
    /// Build a line index for TOML source positions.
    fn new(content: &'a str) -> Self {
        let lines = content.lines().collect::<Vec<_>>();
        let mut sections = std::collections::HashMap::new();
        let mut keys = std::collections::HashMap::new();
        let mut current_section = String::new();
        for (index, raw_line) in lines.iter().enumerate() {
            let line_number = index + 1;
            let line = raw_line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                current_section = normalize_toml_section(line.trim_matches(&['[', ']'][..]).trim());
                sections.insert(current_section.clone(), line_number);
                continue;
            }
            let Some((key, _value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().trim_matches('"').to_string();
            if !key.is_empty() {
                keys.insert((current_section.clone(), key), line_number);
            }
        }
        Self {
            lines,
            sections,
            keys,
        }
    }

    /// Return the source line for a section declaration.
    fn section_line(&self, section: &str) -> Option<usize> {
        self.sections.get(section).copied()
    }

    /// Return the source line for a key in a section.
    fn key_line(&self, section: &str, key: &str) -> Option<usize> {
        self.keys
            .get(&(section.to_string(), key.to_string()))
            .copied()
    }

    /// Return source text for a one-based line number.
    fn line_text(&self, line: usize) -> Option<&'a str> {
        self.lines.get(line.checked_sub(1)?).copied()
    }
}

/// Normalize quoted TOML section components into a dotted lookup key.
fn normalize_toml_section(section: &str) -> String {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in section.chars() {
        match (character, quote) {
            ('"' | '\'', None) => quote = Some(character),
            (value, Some(active)) if value == active => quote = None,
            ('.', None) => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            (value, _) => current.push(value),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts.join(".")
}

/// Extract a graph through tree-sitter when the language has a grammar.
fn extract_tree_sitter_graph(
    path: &str,
    language: Option<&str>,
    content: &str,
) -> Option<SymbolGraph> {
    let language_name = language?;
    let parser_language = tree_sitter_language(language_name)?;
    let mut parser = Parser::new();
    if parser.set_language(&parser_language).is_err() {
        return None;
    }
    let tree = parser.parse(content, None)?;
    let mut graph = empty_graph(path, language, ParserKind::TreeSitter);
    visit_node(tree.root_node(), content, &mut graph);
    languages::augment_language_graph(&mut graph, content);
    if graph.symbols.is_empty() && graph.relations.is_empty() {
        None
    } else {
        Some(graph)
    }
}

/// Return a tree-sitter language for supported source families.
fn tree_sitter_language(language: &str) -> Option<Language> {
    match language {
        "rust" | "rust-build-script" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "kotlin" => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
        "csharp" => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "objective-c" => Some(tree_sitter_objc::LANGUAGE.into()),
        "zig" => Some(tree_sitter_zig::LANGUAGE.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" | "hpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
        _ => None,
    }
}

/// Recursively inspect one tree-sitter node.
fn visit_node(node: Node<'_>, content: &str, graph: &mut SymbolGraph) {
    if graph.symbols.len() < MAX_SYMBOLS_PER_FILE
        && let Some(kind) = declaration_kind(node.kind())
        && should_emit_declaration_symbol(node, content)
    {
        push_tree_symbol(graph, node, content, effective_declaration_kind(node, kind));
    }
    if graph.relations.len() < MAX_RELATIONS_PER_FILE {
        if is_import_node(node.kind()) {
            push_import_relation(graph, node, content);
        } else if is_call_node(node.kind()) {
            push_call_relation(graph, node, content);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_node(child, content, graph);
    }
}

/// Refine a declaration kind using surrounding syntax context.
fn effective_declaration_kind(node: Node<'_>, kind: SymbolKind) -> SymbolKind {
    if kind == SymbolKind::Function && declaration_is_method_context(node) {
        return SymbolKind::Method;
    }
    if kind == SymbolKind::Value
        && !is_local_value_declaration(node)
        && declaration_has_direct_callable_initializer(node)
    {
        return SymbolKind::Function;
    }
    if kind == SymbolKind::Type {
        if has_descendant_kind(node, &["struct_type"]) {
            return SymbolKind::Struct;
        }
        if has_descendant_kind(node, &["interface_type"]) {
            return SymbolKind::Interface;
        }
    }
    kind
}

/// Return whether a function-like declaration belongs to an enclosing type.
fn declaration_is_method_context(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_item" | "function_definition" | "function_declaration" | "function_declarator"
    ) && (has_ancestor_kind(node.parent(), "impl_item")
        || has_ancestor_kind(node.parent(), "class_definition")
        || has_ancestor_kind(node.parent(), "class_declaration")
        || has_ancestor_kind(node.parent(), "class_body")
        || has_ancestor_kind(node.parent(), "class_specifier")
        || has_ancestor_kind(node.parent(), "struct_specifier")
        || has_ancestor_kind(node.parent(), "interface_declaration"))
}

/// Return whether this declaration node should become its own symbol row.
fn should_emit_declaration_symbol(node: Node<'_>, content: &str) -> bool {
    if is_object_literal_method(node) {
        return object_literal_method_owner(node, content).is_some_and(|owner| owner.exported);
    }
    if node.kind() == "field_declaration"
        && has_descendant_kind(node, &["function_declarator", "method_declarator"])
    {
        return false;
    }
    if matches!(node.kind(), "function_declarator" | "method_declarator") {
        if is_type_member_declarator(node) {
            return true;
        }
        return !has_declaration_ancestor(node.parent());
    }
    true
}

/// Return whether a C/C++ declarator is a type member prototype.
fn is_type_member_declarator(node: Node<'_>) -> bool {
    has_ancestor_kind(node.parent(), "field_declaration")
        && (has_ancestor_kind(node.parent(), "class_specifier")
            || has_ancestor_kind(node.parent(), "struct_specifier"))
        && !has_ancestor_kind(node.parent(), "function_definition")
}

/// Return whether a parent chain already has a declaration symbol owner.
fn has_declaration_ancestor(mut node: Option<Node<'_>>) -> bool {
    while let Some(current) = node {
        if declaration_kind(current.kind()).is_some() {
            return true;
        }
        node = current.parent();
    }
    false
}

/// Return whether a value declaration initializes directly to a callable value.
fn declaration_has_direct_callable_initializer(node: Node<'_>) -> bool {
    if !matches!(
        node.kind(),
        "lexical_declaration" | "variable_declaration" | "variable_statement" | "var_declaration"
    ) {
        return false;
    }
    first_variable_initializer(node).is_some_and(|initializer| {
        matches!(
            initializer.kind(),
            "arrow_function"
                | "function"
                | "function_expression"
                | "generator_function"
                | "lambda_expression"
        )
    })
}

/// Return the initializer node for the first variable declarator in a statement.
fn first_variable_initializer(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "variable_declarator" | "variable_declaration") {
        return node.child_by_field_name("value");
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "variable_declarator" | "variable_declaration")
            && let Some(value) = child.child_by_field_name("value")
        {
            return Some(value);
        }
    }
    None
}

/// Return whether a declaration is a local binding inside a callable body.
fn is_local_value_declaration(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "lexical_declaration" | "variable_declaration" | "variable_statement" | "var_declaration"
    ) && has_ancestor_kind_any(
        node.parent(),
        &[
            "arrow_function",
            "function",
            "function_expression",
            "function_declaration",
            "generator_function",
            "method_definition",
            "method_declaration",
            "function_item",
            "function_definition",
            "function_declaration_with_receiver",
            "func_literal",
        ],
    )
}

/// Return whether a method declaration belongs to an object literal, not a type.
fn is_object_literal_method(node: Node<'_>) -> bool {
    node.kind() == "method_definition"
        && has_ancestor_kind_any(node.parent(), &["object", "object_pattern", "pair"])
}

/// Parent object metadata for a JavaScript object-literal method.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObjectLiteralMethodOwner {
    /// Object or export-assignment name that owns the method.
    name: String,
    /// Whether the owning object is part of the module API.
    exported: bool,
}

/// Return the owner of an object-literal method when it is useful to index.
fn object_literal_method_owner(
    method_node: Node<'_>,
    content: &str,
) -> Option<ObjectLiteralMethodOwner> {
    if !is_object_literal_method(method_node) {
        return None;
    }
    let object = nearest_ancestor_kind(method_node.parent(), "object")?;
    object_literal_owner(object, content)
}

/// Return the declaration or assignment that owns an object literal.
fn object_literal_owner(object: Node<'_>, content: &str) -> Option<ObjectLiteralMethodOwner> {
    let parent = object.parent()?;
    match parent.kind() {
        "variable_declarator" | "variable_declaration" => {
            let name = declarator_name(parent, content)?;
            Some(ObjectLiteralMethodOwner {
                name,
                exported: is_directly_exported_declaration(parent),
            })
        }
        "assignment_expression" | "augmented_assignment_expression" => {
            let target = parent
                .child_by_field_name("left")
                .or_else(|| first_named_child(parent))?;
            let name = compact_text(node_text(target, content).as_deref().unwrap_or(""));
            if name.is_empty() {
                return None;
            }
            let exported = name == "module.exports"
                || name.starts_with("module.exports.")
                || name == "exports"
                || name.starts_with("exports.");
            Some(ObjectLiteralMethodOwner { name, exported })
        }
        "export_statement" => Some(ObjectLiteralMethodOwner {
            name: "default".to_string(),
            exported: true,
        }),
        "pair" => {
            let property = parent
                .child_by_field_name("key")
                .and_then(|key| named_text(key, content))
                .unwrap_or_else(|| "object".to_string());
            let outer = nearest_ancestor_kind(parent.parent(), "object")
                .and_then(|outer| object_literal_owner(outer, content));
            outer.map(|owner| ObjectLiteralMethodOwner {
                name: format!("{}.{}", owner.name, property),
                exported: owner.exported,
            })
        }
        _ => None,
    }
}

/// Return whether a declaration statement is directly wrapped in an export.
fn is_directly_exported_declaration(node: Node<'_>) -> bool {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if has_direct_export_parent(candidate) {
            return true;
        }
        if matches!(
            candidate.kind(),
            "lexical_declaration" | "variable_declaration" | "variable_statement"
        ) {
            return false;
        }
        current = candidate.parent();
    }
    false
}

/// Return whether a node has an ancestor of the given tree-sitter kind.
fn has_ancestor_kind(mut node: Option<Node<'_>>, kind: &str) -> bool {
    while let Some(current) = node {
        if current.kind() == kind {
            return true;
        }
        node = current.parent();
    }
    false
}

/// Return whether a node has any ancestor with one of the given tree-sitter kinds.
fn has_ancestor_kind_any(mut node: Option<Node<'_>>, kinds: &[&str]) -> bool {
    while let Some(current) = node {
        if kinds.contains(&current.kind()) {
            return true;
        }
        node = current.parent();
    }
    false
}

/// Return the nearest ancestor with the requested tree-sitter kind.
fn nearest_ancestor_kind<'tree>(mut node: Option<Node<'tree>>, kind: &str) -> Option<Node<'tree>> {
    while let Some(current) = node {
        if current.kind() == kind {
            return Some(current);
        }
        node = current.parent();
    }
    None
}

/// Push a declaration symbol from a tree-sitter node.
fn push_tree_symbol(
    graph: &mut SymbolGraph,
    node: Node<'_>,
    content: &str,
    symbol_kind: SymbolKind,
) {
    let name = node_name(node, content)
        .unwrap_or_else(|| compact_text(node_text(node, content).as_deref().unwrap_or("")));
    if name.is_empty() {
        return;
    }
    let signature = declaration_signature(node, content);
    let parent = symbol_parent(node, content);
    let exported = has_direct_export_parent(node)
        || object_literal_method_owner(node, content).is_some_and(|owner| owner.exported)
        || is_exported_symbol(graph.language.as_deref(), &name, &signature);
    let documentation = symbol_documentation(node, content);
    push_symbol_with_metadata(
        graph,
        &name,
        symbol_kind,
        node.start_position().row + 1,
        node.end_position().row + 1,
        parent.clone(),
        Some(node.kind()),
        &signature,
        exported,
        documentation.as_deref(),
    );
    if let Some(parent_name) = parent {
        push_relation(
            graph,
            &parent_name,
            &name,
            RelationKind::Contains,
            node.start_position().row + 1,
            node.kind(),
        );
    }
}

/// Return whether a declaration is directly wrapped by a JavaScript-like export.
fn has_direct_export_parent(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "export_statement")
}

/// Return source content with a leading `ProjectAtlas` `Purpose:` header blanked.
fn content_without_leading_purpose_header(content: &str) -> Cow<'_, str> {
    let Some(start) = content.find(|character: char| !character.is_whitespace()) else {
        return Cow::Borrowed(content);
    };
    let rest = &content[start..];
    if let Some(end) = leading_purpose_block_end(rest) {
        return Cow::Owned(blank_prefix_preserving_newlines(content, start + end));
    }
    if let Some(end) = leading_purpose_line_end(rest) {
        return Cow::Owned(blank_prefix_preserving_newlines(content, start + end));
    }
    Cow::Borrowed(content)
}

/// Return the byte end of a leading block comment when it is a purpose header.
fn leading_purpose_block_end(rest: &str) -> Option<usize> {
    if !(rest.starts_with("/**") || rest.starts_with("/*")) {
        return None;
    }
    let end = rest.find("*/")? + "*/".len();
    let documentation = rest[..end]
        .lines()
        .filter_map(|line| clean_doc_comment_line(line.trim()))
        .collect::<Vec<_>>()
        .join(" ");
    compact_documentation(&documentation)
        .is_some_and(|value| value.starts_with("Purpose:"))
        .then_some(end)
}

/// Return the byte end of a leading line comment when it is a purpose header.
fn leading_purpose_line_end(rest: &str) -> Option<usize> {
    let line_end = rest.find('\n').map_or(rest.len(), |index| index + 1);
    let line = rest[..line_end].trim();
    let cleaned = line
        .strip_prefix("//")
        .or_else(|| line.strip_prefix('#'))
        .or_else(|| {
            line.strip_prefix("<!--")
                .and_then(|value| value.strip_suffix("-->"))
        })?
        .trim();
    cleaned.starts_with("Purpose:").then_some(line_end)
}

/// Blank a source prefix without changing line numbers.
fn blank_prefix_preserving_newlines(content: &str, end: usize) -> String {
    let mut output = String::with_capacity(content.len());
    for (index, character) in content.char_indices() {
        if index < end && !matches!(character, '\n' | '\r') {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

/// Return the semantic parent for a declaration symbol.
fn symbol_parent(node: Node<'_>, content: &str) -> Option<String> {
    if let Some(owner) = object_literal_method_owner(node, content) {
        return Some(owner.name);
    }
    if node.kind() == "function_item"
        && let Some(impl_node) = nearest_ancestor_kind(node.parent(), "impl_item")
    {
        return impl_type_name(impl_node, content);
    }
    if matches!(node.kind(), "function_declarator" | "method_declarator")
        && let Some(type_node) = nearest_ancestor_kind(node.parent(), "class_specifier")
            .or_else(|| nearest_ancestor_kind(node.parent(), "struct_specifier"))
    {
        return node_name(type_node, content);
    }
    enclosing_symbol_name(node.parent(), content)
}

/// Map tree-sitter node kinds to `ProjectAtlas` symbol kinds.
fn declaration_kind(kind: &str) -> Option<SymbolKind> {
    match kind {
        "function_item"
        | "function_declaration"
        | "function_definition"
        | "function_declarator"
        | "func_literal" => Some(SymbolKind::Function),
        "method_definition"
        | "method_declarator"
        | "method_declaration"
        | "function_declaration_with_receiver"
        | "constructor_declaration"
        | "init_declaration" => Some(SymbolKind::Method),
        "class_declaration"
        | "class_definition"
        | "class_specifier"
        | "class_interface"
        | "class_implementation" => Some(SymbolKind::Class),
        "struct_item" | "struct_specifier" | "struct_declaration" => Some(SymbolKind::Struct),
        "enum_item" | "enum_declaration" | "enum_specifier" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "interface_declaration" | "interface_type" => Some(SymbolKind::Interface),
        "mod_item"
        | "module_declaration"
        | "namespace_declaration"
        | "file_scoped_namespace_declaration"
        | "package_declaration"
        | "package_clause"
        | "package_header" => Some(SymbolKind::Module),
        "type_item" | "type_alias_declaration" | "type_declaration" => Some(SymbolKind::Type),
        "const_item"
        | "static_item"
        | "const_declaration"
        | "field_declaration"
        | "lexical_declaration"
        | "var_declaration"
        | "short_var_declaration" => Some(SymbolKind::Value),
        "use_declaration"
        | "import_statement"
        | "import_declaration"
        | "import_from_statement"
        | "using_directive"
        | "preproc_include" => Some(SymbolKind::Import),
        _ => None,
    }
}

/// Return whether a node is an import-like relation.
fn is_import_node(kind: &str) -> bool {
    matches!(
        kind,
        "use_declaration"
            | "import_statement"
            | "import_declaration"
            | "import_from_statement"
            | "using_directive"
            | "preproc_include"
    )
}

/// Return whether a node is a call-like relation.
fn is_call_node(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression"
            | "method_invocation"
            | "invocation_expression"
            | "call"
            | "macro_invocation"
    )
}

/// Return whether a subtree contains any node with one of the given kinds.
fn has_descendant_kind(node: Node<'_>, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if kinds.contains(&child.kind()) || has_descendant_kind(child, kinds) {
            return true;
        }
    }
    false
}

/// Push an import relation from an import node.
fn push_import_relation(graph: &mut SymbolGraph, node: Node<'_>, content: &str) {
    let import_text = compact_text(node_text(node, content).as_deref().unwrap_or(""));
    if import_text.is_empty() {
        return;
    }
    push_relation(
        graph,
        "<module>",
        &import_text,
        RelationKind::Imports,
        node.start_position().row + 1,
        &import_text,
    );
}

/// Push a call relation from a call node.
fn push_call_relation(graph: &mut SymbolGraph, node: Node<'_>, content: &str) {
    let target_node = node
        .child_by_field_name("function")
        .or_else(|| first_named_child(node));
    let Some(target_node) = target_node else {
        return;
    };
    let target = compact_text(node_text(target_node, content).as_deref().unwrap_or(""));
    if target.is_empty() || target.len() > MAX_SNIPPET_CHARS {
        return;
    }
    let source = enclosing_symbol_name(node.parent(), content).unwrap_or_else(|| "<module>".into());
    let context = compact_text(node_text(node, content).as_deref().unwrap_or(""));
    push_relation(
        graph,
        &source,
        &target,
        RelationKind::Calls,
        node.start_position().row + 1,
        &context,
    );
}

/// Return the first named child of a node.
fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

/// Extract a human-readable symbol name from common tree-sitter fields.
fn node_name(node: Node<'_>, content: &str) -> Option<String> {
    if let Some(name) = declaration_specific_name(node, content) {
        return Some(name);
    }
    if let Some(declarator) = node.child_by_field_name("declarator")
        && let Some(name) = declarator_name(declarator, content)
    {
        return Some(name);
    }
    for field_name in ["name", "field", "property", "type", "path"] {
        if let Some(child) = node.child_by_field_name(field_name)
            && let Some(name) = named_text(child, content)
        {
            return Some(name);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "identifier" | "type_identifier" | "property_identifier" | "field_identifier"
        ) && let Some(name) = named_text(child, content)
        {
            return Some(name);
        }
    }
    None
}

/// Extract names that need language-specific cleanup from a declaration node.
fn declaration_specific_name(node: Node<'_>, content: &str) -> Option<String> {
    match node.kind() {
        "package_declaration" | "package_clause" | "package_header" => {
            prefixed_declaration_name(node, content, &["package"])
        }
        "namespace_declaration" | "file_scoped_namespace_declaration" => {
            prefixed_declaration_name(node, content, &["namespace"])
        }
        "module_declaration" => {
            prefixed_declaration_name(node, content, &["module", "declare module"])
        }
        "type_declaration" => keyword_identifier_name(node, content, "type"),
        "lexical_declaration"
        | "variable_declaration"
        | "variable_statement"
        | "var_declaration" => first_variable_declarator_name(node, content),
        _ => None,
    }
}

/// Extract a declaration name by removing a language keyword prefix.
fn prefixed_declaration_name(node: Node<'_>, content: &str, prefixes: &[&str]) -> Option<String> {
    let text = compact_text(&node_text(node, content)?);
    for prefix in prefixes {
        let Some(rest) = text.strip_prefix(prefix) else {
            continue;
        };
        let name = rest
            .trim()
            .trim_matches('"')
            .trim_end_matches(';')
            .trim_end_matches('{')
            .trim()
            .to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Extract the first identifier after a declaration keyword.
fn keyword_identifier_name(node: Node<'_>, content: &str, keyword: &str) -> Option<String> {
    let text = compact_text(&node_text(node, content)?);
    let rest = text.strip_prefix(keyword)?.trim();
    rest.split_whitespace()
        .next()
        .map(|name| name.trim_matches(';').to_string())
        .filter(|name| !name.is_empty())
}

/// Extract the implemented Rust type name from an `impl` block.
fn impl_type_name(node: Node<'_>, content: &str) -> Option<String> {
    if let Some(type_node) = node.child_by_field_name("type")
        && let Some(name) = named_text(type_node, content)
    {
        return Some(clean_type_name(&name));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "type_identifier" | "scoped_type_identifier" | "generic_type" | "identifier"
        ) && let Some(name) = named_text(child, content)
        {
            return Some(clean_type_name(&name));
        }
    }
    None
}

/// Remove Rust type adornments from a parent type name.
fn clean_type_name(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .split(['<', ' ', '{'])
        .next()
        .unwrap_or(value)
        .trim()
        .to_string()
}

/// Return the first declared variable name in a declaration statement.
fn first_variable_declarator_name(node: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "variable_declarator" | "variable_declaration" | "identifier"
        ) && let Some(name) = declarator_name(child, content)
        {
            return Some(name);
        }
    }
    None
}

/// Extract the declared name from a declarator subtree.
fn declarator_name(node: Node<'_>, content: &str) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name")
        && let Some(name) = named_text(name_node, content)
    {
        return Some(strip_declarator_noise(&name));
    }
    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "property_identifier" | "field_identifier"
    ) && let Some(name) = named_text(node, content)
    {
        return Some(strip_declarator_noise(&name));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(name) = declarator_name(child, content) {
            return Some(name);
        }
    }
    None
}

/// Remove initializer or parameter text accidentally captured with a declarator.
fn strip_declarator_noise(value: &str) -> String {
    value
        .split(['=', '(', ':'])
        .next()
        .unwrap_or(value)
        .trim()
        .to_string()
}

/// Return compact text for a likely name node.
fn named_text(node: Node<'_>, content: &str) -> Option<String> {
    let text = node_text(node, content)?;
    let compact = compact_text(&text);
    if compact.is_empty() {
        None
    } else {
        Some(compact)
    }
}

/// Build a compact declaration signature for a node.
fn declaration_signature(node: Node<'_>, content: &str) -> String {
    let raw = node_text(node, content).unwrap_or_default();
    let first_line = raw.lines().next().unwrap_or("").trim();
    compact_text(first_line)
}

/// Return whether a declaration is exported or publicly visible.
fn is_exported_symbol(language: Option<&str>, name: &str, signature: &str) -> bool {
    let trimmed = signature.trim_start();
    trimmed.starts_with("pub ")
        || trimmed.starts_with("pub(")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("public ")
        || trimmed.starts_with("open ")
        || matches!(language, Some("go")) && starts_with_uppercase(name)
}

/// Return whether a symbol name starts with an uppercase Unicode scalar.
fn starts_with_uppercase(value: &str) -> bool {
    value.chars().next().is_some_and(char::is_uppercase)
}

/// Extract documentation attached to a declaration.
fn symbol_documentation(node: Node<'_>, content: &str) -> Option<String> {
    preceding_documentation(content, node.start_position().row + 1)
        .or_else(|| leading_docstring_literal(node, content))
}

/// Extract contiguous doc-comment text immediately preceding a declaration.
fn preceding_documentation(content: &str, line_start: usize) -> Option<String> {
    let lines = content.lines().collect::<Vec<_>>();
    if line_start <= 1 || lines.is_empty() {
        return None;
    }
    let mut index = line_start.saturating_sub(2);
    let mut collected = Vec::new();
    let mut saw_doc = false;
    loop {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            break;
        }
        if !saw_doc && is_attribute_line(trimmed) {
            if index == 0 {
                break;
            }
            index -= 1;
            continue;
        }
        if let Some(line) = clean_doc_comment_line(trimmed) {
            collected.push(line);
            saw_doc = true;
            if index == 0 {
                break;
            }
            index -= 1;
            continue;
        }
        break;
    }
    collected.reverse();
    compact_documentation(&collected.join(" "))
}

/// Return whether a line is a Rust or language attribute between docs and code.
fn is_attribute_line(trimmed: &str) -> bool {
    trimmed.starts_with("#[") || trimmed.starts_with('@')
}

/// Strip common doc-comment markers from one line.
fn clean_doc_comment_line(trimmed: &str) -> Option<String> {
    let cleaned = if let Some(rest) = trimmed.strip_prefix("///") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("/**") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("*/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix('*') {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("# ") {
        rest
    } else {
        trimmed.strip_prefix("## ")?
    }
    .trim()
    .trim_end_matches("*/")
    .trim()
    .to_string();
    Some(cleaned)
}

/// Extract a Python-style leading string literal from a declaration body.
fn leading_docstring_literal(node: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "block" | "statement_block" | "class_body" | "declaration_list"
        ) && let Some(docstring) = first_block_string_literal(child, content)
        {
            return Some(docstring);
        }
    }
    None
}

/// Return the first string literal in a declaration body when it is the body lead.
fn first_block_string_literal(block: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = block.walk();
    let first = block.named_children(&mut cursor).next()?;
    if first.kind() == "expression_statement" {
        let mut nested_cursor = first.walk();
        if let Some(string_node) = first
            .named_children(&mut nested_cursor)
            .find(|child| child.kind().contains("string"))
        {
            return clean_string_literal_doc(&node_text(string_node, content)?);
        }
    }
    if first.kind().contains("string") {
        return clean_string_literal_doc(&node_text(first, content)?);
    }
    None
}

/// Clean a source string literal into documentation text.
fn clean_string_literal_doc(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix("\"\"\"")
        .and_then(|text| text.strip_suffix("\"\"\""))
        .or_else(|| {
            trimmed
                .strip_prefix("'''")
                .and_then(|text| text.strip_suffix("'''"))
        })
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
        })
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|text| text.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    compact_documentation(unquoted)
}

/// Normalize extracted documentation into one bounded line.
fn compact_documentation(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        None
    } else {
        Some(truncate_chars(&compact, MAX_DOC_CHARS))
    }
}

/// Find the nearest containing declaration symbol name.
fn enclosing_symbol_name(mut node: Option<Node<'_>>, content: &str) -> Option<String> {
    while let Some(current) = node {
        if declaration_kind(current.kind()).is_some()
            && let Some(name) = node_name(current, content)
        {
            return Some(name);
        }
        node = current.parent();
    }
    None
}

/// Return UTF-8 text for a tree-sitter node.
fn node_text(node: Node<'_>, content: &str) -> Option<String> {
    node.utf8_text(content.as_bytes())
        .ok()
        .map(ToString::to_string)
}

/// Extract symbols through conservative declaration regexes.
fn extract_fallback_graph(path: &str, language: Option<&str>, content: &str) -> SymbolGraph {
    let mut graph = empty_graph(path, language, ParserKind::Fallback);
    let patterns = fallback_patterns();
    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        for pattern in &patterns {
            if let Some(capture) = pattern.regex.captures(trimmed)
                && let Some(name) = capture.get(1)
            {
                push_symbol(
                    &mut graph,
                    name.as_str(),
                    pattern.kind,
                    line_index + 1,
                    line_index + 1,
                    None,
                    Some(pattern.detail),
                    trimmed,
                );
                break;
            }
        }
        if is_fallback_import(trimmed) {
            push_relation(
                &mut graph,
                "<module>",
                trimmed,
                RelationKind::Imports,
                line_index + 1,
                trimmed,
            );
        }
    }
    graph
}

/// Regex plus mapped symbol kind for fallback extraction.
struct FallbackPattern {
    /// Compiled fallback regex.
    regex: Regex,
    /// Symbol kind emitted when the regex matches.
    kind: SymbolKind,
    /// Stable detail string for the fallback source.
    detail: &'static str,
}

/// Build fallback declaration regexes.
fn fallback_patterns() -> Vec<FallbackPattern> {
    let specs = [
        (
            r"^(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)",
            SymbolKind::Function,
            "fallback-python-function",
        ),
        (
            r"^class\s+([A-Za-z_][A-Za-z0-9_]*)",
            SymbolKind::Class,
            "fallback-class",
        ),
        (
            r"^(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            SymbolKind::Function,
            "fallback-js-function",
        ),
        (
            r"^(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:withDefaults\s*\(\s*)?(?:defineProps|defineEmits|defineModel|defineSlots|computed|ref|shallowRef|reactive|toRef|toRefs|watch)\b",
            SymbolKind::Value,
            "fallback-composition-binding",
        ),
        (
            r"^(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
            SymbolKind::Function,
            "fallback-rust-function",
        ),
        (
            r"^(?:pub\s+)?(?:struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)",
            SymbolKind::Type,
            "fallback-rust-type",
        ),
        (
            r"^(?:func|fun)\s+([A-Za-z_][A-Za-z0-9_]*)",
            SymbolKind::Function,
            "fallback-function",
        ),
        (
            r"^(?:public|private|protected|internal|static|\s)+\s*[A-Za-z0-9_<>,\[\]?]+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
            SymbolKind::Method,
            "fallback-c-family-method",
        ),
    ];
    let mut patterns = Vec::new();
    for (source, kind, detail) in specs {
        if let Ok(regex) = Regex::new(source) {
            patterns.push(FallbackPattern {
                regex,
                kind,
                detail,
            });
        }
    }
    patterns
}

/// Return whether a line looks import-like in fallback mode.
fn is_fallback_import(line: &str) -> bool {
    matches!(
        line.split_whitespace().next(),
        Some("import" | "from" | "use" | "using" | "include" | "require")
    ) || line.starts_with("#include")
}

/// Create an empty graph shell.
fn empty_graph(path: &str, language: Option<&str>, parser: ParserKind) -> SymbolGraph {
    SymbolGraph {
        path: path.to_string(),
        language: language.map(ToString::to_string),
        parser,
        symbols: Vec::new(),
        relations: Vec::new(),
    }
}

/// Push a symbol while enforcing per-file graph bounds.
fn push_symbol(
    graph: &mut SymbolGraph,
    name: &str,
    kind: SymbolKind,
    line_start: usize,
    line_end: usize,
    parent: Option<String>,
    detail: Option<&str>,
    signature: &str,
) {
    push_symbol_with_metadata(
        graph, name, kind, line_start, line_end, parent, detail, signature, false, None,
    );
}

/// Push a symbol with optional metadata while enforcing graph bounds.
fn push_symbol_with_metadata(
    graph: &mut SymbolGraph,
    name: &str,
    kind: SymbolKind,
    line_start: usize,
    line_end: usize,
    parent: Option<String>,
    detail: Option<&str>,
    signature: &str,
    exported: bool,
    documentation: Option<&str>,
) {
    if graph.symbols.len() >= MAX_SYMBOLS_PER_FILE {
        return;
    }
    let cleaned_name = compact_text(name);
    if cleaned_name.is_empty() {
        return;
    }
    graph.symbols.push(CodeSymbol {
        path: graph.path.clone(),
        language: graph.language.clone(),
        name: cleaned_name,
        kind,
        signature: truncate_chars_at_boundary(&compact_text(signature), MAX_SNIPPET_CHARS),
        exported,
        documentation: documentation.map(ToString::to_string),
        line_start,
        line_end: line_end.max(line_start),
        parent,
        parser: graph.parser,
        detail: detail.map(ToString::to_string),
    });
}

/// Push a relation while enforcing per-file graph bounds.
fn push_relation(
    graph: &mut SymbolGraph,
    source_name: &str,
    target_name: &str,
    kind: RelationKind,
    line: usize,
    context: &str,
) {
    if graph.relations.len() >= MAX_RELATIONS_PER_FILE {
        return;
    }
    let target = compact_text(target_name);
    if target.is_empty() {
        return;
    }
    graph.relations.push(SymbolRelation {
        path: graph.path.clone(),
        source_name: truncate_chars_at_boundary(&compact_text(source_name), MAX_SNIPPET_CHARS),
        target_name: truncate_chars_at_boundary(&target, MAX_SNIPPET_CHARS),
        kind,
        line,
        context: truncate_chars_at_boundary(&compact_text(context), MAX_SNIPPET_CHARS),
        parser: graph.parser,
    });
}

/// Compact whitespace in a parser text fragment.
fn compact_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate a string to a maximum number of Unicode scalar values.
fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

/// Truncate a long snippet at a stable syntactic boundary and mark omission.
fn truncate_chars_at_boundary(value: &str, max_chars: usize) -> String {
    let value_chars = value.chars().count();
    if value_chars <= max_chars {
        return value.to_string();
    }
    let marker = "...";
    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars {
        return value.chars().take(max_chars).collect();
    }
    let target_chars = max_chars - marker_chars;
    let mut fallback_end = 0_usize;
    let mut boundary_end = None;
    for (char_index, (index, character)) in value.char_indices().enumerate() {
        if char_index >= target_chars {
            break;
        }
        fallback_end = index + character.len_utf8();
        if is_snippet_boundary(character) {
            boundary_end = Some(fallback_end);
        }
    }
    let end = boundary_end.unwrap_or(fallback_end);
    let prefix = value[..end]
        .trim_end_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | ':' | '{')
        })
        .to_string();
    if prefix.is_empty() {
        format!(
            "{}{marker}",
            value.chars().take(target_chars).collect::<String>()
        )
    } else {
        format!("{prefix}{marker}")
    }
}

/// Return whether a character is a good truncation boundary for source snippets.
fn is_snippet_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            ',' | ';' | ':' | '{' | '}' | '(' | ')' | '[' | ']' | '/' | '\\' | '.'
        )
}

#[cfg(test)]
mod tests {
    use super::{extract_symbol_graph, specialized_languages};
    use projectatlas_core::symbols::{RelationKind, SymbolKind};

    #[test]
    fn extracts_rust_symbols_and_calls() {
        let source = r"
use std::fs;

pub struct Atlas;

impl Atlas {
    /// Run the atlas scan.
    pub fn scan(&self) {
        helper();
    }
}

fn helper() {}
";
        let graph = extract_symbol_graph("src/lib.rs", Some("rust"), source);
        assert!(
            graph.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Struct && symbol.name.contains("Atlas")
            })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function && symbol.name.contains("helper")
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name.contains("scan")
                && symbol.parent.as_deref() == Some("Atlas")
                && symbol.exported
                && symbol.documentation.as_deref() == Some("Run the atlas scan.")
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Calls && relation.target_name.contains("helper")
        }));
    }

    #[test]
    fn extracts_typescript_symbols() {
        let source = r#"
import { readFile } from "fs";
export interface Reader { read(): string }
export class AtlasReader {
  read() { return readFile; }
}
export function createReader() { return new AtlasReader(); }
export const createWriter = () => createReader();
"#;
        let graph = extract_symbol_graph("src/index.ts", Some("typescript"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Interface
                && symbol.name.contains("Reader")
                && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.name.contains("AtlasReader")
                && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name.contains("createReader")
                && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function && symbol.name == "createWriter" && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "read"
                && symbol.parent.as_deref() == Some("AtlasReader")
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports && relation.target_name.contains("readFile")
        }));
    }

    #[test]
    fn typescript_nested_locals_do_not_inherit_exported_parent() {
        let source = r#"
export function useAtlas() {
  type LocalMode = "fast" | "safe";
  const localCache = new Map<string, string>();
  const computeLocal = () => localCache.size;
  return computeLocal();
}
"#;
        let graph = extract_symbol_graph("src/use-atlas.ts", Some("typescript"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function && symbol.name == "useAtlas" && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "LocalMode"
                && symbol.parent.as_deref() == Some("useAtlas")
                && !symbol.exported
        }));
        for nested_value in ["localCache", "computeLocal"] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.name == nested_value
                        && symbol.kind == SymbolKind::Value
                        && symbol.parent.as_deref() == Some("useAtlas")
                        && !symbol.exported
                }),
                "nested value {nested_value} should remain indexed with parent and no export"
            );
        }
    }

    #[test]
    fn javascript_summary_symbols_ignore_locals_and_iife_constants() {
        let source = r#"
import path from "node:path";
import { createHash } from "node:crypto";

const DATA_DIRECTORY = path.resolve("app/public/data");
const OUTPUT_FILE = path.join(DATA_DIRECTORY, "datasets.manifest.json");
const CACHE_NAME = (() => `sw-${Date.now()}`)();

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function readDatasetEntry(filePath) {
  return sha256(filePath);
}

async function main() {
  const datasetEntries = await Promise.all(["a"].map((file) => readDatasetEntry(file)));
  const versionSeed = datasetEntries.map((entry) => entry.id).join("\n");
  return versionSeed;
}
"#;
        let graph = extract_symbol_graph("scripts/generate.mjs", Some("javascript"), source);
        for name in ["sha256", "readDatasetEntry", "main"] {
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == name),
                "missing top-level function {name}"
            );
        }
        for name in ["DATA_DIRECTORY", "OUTPUT_FILE", "CACHE_NAME"] {
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Value && symbol.name == name),
                "missing top-level constant {name}"
            );
            assert!(
                !graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == name),
                "constant {name} must not be promoted to a function"
            );
        }
        for local in ["datasetEntries", "versionSeed"] {
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Value && symbol.name == local),
                "local binding {local} should remain indexed as a nested value"
            );
            assert!(
                !graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == local),
                "local binding {local} must not become a function"
            );
        }
    }

    #[test]
    fn javascript_object_literal_methods_are_not_file_level_methods() {
        let source = r"
const stub = {
  addListener() {},
  removeListener() {},
  nested: {
    addEventListener() {},
    removeEventListener() {}
  }
};

class Harness {
  run() {}
}
";
        let graph = extract_symbol_graph("tests/browser.spec.js", Some("javascript"), source);
        for object_method in [
            "addListener",
            "removeListener",
            "addEventListener",
            "removeEventListener",
        ] {
            assert!(
                !graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == object_method),
                "object literal method {object_method} must not become a file-level method"
            );
        }
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Class && symbol.name == "Harness" })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "run"
                && symbol.parent.as_deref() == Some("Harness")
        }));
    }

    #[test]
    fn javascript_exported_object_literal_methods_remain_indexed() {
        let source = r"
export const api = {
  list() {},
  nested: {
    refresh() {}
  }
};

module.exports = {
  boot() {}
};
";
        let graph = extract_symbol_graph("src/api.js", Some("javascript"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "list"
                && symbol.parent.as_deref() == Some("api")
                && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "refresh"
                && symbol.parent.as_deref() == Some("api.nested")
                && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "boot"
                && symbol.parent.as_deref() == Some("module.exports")
                && symbol.exported
        }));
    }

    #[test]
    fn javascript_direct_callable_constants_remain_functions() {
        let source = r#"
export const createThing = () => ({ kind: "thing" });
const helper = function helperFactory() { return createThing(); };
"#;
        let graph = extract_symbol_graph("src/factory.js", Some("javascript"), source);
        for name in ["createThing", "helper"] {
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == name),
                "callable constant {name} should remain function-like"
            );
        }
    }

    #[test]
    fn file_purpose_docblock_is_not_symbol_documentation() {
        let source = r#"/**
 * Purpose: Choose a fresher deck start so repeated app opens avoid the same opening cards.
 */
import type { Deal } from "@/types/deals";
export function applyLaunchFreshness() {}
"#;
        let graph = extract_symbol_graph("src/launch-freshness.ts", Some("typescript"), source);
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Import && symbol.documentation.is_none())
        );
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == "applyLaunchFreshness"
                    && symbol.documentation.is_none())
        );
    }

    #[test]
    fn boundary_truncates_long_import_snippet() {
        let truncated = super::truncate_chars_at_boundary(
            "import type { EmailDigestDraft, MarketingChannel, MarketingDatasetDeal } from \"@/marketing\";",
            56,
        );

        assert_eq!(
            truncated,
            "import type { EmailDigestDraft, MarketingChannel..."
        );
    }

    #[test]
    fn import_specific_comment_remains_import_documentation() {
        let source = r#"/** Loads a required browser polyfill. */
import "./polyfill";
"#;
        let graph = extract_symbol_graph("src/polyfills.ts", Some("typescript"), source);
        assert!(
            graph.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Import
                    && symbol.documentation.as_deref() == Some("Loads a required browser polyfill.")
            }),
            "import-specific documentation should remain attached to the import symbol"
        );
    }

    #[test]
    fn extracts_vue_composition_bindings_from_script_setup() {
        let source = r#"
<template><article>{{ currentPriceLabel }}</article></template>
<script setup lang="ts">
import { computed, ref } from "vue";

const props = withDefaults(defineProps<{
  title: string;
}>(), { title: "Deal" });
const emit = defineEmits<{
  select: [id: string];
}>();
const dealTitleId = computed(() => props.title.toLowerCase());
const currentPriceLabel = computed(() => `$${props.title}`);
const retryCount = ref(0);
</script>
"#;
        let graph = extract_symbol_graph("src/DealStage.vue", Some("vue"), source);
        for expected in [
            "props",
            "emit",
            "dealTitleId",
            "currentPriceLabel",
            "retryCount",
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.kind == SymbolKind::Value
                        && symbol.name == expected
                        && symbol.detail.as_deref() == Some("fallback-composition-binding")
                }),
                "missing Vue Composition API binding {expected}"
            );
        }
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports && relation.target_name.contains("computed")
        }));
    }

    #[test]
    fn extracts_python_docstrings() {
        let source = r#"
class Builder:
    """Builds atlas state."""

    def build(self):
        """Build the atlas."""
        return "atlas"
"#;
        let graph = extract_symbol_graph("src/builder.py", Some("python"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.name == "Builder"
                && symbol.documentation.as_deref() == Some("Builds atlas state.")
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "build"
                && symbol.documentation.as_deref() == Some("Build the atlas.")
                && symbol.parent.as_deref() == Some("Builder")
        }));
    }

    #[test]
    fn extracts_java_package_classes_methods_and_calls() {
        let source = r"
package com.example.atlas;

public class AtlasService {
    public void run() {
        helper();
    }

    private void helper() {}
}
";
        let graph = extract_symbol_graph("src/AtlasService.java", Some("java"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Module && symbol.name == "com.example.atlas"
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.name == "AtlasService" && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "run"
                && symbol.parent.as_deref() == Some("AtlasService")
                && symbol.exported
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Calls && relation.target_name == "helper"
        }));
    }

    #[test]
    fn extracts_go_package_functions_methods_and_imports() {
        let source = r#"
package atlas

import "fmt"

type Runner struct {}

func (r Runner) Run() {
    helper()
}

func helper() {
    fmt.Println("ok")
}
"#;
        let graph = extract_symbol_graph("service.go", Some("go"), source);
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Module && symbol.name == "atlas" })
        );
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Struct && symbol.name == "Runner" })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method && symbol.name == "Run" && symbol.exported
        }));
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Function && symbol.name == "helper" })
        );
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports && relation.target_name.contains("\"fmt\"")
        }));
    }

    #[test]
    fn extracts_csharp_namespace_classes_and_methods() {
        let source = r"
namespace Atlas.Core;

public class Runner
{
    public void Run()
    {
        Helper();
    }

    private void Helper() {}
}
";
        let graph = extract_symbol_graph("Runner.cs", Some("csharp"), source);
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Module && symbol.name == "Atlas.Core" })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.name == "Runner" && symbol.exported
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "Run"
                && symbol.parent.as_deref() == Some("Runner")
                && symbol.exported
        }));
    }

    #[test]
    fn extracts_remaining_specialized_language_basics() {
        let samples = [
            (
                "src/main.kt",
                "kotlin",
                r"
package com.example.atlas

class Runner {
    fun run() {}
}
",
                SymbolKind::Class,
                "Runner",
            ),
            (
                "src/main.zig",
                "zig",
                r"
const Runner = struct {
    pub fn run(self: Runner) void {}
};
",
                SymbolKind::Function,
                "run",
            ),
            (
                "src/main.c",
                "c",
                r"
#include <stdio.h>
int run(void) { return 0; }
",
                SymbolKind::Function,
                "run",
            ),
            (
                "src/main.cpp",
                "cpp",
                r"
class Runner {
public:
    void run() {}
};
",
                SymbolKind::Class,
                "Runner",
            ),
            (
                "src/UserManager.m",
                "objective-c",
                r"
@interface UserManager
- (void)run;
@end
@implementation UserManager
- (void)run {}
@end
",
                SymbolKind::Class,
                "UserManager",
            ),
        ];
        for (path, language, source, kind, name) in samples {
            let graph = extract_symbol_graph(path, Some(language), source);
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == kind && symbol.name.contains(name)),
                "expected {language} sample to contain {kind:?} {name}, got {:?}",
                graph.symbols
            );
        }
    }

    #[test]
    fn normalizes_language_specific_edge_summaries() {
        let kotlin = extract_symbol_graph(
            "src/KotlinRunner.kt",
            Some("kotlin"),
            r"
package com.example.atlas
class KotlinRunner { fun run() { helper() } private fun helper() {} }
",
        );
        assert!(kotlin.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Module && symbol.name == "com.example.atlas"
        }));
        assert!(
            kotlin.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Class && symbol.name == "KotlinRunner"
            })
        );
        assert!(kotlin.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "run"
                && symbol.parent.as_deref() == Some("KotlinRunner")
        }));

        let zig = extract_symbol_graph(
            "src/runner.zig",
            Some("zig"),
            "const ZigRunner = struct { pub fn run(self: ZigRunner) void {} };\n",
        );
        assert!(
            zig.symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Struct && symbol.name == "ZigRunner" })
        );
        assert!(zig.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "run"
                && symbol.parent.as_deref() == Some("ZigRunner")
        }));
        assert!(
            !zig.symbols
                .iter()
                .any(|symbol| symbol.name.contains("struct {"))
        );

        let c_graph = extract_symbol_graph(
            "src/runner.c",
            Some("c"),
            "#include <stdio.h>\nint c_run(void) { return 0; }\n",
        );
        let c_run_count = c_graph
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "c_run")
            .count();
        assert_eq!(c_run_count, 1);
        assert!(
            c_graph
                .symbols
                .iter()
                .all(|symbol| symbol.documentation.as_deref() != Some("include <stdio.h>"))
        );

        let cpp_graph = extract_symbol_graph(
            "src/runner.cpp",
            Some("cpp"),
            "class CppRunner { public: void run(); void inline_run() {} };\n",
        );
        let cpp_run_names = cpp_graph
            .symbols
            .iter()
            .filter(|symbol| symbol.parent.as_deref() == Some("CppRunner"))
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(cpp_run_names, vec!["run", "inline_run"]);
        assert!(cpp_graph.symbols.iter().all(|symbol| {
            symbol.parent.as_deref() != Some("CppRunner") || symbol.kind == SymbolKind::Method
        }));

        let objc_graph = extract_symbol_graph(
            "src/ObjRunner.m",
            Some("objective-c"),
            r"
@interface ObjRunner
- (void)run;
@end
@implementation ObjRunner
- (void)run {}
@end
",
        );
        assert_eq!(
            objc_graph
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "ObjRunner")
                .count(),
            1
        );
        assert_eq!(
            objc_graph
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "run")
                .count(),
            1
        );
        assert!(
            !objc_graph
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "run")
        );
        assert!(objc_graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "run"
                && symbol.signature.contains('{')
        }));
    }

    #[test]
    fn extracts_cargo_manifest_symbols() {
        let source = r#"
[package]
name = "projectatlas"

[dependencies]
tree-sitter = "0.26"
serde_json = { workspace = true }
serde_alias = { version = "1", package = "serde" }

[target.'cfg(windows)'.dependencies]
windows-sys = "0.60"
"#;
        let graph = extract_symbol_graph("Cargo.toml", Some("cargo-manifest"), source);
        assert!(
            graph.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Package && symbol.name == "projectatlas"
            })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Dependency && symbol.name == "tree-sitter"
        }));
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Dependency && symbol.name == "serde" })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Dependency && symbol.name == "windows-sys"
        }));
    }

    #[test]
    fn specialized_language_registry_covers_target_set() {
        for expected in [
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "kotlin",
            "csharp",
            "go",
            "objective-c",
            "zig",
        ] {
            assert!(specialized_languages().contains(&expected));
        }
    }
}
