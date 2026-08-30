//! Purpose: Extract tree-sitter-backed `ProjectAtlas` symbol graphs.

mod configured_modules;
mod languages;
mod markdown;
mod resolution_keys;
mod semantic;

pub use configured_modules::{
    ConfiguredModuleError, ConfiguredModuleResolution, EcmaScriptConfigKind,
    EcmaScriptModuleConfig, EcmaScriptPathMapping, MAX_CONFIGURED_MODULE_CONFIGS,
    MAX_CONFIGURED_MODULE_IDENTITY_BYTES, MAX_CONFIGURED_MODULE_MAPPINGS,
    MAX_CONFIGURED_MODULE_TARGETS,
};
pub use markdown::{
    DocumentLinkCandidate, DocumentLinkSource, MAX_DOCUMENT_LINK_CANDIDATES,
    MAX_DOCUMENT_SELECTOR_BYTES, MAX_MARKDOWN_BYTES, MAX_MARKDOWN_EVIDENCE_BYTES,
    MAX_MARKDOWN_HEADINGS, MAX_MARKDOWN_LABEL_BYTES, MarkdownFactCompleteness,
    MarkdownFactCoverage, MarkdownFactLimit, MarkdownFacts, MarkdownHeadingFact,
    MarkdownParserProvenance, MarkdownSourceSelector, MarkdownUnsupportedStructure,
    extract_markdown_facts, extract_markdown_facts_controlled,
};
pub use resolution_keys::{
    ImportReference, ImportSyntax, MAX_RESOLUTION_KEYS_PER_FACT,
    MAX_RESOLUTION_PROJECTION_FAILURES, RelationResolutionKeys, ResolutionKeyProjection,
    ResolutionProjectionContext, ResolutionProjectionError, ResolutionProjectionFact,
    ResolutionProjectionFactFailure, ResolutionProjectionFailure,
    SEMANTIC_RESOLUTION_CONTRACT_VERSION, SymbolResolutionKeys, derive_resolution_keys,
    derive_resolution_keys_with_context, module_aliases_for_path, parse_import_references,
    resolve_relative_import_path, semantic_resolution_contract_digest, source_stems_for_path,
};

use projectatlas_core::graph::QUALIFIED_SYMBOL_SCOPE_PREFIX;
use projectatlas_core::language::{
    EmbeddedHostKind, EmbeddedLanguageCapability, SymbolParserOwner, TreeSitterGrammar,
    builtin_tree_sitter_language_ids, language_capability, tree_sitter_grammar,
};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    SymbolSourceSelector,
};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use regex::Regex;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::convert::Infallible;
use std::ops::ControlFlow;
use std::path::Path;
use toml::Value as TomlValue;
use tree_sitter::{Language, Node, ParseOptions, Parser, Tree};

/// Maximum symbols kept from one file to bound large generated sources.
const MAX_SYMBOLS_PER_FILE: usize = 4_000;
/// Maximum relations kept from one file to bound call-heavy sources.
const MAX_RELATIONS_PER_FILE: usize = 8_000;
/// Maximum text length stored for symbol names, signatures, and relation context.
const MAX_SNIPPET_CHARS: usize = 240;
/// Maximum text length stored for extracted documentation.
const MAX_DOC_CHARS: usize = 500;
/// Maximum parsed rows between cooperative cancellation/deadline checks.
const PARSER_CONTROL_CHECK_INTERVAL: usize = 128;

/// Extract a symbol graph from source or manifest content.
#[must_use]
pub fn extract_symbol_graph(path: &str, language: Option<&str>, content: &str) -> SymbolGraph {
    match extract_symbol_graph_checked(path, language, content, &mut || Ok::<(), Infallible>(())) {
        Ok(graph) => graph,
        Err(unreachable) => match unreachable {},
    }
}

/// Extract a symbol graph while observing the shared indexing cancellation boundary.
///
/// # Errors
///
/// Returns a typed cancellation or deadline failure without returning a partial graph.
pub fn extract_symbol_graph_controlled(
    path: &str,
    language: Option<&str>,
    content: &str,
    control: &IndexWorkControl,
) -> Result<SymbolGraph, IndexWorkFailure> {
    extract_symbol_graph_checked(path, language, content, &mut || {
        control.check(IndexWorkStage::SymbolParsing)
    })
}

/// Extract a symbol graph with one cooperative work checkpoint shared by every parser stage.
fn extract_symbol_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    check()?;
    let parse_content = content_without_leading_purpose_header(content);
    if let Some(capability) = semantic::embedded_source::host_capability(path, language) {
        return extract_embedded_host_graph_checked(
            path,
            language,
            parse_content.as_ref(),
            capability,
            check,
        );
    }
    match symbol_parser_owner(path, language) {
        SymbolParserOwner::CargoManifest => {
            return extract_cargo_manifest_graph_checked(path, language, content, check);
        }
        SymbolParserOwner::Vue => {
            return extract_vue_sfc_graph_checked(path, language, parse_content.as_ref(), check);
        }
        SymbolParserOwner::PowerShell => {
            return extract_powershell_graph_checked(path, language, parse_content.as_ref(), check);
        }
        SymbolParserOwner::Markdown => {
            let facts = markdown::extract_markdown_facts_checked(parse_content.as_ref(), check)?;
            return Ok(facts.symbol_graph(path, language));
        }
        SymbolParserOwner::Unavailable => {
            check()?;
            return Ok(empty_graph(path, language, ParserKind::Structural));
        }
        SymbolParserOwner::TreeSitter(_) | SymbolParserOwner::Fallback => {}
    }
    if let Some(parsed) = extract_tree_sitter_graph(path, language, parse_content.as_ref(), check)?
    {
        if !parsed.graph.symbols.is_empty() || !parsed.graph.relations.is_empty() {
            check()?;
            return Ok(parsed.graph);
        }
        if parsed.had_errors {
            let fallback =
                extract_fallback_graph_checked(path, language, parse_content.as_ref(), check)?;
            if !fallback.symbols.is_empty() || !fallback.relations.is_empty() {
                check()?;
                return Ok(fallback);
            }
        }
        check()?;
        return Ok(parsed.graph);
    }
    extract_fallback_graph_checked(path, language, parse_content.as_ref(), check)
}

/// Extract accepted inline script facts without changing their host-file positions.
fn extract_embedded_host_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    capability: EmbeddedLanguageCapability,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    let mut graph = match capability.host_kind {
        EmbeddedHostKind::HtmlLike => empty_graph(path, language, ParserKind::Structural),
        EmbeddedHostKind::Component => {
            extract_vue_sfc_graph_checked(path, language, content, check)?
        }
        EmbeddedHostKind::Template => {
            extract_fallback_graph_checked(path, language, content, check)?
        }
    };
    let (projections, _incomplete) = semantic::embedded_source::project(content).into_parts();
    // Embedded hosts retain their structural/fallback graph parser. Runtime
    // coverage therefore remains partial even when admitted tree-sitter facts
    // are merged, including when reconciliation stopped after a safe prefix.
    for projection in projections {
        check()?;
        if let Some(parsed) = extract_tree_sitter_graph(
            path,
            Some(projection.language().as_str()),
            projection.source(),
            check,
        )? {
            merge_missing_graph_entries_checked(
                &mut graph,
                parsed.graph,
                capability.host_kind,
                check,
            )?;
        }
    }
    check()?;
    Ok(graph)
}

/// Check cooperative parser control at a bounded row interval.
pub(crate) fn check_parser_iteration<E>(
    iteration: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    if iteration.is_multiple_of(PARSER_CONTROL_CHECK_INTERVAL) {
        check()?;
    }
    Ok(())
}

/// Return whether the language has a specialized tree-sitter parser.
#[must_use]
pub fn has_specialized_parser(language: &str) -> bool {
    tree_sitter_grammar(language).is_some()
}

/// Return all specialized parser language identifiers.
#[must_use]
pub fn specialized_languages() -> &'static [&'static str] {
    builtin_tree_sitter_language_ids()
}

/// Select the accepted parser owner, falling back to legacy path inference only without a language.
fn symbol_parser_owner(path: &str, language: Option<&str>) -> SymbolParserOwner {
    if let Some(language) = language {
        return language_capability(language).map_or(SymbolParserOwner::Fallback, |capability| {
            capability.symbol_parser
        });
    }
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if matches!(file_name, "Cargo.toml" | "Cargo.lock") {
        return SymbolParserOwner::CargoManifest;
    }
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("vue") => SymbolParserOwner::Vue,
        Some(extension)
            if ["ps1", "psm1", "psd1"]
                .iter()
                .any(|expected| extension.eq_ignore_ascii_case(expected)) =>
        {
            SymbolParserOwner::PowerShell
        }
        _ => SymbolParserOwner::Fallback,
    }
}

/// Extract Vue SFC Composition API bindings with cooperative parser control.
fn extract_vue_sfc_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    let mut graph = extract_fallback_graph_checked(path, language, content, check)?;
    graph.parser = ParserKind::Structural;
    let mut structural = empty_graph(path, language, ParserKind::Structural);
    for (line_index, line) in content.lines().enumerate() {
        check_parser_iteration(line_index, check)?;
        let trimmed = line.trim();
        if let Some(name) = vue_composition_binding_name(trimmed) {
            push_symbol(
                &mut structural,
                &name,
                SymbolKind::Value,
                line_index + 1,
                line_index + 1,
                None,
                Some("vue-composition-binding"),
                trimmed,
            );
        }
        if is_fallback_import(trimmed) {
            push_relation(
                &mut structural,
                "<module>",
                trimmed,
                RelationKind::Imports,
                line_index + 1,
                trimmed,
            );
        }
    }
    merge_preferred_graph_entries_checked(&mut graph, structural, check)?;
    check()?;
    Ok(graph)
}

/// Extract `PowerShell` declarations with cooperative parser control.
fn extract_powershell_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    let mut graph = extract_fallback_graph_checked(path, language, content, check)?;
    graph.parser = ParserKind::Structural;
    let mut structural = empty_graph(path, language, ParserKind::Structural);
    for (line_index, line) in content.lines().enumerate() {
        check_parser_iteration(line_index, check)?;
        let trimmed = line.trim();
        if let Some(name) = powershell_function_name(trimmed) {
            push_symbol(
                &mut structural,
                &name,
                SymbolKind::Function,
                line_index + 1,
                line_index + 1,
                None,
                Some("powershell-function"),
                trimmed,
            );
        }
        if let Some(name) = powershell_class_name(trimmed) {
            push_symbol(
                &mut structural,
                &name,
                SymbolKind::Class,
                line_index + 1,
                line_index + 1,
                None,
                Some("powershell-class"),
                trimmed,
            );
        }
        if is_fallback_import(trimmed) {
            push_relation(
                &mut structural,
                "<module>",
                trimmed,
                RelationKind::Imports,
                line_index + 1,
                trimmed,
            );
        }
    }
    merge_preferred_graph_entries_checked(&mut graph, structural, check)?;
    check()?;
    Ok(graph)
}

/// Extract one `PowerShell` function declaration name.
fn powershell_function_name(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("function") {
        return None;
    }
    let raw_name = parts.next()?;
    let name = raw_name.split(['(', '{']).next().unwrap_or_default().trim();
    let name = name.rsplit_once(':').map_or(name, |(_, scoped)| scoped);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    valid.then(|| name.to_string())
}

/// Extract one `PowerShell` class declaration name.
fn powershell_class_name(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("class") {
        return None;
    }
    let raw_name = parts.next()?;
    let name = raw_name
        .split([':', '{', '('])
        .next()
        .unwrap_or_default()
        .trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    valid.then(|| name.to_string())
}

/// Merge preferred graph entries with cooperative parser control.
fn merge_preferred_graph_entries_checked<E>(
    graph: &mut SymbolGraph,
    preferred: SymbolGraph,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for (iteration, symbol) in preferred.symbols.into_iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        if let Some(existing) = graph
            .symbols
            .iter()
            .position(|existing| same_symbol_identity(existing, &symbol))
        {
            graph.symbols[existing] = symbol;
        } else if graph.symbols.len() < MAX_SYMBOLS_PER_FILE {
            graph.symbols.push(symbol);
        }
    }
    for (iteration, relation) in preferred.relations.into_iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        if let Some(existing) = graph
            .relations
            .iter()
            .position(|existing| same_relation_identity(existing, &relation))
        {
            graph.relations[existing] = relation;
        } else if graph.relations.len() < MAX_RELATIONS_PER_FILE {
            graph.relations.push(relation);
        }
    }
    Ok(())
}

/// Merge embedded facts without replacing compatibility facts owned by the host parser.
fn merge_missing_graph_entries_checked<E>(
    graph: &mut SymbolGraph,
    embedded: SymbolGraph,
    host_kind: EmbeddedHostKind,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let mut symbol_identities = graph
        .symbols
        .iter()
        .map(|symbol| {
            (
                symbol.name.clone(),
                symbol.kind as u8,
                symbol.line_start,
                symbol.line_end,
                symbol.parent.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    for (iteration, symbol) in embedded.symbols.into_iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        if graph.symbols.len() >= MAX_SYMBOLS_PER_FILE {
            break;
        }
        if host_kind == EmbeddedHostKind::Component
            && (symbol.kind == SymbolKind::Import
                || (symbol.kind == SymbolKind::Value && !symbol.exported))
        {
            continue;
        }
        let identity = (
            symbol.name.clone(),
            symbol.kind as u8,
            symbol.line_start,
            symbol.line_end,
            symbol.parent.clone(),
        );
        if symbol_identities.insert(identity) {
            graph.symbols.push(symbol);
        }
    }
    let mut relation_identities = graph
        .relations
        .iter()
        .map(|relation| {
            (
                relation.source_name.clone(),
                relation.target_name.clone(),
                relation.kind as u8,
                relation.line,
            )
        })
        .collect::<BTreeSet<_>>();
    for (iteration, relation) in embedded.relations.into_iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        if graph.relations.len() >= MAX_RELATIONS_PER_FILE {
            break;
        }
        let identity = (
            relation.source_name.clone(),
            relation.target_name.clone(),
            relation.kind as u8,
            relation.line,
        );
        if relation_identities.insert(identity) {
            graph.relations.push(relation);
        }
    }
    Ok(())
}

/// Return whether two symbols represent the same declaration.
fn same_symbol_identity(left: &CodeSymbol, right: &CodeSymbol) -> bool {
    left.name == right.name
        && left.kind == right.kind
        && left.line_start == right.line_start
        && left.line_end == right.line_end
        && left.parent == right.parent
}

/// Return whether two relations represent the same source edge.
fn same_relation_identity(left: &SymbolRelation, right: &SymbolRelation) -> bool {
    left.source_name == right.source_name
        && left.target_name == right.target_name
        && left.kind == right.kind
        && left.line == right.line
}

/// Extract a Composition API binding name from a script setup row.
fn vue_composition_binding_name(line: &str) -> Option<String> {
    const MACROS: &[&str] = &[
        "defineProps",
        "defineEmits",
        "defineModel",
        "defineSlots",
        "computed",
        "ref",
        "shallowRef",
        "reactive",
        "toRef",
        "toRefs",
        "watch",
    ];
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("let "))
        .or_else(|| line.strip_prefix("var "))?;
    let (name, initializer) = rest.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let initializer = initializer.trim_start();
    MACROS
        .iter()
        .any(|macro_name| vue_initializer_starts_with_macro(initializer, macro_name))
        .then(|| name.to_string())
}

