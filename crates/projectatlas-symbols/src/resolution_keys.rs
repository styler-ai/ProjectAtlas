//! Canonical import facts and resolution-key projection for extracted symbol graphs.

use crate::semantic;
use blake3::Hasher;
use projectatlas_core::graph::{
    CanonicalResolutionKey, GraphContractError, GraphIdentityText, GraphRelationKind,
    ProjectInstanceId, ResolutionKeyDomain,
};
use projectatlas_core::language::{LANGUAGE_CAPABILITIES, SemanticProviderOwner};
use projectatlas_core::symbols::{RelationKind, SymbolGraph, SymbolKind};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

/// Maximum canonical keys emitted for one source, symbol, or relation fact.
pub const MAX_RESOLUTION_KEYS_PER_FACT: usize = 64;
/// Version of the currently implemented semantic relation-resolution contract.
pub const SEMANTIC_RESOLUTION_CONTRACT_VERSION: u32 = 1;

/// Return a deterministic digest of the live semantic resolution-key contract.
///
/// This deliberately describes only the provider-backed `imports`, `calls`, and
/// Cargo `depends-on` behavior implemented here. The broader accepted relation-
/// family inventory remains a later, independently versioned responsibility.
#[must_use]
pub fn semantic_resolution_contract_digest() -> String {
    let mut hasher = Hasher::new();
    hasher.update(&SEMANTIC_RESOLUTION_CONTRACT_VERSION.to_le_bytes());
    hasher.update(&(MAX_RESOLUTION_KEYS_PER_FACT as u64).to_le_bytes());
    let mut providers = LANGUAGE_CAPABILITIES
        .iter()
        .filter_map(|capability| capability.effective_semantic_provider())
        .collect::<Vec<_>>();
    providers.sort_by_key(|provider| provider.as_str());
    providers.dedup();
    for provider in providers {
        hash_contract_value(&mut hasher, provider.as_str());
        hash_contract_value(
            &mut hasher,
            provider.resolution_family().unwrap_or("unavailable"),
        );
        if semantic::supports_source_dependencies(provider) {
            hash_relation_contract(
                &mut hasher,
                RelationKind::Imports,
                ResolutionKeyDomain::Module,
            );
            hash_relation_contract(
                &mut hasher,
                RelationKind::Calls,
                ResolutionKeyDomain::Declaration,
            );
        }
        if semantic::supports_package_dependencies(provider) {
            hash_relation_contract(
                &mut hasher,
                RelationKind::DependsOn,
                ResolutionKeyDomain::Package,
            );
        }
    }
    for outcome in ["resolved", "ambiguous", "unresolved", "external"] {
        hash_contract_value(&mut hasher, outcome);
    }
    hasher.finalize().to_hex().to_string()
}

/// Hash one supported relation and its target-identity domain.
fn hash_relation_contract(
    hasher: &mut Hasher,
    relation: RelationKind,
    domain: ResolutionKeyDomain,
) {
    hash_contract_value(hasher, &relation.to_string());
    hash_contract_value(hasher, domain.as_str());
}

/// Hash one length-delimited contract label.
fn hash_contract_value(hasher: &mut Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
/// Canonical resolution keys associated with one extracted symbol index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolResolutionKeys {
    /// Index of the source symbol fact owning these keys.
    symbol_index: usize,
    /// Sorted canonical export keys emitted for the symbol.
    keys: Vec<CanonicalResolutionKey>,
}

impl SymbolResolutionKeys {
    /// Return the corresponding index in [`SymbolGraph::symbols`].
    #[must_use]
    pub const fn symbol_index(&self) -> usize {
        self.symbol_index
    }

    /// Borrow the sorted, deduplicated export keys for this symbol fact.
    #[must_use]
    pub fn keys(&self) -> &[CanonicalResolutionKey] {
        &self.keys
    }
}

/// Canonical dependency keys associated with one extracted relation index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationResolutionKeys {
    /// Index of the source relation fact owning these keys.
    relation_index: usize,
    /// Sorted canonical dependency keys emitted for the relation.
    keys: Vec<CanonicalResolutionKey>,
}

impl RelationResolutionKeys {
    /// Return the corresponding index in [`SymbolGraph::relations`].
    #[must_use]
    pub const fn relation_index(&self) -> usize {
        self.relation_index
    }

    /// Borrow the sorted, deduplicated dependency keys for this relation fact.
    #[must_use]
    pub fn keys(&self) -> &[CanonicalResolutionKey] {
        &self.keys
    }
}

/// Parser-owned canonical keys ready for runtime entity/relation binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionKeyProjection {
    /// Canonical module exports owned by the source file.
    source: Vec<CanonicalResolutionKey>,
    /// Canonical declaration exports associated with symbol facts.
    symbols: Vec<SymbolResolutionKeys>,
    /// Canonical dependencies associated with relation facts.
    relations: Vec<RelationResolutionKeys>,
}

impl ResolutionKeyProjection {
    /// Borrow module keys exported by the source file itself.
    #[must_use]
    pub fn source_keys(&self) -> &[CanonicalResolutionKey] {
        &self.source
    }

    /// Borrow symbol-index-associated export keys.
    #[must_use]
    pub fn symbol_keys(&self) -> &[SymbolResolutionKeys] {
        &self.symbols
    }

    /// Borrow relation-index-associated dependency keys.
    #[must_use]
    pub fn relation_keys(&self) -> &[RelationResolutionKeys] {
        &self.relations
    }
}

/// Failure while deriving bounded canonical keys from parser facts.
#[derive(Debug)]
pub enum ResolutionProjectionError {
    /// An extracted or caller-supplied identity violated the graph contract.
    Contract(GraphContractError),
    /// One source fact would exceed the bounded key fan-out.
    KeyLimit {
        /// Kind of parser fact being projected.
        fact: &'static str,
        /// Index in the owning symbol or relation collection when applicable.
        index: usize,
        /// Number of distinct keys requested by the fact.
        requested: usize,
    },
}

impl fmt::Display for ResolutionProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "invalid resolution identity: {error}"),
            Self::KeyLimit {
                fact,
                index,
                requested,
            } => write!(
                formatter,
                "{fact} fact {index} requires {requested} canonical keys, exceeding the per-fact limit of {MAX_RESOLUTION_KEYS_PER_FACT}"
            ),
        }
    }
}