/// Return whether a Vue initializer starts with a supported Composition API macro.
fn vue_initializer_starts_with_macro(initializer: &str, macro_name: &str) -> bool {
    vue_initializer_is_macro_call(initializer, macro_name)
        || initializer
            .strip_prefix("withDefaults(")
            .is_some_and(|nested| vue_initializer_is_macro_call(nested.trim_start(), macro_name))
}

/// Return whether an initializer begins with the named macro call.
fn vue_initializer_is_macro_call(initializer: &str, macro_name: &str) -> bool {
    let Some(rest) = initializer.strip_prefix(macro_name) else {
        return false;
    };
    let rest = rest.trim_start();
    rest.starts_with('(') || rest.starts_with('<')
}

/// Extract Cargo package, workspace, and dependency entries with cooperative parser control.
fn extract_cargo_manifest_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    check()?;
    let mut graph = empty_graph(path, language, ParserKind::Manifest);
    let is_lock = match language {
        Some(language) => language == "cargo-lock",
        None => path.ends_with("Cargo.lock"),
    };
    if is_lock {
        extract_cargo_lock_packages_checked(&mut graph, content, check)?;
        check()?;
        return Ok(graph);
    }
    extract_cargo_toml_entries_checked(&mut graph, content, check)?;
    check()?;
    Ok(graph)
}

/// Extract package names from Cargo.lock with cooperative parser control.
fn extract_cargo_lock_packages_checked<E>(
    graph: &mut SymbolGraph,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let Ok(lockfile) = content.parse::<TomlValue>() else {
        return Ok(());
    };
    check()?;
    let Some(packages) = lockfile.get("package").and_then(TomlValue::as_array) else {
        return Ok(());
    };
    let mut next_package_line = 0;
    for (iteration, package) in packages.iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        let Some(name) = package
            .as_table()
            .and_then(|table| table.get("name"))
            .and_then(TomlValue::as_str)
        else {
            continue;
        };
        let line = cargo_lock_name_line_checked(content, name, next_package_line, check)?;
        if let Some(found_line) = line {
            next_package_line = found_line;
        }
        let line = line.unwrap_or(1);
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
    Ok(())
}

/// Return the one-based source line for a package name with cooperative parser control.
fn cargo_lock_name_line_checked<E>(
    content: &str,
    package_name: &str,
    start_line: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<usize>, E> {
    let mut in_package = false;
    for (iteration, (index, raw_line)) in content.lines().enumerate().skip(start_line).enumerate() {
        check_parser_iteration(iteration, check)?;
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
            return Ok(Some(index + 1));
        }
    }
    Ok(None)
}

/// Extract package, workspace, and dependencies from Cargo.toml with cooperative parser control.
fn extract_cargo_toml_entries_checked<E>(
    graph: &mut SymbolGraph,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let Ok(manifest) = content.parse::<TomlValue>() else {
        return Ok(());
    };
    check()?;
    let Some(root) = manifest.as_table() else {
        return Ok(());
    };
    let line_index = CargoTomlLineIndex::new_checked(content, check)?;
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
    collect_cargo_dependencies_checked(graph, &line_index, &[], root, check)?;
    Ok(())
}