impl Error for ResolutionProjectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::KeyLimit { .. } => None,
        }
    }
}

impl From<GraphContractError> for ResolutionProjectionError {
    fn from(value: GraphContractError) -> Self {
        Self::Contract(value)
    }
}

/// Language syntax that produced one import reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportSyntax {
    /// Rust `use` syntax.
    Rust,
    /// JavaScript or TypeScript `import` syntax.
    EcmaScript,
    /// Python `import` or `from ... import ...` syntax.
    Python,
}

/// One normalized imported module or declaration and its caller-local name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReference {
    /// Source-language syntax that produced the reference.
    syntax: ImportSyntax,
    /// Imported module specifier or path.
    module: String,
    /// Imported declaration name for named imports.
    imported: Option<String>,
    /// Name visible to callers in the importing source file.
    local: String,
}

impl ImportReference {
    /// Construct one normalized provider-owned import reference.
    pub(crate) fn new(
        syntax: ImportSyntax,
        module: &str,
        imported: Option<&str>,
        local: &str,
    ) -> Self {
        Self {
            syntax,
            module: module.to_string(),
            imported: imported.map(ToString::to_string),
            local: local.to_string(),
        }
    }

    /// Return the source-language import syntax.
    #[must_use]
    pub const fn syntax(&self) -> ImportSyntax {
        self.syntax
    }

    /// Borrow the normalized module specifier or path.
    #[must_use]
    pub fn module(&self) -> &str {
        &self.module
    }

    /// Borrow the imported declaration name, or `None` for a module/namespace import.
    #[must_use]
    pub fn imported(&self) -> Option<&str> {
        self.imported.as_deref()
    }

    /// Borrow the name used by calls inside the importing source file.
    #[must_use]
    pub fn local(&self) -> &str {
        &self.local
    }
}

/// Parse one Rust, JavaScript/TypeScript, or Python import statement.
///
/// Malformed and unsupported forms return no references instead of inventing
/// identities from the complete display statement.
#[must_use]
pub fn parse_import_references(import_text: &str) -> Vec<ImportReference> {
    let mut references = semantic::parse_display_import(import_text);
    references.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then_with(|| left.imported.cmp(&right.imported))
            .then_with(|| left.local.cmp(&right.local))
    });
    references.dedup();
    references
}

/// Resolve one relative ECMAScript module specifier against a repository path.
#[must_use]
pub fn resolve_relative_import_path(caller_path: &str, module_spec: &str) -> Option<String> {
    semantic::ecmascript::resolve_relative_import_path(caller_path, module_spec)
}

/// Return deterministic module aliases inferred from a repository source path.
#[must_use]
pub fn module_aliases_for_path(path: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for stem in source_stems_for_path(path) {
        let mut components = stem
            .split('/')
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();
        if components
            .first()
            .is_some_and(|component| *component == "src")
        {
            components.remove(0);
        }
        if components.last().is_some_and(|component| {
            matches!(*component, "lib" | "main" | "mod" | "index" | "__init__")
        }) {
            components.pop();
        }
        if components.is_empty() {
            continue;
        }
        aliases.push(components.join("::"));
        aliases.push(components.join("."));
        if let Some(last) = components.last() {
            aliases.push((*last).to_string());
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

/// Return source path stems, including package-entry aliases.
#[must_use]
pub fn source_stems_for_path(path: &str) -> Vec<String> {
    let stem = strip_known_source_extension(path);
    let mut stems = vec![stem.clone()];
    if let Some((parent, entry_name)) = stem.rsplit_once('/')
        && matches!(entry_name, "index" | "__init__" | "mod")
    {
        stems.push(parent.to_string());
    }
    stems.sort();
    stems.dedup();
    stems
}

/// Derive bounded canonical export and dependency keys from one extracted graph.
///
/// The returned symbol and relation indices associate parser facts with keys;
/// runtime graph normalization binds those keys to final entity and logical-
/// relation owners after resolution.
///
/// # Errors
///
/// Returns an error when a graph identity is invalid or one parser fact exceeds
/// the bounded canonical-key fan-out.
pub fn derive_resolution_keys(
    project: ProjectInstanceId,
    package: Option<&str>,
    graph: &SymbolGraph,
) -> Result<ResolutionKeyProjection, ResolutionProjectionError> {
    let Some(provider_owner) = semantic::provider_for_graph(graph) else {
        return Ok(ResolutionKeyProjection {
            source: Vec::new(),
            symbols: Vec::new(),
            relations: Vec::new(),
        });
    };
    let provider = GraphIdentityText::new(provider_owner.as_str())?;
    let Some(resolution_family) = provider_owner.resolution_family() else {
        return Ok(ResolutionKeyProjection {
            source: Vec::new(),
            symbols: Vec::new(),
            relations: Vec::new(),
        });
    };
    let language = GraphIdentityText::new(resolution_family)?;
    let package = package.map(GraphIdentityText::new).transpose()?;

    let source_scopes = canonical_source_scopes(&graph.path);
    let emits_source_module_keys = semantic::emits_source_module_keys(graph);
    let mut source_keys = Vec::new();
    if emits_source_module_keys {
        source_keys.reserve(source_scopes.len());
        for scope in &source_scopes {
            source_keys.push(canonical_key(
                project,
                ResolutionKeyDomain::Module,
                &provider,
                &language,
                package.as_ref(),
                None,
                RelationKind::Imports,
                scope,
            )?);
        }
    }
    let source_keys = bounded_keys(source_keys, "source", 0)?;

    let mut symbol_keys = Vec::new();
    for (symbol_index, symbol) in graph.symbols.iter().enumerate() {
        let mut keys = Vec::new();
        match symbol.kind {
            SymbolKind::Package if provider_owner == SemanticProviderOwner::Cargo => {
                keys.push(canonical_key(
                    project,
                    ResolutionKeyDomain::Package,
                    &provider,
                    &language,
                    None,
                    None,
                    RelationKind::DependsOn,
                    &symbol.name,
                )?);
            }
            SymbolKind::Dependency | SymbolKind::Import | SymbolKind::Workspace => {}
            _ if emits_source_module_keys
                && semantic::is_export_candidate(provider_owner, graph, symbol_index) =>
            {
                let identity = GraphIdentityText::new(symbol.name.clone())?;
                let mut scopes = source_scopes.clone();
                if let Some(parent) = symbol.parent.as_deref() {
                    scopes.extend(
                        source_scopes
                            .iter()
                            .map(|scope| format!("{scope}/{parent}")),
                    );
                    scopes.push(parent.to_string());
                }
                scopes.push(String::new());
                scopes.sort();
                scopes.dedup();
                for scope in &scopes {
                    let scope = (!scope.is_empty())
                        .then(|| GraphIdentityText::new(scope.clone()))
                        .transpose()?;
                    keys.push(CanonicalResolutionKey::new(
                        project,
                        ResolutionKeyDomain::Declaration,
                        &provider,
                        &language,
                        package.as_ref(),
                        scope.as_ref(),
                        Some(GraphRelationKind::from_legacy(RelationKind::Calls)),
                        &identity,
                    ));
                }
            }
            _ => {}
        }
        let keys = bounded_keys(keys, "symbol", symbol_index)?;
        if !keys.is_empty() {
            symbol_keys.push(SymbolResolutionKeys { symbol_index, keys });
        }
    }

    let parsed_imports = graph
        .relations
        .iter()
        .map(|relation| {
            if relation.kind == RelationKind::Imports {
                semantic::parse_imports(provider_owner, &relation.target_name)
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    let mut aliases: BTreeMap<&str, Vec<&ImportReference>> = BTreeMap::new();
    for references in &parsed_imports {
        for reference in references {
            aliases
                .entry(reference.local())
                .or_default()
                .push(reference);
        }
    }

    let mut relation_keys = Vec::new();
    for (relation_index, relation) in graph.relations.iter().enumerate() {
        let mut keys = match relation.kind {
            RelationKind::Imports if semantic::supports_source_dependencies(provider_owner) => {
                import_dependency_keys(
                    project,
                    provider_owner,
                    &provider,
                    &language,
                    package.as_ref(),
                    &relation.path,
                    &parsed_imports[relation_index],
                )?
            }
            RelationKind::Calls if semantic::supports_source_dependencies(provider_owner) => {
                call_dependency_keys(
                    project,
                    provider_owner,
                    &provider,
                    &language,
                    package.as_ref(),
                    &relation.path,
                    &relation.target_name,
                    &aliases,
                )?
            }
            RelationKind::DependsOn if semantic::supports_package_dependencies(provider_owner) => {
                vec![canonical_key(
                    project,
                    ResolutionKeyDomain::Package,
                    &provider,
                    &language,
                    None,
                    None,
                    RelationKind::DependsOn,
                    &relation.target_name,
                )?]
            }
            RelationKind::Contains => continue,
            RelationKind::Imports | RelationKind::Calls | RelationKind::DependsOn => Vec::new(),
        };
        keys = bounded_keys(keys, "relation", relation_index)?;
        relation_keys.push(RelationResolutionKeys {
            relation_index,
            keys,
        });
    }

    Ok(ResolutionKeyProjection {
        source: source_keys,
        symbols: symbol_keys,
        relations: relation_keys,
    })
}

/// Derive canonical module-target keys for import references.
fn import_dependency_keys(
    project: ProjectInstanceId,
    provider_owner: SemanticProviderOwner,
    provider: &GraphIdentityText,
    language: &GraphIdentityText,
    package: Option<&GraphIdentityText>,
    caller_path: &str,
    references: &[ImportReference],
) -> Result<Vec<CanonicalResolutionKey>, ResolutionProjectionError> {
    let mut keys = Vec::new();
    for reference in references {
        let scopes = semantic::import_scopes(provider_owner, caller_path, reference);
        for scope in &scopes {
            keys.push(canonical_key(
                project,
                ResolutionKeyDomain::Module,
                provider,
                language,
                package,
                None,
                RelationKind::Imports,
                scope,
            )?);
        }
    }
    Ok(keys)
}

/// Derive canonical declaration keys for one call, including local aliases.
fn call_dependency_keys(
    project: ProjectInstanceId,
    provider_owner: SemanticProviderOwner,
    provider: &GraphIdentityText,
    language: &GraphIdentityText,
    package: Option<&GraphIdentityText>,
    caller_path: &str,
    target: &str,
    aliases: &BTreeMap<&str, Vec<&ImportReference>>,
) -> Result<Vec<CanonicalResolutionKey>, ResolutionProjectionError> {
    let (prefix, remainder) = split_qualified_target(target);
    let alias = prefix.unwrap_or(target).trim();
    if let Some(references) = aliases.get(alias) {
        let mut keys = Vec::new();
        for reference in references {
            let identity = remainder.or_else(|| reference.imported());
            let Some(identity) = identity else {
                continue;
            };
            let mut scopes = semantic::import_scopes(provider_owner, caller_path, reference);
            if remainder.is_some()
                && let Some(imported_parent) = reference.imported()
            {
                scopes = scopes
                    .into_iter()
                    .map(|scope| format!("{scope}/{imported_parent}"))
                    .collect();
            }
            for scope in scopes {
                keys.push(canonical_key(
                    project,
                    ResolutionKeyDomain::Declaration,
                    provider,
                    language,
                    package,
                    Some(&scope),
                    RelationKind::Calls,
                    identity,
                )?);
            }
        }
        return Ok(keys);
    }

    let (scope, identity) = split_qualified_target(target);
    if scope.is_some() {
        return Ok(Vec::new());
    }
    let Some(identity) = identity.or_else(|| (!target.trim().is_empty()).then_some(target.trim()))
    else {
        return Ok(Vec::new());
    };
    Ok(vec![canonical_key(
        project,
        ResolutionKeyDomain::Declaration,
        provider,
        language,
        package,
        None,
        RelationKind::Calls,
        identity,
    )?])
}

#[allow(clippy::too_many_arguments)]
/// Construct one validated canonical key from parser-owned identity parts.
fn canonical_key(
    project: ProjectInstanceId,
    domain: ResolutionKeyDomain,
    provider: &GraphIdentityText,
    language: &GraphIdentityText,
    package: Option<&GraphIdentityText>,
    scope: Option<&str>,
    relation: RelationKind,
    identity: &str,
) -> Result<CanonicalResolutionKey, GraphContractError> {
    let scope = scope.map(GraphIdentityText::new).transpose()?;
    let identity = GraphIdentityText::new(identity.to_string())?;
    Ok(CanonicalResolutionKey::new(
        project,
        domain,
        provider,
        language,
        package,
        scope.as_ref(),
        Some(GraphRelationKind::from_legacy(relation)),
        &identity,
    ))
}

/// Sort, deduplicate, and enforce the per-fact canonical-key limit.
fn bounded_keys(
    mut keys: Vec<CanonicalResolutionKey>,
    fact: &'static str,
    index: usize,
) -> Result<Vec<CanonicalResolutionKey>, ResolutionProjectionError> {
    keys.sort();
    keys.dedup();
    if keys.len() > MAX_RESOLUTION_KEYS_PER_FACT {
        return Err(ResolutionProjectionError::KeyLimit {
            fact,
            index,
            requested: keys.len(),
        });
    }
    Ok(keys)
}

/// Return normalized canonical scopes exported by a repository source path.
fn canonical_source_scopes(path: &str) -> Vec<String> {
    let mut scopes = source_stems_for_path(path);
    scopes.extend(module_aliases_for_path(path));
    normalize_scopes(scopes, normalize_repository_scope)
}

/// Normalize, expand, sort, and deduplicate candidate scopes.
fn normalize_scopes(scopes: Vec<String>, normalize: fn(&str) -> String) -> Vec<String> {
    let mut normalized = Vec::new();
    for scope in scopes {
        let scope = normalize(&scope);
        if scope.is_empty() {
            continue;
        }
        normalized.push(scope.clone());
        if let Some(stripped) = scope.strip_prefix("src/") {
            normalized.push(stripped.to_string());
        }
        if let Some(last) = scope.rsplit('/').next() {
            normalized.push(last.to_string());
        }
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

/// Normalize repository path and Rust-style alias separators without changing dots.
fn normalize_repository_scope(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("./")
        .replace("::", "/")
        .trim_matches('/')
        .to_string()
}

/// Split a qualified call target into its scope and final identity.
fn split_qualified_target(target: &str) -> (Option<&str>, Option<&str>) {
    let target = target.trim();
    let rust = target.rsplit_once("::");
    let dotted = target.rsplit_once('.');
    match (rust, dotted) {
        (Some(rust), Some(dotted)) => {
            if rust.0.len() >= dotted.0.len() {
                (Some(rust.0), Some(rust.1))
            } else {
                (Some(dotted.0), Some(dotted.1))
            }
        }
        (Some((scope, identity)), None) | (None, Some((scope, identity))) => {
            (Some(scope), Some(identity))
        }
        (None, None) => (None, None),
    }
}

/// Strip a terminal source extension while preserving dotted directory names.
pub(super) fn strip_known_source_extension(path: &str) -> String {
    for extension in [
        ".d.ts", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".rs",
    ] {
        if let Some(stem) = path.strip_suffix(extension) {
            return stem.to_string();
        }
    }
    path.rsplit_once('.').map_or_else(
        || path.to_string(),
        |(stem, extension)| {
            if extension.contains('/') {
                path.to_string()
            } else {
                stem.to_string()
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ImportReference, ImportSyntax, RelationKind, ResolutionProjectionError,
        derive_resolution_keys, parse_import_references, resolve_relative_import_path,
        semantic_resolution_contract_digest,
    };
    use crate::extract_symbol_graph;
    use projectatlas_core::graph::{CanonicalResolutionKey, ProjectInstanceId};
    use projectatlas_core::symbols::SymbolGraph;
    use std::error::Error;
    use std::io;

    fn reference(
        syntax: ImportSyntax,
        module: &str,
        imported: Option<&str>,
        local: &str,
    ) -> ImportReference {
        ImportReference {
            syntax,
            module: module.to_string(),
            imported: imported.map(ToString::to_string),
            local: local.to_string(),
        }
    }

    #[test]
    fn semantic_resolution_contract_digest_is_bounded_and_deterministic() {
        let first = semantic_resolution_contract_digest();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(first, semantic_resolution_contract_digest());
    }

    #[test]
    fn parses_language_import_aliases_without_display_statement_identity() {
        assert_eq!(
            parse_import_references("use crate::worker::{run as execute, stop};"),
            vec![
                reference(ImportSyntax::Rust, "crate::worker", Some("run"), "execute"),
                reference(ImportSyntax::Rust, "crate::worker", Some("stop"), "stop"),
            ]
        );
        assert_eq!(
            parse_import_references("use crate::worker::run_function_alias as run_rust_function;"),
            vec![reference(
                ImportSyntax::Rust,
                "crate::worker",
                Some("run_function_alias"),
                "run_rust_function"
            ),]
        );
        assert_eq!(
            parse_import_references("import * as reader from './reader';"),
            vec![reference(
                ImportSyntax::EcmaScript,
                "./reader",
                None,
                "reader",
            )]
        );
        assert_eq!(
            parse_import_references("import { runAlias as runLocal } from './worker';"),
            vec![reference(
                ImportSyntax::EcmaScript,
                "./worker",
                Some("runAlias"),
                "runLocal",
            )]
        );
        assert_eq!(
            parse_import_references("from package.reader import read as load, close"),
            vec![
                reference(
                    ImportSyntax::Python,
                    "package.reader",
                    Some("close"),
                    "close"
                ),
                reference(ImportSyntax::Python, "package.reader", Some("read"), "load"),
            ]
        );
        assert_eq!(
            parse_import_references("from package.worker import run_alias as execute"),
            vec![reference(
                ImportSyntax::Python,
                "package.worker",
                Some("run_alias"),
                "execute",
            )]
        );
        assert!(parse_import_references("import { broken from './reader'").is_empty());
        assert!(parse_import_references("use crate::worker::run as ;").is_empty());
        assert!(parse_import_references("import { read } from module './reader'").is_empty());
        for malformed in [
            "use crate::worker::{run, nested::{start, stop}};",
            "use crate::worker::{run, broken as };",
            "import defaultValue, { read } from './reader';",
            "import { read, nested { broken } } from './reader';",
            "import { read as load as fetch } from './reader';",
            "from package.reader import (read, close)",
            "from package.reader import read, broken as ",
            "import package.reader as reader as other",
        ] {
            assert!(
                parse_import_references(malformed).is_empty(),
                "unsupported syntax fabricated references: {malformed}"
            );
        }
    }

    #[test]
    fn resolves_relative_imports_without_platform_path_rules() {
        assert_eq!(
            resolve_relative_import_path("src/features/main.ts", "../shared/reader.ts"),
            Some("src/shared/reader".to_string())
        );
        assert_eq!(
            resolve_relative_import_path("src/main.ts", "../../outside"),
            None
        );
        assert_eq!(resolve_relative_import_path("src/main.ts", "reader"), None);
        assert_eq!(
            resolve_relative_import_path("src/foo.bar/main.ts", "./reader"),
            Some("src/foo.bar/reader".to_string())
        );
    }

    #[test]
    fn real_language_graphs_share_alias_qualified_keys() -> Result<(), Box<dyn Error>> {
        let project = project_id(1)?;

        let rust_target = extract_symbol_graph("src/worker.rs", Some("rust"), "pub fn run() {}\n");
        let rust_caller = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "use crate::worker::run as execute;\nfn main() { execute(); }\n",
        );
        assert_alias_resolution(
            project,
            Some("atlas"),
            &rust_target,
            "run",
            &rust_caller,
            &["execute"],
        )?;

        let typescript_target = extract_symbol_graph(
            "src/shared/reader.ts",
            Some("typescript"),
            "export function read() {}\n",
        );
        let typescript_caller = extract_symbol_graph(
            "src/features/main.ts",
            Some("typescript"),
            "import { read as load } from '../shared/reader';\nimport * as reader from '../shared/reader';\nexport function main() { load(); reader.read(); }\n",
        );
        assert_alias_resolution(
            project,
            Some("web"),
            &typescript_target,
            "read",
            &typescript_caller,
            &["load", "reader.read"],
        )?;

        let dotted_directory_target = extract_symbol_graph(
            "src/foo.bar/reader.ts",
            Some("typescript"),
            "export function read() {}\n",
        );
        let dotted_directory_caller = extract_symbol_graph(
            "src/foo.bar/main.ts",
            Some("typescript"),
            "import { read } from './reader';\nexport function main() { read(); }\n",
        );
        assert_alias_resolution(
            project,
            Some("web"),
            &dotted_directory_target,
            "read",
            &dotted_directory_caller,
            &["read"],
        )?;

        let python_target = extract_symbol_graph(
            "src/package/reader.py",
            Some("python"),
            "def read():\n    pass\n",
        );
        let python_caller = extract_symbol_graph(
            "src/main.py",
            Some("python"),
            "import package.reader as reader\nreader.read()\n",
        );
        assert_alias_resolution(
            project,
            Some("python-app"),
            &python_target,
            "read",
            &python_caller,
            &["reader.read"],
        )?;
        Ok(())
    }

    #[test]
    fn imports_target_modules_and_qualified_calls_target_members() -> Result<(), Box<dyn Error>> {
        let project = project_id(10)?;
        let target = extract_symbol_graph(
            "src/shared/reader.ts",
            Some("typescript"),
            "export function read() {}\nexport function close() {}\nexport function fetch() {}\nexport const client = { fetch() {} };\n",
        );
        let caller = extract_symbol_graph(
            "src/features/main.ts",
            Some("typescript"),
            "import { read, close, client } from '../shared/reader';\nexport function main() { read(); close(); client.fetch(); }\n",
        );
        let target_projection = derive_resolution_keys(project, Some("web"), &target)?;
        let caller_projection = derive_resolution_keys(project, Some("web"), &caller)?;
        let imports = relation_keys_of_kind(&caller, &caller_projection, RelationKind::Imports);
        require(
            imports.len() == 1,
            "one import emitted more than one module target",
        )?;
        require(
            imports[0]
                .iter()
                .all(|key| key.domain() == projectatlas_core::graph::ResolutionKeyDomain::Module),
            "named imports emitted declaration resolution targets",
        )?;
        require(
            keys_intersect(imports[0], target_projection.source_keys()),
            "named import did not target its source module",
        )?;
        for symbol in ["read", "close"] {
            require(
                !keys_intersect(imports[0], symbol_keys(&target, &target_projection, symbol)),
                "module import also targeted a named declaration",
            )?;
        }
        assert_alias_resolution(project, Some("web"), &target, "read", &caller, &["read"])?;
        assert_alias_resolution(project, Some("web"), &target, "close", &caller, &["close"])?;
        let qualified_call = relation_keys(&caller, &caller_projection, "client.fetch");
        let fetch_candidates = target_projection
            .symbol_keys()
            .iter()
            .filter(|entry| target.symbols[entry.symbol_index()].name == "fetch")
            .map(|entry| {
                (
                    target.symbols[entry.symbol_index()].parent.as_deref(),
                    entry.keys(),
                )
            })
            .collect::<Vec<_>>();
        require(
            fetch_candidates.len() == 2,
            "qualified-call fixture lost one fetch declaration",
        )?;
        require(
            fetch_candidates.iter().any(|(parent, keys)| {
                *parent == Some("client") && keys_intersect(qualified_call, keys)
            }),
            "qualified call did not target the imported parent member",
        )?;
        require(
            fetch_candidates
                .iter()
                .all(|(parent, keys)| parent.is_some() || !keys_intersect(qualified_call, keys)),
            "qualified call also targeted a same-module top-level declaration",
        )
    }

    #[test]
    fn actual_complex_import_syntax_abstains_instead_of_fabricating_keys()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(15)?;
        for (path, language, source) in [
            (
                "src/main.rs",
                "rust",
                "use crate::worker::{run, nested::{start, stop}};\nfn main() { run(); }\n",
            ),
            (
                "src/main.ts",
                "typescript",
                "import defaultValue, { read } from './reader';\ndefaultValue();\n",
            ),
            (
                "src/main.py",
                "python",
                "from package.reader import (read, close)\nread()\n",
            ),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let projection = derive_resolution_keys(project, Some("app"), &graph)?;
            let imports = relation_keys_of_kind(&graph, &projection, RelationKind::Imports);
            require(
                !imports.is_empty(),
                "complex-import fixture did not reach provider normalization",
            )?;
            require(
                imports.iter().all(|keys| keys.is_empty()),
                "unsupported complex import syntax fabricated resolution keys",
            )?;
        }
        Ok(())
    }

    #[test]
    fn provider_scopes_abstain_instead_of_matching_packages_or_wrong_siblings()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(11)?;
        let correct_typescript = extract_symbol_graph(
            "src/a/reader.ts",
            Some("typescript"),
            "export function read() {}\n",
        );
        let wrong_typescript = extract_symbol_graph(
            "src/b/reader.ts",
            Some("typescript"),
            "export function read() {}\n",
        );
        let typescript_caller = extract_symbol_graph(
            "src/main.ts",
            Some("typescript"),
            "import { read } from './a/reader';\nread();\n",
        );
        assert_import_matches_only_source(
            project,
            Some("web"),
            &typescript_caller,
            &correct_typescript,
            &wrong_typescript,
        )?;

        let package_caller = extract_symbol_graph(
            "src/package.ts",
            Some("typescript"),
            "import { read } from 'reader-package';\nread();\n",
        );
        let package_projection = derive_resolution_keys(project, Some("web"), &package_caller)?;
        require(
            relation_keys_of_kind(&package_caller, &package_projection, RelationKind::Imports)
                .iter()
                .all(|keys| keys.is_empty()),
            "bare ECMAScript package import entered a repository-local module scope",
        )?;
        require(
            relation_keys(&package_caller, &package_projection, "read").is_empty(),
            "call through an unresolved package import fell back to a global declaration",
        )?;

        for (path, language, source, target) in [
            (
                "src/unimported.ts",
                "typescript",
                "export function run() { return client.fetch(); }\n",
                "client.fetch",
            ),
            (
                "src/unimported.rs",
                "rust",
                "pub fn run() { worker::execute(); }\n",
                "worker::execute",
            ),
            (
                "src/unimported.py",
                "python",
                "def run():\n    return reader.read()\n",
                "reader.read",
            ),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let projection = derive_resolution_keys(project, Some("app"), &graph)?;
            require(
                graph.relations.iter().any(|relation| {
                    relation.kind == RelationKind::Calls && relation.target_name == target
                }),
                "unimported qualified-call fixture did not reach semantic normalization",
            )?;
            require(
                relation_keys(&graph, &projection, target).is_empty(),
                "unimported qualified call fell back to a project basename scope",
            )?;
        }

        let rust_target =
            extract_symbol_graph("src/feature/worker.rs", Some("rust"), "pub fn run() {}\n");
        let wrong_rust =
            extract_symbol_graph("src/other/worker.rs", Some("rust"), "pub fn run() {}\n");
        let rust_caller = extract_symbol_graph(
            "src/feature/main.rs",
            Some("rust"),
            "use super::worker::run;\nfn main() { run(); }\n",
        );
        let rust_crate_caller = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "use crate::feature::worker::run;\nfn main() { run(); }\n",
        );
        let rust_self_caller = extract_symbol_graph(
            "src/feature/mod.rs",
            Some("rust"),
            "use self::worker::run;\nfn main() { run(); }\n",
        );
        for caller in [&rust_caller, &rust_crate_caller, &rust_self_caller] {
            assert_import_matches_only_source(
                project,
                Some("atlas"),
                caller,
                &rust_target,
                &wrong_rust,
            )?;
        }

        let python_target = extract_symbol_graph(
            "src/package/reader.py",
            Some("python"),
            "def read():\n    pass\n",
        );
        let wrong_python = extract_symbol_graph(
            "src/other/reader.py",
            Some("python"),
            "def read():\n    pass\n",
        );
        let python_caller = extract_symbol_graph(
            "src/package/main.py",
            Some("python"),
            "from .reader import read\nread()\n",
        );
        assert_import_matches_only_source(
            project,
            Some("python-app"),
            &python_caller,
            &python_target,
            &wrong_python,
        )?;
        let parent_python_target = extract_symbol_graph(
            "src/package/reader.py",
            Some("python"),
            "def read():\n    pass\n",
        );
        let parent_python_wrong = extract_symbol_graph(
            "src/package/sub/reader.py",
            Some("python"),
            "def read():\n    pass\n",
        );
        let parent_python_caller = extract_symbol_graph(
            "src/package/sub/main.py",
            Some("python"),
            "from ..reader import read\nread()\n",
        );
        assert_import_matches_only_source(
            project,
            Some("python-app"),
            &parent_python_caller,
            &parent_python_target,
            &parent_python_wrong,
        )
    }

    #[test]
    fn direct_provider_families_retain_duplicate_ambiguity_keys() -> Result<(), Box<dyn Error>> {
        let project = project_id(12)?;
        for (path, language, target_source, caller_path, caller_source, target_name) in [
            (
                "src/shared/reader.ts",
                "typescript",
                "export function read() {}\nexport function read() {}\n",
                "src/main.ts",
                "import { read } from './shared/reader';\nread();\n",
                "read",
            ),
            (
                "src/package/reader.py",
                "python",
                "def read():\n    pass\ndef read():\n    pass\n",
                "src/main.py",
                "from package.reader import read\nread()\n",
                "read",
            ),
        ] {
            let target = extract_symbol_graph(path, Some(language), target_source);
            let caller = extract_symbol_graph(caller_path, Some(language), caller_source);
            let target_projection = derive_resolution_keys(project, Some("app"), &target)?;
            let caller_projection = derive_resolution_keys(project, Some("app"), &caller)?;
            let duplicate_keys = target_projection
                .symbol_keys()
                .iter()
                .filter(|entry| target.symbols[entry.symbol_index()].name == target_name)
                .map(super::SymbolResolutionKeys::keys)
                .collect::<Vec<_>>();
            require(
                duplicate_keys.len() == 2,
                "duplicate declarations were not both projected",
            )?;
            let dependency = relation_keys(&caller, &caller_projection, target_name);
            require(
                duplicate_keys
                    .iter()
                    .all(|keys| keys_intersect(dependency, keys)),
                "call dependency did not retain every duplicate declaration candidate",
            )?;
        }
        Ok(())
    }

    #[test]
    fn resolution_identity_separates_provider_from_cross_dialect_family()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(13)?;
        let typescript = extract_symbol_graph(
            "src/shared/reader.ts",
            Some("typescript"),
            "export function read() {}\n",
        );
        let javascript = extract_symbol_graph(
            "src/shared/reader.ts",
            Some("javascript"),
            "export function read() {}\n",
        );
        let typescript_projection = derive_resolution_keys(project, Some("web"), &typescript)?;
        let javascript_projection = derive_resolution_keys(project, Some("web"), &javascript)?;
        require(
            typescript_projection.source_keys() == javascript_projection.source_keys(),
            "ECMAScript-compatible dialects entered different resolution families",
        )?;
        let provider = projectatlas_core::graph::GraphIdentityText::new("ecma-script")?;
        let family = projectatlas_core::graph::GraphIdentityText::new("ecmascript")?;
        let package = projectatlas_core::graph::GraphIdentityText::new("web")?;
        let identity = projectatlas_core::graph::GraphIdentityText::new("src/shared/reader")?;
        let expected = CanonicalResolutionKey::new(
            project,
            projectatlas_core::graph::ResolutionKeyDomain::Module,
            &provider,
            &family,
            Some(&package),
            None,
            Some(projectatlas_core::graph::GraphRelationKind::from_legacy(
                RelationKind::Imports,
            )),
            &identity,
        );
        require(
            typescript_projection.source_keys().contains(&expected),
            "provider owner and cross-dialect family were not projected independently",
        )
    }

    #[test]
    fn embedded_hosts_publish_module_keys_only_for_admitted_component_facts()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(14)?;
        for (path, language, source) in [
            (
                "page.html",
                "html",
                "<script>export function run() {}</script>",
            ),
            ("empty.vue", "vue", "<template><p>empty</p></template>"),
            (
                "external.vue",
                "vue",
                "<script src='./external.js'></script>",
            ),
            ("empty.svelte", "svelte", "<p>empty</p>"),
            (
                "external.svelte",
                "svelte",
                "<script src='./external.js'></script>",
            ),
            (
                "external-inline.vue",
                "vue",
                "<script>import * as fs from 'node:fs';</script>",
            ),
            (
                "external-inline.svelte",
                "svelte",
                "<script>import * as fs from 'node:fs';</script>",
            ),
            ("malformed.vue", "vue", "<script"),
            ("malformed.svelte", "svelte", "<script"),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let projection = derive_resolution_keys(project, Some("web"), &graph)?;
            require(
                projection.source_keys().is_empty(),
                "host without an admitted component module surface emitted source keys",
            )?;
            if path.starts_with("external-inline") {
                require(
                    relation_keys_of_kind(&graph, &projection, RelationKind::Imports).len() == 1,
                    "external-only component lost its outbound import relationship",
                )?;
            }
        }
        for (path, language) in [("component.vue", "vue"), ("component.svelte", "svelte")] {
            let graph = extract_symbol_graph(
                path,
                Some(language),
                "<script>export function run() {}</script>",
            );
            require(
                !derive_resolution_keys(project, Some("web"), &graph)?
                    .source_keys()
                    .is_empty(),
                "admitted component script facts did not expose their host module",
            )?;
        }
        Ok(())
    }

    #[test]
    fn canonical_projection_is_stable_and_retains_ambiguous_unresolved_and_package_keys()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(2)?;
        let first = extract_symbol_graph("src/worker.rs", Some("rust"), "pub fn run() {}\n");
        let moved = extract_symbol_graph(
            "src/worker.rs",
            Some("rust"),
            "\n\n// formatting moved the declaration\npub fn run() {}\n",
        );
        let first_projection = derive_resolution_keys(project, Some("atlas"), &first)?;
        let moved_projection = derive_resolution_keys(project, Some("atlas"), &moved)?;
        require(
            symbol_keys(&first, &first_projection, "run")
                == symbol_keys(&moved, &moved_projection, "run"),
            "formatting-only line movement changed canonical symbol identity",
        )?;
        require(
            first_projection.source_keys()
                != derive_resolution_keys(project_id(3)?, Some("atlas"), &first)?.source_keys(),
            "different projects produced the same canonical source identity",
        )?;
        require(
            symbol_keys(&first, &first_projection, "run")
                != symbol_keys(
                    &first,
                    &derive_resolution_keys(project, Some("other-package"), &first)?,
                    "run",
                ),
            "different packages produced the same canonical declaration identity",
        )?;

        let duplicate = extract_symbol_graph("src/other.rs", Some("rust"), "pub fn run() {}\n");
        let caller = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "fn main() { run(); missing(); }\n",
        );
        let duplicate_projection = derive_resolution_keys(project, Some("atlas"), &duplicate)?;
        let caller_projection = derive_resolution_keys(project, Some("atlas"), &caller)?;
        let run_dependencies = relation_keys(&caller, &caller_projection, "run");
        require(
            keys_intersect(
                run_dependencies,
                symbol_keys(&first, &first_projection, "run"),
            ),
            "ambiguous call did not retain the first matching declaration key",
        )?;
        require(
            keys_intersect(
                run_dependencies,
                symbol_keys(&duplicate, &duplicate_projection, "run"),
            ),
            "ambiguous call did not retain the duplicate declaration key",
        )?;
        let missing_dependencies = relation_keys(&caller, &caller_projection, "missing");
        require(
            !missing_dependencies.is_empty(),
            "unresolved call lost its canonical dependency key",
        )?;
        require(
            !keys_intersect(
                missing_dependencies,
                symbol_keys(&first, &first_projection, "run"),
            ),
            "unresolved call matched an unrelated declaration key",
        )?;

        let package = extract_symbol_graph(
            "vendor/library/Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"library\"\n",
        );
        let manifest = extract_symbol_graph(
            "Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"app\"\n[dependencies]\nlibrary = \"1\"\n",
        );
        let package_projection = derive_resolution_keys(project, None, &package)?;
        let manifest_projection = derive_resolution_keys(project, None, &manifest)?;
        require(
            package_projection.source_keys().is_empty()
                && manifest_projection.source_keys().is_empty(),
            "Cargo manifests advertised ordinary source-module identities",
        )?;
        require(
            keys_intersect(
                relation_keys(&manifest, &manifest_projection, "library"),
                symbol_keys(&package, &package_projection, "library"),
            ),
            "Cargo dependency did not match its package declaration key",
        )?;
        Ok(())
    }

    #[test]
    fn cargo_provider_abstains_on_malformed_input_and_retains_duplicate_package_candidates()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(16)?;
        let malformed = extract_symbol_graph(
            "Cargo.toml",
            Some("cargo-manifest"),
            "[package\nname = \"broken\"\n",
        );
        let malformed_projection = derive_resolution_keys(project, None, &malformed)?;
        require(
            malformed_projection.source_keys().is_empty()
                && malformed_projection.symbol_keys().is_empty()
                && malformed_projection.relation_keys().is_empty(),
            "malformed Cargo input fabricated semantic identities",
        )?;

        let first = extract_symbol_graph(
            "vendor/first/Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"shared-package\"\n",
        );
        let second = extract_symbol_graph(
            "vendor/second/Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"shared-package\"\n",
        );
        let caller = extract_symbol_graph(
            "Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"app\"\n[dependencies]\nshared-package = \"1\"\n",
        );
        let first_projection = derive_resolution_keys(project, None, &first)?;
        let second_projection = derive_resolution_keys(project, None, &second)?;
        let caller_projection = derive_resolution_keys(project, None, &caller)?;
        let dependency = relation_keys(&caller, &caller_projection, "shared-package");
        require(
            keys_intersect(
                dependency,
                symbol_keys(&first, &first_projection, "shared-package"),
            ) && keys_intersect(
                dependency,
                symbol_keys(&second, &second_projection, "shared-package"),
            ),
            "duplicate Cargo packages did not remain ambiguity candidates",
        )
    }

    #[test]
    fn projection_rejects_invalid_identity_and_excessive_key_fanout() -> Result<(), Box<dyn Error>>
    {
        let graph = extract_symbol_graph("src/lib.rs", Some("rust"), "pub fn run() {}\n");
        require(
            matches!(
                derive_resolution_keys(project_id(4)?, Some("bad\0package"), &graph),
                Err(ResolutionProjectionError::Contract(_))
            ),
            "invalid package identity was accepted",
        )?;

        let imports = (0..65)
            .map(|index| format!("import {{ run as execute }} from './module{index}';"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("{imports}\nexecute();\n");
        let graph = extract_symbol_graph("src/main.ts", Some("typescript"), &source);
        require(
            matches!(
                derive_resolution_keys(project_id(4)?, Some("web"), &graph),
                Err(ResolutionProjectionError::KeyLimit {
                    fact: "relation",
                    ..
                })
            ),
            "excessive relation-key fan-out was not rejected",
        )?;
        Ok(())
    }

    #[test]
    fn unsupported_and_lockfile_languages_do_not_gain_generic_name_resolution()
    -> Result<(), Box<dyn Error>> {
        for graph in [
            extract_symbol_graph(
                "src/Service.java",
                Some("java"),
                "public class Service { void run() {} }\n",
            ),
            extract_symbol_graph(
                "Cargo.lock",
                Some("cargo-lock"),
                "[[package]]\nname = \"dependency\"\nversion = \"1.0.0\"\n",
            ),
        ] {
            let projection = derive_resolution_keys(project_id(9)?, Some("app"), &graph)?;
            require(
                projection.source_keys().is_empty()
                    && projection.symbol_keys().is_empty()
                    && projection.relation_keys().is_empty(),
                "unsupported language emitted generic resolution keys",
            )?;
        }
        Ok(())
    }

    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn project_id(byte: u8) -> Result<ProjectInstanceId, Box<dyn Error>> {
        Ok(ProjectInstanceId::from_bytes([byte; 16])?)
    }

    fn assert_alias_resolution(
        project: ProjectInstanceId,
        package: Option<&str>,
        target: &SymbolGraph,
        symbol_name: &str,
        caller: &SymbolGraph,
        call_targets: &[&str],
    ) -> Result<(), Box<dyn Error>> {
        let target_projection = derive_resolution_keys(project, package, target)?;
        let caller_projection = derive_resolution_keys(project, package, caller)?;
        let exports = symbol_keys(target, &target_projection, symbol_name);
        for call_target in call_targets {
            if !keys_intersect(
                relation_keys(caller, &caller_projection, call_target),
                exports,
            ) {
                return Err(io::Error::other(format!(
                    "{call_target} did not share a canonical key with {symbol_name}"
                ))
                .into());
            }
        }
        Ok(())
    }

    fn assert_import_matches_only_source(
        project: ProjectInstanceId,
        package: Option<&str>,
        caller: &SymbolGraph,
        expected: &SymbolGraph,
        wrong: &SymbolGraph,
    ) -> Result<(), Box<dyn Error>> {
        let caller_projection = derive_resolution_keys(project, package, caller)?;
        let expected_projection = derive_resolution_keys(project, package, expected)?;
        let wrong_projection = derive_resolution_keys(project, package, wrong)?;
        let imports = relation_keys_of_kind(caller, &caller_projection, RelationKind::Imports);
        require(
            imports.len() == 1,
            "caller did not retain one import relationship",
        )?;
        require(
            keys_intersect(imports[0], expected_projection.source_keys()),
            "caller import missed its exact source",
        )?;
        require(
            !keys_intersect(imports[0], wrong_projection.source_keys()),
            "caller import matched a wrong-sibling basename",
        )
    }

    fn symbol_keys<'a>(
        graph: &SymbolGraph,
        projection: &'a super::ResolutionKeyProjection,
        name: &str,
    ) -> &'a [CanonicalResolutionKey] {
        projection
            .symbol_keys()
            .iter()
            .find(|entry| graph.symbols[entry.symbol_index()].name == name)
            .map_or(&[], super::SymbolResolutionKeys::keys)
    }

    fn relation_keys<'a>(
        graph: &SymbolGraph,
        projection: &'a super::ResolutionKeyProjection,
        target: &str,
    ) -> &'a [CanonicalResolutionKey] {
        projection
            .relation_keys()
            .iter()
            .find(|entry| {
                let relation = &graph.relations[entry.relation_index()];
                matches!(relation.kind, RelationKind::Calls | RelationKind::DependsOn)
                    && relation.target_name == target
            })
            .map_or(&[], super::RelationResolutionKeys::keys)
    }

    fn relation_keys_of_kind<'a>(
        graph: &SymbolGraph,
        projection: &'a super::ResolutionKeyProjection,
        kind: RelationKind,
    ) -> Vec<&'a [CanonicalResolutionKey]> {
        projection
            .relation_keys()
            .iter()
            .filter(|entry| graph.relations[entry.relation_index()].kind == kind)
            .map(super::RelationResolutionKeys::keys)
            .collect()
    }

    fn keys_intersect(left: &[CanonicalResolutionKey], right: &[CanonicalResolutionKey]) -> bool {
        left.iter().any(|key| right.binary_search(key).is_ok())
    }
}