/// Recursively collect dependency tables from parsed Cargo TOML with cooperative control.
fn collect_cargo_dependencies_checked<E>(
    graph: &mut SymbolGraph,
    line_index: &CargoTomlLineIndex,
    path: &[String],
    table: &toml::map::Map<String, TomlValue>,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    check()?;
    let section = path.join(".");
    if is_dependency_table_path(path) {
        for (iteration, (name, value)) in table.iter().enumerate() {
            check_parser_iteration(iteration, check)?;
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
        return Ok(());
    }
    for (iteration, (key, value)) in table.iter().enumerate() {
        check_parser_iteration(iteration, check)?;
        let Some(child) = value.as_table() else {
            continue;
        };
        let mut child_path = path.to_owned();
        child_path.push(key.clone());
        collect_cargo_dependencies_checked(graph, line_index, &child_path, child, check)?;
    }
    Ok(())
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
    /// Build a line index for TOML source positions with cooperative parser control.
    fn new_checked<E>(
        content: &'a str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Self, E> {
        let lines = content.lines().collect::<Vec<_>>();
        let mut sections = std::collections::HashMap::new();
        let mut keys = std::collections::HashMap::new();
        let mut current_section = String::new();
        for (index, raw_line) in lines.iter().enumerate() {
            check_parser_iteration(index, check)?;
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
        Ok(Self {
            lines,
            sections,
            keys,
        })
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

/// Tree-sitter extraction result with parse health metadata.
struct TreeSitterParse {
    /// Extracted symbol graph.
    graph: SymbolGraph,
    /// Whether tree-sitter found syntax errors while parsing.
    had_errors: bool,
}

/// PHP mixed-grammar result with its grammar-owned opening-tag classification.
struct PhpParse {
    /// Parsed full-file PHP/mixed tree.
    tree: Tree,
    /// Whether a PHP opening tag occurs outside PHP literals or comments.
    has_opening_tag: bool,
}

/// Extract a graph through tree-sitter when the language has a grammar.
fn extract_tree_sitter_graph<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<TreeSitterParse>, E> {
    let Some(language_name) = language else {
        return Ok(None);
    };
    let Some(grammar) = tree_sitter_grammar(language_name) else {
        return Ok(None);
    };
    let (tree, has_php_opening_tag) = if grammar == TreeSitterGrammar::Php {
        let Some(parsed) = parse_php_tree(content, check)? else {
            return Ok(None);
        };
        (parsed.tree, Some(parsed.has_opening_tag))
    } else {
        let Some(parser_language) = tree_sitter_language(language_name) else {
            return Ok(None);
        };
        let Some(tree) = parse_tree_sitter_language(&parser_language, content, check)? else {
            return Ok(None);
        };
        (tree, None)
    };
    check()?;
    let mut graph = empty_graph(path, language, ParserKind::TreeSitter);
    let root = tree.root_node();
    if has_php_opening_tag == Some(false) {
        return Ok(Some(TreeSitterParse {
            graph,
            had_errors: false,
        }));
    }
    let had_errors = root.has_error() && has_php_opening_tag != Some(false);
    let mut php_namespace_context = if has_php_opening_tag == Some(true) {
        Some(PhpNamespaceContext::from_program(root, content, check)?)
    } else {
        None
    };
    visit_node(
        root,
        content,
        &mut graph,
        check,
        php_namespace_context.as_mut(),
    )?;
    check()?;
    languages::augment_language_graph(&mut graph, content, check)?;
    check()?;
    Ok(Some(TreeSitterParse { graph, had_errors }))
}

/// Precomputed source-order ownership ranges for semicolon PHP namespaces.
struct PhpNamespaceContext {
    /// Non-overlapping ranges whose declarations belong to a namespace.
    ranges: Vec<PhpNamespaceRange>,
    /// Next range to inspect while declarations are visited in source order.
    next_range: usize,
    /// Number of program children examined while building the ranges.
    #[cfg(test)]
    examined_children: usize,
    /// Number of source-order lookups made while visiting top-level nodes.
    #[cfg(test)]
    parent_lookups: usize,
}

/// One semicolon namespace's source-order ownership range.
struct PhpNamespaceRange {
    /// First byte after the namespace declaration.
    start_byte: usize,
    /// First byte of the next namespace declaration or end of source.
    end_byte: usize,
    /// Namespace name owned by this range.
    name: String,
}

impl PhpNamespaceContext {
    /// Build namespace ranges in one forward pass over the program children.
    fn from_program<E>(
        root: Node<'_>,
        content: &str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Self, E> {
        let mut context = Self {
            ranges: Vec::new(),
            next_range: 0,
            #[cfg(test)]
            examined_children: 0,
            #[cfg(test)]
            parent_lookups: 0,
        };
        let mut active = None;
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            check()?;
            #[cfg(test)]
            {
                context.examined_children += 1;
            }
            if child.kind() != "namespace_definition" {
                continue;
            }
            if let Some((start_byte, name)) = active.take() {
                context.ranges.push(PhpNamespaceRange {
                    start_byte,
                    end_byte: child.start_byte(),
                    name,
                });
            }
            if !child.has_error()
                && child.child_by_field_name("body").is_none()
                && let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|name| named_text(name, content))
            {
                active = Some((child.end_byte(), name));
            }
        }
        if let Some((start_byte, name)) = active {
            context.ranges.push(PhpNamespaceRange {
                start_byte,
                end_byte: content.len(),
                name,
            });
        }
        Ok(context)
    }

    /// Return the active namespace for the next source-order top-level node.
    fn parent_for(&mut self, node: Node<'_>) -> Option<String> {
        #[cfg(test)]
        {
            self.parent_lookups += 1;
        }
        while self
            .ranges
            .get(self.next_range)
            .is_some_and(|range| range.end_byte <= node.start_byte())
        {
            self.next_range += 1;
        }
        self.ranges
            .get(self.next_range)
            .filter(|range| {
                range.start_byte <= node.start_byte() && node.start_byte() < range.end_byte
            })
            .map(|range| range.name.clone())
    }
}

/// Select the official PHP-only or mixed grammar from their parsed roots.
fn parse_php_tree<E>(
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<PhpParse>, E> {
    let mixed_language: Language = tree_sitter_php::LANGUAGE_PHP.into();
    let Some(mixed) = parse_tree_sitter_language(&mixed_language, content, check)? else {
        return Ok(None);
    };
    let mut examined_nodes = 0;
    let Some(first_tag_start) = first_php_tag_start(mixed.root_node(), check, &mut examined_nodes)?
    else {
        return Ok(Some(PhpParse {
            tree: mixed,
            has_opening_tag: false,
        }));
    };
    let first_content_start = content.find(|character: char| !character.is_whitespace());
    if !mixed.root_node().has_error() && first_content_start == Some(first_tag_start) {
        return Ok(Some(PhpParse {
            tree: mixed,
            has_opening_tag: true,
        }));
    }

    // The PHP-only grammar is only a bounded probe for opening tags that the
    // mixed grammar can see inside a literal or comment. The full-file result
    // remains the mixed grammar so tagless `.php` source is represented as
    // inline text instead of executable PHP.
    let php_only_language: Language = tree_sitter_php::LANGUAGE_PHP_ONLY.into();
    let Some(php_only) = parse_tree_sitter_language(&php_only_language, content, check)? else {
        return Ok(Some(PhpParse {
            tree: mixed,
            has_opening_tag: true,
        }));
    };
    let has_opening_tag =
        tree_contains_php_tag_outside_literals(mixed.root_node(), php_only.root_node());
    Ok(Some(PhpParse {
        tree: mixed,
        has_opening_tag,
    }))
}

/// Return a tree-sitter language for supported source families.
fn tree_sitter_language(language: &str) -> Option<Language> {
    Some(match tree_sitter_grammar(language)? {
        TreeSitterGrammar::Rust => tree_sitter_rust::LANGUAGE.into(),
        TreeSitterGrammar::Python => tree_sitter_python::LANGUAGE.into(),
        TreeSitterGrammar::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        TreeSitterGrammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TreeSitterGrammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        TreeSitterGrammar::Java => tree_sitter_java::LANGUAGE.into(),
        TreeSitterGrammar::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        TreeSitterGrammar::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        TreeSitterGrammar::Go => tree_sitter_go::LANGUAGE.into(),
        TreeSitterGrammar::ObjectiveC => tree_sitter_objc::LANGUAGE.into(),
        TreeSitterGrammar::Zig => tree_sitter_zig::LANGUAGE.into(),
        TreeSitterGrammar::C => tree_sitter_c::LANGUAGE.into(),
        TreeSitterGrammar::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        TreeSitterGrammar::Php => tree_sitter_php::LANGUAGE_PHP_ONLY.into(),
    })
}

/// Parse source with one pinned tree-sitter grammar while observing cancellation.
fn parse_tree_sitter_language<E>(
    parser_language: &Language,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<Tree>, E> {
    check()?;
    let mut parser = Parser::new();
    if parser.set_language(parser_language).is_err() {
        return Ok(None);
    }
    let mut parse_failure = None;
    let mut progress = |_: &tree_sitter::ParseState| match check() {
        Ok(()) => ControlFlow::Continue(()),
        Err(error) => {
            parse_failure = Some(error);
            ControlFlow::Break(())
        }
    };
    let bytes = content.as_bytes();
    let mut read = |offset, _| bytes.get(offset..).unwrap_or_default();
    let tree = parser.parse_with_options(
        &mut read,
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if let Some(error) = parse_failure {
        return Err(error);
    }
    check()?;
    Ok(tree)
}

/// Return the first opening-tag byte offset in a mixed PHP parse.
fn first_php_tag_start<E>(
    node: Node<'_>,
    check: &mut impl FnMut() -> Result<(), E>,
    examined_nodes: &mut usize,
) -> Result<Option<usize>, E> {
    *examined_nodes += 1;
    check_parser_iteration(*examined_nodes, check)?;
    if node.kind() == "php_tag" {
        return Ok(Some(node.start_byte()));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(start) = first_php_tag_start(child, check, examined_nodes)? {
            return Ok(Some(start));
        }
    }
    Ok(None)
}

/// Return whether a mixed parse contains a tag outside a PHP literal or comment.
fn tree_contains_php_tag_outside_literals(mixed: Node<'_>, php_only: Node<'_>) -> bool {
    let mut opaque_ranges = Vec::new();
    collect_php_only_opaque_ranges(php_only, &mut opaque_ranges);
    let mut next_opaque = 0;
    tree_contains_php_tag_outside_ranges(mixed, &opaque_ranges, &mut next_opaque)
}

/// Collect top-level literal/comment ranges from the PHP-only parse in source order.
fn collect_php_only_opaque_ranges(node: Node<'_>, ranges: &mut Vec<(usize, usize)>) {
    if is_php_opaque_node(node.kind()) {
        ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .for_each(|child| collect_php_only_opaque_ranges(child, ranges));
}

/// Return whether a mixed parse contains a tag outside the sorted opaque ranges.
fn tree_contains_php_tag_outside_ranges(
    node: Node<'_>,
    opaque_ranges: &[(usize, usize)],
    next_opaque: &mut usize,
) -> bool {
    if node.kind() == "php_tag" {
        while *next_opaque < opaque_ranges.len()
            && opaque_ranges[*next_opaque].1 <= node.start_byte()
        {
            *next_opaque += 1;
        }
        if *next_opaque >= opaque_ranges.len() || opaque_ranges[*next_opaque].0 >= node.end_byte() {
            return true;
        }
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| tree_contains_php_tag_outside_ranges(child, opaque_ranges, next_opaque))
}

/// Return whether the PHP-only parse node is opaque to mixed-grammar tags.
fn is_php_opaque_node(kind: &str) -> bool {
    matches!(
        kind,
        "comment"
            | "encapsed_string"
            | "heredoc"
            | "nowdoc"
            | "shell_command_expression"
            | "string"
    )
}

/// Return whether the official PHP grammars recognize an opening tag.
#[cfg(test)]
fn contains_php_opening_tag(content: &str) -> bool {
    let mut check = || Ok::<(), Infallible>(());
    parse_php_tree(content, &mut check)
        .ok()
        .flatten()
        .is_some_and(|parsed| parsed.has_opening_tag)
}

/// Recursively inspect one tree-sitter node.
fn visit_node<E>(
    node: Node<'_>,
    content: &str,
    graph: &mut SymbolGraph,
    check: &mut impl FnMut() -> Result<(), E>,
    mut php_namespace_context: Option<&mut PhpNamespaceContext>,
) -> Result<(), E> {
    check()?;
    if graph.symbols.len() < MAX_SYMBOLS_PER_FILE
        && let Some(kind) = declaration_kind(node.kind())
        && should_emit_declaration_symbol(node, content)
    {
        push_tree_symbol(
            graph,
            node,
            content,
            effective_declaration_kind(node, kind),
            php_namespace_context.as_deref_mut(),
        );
    }
    if graph.relations.len() < MAX_RELATIONS_PER_FILE {
        if is_php_trait_use_declaration(node) {
            push_php_trait_use_relations(graph, node, content);
        } else if is_import_node(node.kind()) {
            push_import_relation(graph, node, content);
        } else if is_call_node(node.kind()) {
            push_call_relation(graph, node, content);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_node(
            child,
            content,
            graph,
            check,
            php_namespace_context.as_deref_mut(),
        )?;
    }
    Ok(())
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
    if is_php_trait_use_declaration(node) {
        return false;
    }
    if is_object_literal_method(node) {
        return object_literal_method_owner(node, content).is_some_and(|owner| owner.exported);
    }
    if node.kind() == "field_declaration"
        && has_descendant_kind(node, &["function_declarator", "method_declarator"])
    {
        return false;
    }
    if node.kind() == "property_declaration" && has_descendant_kind(node, &["property_element"]) {
        return false;
    }
    if node.kind() == "const_declaration" && has_descendant_kind(node, &["const_element"]) {
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
    first_declaration_initializer(node).is_some_and(|initializer| {
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

/// Return the first direct declaration initializer.
fn first_declaration_initializer(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(value) = node.child_by_field_name("value") {
        return Some(value);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(value) = child.child_by_field_name("value") {
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
    php_namespace_context: Option<&mut PhpNamespaceContext>,
) {
    let Some(name) = node_name(node, content) else {
        return;
    };
    let signature = declaration_signature(node, content);
    let parent = symbol_parent(node, content, php_namespace_context)
        .and_then(|parent| compact_symbol_identity(&parent));
    let exported = has_direct_export_parent(node)
        || object_literal_method_owner(node, content).is_some_and(|owner| owner.exported)
        || is_exported_symbol(graph.language.as_deref(), node, content, &name, &signature);
    let documentation = symbol_documentation(node, content);
    let admitted = push_symbol_with_metadata(
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
    if admitted
        && is_php_language(graph.language.as_deref())
        && let Some(symbol) = graph.symbols.last_mut()
    {
        symbol.source_selector = Some(tree_source_selector(node, content));
    }
    if admitted && let Some(parent_name) = parent {
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

/// Build the exact persisted selector represented by a Tree-sitter node.
fn tree_source_selector(node: Node<'_>, content: &str) -> SymbolSourceSelector {
    let start = node.start_position();
    let end = node.end_position();
    SymbolSourceSelector {
        byte_start: node.start_byte(),
        byte_end: node.end_byte(),
        column_start: tree_source_column(node.start_byte(), start.column, content),
        column_end: tree_source_column(node.end_byte(), end.column, content),
    }
}

/// Convert Tree-sitter's byte column to the Unicode-scalar column persisted by the graph.
fn tree_source_column(byte_offset: usize, byte_column: usize, content: &str) -> usize {
    if content.is_ascii() {
        return byte_column;
    }
    let byte_offset = byte_offset.min(content.len());
    let line_start = content[..byte_offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    content[line_start..byte_offset].chars().count()
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

/// Blank a source prefix without changing byte offsets or line numbers.
fn blank_prefix_preserving_newlines(content: &str, end: usize) -> String {
    let mut output = String::with_capacity(content.len());
    debug_assert!(content.is_char_boundary(end));
    for byte in &content.as_bytes()[..end] {
        output.push(if matches!(*byte, b'\n' | b'\r') {
            char::from(*byte)
        } else {
            ' '
        });
    }
    output.push_str(&content[end..]);
    output
}

/// Return the semantic parent for a declaration symbol.
fn symbol_parent(
    node: Node<'_>,
    content: &str,
    php_namespace_context: Option<&mut PhpNamespaceContext>,
) -> Option<String> {
    if let Some(owner) = object_literal_method_owner(node, content) {
        return Some(owner.name);
    }
    if node.kind() == "property_promotion_parameter"
        && let Some(class) = nearest_ancestor_kind(node.parent(), "class_declaration")
    {
        return node_name(class, content);
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
    let parent = if matches!(node.kind(), "property_element" | "const_element") {
        node.parent().and_then(|declaration| declaration.parent())
    } else {
        node.parent()
    };
    enclosing_symbol_name(parent, content)
        .or_else(|| php_semicolon_namespace_parent(node, php_namespace_context))
}

/// Return the active PHP namespace for a declaration in a semicolon namespace.
fn php_semicolon_namespace_parent(
    node: Node<'_>,
    php_namespace_context: Option<&mut PhpNamespaceContext>,
) -> Option<String> {
    if node.kind() == "namespace_definition" {
        return None;
    }
    php_namespace_context.and_then(|context| context.parent_for(node))
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
        "trait_item" | "trait_declaration" => Some(SymbolKind::Trait),
        "interface_declaration" | "interface_type" => Some(SymbolKind::Interface),
        "mod_item"
        | "module_declaration"
        | "namespace_declaration"
        | "namespace_definition"
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
        | "short_var_declaration"
        | "property_declaration"
        | "property_element"
        | "property_promotion_parameter"
        | "const_element"
        | "enum_case" => Some(SymbolKind::Value),
        "use_declaration"
        | "import_statement"
        | "import_declaration"
        | "import_from_statement"
        | "using_directive"
        | "preproc_include"
        | "namespace_use_declaration" => Some(SymbolKind::Import),
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
            | "namespace_use_declaration"
            | "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression"
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
            | "function_call_expression"
            | "member_call_expression"
            | "nullsafe_member_call_expression"
            | "scoped_call_expression"
    )
}

/// Return whether a node is one of PHP's include/require expressions.
fn is_php_include_node(kind: &str) -> bool {
    matches!(
        kind,
        "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression"
    )
}

/// Return whether a PHP `use` declaration composes traits inside a type.
fn is_php_trait_use_declaration(node: Node<'_>) -> bool {
    node.kind() == "use_declaration"
        && has_ancestor_kind_any(
            node.parent(),
            &["class_declaration", "trait_declaration", "enum_declaration"],
        )
}

/// Return the owning type for a PHP trait composition declaration.
fn php_trait_use_owner(node: Node<'_>, content: &str) -> Option<String> {
    if !is_php_trait_use_declaration(node) {
        return None;
    }
    let mut current = node.parent();
    while let Some(candidate) = current {
        if declaration_kind(candidate.kind()).is_some() {
            return matches!(
                candidate.kind(),
                "class_declaration" | "trait_declaration" | "enum_declaration"
            )
            .then(|| node_name(candidate, content))
            .flatten();
        }
        current = candidate.parent();
    }
    None
}

/// Return direct trait targets, excluding alias and adaptation clause names.
fn php_trait_use_targets(node: Node<'_>, content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if targets.len() >= MAX_RELATIONS_PER_FILE {
            break;
        }
        if matches!(child.kind(), "name" | "qualified_name" | "relative_name")
            && let Some(target) = named_text(child, content)
            && target.chars().count() <= MAX_SNIPPET_CHARS
        {
            targets.push(target);
        }
    }
    targets
}

/// Publish exact trait-composition targets under their owning PHP type.
fn push_php_trait_use_relations(graph: &mut SymbolGraph, node: Node<'_>, content: &str) {
    let Some(owner) = php_trait_use_owner(node, content) else {
        return;
    };
    for target in php_trait_use_targets(node, content) {
        if graph.relations.len() >= MAX_RELATIONS_PER_FILE {
            break;
        }
        push_relation(
            graph,
            &owner,
            &target,
            RelationKind::Imports,
            node.start_position().row + 1,
            &target,
        );
    }
}

/// Return whether a supplied language identifier selects the PHP owner.
fn is_php_language(language: Option<&str>) -> bool {
    language.is_some_and(|language| language.eq_ignore_ascii_case("php"))
}

/// Return a static PHP include target, omitting dynamic or ambiguous expressions.
fn php_static_include_target(node: Node<'_>, content: &str) -> Option<String> {
    let mut expression = first_named_child(node)?;
    if expression.kind() == "parenthesized_expression" {
        let mut cursor = expression.walk();
        let mut children = expression.named_children(&mut cursor);
        let inner = children.next()?;
        if children.next().is_some() {
            return None;
        }
        expression = inner;
    }
    let target = match expression.kind() {
        "string" | "encapsed_string" => php_static_string_target(expression, content)?,
        _ => return None,
    };
    Some(target).filter(|target| !target.is_empty() && target.chars().count() <= MAX_SNIPPET_CHARS)
}

/// Return the plain content of a PHP string literal when it has no interpolation.
fn php_static_string_target(node: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    let mut target = String::new();
    let mut has_part = false;
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "string_content" => target.push_str(&named_text(child, content)?),
            "escape_sequence" if node.kind() == "string" => {
                match node_text(child, content)?.as_str() {
                    r"\\" => target.push('\\'),
                    r"\'" => target.push('\''),
                    _ => return None,
                }
            }
            "escape_sequence" if node.kind() == "encapsed_string" => {
                target.push_str(php_double_quoted_escape_target(child, content)?);
            }
            _ => return None,
        }
        has_part = true;
    }
    has_part.then_some(target)
}

/// Decode one grammar-recognized, non-interpolating PHP double-quoted escape.
fn php_double_quoted_escape_target(node: Node<'_>, content: &str) -> Option<&'static str> {
    match node_text(node, content)?.as_str() {
        r"\\" => Some("\\"),
        r#"\""# => Some("\""),
        r"\n" => Some("\n"),
        r"\r" => Some("\r"),
        r"\t" => Some("\t"),
        r"\v" => Some("\x0b"),
        r"\e" => Some("\x1b"),
        r"\f" => Some("\x0c"),
        r"\$" => Some("$"),
        r"\`" => Some("`"),
        _ => None,
    }
}

/// Return every static namespace path from a PHP `use` declaration.
fn php_namespace_use_targets(node: Node<'_>, content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut prefix = None;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if targets.len() >= MAX_RELATIONS_PER_FILE {
            break;
        }
        match child.kind() {
            "namespace_use_clause" => {
                if let Some(target) = php_namespace_use_clause_target(child, content, None) {
                    targets.push(target);
                }
            }
            "namespace_name" => prefix = named_text(child, content),
            "namespace_use_group" => {
                php_namespace_use_group_targets(child, content, prefix.as_deref(), &mut targets);
            }
            _ => {}
        }
    }
    targets
}

/// Collect the clauses in a grouped PHP `use` declaration.
fn php_namespace_use_group_targets(
    node: Node<'_>,
    content: &str,
    prefix: Option<&str>,
    targets: &mut Vec<String>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if targets.len() >= MAX_RELATIONS_PER_FILE {
            break;
        }
        if child.kind() == "namespace_use_clause"
            && let Some(target) = php_namespace_use_clause_target(child, content, prefix)
        {
            targets.push(target);
        }
    }
}

/// Compose one PHP namespace-use clause with an optional grouped prefix.
fn php_namespace_use_clause_target(
    node: Node<'_>,
    content: &str,
    prefix: Option<&str>,
) -> Option<String> {
    let target = first_named_child(node).and_then(|child| {
        matches!(child.kind(), "name" | "qualified_name" | "relative_name")
            .then(|| named_text(child, content))
            .flatten()
    })?;
    let target = match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}\\{target}"),
        _ => target,
    };
    (target.chars().count() <= MAX_SNIPPET_CHARS).then_some(target)
}

/// Return the first static namespace path from a PHP `use` declaration.
fn php_namespace_use_target(node: Node<'_>, content: &str) -> Option<String> {
    php_namespace_use_targets(node, content).into_iter().next()
}

/// Return a conservative PHP call target, suppressing dynamic calls.
fn php_call_target(node: Node<'_>, content: &str) -> Option<String> {
    if node.kind() == "function_call_expression"
        && node
            .child_by_field_name("function")
            .and_then(|function| named_text(function, content))
            .is_some_and(|name| name.eq_ignore_ascii_case("eval"))
    {
        return None;
    }
    let target = match node.kind() {
        "scoped_call_expression" => {
            let scope = node.child_by_field_name("scope")?;
            let name = node.child_by_field_name("name")?;
            let scope = php_static_call_part(scope, content)?;
            let name = php_static_call_part(name, content)?;
            format!("{scope}::{name}")
        }
        "member_call_expression" | "nullsafe_member_call_expression" => {
            let name = node.child_by_field_name("name")?;
            php_static_call_part(name, content)?
        }
        _ => php_static_call_part(node.child_by_field_name("function")?, content)?,
    };
    let target = compact_text(&target);
    (!target.is_empty() && target.chars().count() <= MAX_SNIPPET_CHARS).then_some(target)
}

/// Return a static PHP name-like call component, excluding variables and expressions.
fn php_static_call_part(node: Node<'_>, content: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "dynamic_variable_name"
            | "variable_name"
            | "expression"
            | "parenthesized_expression"
            | "member_call_expression"
            | "nullsafe_member_call_expression"
            | "function_call_expression"
            | "scoped_call_expression"
    ) {
        return None;
    }
    matches!(
        node.kind(),
        "name" | "qualified_name" | "relative_name" | "relative_scope" | "identifier"
    )
    .then(|| named_text(node, content))
    .flatten()
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

/// Return whether a node has a direct named child of the requested kind.
fn has_direct_child_kind(node: Node<'_>, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| child.kind() == kind)
}

/// Push an import relation from an import node.
fn push_import_relation(graph: &mut SymbolGraph, node: Node<'_>, content: &str) {
    if node.kind() == "namespace_use_declaration" {
        for import_text in php_namespace_use_targets(node, content) {
            if graph.relations.len() >= MAX_RELATIONS_PER_FILE {
                break;
            }
            if !import_text.is_empty() && import_text.chars().count() <= MAX_SNIPPET_CHARS {
                push_relation(
                    graph,
                    "<module>",
                    &import_text,
                    RelationKind::Imports,
                    node.start_position().row + 1,
                    &import_text,
                );
            }
        }
        return;
    }
    let import_text = if is_php_include_node(node.kind()) {
        php_static_include_target(node, content)
    } else {
        Some(compact_text(
            node_text(node, content).as_deref().unwrap_or(""),
        ))
    };
    let Some(import_text) = import_text else {
        return;
    };
    if import_text.is_empty() || import_text.chars().count() > MAX_SNIPPET_CHARS {
        return;
    }
    if is_php_include_node(node.kind()) {
        push_relation_preserving_target(
            graph,
            "<module>",
            &import_text,
            RelationKind::Imports,
            node.start_position().row + 1,
            &import_text,
        );
    } else {
        push_relation(
            graph,
            "<module>",
            &import_text,
            RelationKind::Imports,
            node.start_position().row + 1,
            &import_text,
        );
    }
}

/// Push a call relation from a call node.
fn push_call_relation(graph: &mut SymbolGraph, node: Node<'_>, content: &str) {
    if is_php_language(graph.language.as_deref())
        && node
            .child_by_field_name("arguments")
            .is_some_and(|arguments| has_direct_child_kind(arguments, "variadic_placeholder"))
    {
        return;
    }
    let target_node = node
        .child_by_field_name("function")
        .or_else(|| first_named_child(node));
    let Some(target_node) = target_node else {
        return;
    };
    let target = if is_php_language(graph.language.as_deref()) {
        let Some(target) = php_call_target(node, content) else {
            return;
        };
        target
    } else {
        compact_text(node_text(target_node, content).as_deref().unwrap_or(""))
    };
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
    if graph.language.as_deref() == Some("rust")
        && rust_target_invokes_function_item(target_node, content)
        && let Some(arguments) = node.child_by_field_name("arguments")
        && let Some(callback) = first_named_child(arguments)
        && callback.kind() == "scoped_identifier"
    {
        let callback = compact_text(node_text(callback, content).as_deref().unwrap_or(""));
        if !callback.is_empty() && callback.len() <= MAX_SNIPPET_CHARS {
            push_relation(
                graph,
                &source,
                &callback,
                RelationKind::Calls,
                node.start_position().row + 1,
                &context,
            );
        }
    }
}

/// Return whether one Rust method target proves that its function-item argument is invoked.
fn rust_target_invokes_function_item(target: Node<'_>, content: &str) -> bool {
    if target.kind() != "field_expression"
        || target
            .child_by_field_name("field")
            .and_then(|field| node_text(field, content))
            .as_deref()
            != Some("then")
    {
        return false;
    }
    target
        .child_by_field_name("value")
        .is_some_and(|receiver| rust_expression_is_definitely_bool(receiver, content))
}

/// Recognize Rust expressions whose syntax itself guarantees a Boolean value.
fn rust_expression_is_definitely_bool(mut expression: Node<'_>, content: &str) -> bool {
    while expression.kind() == "parenthesized_expression" {
        let Some(inner) = first_named_child(expression) else {
            return false;
        };
        expression = inner;
    }
    match expression.kind() {
        "boolean_literal" => true,
        "binary_expression" => {
            let (Some(left), Some(right)) = (
                expression.child_by_field_name("left"),
                expression.child_by_field_name("right"),
            ) else {
                return false;
            };
            content
                .get(left.end_byte()..right.start_byte())
                .is_some_and(|operator| {
                    matches!(
                        operator.trim(),
                        "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
                    )
                })
        }
        _ => false,
    }
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
        kind if is_import_node(kind) => import_declaration_name(node, content),
        "namespace_definition" => node
            .child_by_field_name("name")
            .and_then(|name| named_text(name, content)),
        "property_declaration"
        | "property_element"
        | "property_promotion_parameter"
        | "const_declaration"
        | "const_element"
        | "enum_case" => php_declaration_name(node, content),
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
        | "field_declaration"
        | "variable_declaration"
        | "variable_statement"
        | "var_declaration" => first_variable_declarator_name(node, content),
        _ => None,
    }
}

/// Extract the semantic target of an import-like declaration.
fn import_declaration_name(node: Node<'_>, content: &str) -> Option<String> {
    if node.kind() == "namespace_use_declaration" {
        return php_namespace_use_target(node, content);
    }
    if is_php_include_node(node.kind()) {
        return php_static_include_target(node, content);
    }
    if node.kind() == "import_spec_list" {
        let mut cursor = node.walk();
        let mut children = node.named_children(&mut cursor);
        let only_child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        return import_declaration_name(only_child, content);
    }
    for field_name in ["argument", "source", "module_name", "path", "name"] {
        if let Some(target) = node.child_by_field_name(field_name)
            && let Some(name) = named_text(target, content)
        {
            return Some(name);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "import_spec" | "import_spec_list")
            && let Some(name) = import_declaration_name(child, content)
        {
            return Some(name);
        }
        if matches!(
            child.kind(),
            "identifier"
                | "scoped_identifier"
                | "dotted_name"
                | "string"
                | "string_literal"
                | "system_lib_string"
                | "type"
        ) && let Some(name) = named_text(child, content)
        {
            return Some(name);
        }
    }
    None
}

/// Extract a PHP property, constant, or enum-case name without initializer text.
fn php_declaration_name(node: Node<'_>, content: &str) -> Option<String> {
    if let Some(name) = node.child_by_field_name("name")
        && let Some(name) = named_text(name, content)
    {
        return Some(name.trim_start_matches('$').to_string());
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "name" | "variable_name")
            && let Some(name) = named_text(child, content)
        {
            return Some(name.trim_start_matches('$').to_string());
        }
        if matches!(child.kind(), "property_element" | "const_element")
            && let Some(name) = php_declaration_name(child, content)
        {
            return Some(name);
        }
    }
    None
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
        if matches!(child.kind(), "variable_declarator" | "identifier")
            && let Some(name) = declarator_name(child, content)
        {
            return Some(name);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declaration"
            && let Some(name) = first_variable_declarator_name(child, content)
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
    if matches!(node.kind(), "property_element" | "const_element") {
        return php_element_signature(node, content);
    }
    let header_end = declaration_body_start(node).unwrap_or_else(|| node.end_byte());
    let mut signature = String::new();
    append_declaration_tokens(node, content, header_end, &mut signature);
    if signature.is_empty() {
        node_text(node, content).map_or_else(String::new, |raw| compact_text(&raw))
    } else {
        signature
    }
}

/// Build a PHP property or constant element signature with its declaration header.
fn php_element_signature(node: Node<'_>, content: &str) -> String {
    let Some(parent) = node.parent() else {
        return node_text(node, content).map_or_else(String::new, |raw| compact_text(&raw));
    };
    let mut cursor = parent.walk();
    let first_element_start = parent
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "property_element" | "const_element"))
        .map_or(node.start_byte(), |child| child.start_byte());
    let mut signature = String::new();
    append_declaration_tokens(parent, content, first_element_start, &mut signature);
    let element_end = declaration_body_start(node).unwrap_or_else(|| node.end_byte());
    append_declaration_tokens(node, content, element_end, &mut signature);
    if signature.is_empty() {
        node_text(node, content).map_or_else(String::new, |raw| compact_text(&raw))
    } else {
        signature
    }
}

/// Return the byte at which executable or member body syntax begins.
fn declaration_body_start(node: Node<'_>) -> Option<usize> {
    if matches!(
        node.kind(),
        "property_declaration"
            | "property_element"
            | "const_declaration"
            | "const_element"
            | "enum_case"
    ) && let Some(initializer) = php_initializer_start(node)
    {
        return Some(initializer);
    }
    if declaration_has_direct_callable_initializer(node)
        && let Some(initializer) = first_declaration_initializer(node)
        && let Some(body) = initializer.child_by_field_name("body")
    {
        return Some(body.start_byte());
    }
    if declaration_kind(node.kind()) == Some(SymbolKind::Value)
        && let Some(initializer) = first_declaration_initializer(node)
    {
        return Some(initializer.start_byte());
    }
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body.start_byte());
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| {
            matches!(
                child.kind(),
                "block" | "compound_statement" | "statement_block"
            ) || child.kind().ends_with("_body")
        })
        .map(|body| body.start_byte())
}

/// Return the first byte of a PHP value initializer, if one is present.
fn php_initializer_start(node: Node<'_>) -> Option<usize> {
    let mut cursor = node.walk();
    let mut after_equals = false;
    for child in node.children(&mut cursor) {
        if after_equals && child.is_named() {
            return Some(child.start_byte());
        }
        after_equals = child.kind() == "=";
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(php_initializer_start)
}

/// Append non-comment leaf tokens before a declaration body in source order.
fn append_declaration_tokens(
    node: Node<'_>,
    content: &str,
    header_end: usize,
    signature: &mut String,
) {
    if node.start_byte() >= header_end || node.kind().contains("comment") {
        return;
    }
    if node.child_count() == 0 {
        if node.end_byte() <= header_end
            && let Ok(token) = node.utf8_text(content.as_bytes())
        {
            let token = token.trim();
            if !token.is_empty() {
                if !signature.is_empty() {
                    signature.push(' ');
                }
                signature.push_str(token);
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        append_declaration_tokens(child, content, header_end, signature);
    }
}

/// Return whether a declaration is exported or publicly visible.
fn is_exported_symbol(
    language: Option<&str>,
    node: Node<'_>,
    content: &str,
    name: &str,
    signature: &str,
) -> bool {
    if is_php_language(language) {
        return php_declaration_is_exported(node, content);
    }
    let trimmed = signature.trim_start();
    trimmed.starts_with("pub ")
        || trimmed.starts_with("pub(")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("public ")
        || trimmed.starts_with("open ")
        || matches!(language, Some("go")) && starts_with_uppercase(name)
}

/// Return whether a PHP declaration has no private or protected visibility modifier.
fn php_declaration_is_exported(node: Node<'_>, content: &str) -> bool {
    if is_import_node(node.kind()) {
        return false;
    }
    let declaration = match node.kind() {
        "property_element" | "const_element" => node.parent(),
        _ => Some(node),
    };
    let Some(declaration) = declaration else {
        return true;
    };
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "visibility_modifier")
        .find_map(|modifier| node_text(modifier, content))
        .is_none_or(|modifier| {
            let modifier = modifier.trim();
            !modifier.eq_ignore_ascii_case("private") && !modifier.eq_ignore_ascii_case("protected")
        })
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
#[cfg(test)]
fn extract_fallback_graph(path: &str, language: Option<&str>, content: &str) -> SymbolGraph {
    match extract_fallback_graph_checked(path, language, content, &mut || Ok::<(), Infallible>(()))
    {
        Ok(graph) => graph,
        Err(unreachable) => match unreachable {},
    }
}

/// Extract fallback symbols while observing cooperative parser control.
fn extract_fallback_graph_checked<E>(
    path: &str,
    language: Option<&str>,
    content: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SymbolGraph, E> {
    check()?;
    let mut graph = empty_graph(path, language, ParserKind::Fallback);
    let patterns = fallback_patterns();
    check()?;
    for (line_index, line) in content.lines().enumerate() {
        check_parser_iteration(line_index, check)?;
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
    check()?;
    languages::augment_fallback_language_graph(&mut graph, content, check)?;
    check()?;
    Ok(graph)
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
            r"^function\s+([A-Za-z_][A-Za-z0-9_]*(?:-[A-Za-z_][A-Za-z0-9_]*)+)\b",
            SymbolKind::Function,
            "fallback-powershell-function",
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
) -> bool {
    if graph.symbols.len() >= MAX_SYMBOLS_PER_FILE {
        return false;
    }
    let Some(cleaned_name) = compact_symbol_identity(name) else {
        return false;
    };
    let parent = parent.and_then(|parent| compact_symbol_identity(&parent));
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
        source_selector: None,
        parent,
        parser: graph.parser,
        detail: detail.map(ToString::to_string),
    });
    true
}

/// Return one compact identity that can be represented by every graph consumer.
fn compact_symbol_identity(value: &str) -> Option<String> {
    let value = compact_text(value);
    (!value.is_empty()
        && value.chars().count() <= MAX_SNIPPET_CHARS
        && !value.starts_with(QUALIFIED_SYMBOL_SCOPE_PREFIX))
    .then_some(value)
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

/// Push a bounded relation while preserving a target already decoded by a mapper.
fn push_relation_preserving_target(
    graph: &mut SymbolGraph,
    source_name: &str,
    target_name: &str,
    kind: RelationKind,
    line: usize,
    context: &str,
) {
    if graph.relations.len() >= MAX_RELATIONS_PER_FILE
        || target_name.is_empty()
        || target_name.chars().count() > MAX_SNIPPET_CHARS
        || context.chars().count() > MAX_SNIPPET_CHARS
    {
        return;
    }
    graph.relations.push(SymbolRelation {
        path: graph.path.clone(),
        source_name: truncate_chars_at_boundary(&compact_text(source_name), MAX_SNIPPET_CHARS),
        target_name: target_name.to_string(),
        kind,
        line,
        context: context.to_string(),
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
    use super::{
        MAX_SNIPPET_CHARS, MAX_SYMBOLS_PER_FILE, PhpNamespaceContext,
        QUALIFIED_SYMBOL_SCOPE_PREFIX, compact_symbol_identity,
        content_without_leading_purpose_header, empty_graph, extract_cargo_manifest_graph_checked,
        extract_fallback_graph, extract_fallback_graph_checked, extract_powershell_graph_checked,
        extract_symbol_graph, extract_symbol_graph_checked, extract_symbol_graph_controlled,
        extract_vue_sfc_graph_checked, languages, specialized_languages,
    };
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolSourceSelector,
    };
    use projectatlas_core::{
        IndexCancellation, IndexWorkControl, IndexWorkFailure, IndexWorkStage,
    };
    use std::convert::Infallible;
    use std::fmt::Write as _;

    fn tree_symbol<'a>(
        graph: &'a SymbolGraph,
        kind: SymbolKind,
        name: &str,
        parent: Option<&str>,
        signature_fragment: &str,
    ) -> Option<&'a CodeSymbol> {
        graph.symbols.iter().find(|symbol| {
            symbol.parser == ParserKind::TreeSitter
                && symbol.kind == kind
                && symbol.name == name
                && symbol.parent.as_deref() == parent
                && symbol.signature.contains(signature_fragment)
        })
    }

    fn large_semicolon_namespace_source(declaration_count: usize) -> String {
        let mut source = String::from(
            "<?php\nnamespace Prefix;\nfunction prefix(): void {}\nnamespace Scale;\n",
        );
        for index in 0..(declaration_count - 1) {
            assert!(writeln!(source, "function function_{index}(): void {{}}").is_ok());
        }
        source
    }

    #[test]
    fn controlled_extraction_consumes_cancellation_and_preserves_compatibility() {
        let fixtures = [
            (
                "src/lib.rs",
                Some("rust"),
                "pub struct Atlas;\nimpl Atlas { pub fn scan(&self) {} }\n",
            ),
            (
                "Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"atlas\"\n[dependencies]\nserde = \"1\"\n",
            ),
            (
                "Cargo.lock",
                Some("cargo-lock"),
                "[[package]]\nname = \"atlas\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/App.vue",
                Some("vue"),
                "const props = defineProps<{ id: string }>()\n",
            ),
            (
                "scripts/Invoke-Atlas.ps1",
                Some("powershell"),
                "function Invoke-Atlas { return 1 }\n",
            ),
            (
                "config/atlas.txt",
                Some("text"),
                "function fallbackOnly() {}\n",
            ),
            (
                "src/Service.php",
                Some("php"),
                "<?php class Service { public function run(): void {} }\n",
            ),
            (
                "src/Atlas.kt",
                Some("kotlin"),
                "package atlas\nclass Atlas {\nfun scan() {}\n}\n",
            ),
            ("build.gradle", Some("groovy"), "tasks.register('atlas')\n"),
        ];
        for &(path, language, source) in &fixtures {
            let expected = extract_symbol_graph(path, language, source);
            let active = IndexWorkControl::new(IndexCancellation::new(), None);
            assert_eq!(
                extract_symbol_graph_controlled(path, language, source, &active),
                Ok(expected),
                "controlled extraction changed compatibility output for {path}"
            );
        }

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let mut checkpoints = 0;
        let cancelled =
            extract_symbol_graph_checked("src/lib.rs", Some("rust"), fixtures[0].2, &mut || {
                checkpoints += 1;
                if checkpoints == 3 {
                    cancellation.cancel();
                }
                control.check(IndexWorkStage::SymbolParsing)
            });
        assert_eq!(
            cancelled,
            Err(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SymbolParsing,
            })
        );
        assert_eq!(checkpoints, 3);

        macro_rules! assert_parser_cancels_at {
            ($label:expr, $checkpoint:expr, $run:expr) => {{
                let cancellation = IndexCancellation::new();
                let control = IndexWorkControl::new(cancellation.clone(), None);
                let mut checkpoints = 0;
                let mut check = || {
                    checkpoints += 1;
                    if checkpoints == $checkpoint {
                        cancellation.cancel();
                    }
                    control.check(IndexWorkStage::SymbolParsing)
                };
                let cancelled = ($run)(&mut check);
                assert_eq!(
                    cancelled,
                    Err(IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::SymbolParsing,
                    }),
                    "{} did not observe cancellation inside its owned parser loop",
                    $label
                );
                assert_eq!(
                    checkpoints, $checkpoint,
                    "unexpected parser checkpoint path for {}",
                    $label
                );
            }};
        }

        assert_parser_cancels_at!("Cargo manifest", 3, |check| {
            extract_cargo_manifest_graph_checked(fixtures[1].0, fixtures[1].1, fixtures[1].2, check)
        });
        assert_parser_cancels_at!("fallback", 3, |check| {
            extract_fallback_graph_checked(fixtures[5].0, fixtures[5].1, fixtures[5].2, check)
        });
        assert_parser_cancels_at!("PHP tree-sitter", 3, |check| {
            extract_symbol_graph_checked(fixtures[6].0, fixtures[6].1, fixtures[6].2, check)
        });
        assert_parser_cancels_at!("Vue structural adapter", 8, |check| {
            extract_vue_sfc_graph_checked(fixtures[3].0, fixtures[3].1, fixtures[3].2, check)
        });
        assert_parser_cancels_at!("PowerShell structural adapter", 8, |check| {
            extract_powershell_graph_checked(fixtures[4].0, fixtures[4].1, fixtures[4].2, check)
        });
        assert_parser_cancels_at!("Markdown structural adapter", 2, |check| {
            super::markdown::extract_markdown_facts_checked(
                "# Guide\n\n[src](../src/lib.rs)\n",
                check,
            )
        });

        let mut native_augmentation =
            empty_graph(fixtures[7].0, fixtures[7].1, ParserKind::TreeSitter);
        assert_parser_cancels_at!("native language augmentation", 3, |check| {
            languages::augment_language_graph(&mut native_augmentation, fixtures[7].2, check)
                .map(|()| native_augmentation.clone())
        });

        let mut fallback_augmentation =
            empty_graph(fixtures[8].0, fixtures[8].1, ParserKind::Fallback);
        assert_parser_cancels_at!("fallback language augmentation", 4, |check| {
            languages::augment_fallback_language_graph(
                &mut fallback_augmentation,
                fixtures[8].2,
                check,
            )
            .map(|()| fallback_augmentation.clone())
        });
    }

    #[test]
    fn supplied_language_selects_specialized_symbol_owner() {
        let cargo = extract_symbol_graph(
            "config/manifest.txt",
            Some("cargo-manifest"),
            "[package]\nname = \"atlas\"\n",
        );
        assert_eq!(cargo.parser, ParserKind::Manifest);
        assert!(
            cargo
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Package && symbol.name == "atlas" })
        );

        let vue = extract_symbol_graph(
            "config/component.txt",
            Some("vue"),
            "const count = ref(0);\n",
        );
        assert_eq!(vue.parser, ParserKind::Structural);
        assert!(vue.symbols.iter().any(|symbol| {
            symbol.name == "count" && symbol.detail.as_deref() == Some("vue-composition-binding")
        }));

        let powershell = extract_symbol_graph(
            "config/script.txt",
            Some("powershell"),
            "function Get-Atlas { return 1 }\n",
        );
        assert_eq!(powershell.parser, ParserKind::Structural);
        assert!(powershell.symbols.iter().any(|symbol| {
            symbol.name == "Get-Atlas" && symbol.detail.as_deref() == Some("powershell-function")
        }));

        for (path, source, forbidden_detail) in [
            (
                "Cargo.toml",
                "[package]\nname = \"atlas\"\n",
                "cargo-package",
            ),
            (
                "src/App.vue",
                "const count = ref(0);\n",
                "vue-composition-binding",
            ),
            (
                "scripts/Get-Atlas.ps1",
                "function Get-Atlas { return 1 }\n",
                "powershell-function",
            ),
        ] {
            let overridden = extract_symbol_graph(path, Some("text"), source);
            assert_eq!(overridden.parser, ParserKind::Structural, "{path}");
            assert!(overridden.symbols.is_empty(), "{path}");
            assert!(
                overridden
                    .symbols
                    .iter()
                    .all(|symbol| symbol.detail.as_deref() != Some(forbidden_detail)),
                "{path} ignored its supplied language: {:?}",
                overridden.symbols
            );
        }

        let cargo_lock_override = extract_symbol_graph(
            "Cargo.toml",
            Some("cargo-lock"),
            "[[package]]\nname = \"atlas-lock\"\nversion = \"1.0.0\"\n",
        );
        assert!(cargo_lock_override.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Dependency && symbol.name == "atlas-lock"
        }));
    }

    #[test]
    fn unavailable_symbol_owner_does_not_run_fallback_extraction() {
        let graph = extract_symbol_graph(
            "README.md",
            Some("markdown"),
            "pub fn forged_symbol() {}\nclass ForgedType {}\n",
        );

        assert_eq!(graph.parser, ParserKind::Structural);
        assert!(graph.symbols.is_empty());
        assert!(graph.relations.is_empty());
    }

    #[test]
    fn missing_language_preserves_specialized_symbol_path_inference() {
        let cargo = extract_symbol_graph("Cargo.toml", None, "[package]\nname = \"atlas\"\n");
        assert_eq!(cargo.parser, ParserKind::Manifest);
        assert!(
            cargo
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Package)
        );

        let vue = extract_symbol_graph("src/App.vue", None, "const count = ref(0);\n");
        assert_eq!(vue.parser, ParserKind::Structural);
        assert!(
            vue.symbols
                .iter()
                .any(|symbol| { symbol.detail.as_deref() == Some("vue-composition-binding") })
        );

        let powershell = extract_symbol_graph(
            "scripts/Get-Atlas.ps1",
            None,
            "function Get-Atlas { return 1 }\n",
        );
        assert_eq!(powershell.parser, ParserKind::Structural);
        assert!(
            powershell
                .symbols
                .iter()
                .any(|symbol| { symbol.detail.as_deref() == Some("powershell-function") })
        );
    }

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
    fn declaration_signatures_ignore_location_formatting_and_body_edits() {
        let before = extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "struct Atlas;\nimpl Atlas { pub fn run(&self, value: i32) -> i32 { value + 1 } }\n",
        );
        let after = extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "\n// moved\nstruct Atlas;\nimpl Atlas {\n pub fn run(\n  &self,\n  value: i32\n ) -> i32 {\n  value.saturating_mul(20)\n }\n}\n",
        );
        let before = tree_symbol(&before, SymbolKind::Method, "run", Some("Atlas"), "i32");
        let after = tree_symbol(&after, SymbolKind::Method, "run", Some("Atlas"), "i32");
        assert!(before.is_some() && after.is_some());
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        assert_ne!(before.line_start, after.line_start);
        assert_eq!(before.signature, after.signature);
        assert!(!after.signature.contains("saturating_mul"));

        let first = extract_symbol_graph(
            "src/run.ts",
            Some("typescript"),
            "export const run = (value: number): number => { return value + 1; };",
        );
        let changed = extract_symbol_graph(
            "src/run.ts",
            Some("typescript"),
            "export const run = (value: number): number => { return value * 20; };",
        );
        let first = tree_symbol(&first, SymbolKind::Function, "run", None, "number");
        let changed = tree_symbol(&changed, SymbolKind::Function, "run", None, "number");
        assert!(first.is_some() && changed.is_some());
        let (Some(first), Some(changed)) = (first, changed) else {
            return;
        };
        assert_eq!(first.signature, changed.signature);
        assert!(!first.signature.contains("return"));
    }

    #[test]
    fn declaration_value_signatures_ignore_initializers_but_retain_declared_types() {
        for (path, language, before, initializer_changed, type_changed, name, declared_type) in [
            (
                "src/lib.rs",
                "rust",
                "pub const LIMIT: usize = 10;",
                "pub const LIMIT: usize = calculate_limit();",
                "pub const LIMIT: u64 = 10;",
                "LIMIT",
                "usize",
            ),
            (
                "src/config.ts",
                "typescript",
                "export const retryCount: number = 3;",
                "export const retryCount: number = calculateRetries();",
                "export const retryCount: string = '3';",
                "retryCount",
                "number",
            ),
        ] {
            let before = extract_symbol_graph(path, Some(language), before);
            let initializer_changed =
                extract_symbol_graph(path, Some(language), initializer_changed);
            let type_changed = extract_symbol_graph(path, Some(language), type_changed);
            let before = tree_symbol(&before, SymbolKind::Value, name, None, declared_type);
            let initializer_changed = tree_symbol(
                &initializer_changed,
                SymbolKind::Value,
                name,
                None,
                declared_type,
            );
            let type_changed = tree_symbol(&type_changed, SymbolKind::Value, name, None, "");
            assert!(before.is_some() && initializer_changed.is_some() && type_changed.is_some());
            let (Some(before), Some(initializer_changed), Some(type_changed)) =
                (before, initializer_changed, type_changed)
            else {
                return;
            };
            assert_eq!(before.signature, initializer_changed.signature);
            assert_ne!(before.signature, type_changed.signature);
            assert!(!initializer_changed.signature.contains("calculate"));
        }
    }

    #[test]
    fn declaration_identity_material_disambiguates_overloads_and_parents() {
        let rust = extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "struct Left; impl Left { fn run(&self, value: i32) {} }\nstruct Right; impl Right { fn run(&self, value: i32) {} }\n",
        );
        let left = tree_symbol(&rust, SymbolKind::Method, "run", Some("Left"), "i32");
        let right = tree_symbol(&rust, SymbolKind::Method, "run", Some("Right"), "i32");
        assert!(left.is_some() && right.is_some());
        let (Some(left), Some(right)) = (left, right) else {
            return;
        };
        assert_eq!(left.signature, right.signature);
        assert_ne!(left.parent, right.parent);

        let java = extract_symbol_graph(
            "src/Runner.java",
            Some("java"),
            "class Runner { int run(\n int value\n) { return value; } String run(\n String value\n) { return value; } }",
        );
        let numeric = tree_symbol(
            &java,
            SymbolKind::Method,
            "run",
            Some("Runner"),
            "int value",
        );
        let textual = tree_symbol(
            &java,
            SymbolKind::Method,
            "run",
            Some("Runner"),
            "String value",
        );
        assert!(numeric.is_some() && textual.is_some());
        let (Some(numeric), Some(textual)) = (numeric, textual) else {
            return;
        };
        assert_ne!(numeric.signature, textual.signature);
        assert_eq!(numeric.parent, textual.parent);
    }

    #[test]
    fn native_parser_graph_survives_when_empty() {
        let graph = extract_symbol_graph("src/empty.rs", Some("rust"), "// comment only\n");
        assert_eq!(graph.parser, ParserKind::TreeSitter);
        assert!(graph.symbols.is_empty());
        assert!(graph.relations.is_empty());
    }

    #[test]
    fn native_parser_ignores_fallback_patterns_inside_comments() {
        let graph = extract_symbol_graph(
            "src/commented.rs",
            Some("rust"),
            "/*\ndef leaked():\n    pass\nfunction leaked() {}\nimport x\n*/\n",
        );
        assert_eq!(graph.parser, ParserKind::TreeSitter);
        assert!(graph.symbols.is_empty());
        assert!(graph.relations.is_empty());

        let graph = extract_symbol_graph(
            "src/commented.ts",
            Some("typescript"),
            "/*\ndef leaked():\n    pass\nfunction leaked() {}\nimport x\n*/\n",
        );
        assert_eq!(graph.parser, ParserKind::TreeSitter);
        assert!(graph.symbols.is_empty());
        assert!(graph.relations.is_empty());
    }

    #[test]
    fn native_empty_graph_keeps_fallback_rescue() {
        let graph = extract_symbol_graph(
            "src/misdetected.rs",
            Some("rust"),
            "def rescued():\n    return 1\n",
        );
        assert_eq!(graph.parser, ParserKind::Fallback);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "rescued"
                && symbol.kind == SymbolKind::Function
                && symbol.detail.as_deref() == Some("fallback-python-function")
        }));
    }

    #[test]
    fn fallback_preserves_full_powershell_function_names() {
        let graph = extract_symbol_graph(
            "scripts/install-runtime.ps1",
            Some("powershell"),
            "class RuntimeConfig {\n}\nfunction Resolve-DefaultProjectRoot {\n}\nfunction Get-ReleaseRuntimeInstallPath {\n}\nfunction Install-ReleaseBinary {\n}\n",
        );
        assert_eq!(graph.parser, ParserKind::Structural);
        assert!(
            graph.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Class
                    && symbol.name == "RuntimeConfig"
                    && symbol.detail.as_deref() == Some("powershell-class")
            }),
            "missing PowerShell class symbol: {:?}",
            graph.symbols
        );

        for name in [
            "Resolve-DefaultProjectRoot",
            "Get-ReleaseRuntimeInstallPath",
            "Install-ReleaseBinary",
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.kind == SymbolKind::Function
                        && symbol.name == name
                        && symbol.detail.as_deref() == Some("powershell-function")
                }),
                "missing full PowerShell function name {name}: {:?}",
                graph.symbols
            );
        }
        assert!(
            !graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == "Resolve" || symbol.name == "Install"),
            "PowerShell function names must not be truncated to verbs: {:?}",
            graph.symbols
        );
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
 * Purpose: Choose a fresher catalog start so repeated app opens avoid the same opening items.
 */
import type { CatalogItem } from "@/types/catalog";
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
            "import type { DigestDraft, DeliveryChannel, CatalogDatasetItem } from \"@/catalog\";",
            56,
        );

        assert_eq!(truncated, "import type { DigestDraft, DeliveryChannel...");
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
}>(), { title: "Product" });
const emit = defineEmits<{
  select: [id: string];
}>();
const productTitleId = computed(() => props.title.toLowerCase());
const currentPriceLabel = computed(() => `$${props.title}`);
const retryCount = ref(0);
</script>
"#;
        let graph = extract_symbol_graph("src/ProductPanel.vue", Some("vue"), source);
        for expected in [
            "props",
            "emit",
            "productTitleId",
            "currentPriceLabel",
            "retryCount",
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.kind == SymbolKind::Value
                        && symbol.name == expected
                        && symbol.detail.as_deref() == Some("vue-composition-binding")
                        && symbol.parser == ParserKind::Structural
                }),
                "missing Vue Composition API binding {expected}"
            );
        }
        assert_eq!(graph.parser, ParserKind::Structural);
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports
                && relation.target_name.contains("computed")
                && relation.parser == ParserKind::Structural
        }));
        assert!(
            graph
                .symbols
                .iter()
                .all(|symbol| symbol.parser == ParserKind::Structural)
        );
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Calls
                && relation.target_name == "computed"
                && relation.parser == ParserKind::TreeSitter
        }));
    }

    #[test]
    fn extracts_embedded_html_and_svelte_facts_at_host_lines() {
        let html = r#"<main>not source</main>
<script lang="ts">
export interface ProductRecord { id: string }
export function loadProduct() { return ProductRecord; }
</script>
"#;
        let graph = extract_symbol_graph("public/index.html", Some("html"), html);
        assert_eq!(graph.parser, ParserKind::Structural);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "ProductRecord"
                && symbol.kind == SymbolKind::Interface
                && symbol.line_start == 3
                && symbol.parser == ParserKind::TreeSitter
                && symbol.language.as_deref() == Some("typescript")
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "loadProduct"
                && symbol.line_start == 4
                && symbol.parser == ParserKind::TreeSitter
        }));

        let svelte = r#"<h1>{title}</h1>
<script lang="ts">
export interface PageData { title: string }
</script>
"#;
        let graph = extract_symbol_graph("src/Page.svelte", Some("svelte"), svelte);
        assert_eq!(graph.parser, ParserKind::Fallback);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "PageData"
                && symbol.kind == SymbolKind::Interface
                && symbol.line_start == 3
                && symbol.parser == ParserKind::TreeSitter
        }));
    }

    #[test]
    fn embedded_hosts_ignore_external_and_retain_only_safe_facts_on_incomplete_input() {
        for source in [
            "<script src=\"external.js\">export function forged() {}</script>",
            "<script lang=\"ts\">export function incomplete() {}",
        ] {
            let graph = extract_symbol_graph("public/index.html", Some("html"), source);
            assert!(
                graph.symbols.is_empty(),
                "unexpected symbols: {:?}",
                graph.symbols
            );
            assert!(
                graph.relations.is_empty(),
                "unexpected relations: {:?}",
                graph.relations
            );
        }

        let source = "<script>export function admitted() {}</script>"
            .repeat(super::semantic::embedded_source::MAX_EMBEDDED_SCRIPT_REGIONS + 1);
        let graph = extract_symbol_graph("public/index.html", Some("html"), &source);
        assert!(graph.symbols.iter().any(|symbol| symbol.name == "admitted"));
        assert_eq!(graph.parser, ParserKind::Structural);
    }

    #[test]
    fn purpose_header_mask_preserves_utf8_byte_offsets_for_embedded_hosts() {
        let source = concat!(
            "<!-- Purpose: Grüße and routing -->\n",
            "<script lang=\"ts\">export const admitted = 1;</script>\n"
        );
        let masked = content_without_leading_purpose_header(source);
        assert_eq!(masked.len(), source.len());
        assert_eq!(masked.find("<script"), source.find("<script"));

        let graph = extract_symbol_graph("public/index.html", Some("html"), source);
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "admitted"
                && symbol.line_start == 2
                && symbol.parser == ParserKind::TreeSitter
        }));
    }

    #[test]
    fn vue_sfc_preserves_fallback_declarations() {
        let source = r#"
<script lang="ts">
export function submitOrder() {
  return true;
}

class Store {
}
</script>
<script setup lang="ts">
import { ref } from "vue";
const selected = ref(false);
</script>
"#;
        let graph = extract_symbol_graph("src/CheckoutPanel.vue", Some("vue"), source);

        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Value
                && symbol.name == "selected"
                && symbol.detail.as_deref() == Some("vue-composition-binding")
                && symbol.parser == ParserKind::Structural
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "submitOrder"
                && symbol.detail.as_deref() == Some("fallback-js-function")
                && symbol.parser == ParserKind::Fallback
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.name == "Store"
                && symbol.detail.as_deref() == Some("fallback-class")
                && symbol.parser == ParserKind::Fallback
        }));
    }

    #[test]
    fn vue_sfc_preserves_fallback_declarations_when_bindings_exceed_cap() {
        let mut source = String::from(
            r#"
<script setup lang="ts">
export function submitOrder() {
  return true;
}

class Store {
}
"#,
        );
        for index in 0..(MAX_SYMBOLS_PER_FILE + 50) {
            source.push_str("const value");
            source.push_str(&index.to_string());
            source.push_str(" = ref(false);\n");
        }
        source.push_str("</script>\n");

        let graph = extract_symbol_graph("src/LargePanel.vue", Some("vue"), &source);

        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "submitOrder"
                && symbol.detail.as_deref() == Some("fallback-js-function")
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Class
                && symbol.name == "Store"
                && symbol.detail.as_deref() == Some("fallback-class")
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Value
                && symbol.name == "value0"
                && symbol.detail.as_deref() == Some("vue-composition-binding")
                && symbol.parser == ParserKind::Structural
        }));
    }

    #[test]
    fn vue_composition_binding_detection_requires_macro_call_boundary() {
        let source = r#"
<script setup lang="ts">
const data = refreshData();
const value = computedValue();
const typed = ref<string>("ok");
const delayed = computed (() => typed.value);
const props = withDefaults(defineProps<{ title: string }>(), { title: "Product" });
</script>
"#;
        let graph = extract_symbol_graph("src/Widget.vue", Some("vue"), source);

        for absent in ["data", "value"] {
            assert!(
                graph.symbols.iter().all(|symbol| symbol.name != absent),
                "ordinary function call {absent} was incorrectly treated as a Vue binding"
            );
        }
        for expected in ["typed", "delayed", "props"] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.kind == SymbolKind::Value
                        && symbol.name == expected
                        && symbol.detail.as_deref() == Some("vue-composition-binding")
                }),
                "missing Vue macro binding {expected}"
            );
        }
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
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Import && symbol.name == "\"fmt\"" })
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
    fn csharp_field_identity_is_stable_across_large_initializer_boundary() {
        for entry_count in [224, 225] {
            let mut entries = String::new();
            for index in 0..entry_count {
                let result = write!(entries, "[\"key{index}\"] = \"value{index}\",");
                assert!(result.is_ok(), "writing to a String must succeed");
            }
            let source = format!(
                r"
using System.Collections.Generic;

public class Registry
{{
    public static readonly Dictionary<string, string> D = new()
    {{
        {entries}
    }};
}}
"
            );

            let graph = extract_symbol_graph("Registry.cs", Some("csharp"), &source);

            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind == SymbolKind::Value && symbol.name == "D"),
                "missing exact D identity with {entry_count} initializer entries"
            );
            assert!(
                graph
                    .symbols
                    .iter()
                    .all(|symbol| !symbol.name.contains("Dictionary") && !symbol.name.contains('=')),
                "complete declaration became an identity with {entry_count} initializer entries"
            );
        }
    }

    #[test]
    fn invalid_csharp_field_identities_do_not_hide_valid_siblings() {
        let admitted_unicode_name = "名".repeat(MAX_SNIPPET_CHARS);
        let overbound_unicode_name = "名".repeat(MAX_SNIPPET_CHARS + 1);
        let source = format!(
            r"
using System.Collections.Generic;

public class Registry
{{
    public int Before = 1;
    public int {admitted_unicode_name} = 2;
    public int {overbound_unicode_name} = 3;
    public static readonly Dictionary<string, string> = new();
    public int After = 4;
}}
"
        );

        let graph = extract_symbol_graph("Registry.cs", Some("csharp"), &source);

        for expected in ["Before", admitted_unicode_name.as_str(), "After"] {
            assert!(
                graph.symbols.iter().any(|symbol| symbol.name == expected),
                "missing valid sibling {expected}"
            );
        }
        assert!(
            !graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == overbound_unicode_name),
            "overbound Unicode identity was admitted"
        );
        assert!(
            graph.symbols.iter().all(|symbol| {
                symbol.name.chars().count() <= MAX_SNIPPET_CHARS
                    && !symbol.name.contains("Dictionary")
                    && !symbol.name.contains('=')
            }),
            "unnameable declaration or overbound identity leaked into the graph"
        );
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == admitted_unicode_name
                    && symbol.name.len() == admitted_unicode_name.len()),
            "admitted Unicode identity was not preserved exactly"
        );
    }

    #[test]
    fn invalid_parent_identity_detaches_valid_child() {
        let overbound_parent = "P".repeat(MAX_SNIPPET_CHARS + 1);
        let source = format!(
            "public class {overbound_parent} {{ public void Retained() {{}} }}\n\
             public class Valid {{ public void Sibling() {{}} }}\n"
        );

        let graph = extract_symbol_graph("Parents.cs", Some("csharp"), &source);

        assert!(
            !graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == overbound_parent)
        );
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.name == "Retained" && symbol.parent.is_none() })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.name == "Sibling" && symbol.parent.as_deref() == Some("Valid")
        }));
        assert!(!graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Contains && relation.target_name == "Retained"
        }));
    }

    #[test]
    fn derived_qualified_scope_namespace_is_reserved_from_source_symbols() {
        let reserved = format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}literal");
        assert!(compact_symbol_identity(&reserved).is_none());
        assert_eq!(
            compact_symbol_identity("ordinary"),
            Some("ordinary".to_string())
        );
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

        for path in ["src/Worker.kt", "scripts/tasks.kts"] {
            let ordinary_kotlin = extract_symbol_graph(
                path,
                Some("kotlin"),
                r#"
class Worker {
    fun queue(tasks: TaskContainer) {
        tasks.register("notGradleTask")
    }
}
"#,
            );
            assert!(
                !ordinary_kotlin.symbols.iter().any(|symbol| {
                    symbol.name == "notGradleTask"
                        || symbol.detail.as_deref() == Some("gradle-kotlin-dsl-task")
                }),
                "ordinary Kotlin path {path} should not emit Gradle task symbols: {:?}",
                ordinary_kotlin.symbols
            );
        }

        let gradle_kotlin = extract_symbol_graph(
            "build.gradle.kts",
            Some("kotlin"),
            r#"
import org.springframework.boot.gradle.tasks.run.BootRun

fun loadDotEnv() = emptyMap<String, String>()

tasks.register<BootRun>("bootRunE2E") {
    group = "verification"
}

val verifyAtlas by tasks.registering {
    group = "verification"
}

tasks {
    register<Copy>("copyE2EReports") {
        group = "verification"
    }
}

task("publishKtsE2E") {}
"#,
        );
        assert_eq!(gradle_kotlin.parser, ParserKind::TreeSitter);
        for task in [
            "bootRunE2E",
            "copyE2EReports",
            "verifyAtlas",
            "publishKtsE2E",
        ] {
            assert!(gradle_kotlin.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Function
                    && symbol.name == task
                    && symbol.detail.as_deref() == Some("gradle-kotlin-dsl-task")
            }));
        }
        let fallback_gradle_kotlin = extract_fallback_graph(
            "build.gradle.kts",
            Some("kotlin"),
            r#"
tasks.register<BootRun>("bootRunE2E") {
    group = "verification"
}

fun broken(
"#,
        );
        assert_eq!(fallback_gradle_kotlin.parser, ParserKind::Fallback);
        assert!(
            fallback_gradle_kotlin.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Function
                    && symbol.name == "bootRunE2E"
                    && symbol.detail.as_deref() == Some("gradle-kotlin-dsl-task")
            }),
            "fallback Gradle KTS graph should retain task symbols: {:?}",
            fallback_gradle_kotlin.symbols
        );

        let gradle_groovy = extract_symbol_graph(
            "build.gradle",
            Some("groovy"),
            r"
plugins { id 'java' }

tasks.register('bootRunSmoke', BootRun) {
    group = 'verification'
}

task cleanE2E(type: Delete) {}

tasks {
    create('copyGroovyReports') {
        group = 'verification'
    }
}

task('publishE2E') {}
",
        );
        assert_eq!(gradle_groovy.parser, ParserKind::Fallback);
        for task in [
            "bootRunSmoke",
            "cleanE2E",
            "copyGroovyReports",
            "publishE2E",
        ] {
            assert!(gradle_groovy.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Function
                    && symbol.name == task
                    && symbol.detail.as_deref() == Some("gradle-groovy-dsl-task")
            }));
        }

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
                && symbol.signature.contains("run")
                && !symbol.signature.contains('{')
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
    fn cargo_lock_duplicate_package_names_keep_distinct_lines() {
        let source = r#"[[package]]
name = "windows-sys"
version = "0.59.0"

[[package]]
name = "windows-sys"
version = "0.60.0"
"#;
        let graph = extract_symbol_graph("Cargo.lock", Some("cargo-lock"), source);
        let lines = graph
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Dependency && symbol.name == "windows-sys")
            .map(|symbol| symbol.line_start)
            .collect::<Vec<_>>();
        assert_eq!(lines, vec![2, 6]);
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
            "php",
        ] {
            assert!(specialized_languages().contains(&expected));
        }
    }

    #[test]
    fn extracts_php_symbols_relations_and_exact_selectors() {
        let source = r#"<?php
namespace Atlas\Domain;
use Vendor\Thing as ThingAlias;
require_once "bootstrap.php";
include $dynamic;
interface Contract {}
trait Auditable {}
enum State: string { case Ready = 'ready'; }
class Service {
    public const VERSION = 1;
    private string $name = 'service';
    public function run(string $value): string {
        helper();
        $this->save();
        Service::boot();
    }
}
function helper(string $value): void {}
"#;
        let graph = extract_symbol_graph("src/Service.php", Some("php"), source);
        assert_eq!(graph.parser, ParserKind::TreeSitter);

        for name in [
            "Atlas\\Domain",
            "Contract",
            "Auditable",
            "State",
            "Ready",
            "Service",
            "VERSION",
            "name",
            "run",
            "helper",
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| symbol.name == name),
                "missing PHP symbol {name}: {:?}",
                graph.symbols
            );
        }
        for (name, kind, parent) in [
            ("Contract", SymbolKind::Interface, Some("Atlas\\Domain")),
            ("Auditable", SymbolKind::Trait, Some("Atlas\\Domain")),
            ("State", SymbolKind::Enum, Some("Atlas\\Domain")),
            ("Ready", SymbolKind::Value, Some("State")),
            ("Service", SymbolKind::Class, Some("Atlas\\Domain")),
            ("VERSION", SymbolKind::Value, Some("Service")),
            ("name", SymbolKind::Value, Some("Service")),
            ("run", SymbolKind::Method, Some("Service")),
            ("helper", SymbolKind::Function, Some("Atlas\\Domain")),
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| {
                    symbol.name == name && symbol.kind == kind && symbol.parent.as_deref() == parent
                }),
                "missing PHP kind/parent for {name}: {:?}",
                graph.symbols
            );
        }
        assert!(
            source.contains("public function run"),
            "method start missing from PHP fixture"
        );
        let method_start = source.find("public function run").unwrap_or_default();
        assert!(
            source[method_start..].contains("\n    }\n"),
            "method end missing from PHP fixture"
        );
        let method_relative_end = source[method_start..].find("\n    }\n").unwrap_or_default();
        let method_end = method_start + method_relative_end + 6;
        assert!(
            graph.symbols.iter().any(|symbol| symbol.name == "run"),
            "method symbol missing from PHP graph"
        );
        let Some(method) = graph.symbols.iter().find(|symbol| symbol.name == "run") else {
            return;
        };
        assert_eq!(
            method.source_selector,
            Some(SymbolSourceSelector {
                byte_start: method_start,
                byte_end: method_end,
                column_start: 4,
                column_end: 5,
            })
        );
        assert!(method.signature.contains("public function run"));
        assert!(!method.signature.contains("helper"));
        assert!(
            graph.symbols.iter().any(|symbol| symbol.name == "name"),
            "property symbol missing from PHP graph"
        );
        let Some(property) = graph.symbols.iter().find(|symbol| symbol.name == "name") else {
            return;
        };
        assert!(property.signature.contains("private string"));
        assert!(!property.signature.contains("service"));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports && relation.target_name == "Vendor\\Thing"
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports && relation.target_name == "bootstrap.php"
        }));
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Import && symbol.name == "Vendor\\Thing" && !symbol.exported
        }));
        for target in ["helper", "save", "Service::boot"] {
            assert!(
                graph.relations.iter().any(|relation| {
                    relation.kind == RelationKind::Calls && relation.target_name == target
                }),
                "missing PHP call target {target}: {:?}",
                graph.relations
            );
        }
        assert!(
            graph
                .relations
                .iter()
                .all(|relation| relation.target_name != "$dynamic")
        );

        let multiple = extract_symbol_graph(
            "src/Multiple.php",
            Some("php"),
            "<?php class Multiple { public string $first = 'one', $second = 'two'; const FIRST = 1, SECOND = 2; }",
        );
        for (name, signature) in [
            ("first", "public string $ first ="),
            ("second", "public string $ second ="),
            ("FIRST", "const FIRST ="),
            ("SECOND", "const SECOND ="),
        ] {
            assert!(
                multiple
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == name && symbol.signature == signature),
                "missing PHP element {name} with signature {signature}: {:?}",
                multiple.symbols
            );
        }

        let braced = extract_symbol_graph(
            "src/Braced.php",
            Some("php"),
            "<?php namespace Atlas { class Service { public function run(): void {} } }",
        );
        assert!(braced.symbols.iter().any(|symbol| {
            symbol.name == "Service" && symbol.parent.as_deref() == Some("Atlas")
        }));
        assert!(
            braced.symbols.iter().any(|symbol| {
                symbol.name == "run" && symbol.parent.as_deref() == Some("Service")
            })
        );

        let duplicates = extract_symbol_graph(
            "src/duplicates.php",
            Some("php"),
            "<?php function same(): void {} function same(): void {}",
        );
        assert_eq!(
            duplicates
                .symbols
                .iter()
                .filter(|symbol| symbol.name == "same" && symbol.kind == SymbolKind::Function)
                .count(),
            2,
            "duplicate PHP declarations must remain visible rather than being merged"
        );
    }

    #[test]
    fn php_constructor_promoted_properties_are_class_members() {
        let source = r"<?php
class Account {
    public function __construct(
        public readonly string $name,
        private int $id = 0,
    ) {}
}
";
        let graph = extract_symbol_graph("src/Account.php", Some("php"), source);

        for (name, exported, signature) in [
            ("name", true, "public readonly string"),
            ("id", false, "private int"),
        ] {
            let symbol = graph
                .symbols
                .iter()
                .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Value);
            assert!(
                symbol.is_some(),
                "missing promoted property {name}: {graph:?}"
            );
            let Some(symbol) = symbol else { continue };
            assert_eq!(symbol.parent.as_deref(), Some("Account"));
            assert_eq!(symbol.exported, exported);
            assert!(symbol.signature.contains(signature));
            assert!(symbol.source_selector.is_some());
        }
        assert!(!graph.symbols.iter().any(|symbol| {
            symbol.name == "name" && symbol.parent.as_deref() == Some("__construct")
        }));
    }

    #[test]
    fn php_visibility_modifiers_control_exported_symbol_queries() {
        let source = r"<?php
class Service {
    final protected function guarded(): void {}
    static private string $cache;
    private(set) string $readablePrivateSet;
    protected(set) string $readableProtectedSet;
    public(set) string $readablePublicSet;
    public static function exposed(): void {}
    function defaulted(): void {}
}
";
        let graph = extract_symbol_graph("src/Service.php", Some("php"), source);

        for (name, exported) in [
            ("Service", true),
            ("guarded", false),
            ("cache", false),
            ("readablePrivateSet", true),
            ("readableProtectedSet", true),
            ("readablePublicSet", true),
            ("exposed", true),
            ("defaulted", true),
        ] {
            assert!(
                graph.symbols.iter().any(|symbol| symbol.name == name),
                "missing PHP symbol {name}: {:?}",
                graph.symbols
            );
            let Some(symbol) = graph.symbols.iter().find(|symbol| symbol.name == name) else {
                continue;
            };
            assert_eq!(
                symbol.exported, exported,
                "unexpected exported state for {name}: {symbol:?}"
            );
            assert_eq!(symbol.parser, ParserKind::TreeSitter);
            assert!(
                symbol.source_selector.is_some(),
                "missing selector for {name}"
            );
        }

        let exported_names = graph
            .symbols
            .iter()
            .filter(|symbol| symbol.exported)
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();
        assert!(exported_names.contains(&"exposed"));
        assert!(exported_names.contains(&"defaulted"));
        assert!(exported_names.contains(&"readablePrivateSet"));
        assert!(exported_names.contains(&"readableProtectedSet"));
        assert!(exported_names.contains(&"readablePublicSet"));
        assert!(!exported_names.contains(&"guarded"));
        assert!(!exported_names.contains(&"cache"));
    }

    #[test]
    fn php_relative_scope_calls_preserve_exact_targets_and_source_evidence() {
        let source = r"<?php
class Child extends Base {
    public function run(): void {
        self::local();
        parent::inherited();
        static::lateBound();
        $scope::dynamic();
    }
}
";
        let graph = extract_symbol_graph("src/Child.php", Some("php"), source);
        let calls = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Calls)
            .collect::<Vec<_>>();

        assert_eq!(calls.len(), 3, "dynamic PHP scopes must remain unresolved");
        for (target, line) in [
            ("self::local", 4),
            ("parent::inherited", 5),
            ("static::lateBound", 6),
        ] {
            assert!(
                calls.iter().any(|relation| {
                    relation.target_name == target && relation.source_name == "run"
                }),
                "missing PHP call relation {target}: {calls:?}"
            );
            let Some(relation) = calls
                .iter()
                .find(|relation| relation.target_name == target && relation.source_name == "run")
            else {
                continue;
            };
            assert_eq!(relation.path, "src/Child.php");
            assert_eq!(relation.line, line);
            assert!(relation.context.contains(target));
        }
        assert!(calls.iter().all(|relation| {
            !relation.target_name.contains("dynamic") && !relation.target_name.contains("scope")
        }));
    }

    #[test]
    fn php_dynamic_execution_is_not_published_as_a_call() {
        let source = r"<?php
function run(string $code): void {
    eval($code);
    $callable();
    helper();
}
";
        let graph = extract_symbol_graph("src/DynamicExecution.php", Some("php"), source);
        let calls = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Calls)
            .collect::<Vec<_>>();

        assert!(calls.iter().all(|relation| relation.target_name != "eval"));
        assert!(
            calls
                .iter()
                .all(|relation| relation.target_name != "$callable")
        );
        assert!(calls.iter().any(|relation| {
            relation.target_name == "helper"
                && relation.source_name == "run"
                && relation.path == "src/DynamicExecution.php"
                && relation.context.contains("helper()")
        }));
    }

    #[test]
    fn php_callable_acquisition_and_import_targets_stay_precise() {
        let source = r#"<?php
use Vendor\One, Vendor\Two as TwoAlias;
use Vendor\Group\{First, Second as GroupAlias};
require 'vendor\\bootstrap.php';
require 'vendor\\it\'s.php';
require 'bootstrap.php';
require "bootstrap.php";
require "boot/$name.php";
require $dynamic;
require [];
function run(): void {
    foo(...);
    foo();
    foo(...$args);
    Service::boot(...);
    Service::boot();
    $object->save(...);
    $object->save();
}
"#;
        let graph = extract_symbol_graph("src/Callable.php", Some("php"), source);

        let imports = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .collect::<Vec<_>>();
        for (target, lines) in [
            ("Vendor\\One", vec![2]),
            ("Vendor\\Two", vec![2]),
            ("Vendor\\Group\\First", vec![3]),
            ("Vendor\\Group\\Second", vec![3]),
            ("vendor\\bootstrap.php", vec![4]),
            ("vendor\\it's.php", vec![5]),
            ("bootstrap.php", vec![6, 7]),
        ] {
            assert_eq!(
                imports
                    .iter()
                    .filter(|relation| relation.target_name == target)
                    .count(),
                lines.len(),
                "missing exact PHP import target {target}: {imports:?}"
            );
            let mut observed_lines = imports
                .iter()
                .filter(|relation| relation.target_name == target)
                .map(|relation| {
                    assert_eq!(relation.path, "src/Callable.php");
                    assert_eq!(relation.context, target);
                    relation.line
                })
                .collect::<Vec<_>>();
            observed_lines.sort_unstable();
            assert_eq!(observed_lines, lines);
        }
        assert!(imports.iter().all(|relation| {
            !relation.target_name.contains("boot/")
                && !relation.target_name.contains("dynamic")
                && relation.target_name != "[]"
                && !relation.target_name.contains("TwoAlias")
                && !relation.target_name.contains("GroupAlias")
        }));

        let calls = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Calls)
            .collect::<Vec<_>>();
        for (target, lines) in [
            ("foo", vec![13, 14]),
            ("Service::boot", vec![16]),
            ("save", vec![18]),
        ] {
            assert_eq!(
                calls
                    .iter()
                    .filter(|relation| relation.target_name == target)
                    .count(),
                lines.len(),
                "callable acquisition must not be published as invocation for {target}: {calls:?}"
            );
            let mut observed_lines = calls
                .iter()
                .filter(|relation| relation.target_name == target)
                .map(|relation| {
                    assert_eq!(relation.source_name, "run");
                    assert_eq!(relation.path, "src/Callable.php");
                    assert!(relation.context.contains(target));
                    relation.line
                })
                .collect::<Vec<_>>();
            observed_lines.sort_unstable();
            assert_eq!(observed_lines, lines);
        }
        assert!(calls.iter().all(|relation| {
            !relation.context.contains("foo(...)")
                && !relation.context.contains("Service::boot(...)")
                && !relation.context.contains("$object->save(...)")
                && relation.target_name != "$dynamic"
                && relation.target_name != "$name"
        }));
    }

    #[test]
    fn php_static_include_literals_reject_constants_and_decode_double_quoted_escapes() {
        let source = r#"<?php
require "vendor\\bootstrap.php";
require "vendor\"quoted.php";
require "control\npath.php";
require "dollar\$name.php";
require "unsupported\x41.php";
require "boot/$name.php";
require $dynamic;
require BOOTSTRAP;
require Vendor\BOOTSTRAP;
require (1 + 2);
"#;
        let graph = extract_symbol_graph("src/StaticIncludes.php", Some("php"), source);
        let imports = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .collect::<Vec<_>>();

        for (target, line) in [
            ("vendor\\bootstrap.php", 2),
            ("vendor\"quoted.php", 3),
            ("control\npath.php", 4),
            ("dollar$name.php", 5),
        ] {
            let matches = imports
                .iter()
                .filter(|relation| relation.target_name == target)
                .collect::<Vec<_>>();
            assert_eq!(
                matches.len(),
                1,
                "missing exact escaped PHP include {target}: {imports:?}"
            );
            let relation = matches[0];
            assert_eq!(relation.path, "src/StaticIncludes.php");
            assert_eq!(relation.line, line);
            assert_eq!(relation.context, target);
        }
        assert!(imports.iter().all(|relation| {
            !relation.target_name.contains("unsupported")
                && !relation.target_name.contains("boot/")
                && relation.target_name != "$dynamic"
                && relation.target_name != "BOOTSTRAP"
                && !relation.target_name.contains("Vendor")
                && !relation.target_name.contains("1 + 2")
        }));
    }

    #[test]
    fn php_parenthesized_static_include_targets_stay_precise() {
        let source = r#"<?php
require('parenthesized.php');
include_once("parent-config.php");
require(('nested.php'));
require("malformed.php" + );
require("boot/$name.php");
require($dynamic);
require [];
require(BOOTSTRAP);
include_once(Vendor\BOOTSTRAP);
"#;
        let graph = extract_symbol_graph("src/Includes.php", Some("php"), source);
        let imports = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .collect::<Vec<_>>();

        for (target, line) in [("parenthesized.php", 2), ("parent-config.php", 3)] {
            let matches = imports
                .iter()
                .filter(|relation| relation.target_name == target)
                .collect::<Vec<_>>();
            assert_eq!(
                matches.len(),
                1,
                "missing exact static include {target}: {imports:?}"
            );
            let relation = matches[0];
            assert_eq!(relation.path, "src/Includes.php");
            assert_eq!(relation.line, line);
            assert_eq!(relation.context, target);
        }
        assert!(imports.iter().all(|relation| {
            !matches!(
                relation.target_name.as_str(),
                "nested.php" | "malformed.php" | "boot/$name.php" | "$dynamic" | "[]" | "BOOTSTRAP"
            ) && !relation.target_name.contains("Vendor")
        }));
    }

    #[test]
    fn php_trait_use_relations_preserve_type_ownership_and_ignore_adaptations() {
        let source = r"<?php
trait Auditable {}
trait FirstTrait {}
class Service {
    use Auditable;
    use FirstTrait, Vendor\SecondTrait {
        FirstTrait::audit insteadof Vendor\SecondTrait;
        Vendor\SecondTrait::audit as protected auditFromSecond;
    }
}
";
        let graph = extract_symbol_graph("src/Traits.php", Some("php"), source);
        let imports = graph
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::Imports)
            .collect::<Vec<_>>();

        for target in ["Auditable", "FirstTrait", "Vendor\\SecondTrait"] {
            assert!(
                imports.iter().any(|relation| {
                    relation.source_name == "Service" && relation.target_name == target
                }),
                "missing class-owned PHP trait relation {target}: {imports:?}"
            );
        }
        assert!(imports.iter().all(|relation| {
            relation.source_name != "<module>"
                && !relation.target_name.starts_with("use ")
                && !relation.target_name.contains("audit")
                && !relation.target_name.contains("protected")
        }));
        assert!(!graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Import && symbol.parent.as_deref() == Some("Service")
        }));
    }

    #[test]
    fn php_namespace_context_preserves_semicolon_and_braced_ownership() {
        let source = r"<?php
namespace First;
use Vendor\First as FirstAlias;
class FirstService {}
function first_helper(): void {}
namespace Second;
class SecondService {}
function second_helper(): void {}
namespace Third { class BracedService {} }
namespace Fourth;
class FourthService {}
namespace { class GlobalService {} }
class OutsideGlobal {}
";
        let graph = extract_symbol_graph("src/Namespaces.php", Some("php"), source);

        for (name, parent) in [
            ("Vendor\\First", "First"),
            ("FirstService", "First"),
            ("first_helper", "First"),
            ("SecondService", "Second"),
            ("second_helper", "Second"),
            ("BracedService", "Third"),
            ("FourthService", "Fourth"),
        ] {
            let symbol = graph.symbols.iter().find(|symbol| symbol.name == name);
            assert!(
                symbol.is_some(),
                "missing PHP symbol {name}: {:?}",
                graph.symbols
            );
            let Some(symbol) = symbol else { return };
            assert_eq!(
                symbol.parent.as_deref(),
                Some(parent),
                "wrong parent for {name}"
            );
            assert!(graph.relations.iter().any(|relation| {
                relation.kind == RelationKind::Contains
                    && relation.source_name == parent
                    && relation.target_name == name
            }));
        }

        let global_symbol = graph
            .symbols
            .iter()
            .find(|symbol| symbol.name == "GlobalService");
        assert!(
            global_symbol.is_some(),
            "missing global PHP symbol: {:?}",
            graph.symbols
        );
        let Some(global_symbol) = global_symbol else {
            return;
        };
        assert!(global_symbol.parent.is_none());
        assert!(!graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Contains && relation.target_name == "GlobalService"
        }));

        let outside_global = graph
            .symbols
            .iter()
            .find(|symbol| symbol.name == "OutsideGlobal");
        assert_eq!(
            outside_global.and_then(|symbol| symbol.parent.as_deref()),
            None
        );

        let malformed = extract_symbol_graph(
            "src/MalformedNamespace.php",
            Some("php"),
            "<?php\nnamespace Before;\nclass BeforeService {}\nnamespace Broken\\;\nclass AfterMalformed {}\n",
        );
        assert!(malformed.symbols.iter().any(|symbol| {
            symbol.name == "BeforeService" && symbol.parent.as_deref() == Some("Before")
        }));
        assert!(
            malformed
                .symbols
                .iter()
                .any(|symbol| { symbol.name == "AfterMalformed" && symbol.parent.is_none() })
        );
        assert!(!malformed.relations.iter().any(|relation| {
            relation.kind == RelationKind::Contains && relation.target_name == "AfterMalformed"
        }));
    }

    #[test]
    fn php_conditional_namespace_declarations_preserve_scope_without_crossing_symbol_owners() {
        let source = r"<?php
namespace Conditional;
if ($enabled) {
    function boot(): void {}
    class ConditionalService {}
}
class Owner {
    public function run(): void {
        if ($enabled) {
            function nested(): void {}
        }
    }
}
";
        let graph = extract_symbol_graph("src/Conditional.php", Some("php"), source);

        for name in ["boot", "ConditionalService"] {
            let symbol = graph.symbols.iter().find(|symbol| symbol.name == name);
            assert!(symbol.is_some(), "missing conditional PHP symbol {name}");
            let Some(symbol) = symbol else { return };
            assert_eq!(symbol.parent.as_deref(), Some("Conditional"));
            assert!(graph.relations.iter().any(|relation| {
                relation.kind == RelationKind::Contains
                    && relation.source_name == "Conditional"
                    && relation.target_name == name
            }));
        }

        for (name, parent) in [("run", "Owner"), ("nested", "run")] {
            let symbol = graph.symbols.iter().find(|symbol| symbol.name == name);
            assert!(symbol.is_some(), "missing nested PHP symbol {name}");
            let Some(symbol) = symbol else { return };
            assert_eq!(symbol.parent.as_deref(), Some(parent));
            assert!(graph.relations.iter().any(|relation| {
                relation.kind == RelationKind::Contains
                    && relation.source_name == parent
                    && relation.target_name == name
            }));
            assert_ne!(symbol.parent.as_deref(), Some("Conditional"));
        }
    }

    #[test]
    fn php_mixed_recovery_dynamic_and_bounded_inputs_stay_conservative() {
        let mixed = extract_symbol_graph(
            "templates/page.php",
            Some("php"),
            "<main>static</main><?php function render(): void { helper(); } ?>",
        );
        assert_eq!(mixed.parser, ParserKind::TreeSitter);
        assert!(mixed.symbols.iter().any(|symbol| symbol.name == "render"));

        let pure = extract_symbol_graph(
            "src/pure.php",
            Some("php"),
            "function pure(): void { return; }",
        );
        assert_eq!(pure.parser, ParserKind::TreeSitter);
        assert!(
            pure.symbols.is_empty(),
            "tagless PHP files are inline text, not PHP-only fragments: {pure:?}"
        );

        let dynamic = extract_symbol_graph(
            "src/dynamic.php",
            Some("php"),
            "<?php $callable(); $object->$method(); include $path;",
        );
        assert!(dynamic.relations.iter().all(|relation| {
            !matches!(relation.kind, RelationKind::Calls | RelationKind::Imports)
                || ![
                    "$callable",
                    "$method",
                    "$path",
                    "callable",
                    "method",
                    "path",
                ]
                .contains(&relation.target_name.as_str())
        }));

        let malformed = extract_symbol_graph(
            "src/broken.php",
            Some("php"),
            "<?php function broken( { $unknown->();",
        );
        assert!(malformed.symbols.len() <= MAX_SYMBOLS_PER_FILE);
        assert!(malformed.relations.len() <= 8_000);
    }

    #[test]
    fn php_namespace_context_builds_one_forward_cursor_at_intended_scale() {
        let declaration_count = 8_050;
        let source = large_semicolon_namespace_source(declaration_count);
        let mut parse_check = || Ok::<(), Infallible>(());
        let parsed = super::parse_php_tree(&source, &mut parse_check)
            .ok()
            .flatten();
        assert!(parsed.is_some(), "large PHP source should have a tree");
        let Some(parsed) = parsed else { return };
        let root = parsed.tree.root_node();
        let named_child_count = root.named_child_count();
        let mut context_check = || Ok::<(), Infallible>(());
        let mut context = PhpNamespaceContext::from_program(root, &source, &mut context_check)
            .expect("namespace context should build");
        let mut cursor = root.walk();
        let mut declaration_lookups = 0;
        for child in root.named_children(&mut cursor) {
            if matches!(child.kind(), "namespace_definition" | "php_tag") {
                continue;
            }
            declaration_lookups += 1;
            let expected_parent = if declaration_lookups == 1 {
                "Prefix"
            } else {
                "Scale"
            };
            assert_eq!(context.parent_for(child).as_deref(), Some(expected_parent));
        }
        assert_eq!(named_child_count, declaration_count + 3);
        assert_eq!(context.examined_children, named_child_count);
        assert_eq!(context.parent_lookups, declaration_lookups);
        assert_eq!(declaration_lookups, declaration_count);
        assert_eq!(context.next_range, context.ranges.len() - 1);

        let bounded = extract_symbol_graph("src/large.php", Some("php"), &source);
        assert_eq!(bounded.parser, ParserKind::TreeSitter);
        assert_eq!(bounded.symbols.len(), MAX_SYMBOLS_PER_FILE);
    }

    #[test]
    fn php_namespace_prepass_honors_cancellation_at_intended_scale() {
        let source = large_semicolon_namespace_source(8_050);
        let mut parse_check = || Ok::<(), Infallible>(());
        let parsed = super::parse_php_tree(&source, &mut parse_check)
            .ok()
            .flatten();
        assert!(parsed.is_some(), "large PHP source should have a tree");
        let Some(parsed) = parsed else { return };
        assert!(parsed.tree.root_node().named_child_count() > 8_000);

        let mut checks = 0;
        let result =
            PhpNamespaceContext::from_program(parsed.tree.root_node(), &source, &mut || {
                checks += 1;
                if checks > 128 {
                    Err("cancelled")
                } else {
                    Ok(())
                }
            });
        assert!(matches!(result, Err("cancelled")));
        assert_eq!(checks, 129);
    }

    #[test]
    fn php_opening_tag_detection_ignores_literals_and_comments() {
        for source in [
            r#"function marker(): string { return "<?"; }"#,
            "// <?\nfunction marker(): string { return 'marker'; }",
            "# <?\nfunction marker(): string { return 'marker'; }",
            "/* <? */ function marker(): string { return 'marker'; }",
            r"function marker(): string { return <<<TEXT
<?
TEXT;
}",
            r"function marker(): string { return <<<'TEXT'
<?
TEXT;
}",
            "function marker(): string { return `echo <?`; }",
            "// <?\rfunction marker(): string { return 'marker'; }",
        ] {
            assert!(
                !super::contains_php_opening_tag(source),
                "PHP-only source was classified as mixed: {source:?}"
            );
            let graph = extract_symbol_graph("src/marker.php", Some("php"), source);
            assert!(
                graph.symbols.is_empty(),
                "tagless PHP source must remain inline text: {graph:?}"
            );
        }

        for (source, symbol_name) in [
            ("<?php function tagged(): void {} ?>", "tagged"),
            ("<? function short_tagged(): void {} ?>", "short_tagged"),
            (
                "<?= $value ?><?php function after_echo(): void {} ?>",
                "after_echo",
            ),
            (
                "<main>content</main><?php function mixed(): void {} ?>",
                "mixed",
            ),
            ("// <? ?><?php function reopened(): void {} ?>", "reopened"),
            (
                "# <? ?><?php function reopened_hash(): void {} ?>",
                "reopened_hash",
            ),
            (
                "/* <? ?> */<?php function reopened_block(): void {} ?>",
                "reopened_block",
            ),
        ] {
            assert!(
                super::contains_php_opening_tag(source),
                "genuine PHP opening tag was not classified as mixed: {source:?}"
            );
            let graph = extract_symbol_graph("src/tagged.php", Some("php"), source);
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == symbol_name),
                "mixed PHP symbol disappeared after opening-tag classification: {source:?}"
            );
        }
    }

    #[test]
    fn php_opening_tag_search_stops_at_first_source_order_tag() {
        let mut source = String::from("<?php\n");
        for index in 0..512 {
            assert!(writeln!(source, "function function_{index}(): void {{}}").is_ok());
        }
        let language = super::tree_sitter_language("php");
        assert!(language.is_some(), "PHP grammar should be registered");
        let Some(language) = language else { return };
        let mut parse_check = || Ok::<(), Infallible>(());
        let tree = super::parse_tree_sitter_language(&language, &source, &mut parse_check)
            .ok()
            .flatten();
        assert!(tree.is_some(), "mixed PHP source should produce a tree");
        let Some(tree) = tree else { return };
        let mut examined_nodes = 0;
        let first = super::first_php_tag_start(
            tree.root_node(),
            &mut || Ok::<(), Infallible>(()),
            &mut examined_nodes,
        );
        assert_eq!(first, Ok(Some(0)));
        assert!(
            examined_nodes < 16,
            "source-order tag search should stop before walking the function body: {examined_nodes}"
        );
    }

    #[test]
    fn php_opening_tag_search_checks_cancellation_on_large_tagless_tree() {
        let mut source = String::new();
        for index in 0..512 {
            assert!(writeln!(source, "function function_{index}(): void {{}}").is_ok());
        }
        let language = super::tree_sitter_language("php");
        assert!(language.is_some(), "PHP grammar should be registered");
        let Some(language) = language else { return };
        let mut parse_check = || Ok::<(), Infallible>(());
        let tree = super::parse_tree_sitter_language(&language, &source, &mut parse_check)
            .ok()
            .flatten();
        assert!(tree.is_some(), "PHP-only source should produce a tree");
        let Some(tree) = tree else { return };
        let mut examined_nodes = 0;
        let mut checks = 0;
        let result = super::first_php_tag_start(
            tree.root_node(),
            &mut || {
                checks += 1;
                Err::<(), _>("cancelled")
            },
            &mut examined_nodes,
        );
        assert_eq!(result, Err("cancelled"));
        assert_eq!(checks, 1);
        assert!(examined_nodes >= super::PARSER_CONTROL_CHECK_INTERVAL);
    }

    #[test]
    fn indexes_composer_style_php_source_through_the_builtin_owner() {
        let source = r"<?php
namespace Composer\Autoload;

use Composer\Autoload\ClassLoader as Loader;

class ClassLoader {
    public function loadClass(string $class): bool {
        return $this->findFile($class) !== null;
    }
    private function findFile(string $class): ?string { return null; }
}
";
        let graph = extract_symbol_graph("vendor/composer/ClassLoader.php", Some("php"), source);
        assert_eq!(graph.parser, ParserKind::TreeSitter);
        assert!(
            graph
                .symbols
                .iter()
                .any(|symbol| { symbol.kind == SymbolKind::Class && symbol.name == "ClassLoader" })
        );
        assert!(graph.symbols.iter().any(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "loadClass"
                && symbol.parent.as_deref() == Some("ClassLoader")
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Imports
                && relation.target_name == "Composer\\Autoload\\ClassLoader"
        }));
        assert!(graph.relations.iter().any(|relation| {
            relation.kind == RelationKind::Calls && relation.target_name == "findFile"
        }));
    }
}
