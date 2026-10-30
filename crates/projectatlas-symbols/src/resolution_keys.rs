//! Canonical import facts and resolution-key projection for extracted symbol graphs.

use crate::ConfiguredModuleResolution;
use crate::configured_modules::ConfiguredModuleSource;
use crate::semantic;
use crate::{SourceCallBinding, SourceCallFact, SourceSemanticFacts};
use blake3::Hasher;
use projectatlas_core::graph::{
    CanonicalResolutionKey, GraphContractError, GraphIdentityText, GraphLimits, GraphRelationKind,
    ProjectInstanceId, ResolutionKeyDomain, SourceSpan,
};
use projectatlas_core::language::{LANGUAGE_CAPABILITIES, SemanticProviderOwner};
use projectatlas_core::symbols::{RelationKind, SymbolGraph, SymbolKind};
use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// Maximum canonical keys emitted for one source, symbol, or relation fact.
pub const MAX_RESOLUTION_KEYS_PER_FACT: usize = 64;
/// Version of the currently implemented semantic relation-resolution contract.
pub const SEMANTIC_RESOLUTION_CONTRACT_VERSION: u32 = 17;
/// Disjoint ECMAScript lookup namespaces for explicit files and inferred modules.
const ECMA_FILE_SCOPE: &str = "file:";
/// Legacy extensionless and package-entry aliases never identify an explicit file.
const ECMA_MODULE_SCOPE: &str = "module:";

/// Return a deterministic digest of the live semantic resolution-key contract.
///
/// This deliberately describes only the provider-backed `imports`, `calls`, and
/// Cargo `depends-on` behavior implemented here. The broader accepted relation-
/// family inventory remains a later, independently versioned responsibility.
#[must_use]
pub fn semantic_resolution_contract_digest() -> String {
    semantic_resolution_contract_digest_for_identifier_profile(unicode_ident::UNICODE_VERSION)
}

/// Bind the Python identifier property's data version without duplicating its tables.
fn semantic_resolution_contract_digest_for_identifier_profile(
    identifier_version: (u8, u8, u8),
) -> String {
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
        if provider == SemanticProviderOwner::Python {
            hash_contract_value(&mut hasher, "python_xid_unicode_version");
            let (major, minor, patch) = identifier_version;
            hasher.update(&[major, minor, patch]);
        }
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

/// Lookup keys for one explicit forwarding slot, owned by its source file.
///
/// A matching file is export evidence to follow, never a callable declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportResolutionKeys {
    /// Ordinal in the source semantic export vector.
    export_index: usize,
    /// Separately bounded keys for this slot rather than one aggregate file budget.
    keys: Vec<CanonicalResolutionKey>,
}

impl ExportResolutionKeys {
    /// Return the explicit source export ordinal owning these lookup keys.
    #[must_use]
    pub const fn export_index(&self) -> usize {
        self.export_index
    }

    /// Borrow the slot's sorted, deduplicated file-owned lookup keys.
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
    /// File-owned lookup evidence for explicit forwarding slots.
    exports: Vec<ExportResolutionKeys>,
    /// Canonical dependencies associated with relation facts.
    relations: Vec<RelationResolutionKeys>,
    /// Source-bound call decisions; absence retains legacy graph-only helper behavior.
    source_calls: Option<Vec<SourceCallFact>>,
    /// Exact admitted occurrence coordinates by raw relation ordinal.
    source_spans: BTreeMap<usize, SourceSpan>,
    /// Native Rust import outcomes; absent metadata retains graph-only compatibility.
    rust_toolchain_imports: Option<BTreeMap<usize, Option<ImportReference>>>,
}

/// Repository configuration supplied to provider-owned semantic projection.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResolutionProjectionContext<'a> {
    /// Validated configured-module snapshot, when the runtime supplied one.
    configured_modules: Option<&'a ConfiguredModuleResolution>,
    /// Exact admitted parser proof for this graph, when supplied by the runtime.
    source_semantics: Option<&'a SourceSemanticFacts>,
    /// Owning Cargo manifest and repository-relative package root, when established.
    rust_package: Option<(&'a str, &'a str)>,
}

/// Provider scopes expanded once per unique import in one source graph.
type ImportScopeCache = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// Bounded contract failures retained while a graph keeps valid siblings.
struct ContractFailures {
    /// Rejected derived-key facts retained for typed adapter detail.
    failures: Vec<ResolutionProjectionFactFailure>,
    /// Number of distinct rejected derived-key facts observed.
    rejected_count: usize,
    /// Whether the source fact has already been rejected.
    source_seen: bool,
    /// Rejected symbol ordinals, bit-packed by `Vec<bool>`.
    symbol_seen: Vec<bool>,
    /// Rejected relation ordinals, bit-packed by `Vec<bool>`.
    relation_seen: Vec<bool>,
    /// Rejected explicit export ordinals.
    export_seen: Vec<bool>,
}

impl ContractFailures {
    /// Allocate closed ordinal tracking for one parser graph.
    fn new(symbol_count: usize, relation_count: usize, export_count: usize) -> Self {
        Self {
            failures: Vec::new(),
            rejected_count: 0,
            source_seen: false,
            symbol_seen: vec![false; symbol_count],
            relation_seen: vec![false; relation_count],
            export_seen: vec![false; export_count],
        }
    }

    /// Record the first failure for one dense parser fact.
    fn remember(&mut self, fact: ResolutionProjectionFact, error: GraphContractError) {
        let seen = match fact {
            ResolutionProjectionFact::Source => {
                let seen = self.source_seen;
                self.source_seen = true;
                !seen
            }
            ResolutionProjectionFact::Symbol(index) => {
                self.symbol_seen.get_mut(index).is_some_and(|seen| {
                    let was_seen = *seen;
                    *seen = true;
                    !was_seen
                })
            }
            ResolutionProjectionFact::Export(index) => {
                self.export_seen.get_mut(index).is_some_and(|seen| {
                    let was_seen = *seen;
                    *seen = true;
                    !was_seen
                })
            }
            ResolutionProjectionFact::Relation(index) => {
                self.relation_seen.get_mut(index).is_some_and(|seen| {
                    let was_seen = *seen;
                    *seen = true;
                    !was_seen
                })
            }
        };
        if !seen {
            return;
        }
        self.rejected_count = self.rejected_count.saturating_add(1);
        if self.failures.len() < MAX_RESOLUTION_PROJECTION_FAILURES {
            self.failures
                .push(ResolutionProjectionFactFailure { error, fact });
        }
    }
}

impl<'a> ResolutionProjectionContext<'a> {
    /// Construct a projection context with one validated configured-module snapshot.
    #[must_use]
    pub const fn with_configured_modules(
        configured_modules: &'a ConfiguredModuleResolution,
    ) -> Self {
        Self {
            configured_modules: Some(configured_modules),
            source_semantics: None,
            rust_package: None,
        }
    }

    /// Attach source-owned ordinals for exact lexical/import projection.
    #[must_use]
    pub const fn with_source_semantics(mut self, semantics: &'a SourceSemanticFacts) -> Self {
        self.source_semantics = Some(semantics);
        self
    }

    /// Attach actual Cargo ownership for automatic-target invalidation keys.
    #[must_use]
    pub const fn with_rust_package(mut self, manifest: &'a str, root: &'a str) -> Self {
        self.rust_package = Some((manifest, root));
        self
    }
}

impl ResolutionKeyProjection {
    /// Construct an empty projection when one parser graph has no admissible
    /// canonical identity keys. Entity and relation rows can still be staged;
    /// the graph publication layer records the rejected key provenance.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            source: Vec::new(),
            symbols: Vec::new(),
            exports: Vec::new(),
            relations: Vec::new(),
            source_calls: None,
            source_spans: BTreeMap::new(),
            rust_toolchain_imports: None,
        }
    }

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

    /// Estimate retained lookup vector metadata separately from canonical key text.
    #[must_use]
    pub fn export_lookup_vector_bytes(&self) -> u64 {
        (self.exports.capacity() as u64)
            .saturating_mul(std::mem::size_of::<ExportResolutionKeys>() as u64)
            .saturating_add(
                self.exports
                    .iter()
                    .map(|export| {
                        (export.keys.capacity() as u64)
                            .saturating_mul(std::mem::size_of::<CanonicalResolutionKey>() as u64)
                    })
                    .fold(0_u64, u64::saturating_add),
            )
    }

    /// Borrow separately bounded lookup keys owned by explicit source export slots.
    #[must_use]
    pub fn export_keys(&self) -> &[ExportResolutionKeys] {
        &self.exports
    }

    /// Borrow relation-index-associated dependency keys.
    #[must_use]
    pub fn relation_keys(&self) -> &[RelationResolutionKeys] {
        &self.relations
    }

    /// Borrow source-owned call policies, including an explicitly empty proof set.
    #[must_use]
    pub fn source_calls(&self) -> Option<&[SourceCallFact]> {
        self.source_calls.as_deref()
    }

    /// Estimate retained call-policy vector and payload capacities, including shared sets.
    #[must_use]
    pub fn source_calls_retained_bytes(&self) -> u64 {
        self.source_calls.as_ref().map_or(0, |calls| {
            (calls.capacity() as u64)
                .saturating_mul(std::mem::size_of::<SourceCallFact>() as u64)
                .saturating_add(crate::source_semantics::call_binding_heap_bytes(calls))
        })
    }

    /// Return exact source coordinates only for the corresponding admitted relation.
    #[must_use]
    pub fn source_span(&self, relation: usize) -> Option<SourceSpan> {
        self.source_spans.get(&relation).copied()
    }

    /// Account conservative retained coordinate map rows, excluding inline map metadata.
    #[must_use]
    pub fn source_spans_retained_bytes(&self) -> u64 {
        (self.source_spans.len() as u64).saturating_mul(
            (std::mem::size_of::<(usize, SourceSpan)>() + 4 * std::mem::size_of::<usize>()) as u64,
        )
    }

    /// Borrow source-proven Rust import ownership, including explicit non-toolchain rows.
    #[must_use]
    pub fn rust_toolchain_imports(&self) -> Option<&BTreeMap<usize, Option<ImportReference>>> {
        self.rust_toolchain_imports.as_ref()
    }

    /// Account the bounded Rust import policy map and its owned canonical paths.
    #[must_use]
    pub fn rust_toolchain_imports_retained_bytes(&self) -> u64 {
        self.rust_toolchain_imports.as_ref().map_or(0, |imports| {
            let mut modules = HashSet::new();
            imports.values().fold(0_u64, |bytes, reference| {
                bytes.saturating_add(128).saturating_add(
                    reference
                        .as_ref()
                        .map_or(0, |reference| reference.retained_heap_bytes(&mut modules)),
                )
            })
        })
    }
}

/// Parser fact whose derived canonical key was rejected by graph admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionProjectionFact {
    /// A source-file export scope.
    Source,
    /// A symbol-index-associated export.
    Symbol(usize),
    /// A relation-index-associated dependency.
    Relation(usize),
    /// A source-semantic export-slot lookup.
    Export(usize),
}

/// Maximum rejected derived-key facts retained from one parser graph.
pub const MAX_RESOLUTION_PROJECTION_FAILURES: usize = GraphLimits::MAX_ROWS as usize;

/// One parser fact whose derived canonical key violated the graph contract.
#[derive(Debug)]
pub struct ResolutionProjectionFactFailure {
    /// Stable contract error category for the rejected derived key.
    error: GraphContractError,
    /// Exact parser fact that produced the rejected derived key.
    fact: ResolutionProjectionFact,
}

impl ResolutionProjectionFactFailure {
    /// Return the graph-contract error without exposing rejected identity text.
    #[must_use]
    pub fn error(&self) -> &GraphContractError {
        &self.error
    }

    /// Return the parser fact that produced the rejected derived key.
    #[must_use]
    pub const fn fact(&self) -> ResolutionProjectionFact {
        self.fact
    }

    /// Consume one failure at an adapter boundary that needs its typed error.
    #[must_use]
    pub fn into_parts(self) -> (ResolutionProjectionFact, GraphContractError) {
        (self.fact, self.error)
    }
}

/// Bounded rejected derived-key facts and the partial valid projection.
#[derive(Debug)]
pub struct ResolutionProjectionFailure {
    /// Every retained rejected derived-key fact in deterministic traversal order.
    failures: Vec<ResolutionProjectionFactFailure>,
    /// Number of rejected derived-key facts, including rows beyond the detail bound.
    rejected_count: usize,
    /// All valid canonical keys retained from the graph.
    projection: ResolutionKeyProjection,
}

impl ResolutionProjectionFailure {
    /// Borrow every retained rejected derived-key fact.
    #[must_use]
    pub fn failures(&self) -> &[ResolutionProjectionFactFailure] {
        &self.failures
    }

    /// Return the total rejected derived-key count before bounded detail retention.
    #[must_use]
    pub const fn rejected_count(&self) -> usize {
        self.rejected_count
    }

    /// Borrow the projection containing all valid keys derived before and after the failure.
    #[must_use]
    pub fn projection(&self) -> &ResolutionKeyProjection {
        &self.projection
    }

    /// Consume the failure at an adapter boundary that needs all retained state.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        Vec<ResolutionProjectionFactFailure>,
        usize,
        ResolutionKeyProjection,
    ) {
        (self.failures, self.rejected_count, self.projection)
    }
}

/// Failure while deriving bounded canonical keys from parser facts.
#[derive(Debug)]
pub enum ResolutionProjectionError {
    /// Source ordinals do not belong to the supplied admitted graph.
    SourceEvidenceMismatch,
    /// An extracted or caller-supplied identity violated the graph contract.
    Contract(Box<ResolutionProjectionFailure>),
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
            Self::SourceEvidenceMismatch => {
                formatter.write_str("source semantic evidence does not match the projection graph")
            }
            Self::Contract(failure) => match failure.failures.first() {
                Some(failure) => {
                    write!(formatter, "invalid resolution identity: {}", failure.error)
                }
                None => formatter.write_str("invalid resolution identity"),
            },
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
            Self::Contract(failure) => failure
                .failures
                .first()
                .map(|failure| &failure.error as &(dyn Error + 'static)),
            Self::KeyLimit { .. } | Self::SourceEvidenceMismatch => None,
        }
    }
}

impl From<GraphContractError> for ResolutionProjectionError {
    fn from(value: GraphContractError) -> Self {
        Self::Contract(Box::new(ResolutionProjectionFailure {
            failures: vec![ResolutionProjectionFactFailure {
                error: value,
                fact: ResolutionProjectionFact::Source,
            }],
            rejected_count: 1,
            projection: ResolutionKeyProjection::empty(),
        }))
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
    /// Immutable module text shared by source slots and consumer clones.
    module: Arc<str>,
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
        Self::with_shared_module(syntax, Arc::from(module), imported, local)
    }

    /// Retain one statement's module allocation without copying it per slot.
    pub(crate) fn with_shared_module(
        syntax: ImportSyntax,
        module: Arc<str>,
        imported: Option<&str>,
        local: &str,
    ) -> Self {
        Self {
            syntax,
            module,
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

    /// Share the normalized module allocation without copying its source-owned bytes.
    #[must_use]
    pub fn shared_module(&self) -> Arc<str> {
        Arc::clone(&self.module)
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

    /// Charge a shared module once within its containing source estimate.
    pub(crate) fn retained_heap_bytes(&self, modules: &mut HashSet<*const str>) -> u64 {
        let module = if modules.insert(Arc::as_ptr(&self.module)) {
            (self.module.len() as u64).saturating_add((2 * std::mem::size_of::<usize>()) as u64)
        } else {
            0
        };
        module
            .saturating_add(self.imported.as_ref().map_or(0, String::capacity) as u64)
            .saturating_add(self.local.capacity() as u64)
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
    derive_resolution_keys_with_context(
        project,
        package,
        graph,
        ResolutionProjectionContext::default(),
    )
}

/// Derive bounded canonical keys with explicit repository module configuration.
///
/// # Errors
///
/// Returns an error when a graph identity is invalid or one parser fact exceeds
/// the bounded canonical-key fan-out.
pub fn derive_resolution_keys_with_context(
    project: ProjectInstanceId,
    package: Option<&str>,
    graph: &SymbolGraph,
    context: ResolutionProjectionContext<'_>,
) -> Result<ResolutionKeyProjection, ResolutionProjectionError> {
    if context
        .source_semantics
        .is_some_and(|facts| !facts.matches_graph(graph))
    {
        return Err(ResolutionProjectionError::SourceEvidenceMismatch);
    }
    let source_calls = context.source_semantics.map(|facts| facts.calls().to_vec());
    let mut source_spans = BTreeMap::new();
    if let Some(facts) = context.source_semantics {
        for (relation, span) in facts
            .calls()
            .iter()
            .map(|fact| (fact.relation, fact.source_span))
            .chain(
                facts
                    .imports()
                    .iter()
                    .map(|fact| (fact.relation, fact.source_span)),
            )
        {
            if let Some(span) = span
                && source_spans
                    .insert(relation, span)
                    .is_some_and(|previous| previous != span)
            {
                return Err(ResolutionProjectionError::SourceEvidenceMismatch);
            }
        }
    }
    let rust_toolchain_imports = context
        .source_semantics
        .filter(|_| semantic::provider_for_graph(graph) == Some(SemanticProviderOwner::Rust))
        .map(|facts| {
            facts
                .imports()
                .iter()
                .map(|import| (import.relation, import.rust_toolchain.as_deref().cloned()))
                .collect::<BTreeMap<_, _>>()
        });
    let Some(provider_owner) = semantic::provider_for_graph(graph) else {
        return Ok(ResolutionKeyProjection {
            source: Vec::new(),
            symbols: Vec::new(),
            exports: Vec::new(),
            relations: Vec::new(),
            source_calls,
            source_spans,
            rust_toolchain_imports,
        });
    };
    let provider = GraphIdentityText::new(provider_owner.as_str())?;
    let Some(resolution_family) = provider_owner.resolution_family() else {
        return Ok(ResolutionKeyProjection {
            source: Vec::new(),
            symbols: Vec::new(),
            exports: Vec::new(),
            relations: Vec::new(),
            source_calls,
            source_spans,
            rust_toolchain_imports,
        });
    };
    let mut contract_failures = ContractFailures::new(
        graph.symbols.len(),
        graph.relations.len(),
        context
            .source_semantics
            .map_or(0, |facts| facts.exports().len()),
    );
    let language = GraphIdentityText::new(resolution_family)?;
    let package = package.map(GraphIdentityText::new).transpose()?;

    let source_scopes = canonical_source_scopes(&graph.path, provider_owner);
    let configured_source_scopes =
        if provider_owner == SemanticProviderOwner::EcmaScript && package.is_some() {
            canonical_configured_source_scopes(&graph.path)
        } else {
            Vec::new()
        };
    let emits_source_module_keys = semantic::emits_source_module_keys(graph)
        || (provider_owner == SemanticProviderOwner::EcmaScript
            && context.source_semantics.is_some_and(|facts| {
                !facts.imports().is_empty()
                    || !facts.calls().is_empty()
                    || !facts.exports().is_empty()
            }));
    let mut source_keys = Vec::new();
    if emits_source_module_keys {
        source_keys.reserve(source_scopes.len() + configured_source_scopes.len());
        for scope in &source_scopes {
            if let Some(key) = retain_derived_key(
                canonical_key(
                    project,
                    ResolutionKeyDomain::Module,
                    &provider,
                    &language,
                    package.as_ref(),
                    None,
                    RelationKind::Imports,
                    scope,
                ),
                ResolutionProjectionFact::Source,
                &mut contract_failures,
            ) {
                source_keys.push(key);
            }
        }
        for scope in &configured_source_scopes {
            if let Some(key) = retain_derived_key(
                canonical_key(
                    project,
                    ResolutionKeyDomain::Module,
                    &provider,
                    &language,
                    None,
                    None,
                    RelationKind::Imports,
                    scope,
                ),
                ResolutionProjectionFact::Source,
                &mut contract_failures,
            ) {
                source_keys.push(key);
            }
        }
    }
    if provider_owner == SemanticProviderOwner::Rust
        && let Some((manifest, root)) = context.rust_package
        && let Some(package) = package.as_ref()
        && let Some(directory) =
            crate::rust_binding_source::automatic_target_directory(&graph.path, root)
        && let Some(key) = retain_derived_key(
            crate::cargo_target_inventory_key(project, package.as_str(), manifest, directory),
            ResolutionProjectionFact::Source,
            &mut contract_failures,
        )
    {
        source_keys.push(key);
    }
    let source_keys = bounded_keys(source_keys, "source", 0)?;

    let explicit_export_names = context
        .source_semantics
        .filter(|_| {
            matches!(
                provider_owner,
                SemanticProviderOwner::EcmaScript | SemanticProviderOwner::Python
            )
        })
        .map(|facts| {
            let mut names = BTreeMap::<usize, Vec<&str>>::new();
            if !facts.exports_complete() {
                return (0..graph.symbols.len())
                    .map(|symbol| (symbol, Vec::new()))
                    .collect();
            }
            let mut slots = BTreeMap::<&str, usize>::new();
            for export in facts
                .exports()
                .iter()
                .filter(|export| export.namespace == crate::SourceExportNamespace::Value)
            {
                *slots.entry(export.name.as_str()).or_default() += 1;
            }
            for export in facts
                .exports()
                .iter()
                .filter(|export| export.namespace == crate::SourceExportNamespace::Value)
            {
                match &export.binding {
                    crate::SourceExportBinding::LocalDeclaration(symbol) => {
                        let names = names.entry(*symbol).or_default();
                        if slots.get(export.name.as_str()) == Some(&1) {
                            names.push(export.name.as_str());
                        }
                    }
                    crate::SourceExportBinding::AmbiguousDeclarations(symbols)
                    | crate::SourceExportBinding::UnresolvedDeclarations(symbols) => {
                        for symbol in symbols.iter() {
                            names.entry(*symbol).or_default();
                        }
                    }
                    crate::SourceExportBinding::ReExport(_)
                    | crate::SourceExportBinding::RustReExport(_)
                    | crate::SourceExportBinding::Unresolved => {}
                }
            }
            for names in names.values_mut() {
                names.sort_unstable();
                names.dedup();
            }
            names
        });
    let mut symbol_keys = Vec::new();
    for (symbol_index, symbol) in graph.symbols.iter().enumerate() {
        let mut keys = Vec::new();
        match symbol.kind {
            SymbolKind::Package if provider_owner == SemanticProviderOwner::Cargo => {
                if let Some(key) = retain_derived_key(
                    canonical_key(
                        project,
                        ResolutionKeyDomain::Package,
                        &provider,
                        &language,
                        None,
                        None,
                        RelationKind::DependsOn,
                        &symbol.name,
                    ),
                    ResolutionProjectionFact::Symbol(symbol_index),
                    &mut contract_failures,
                ) {
                    keys.push(key);
                }
            }
            SymbolKind::Dependency | SymbolKind::Import | SymbolKind::Workspace => {}
            _ if emits_source_module_keys
                && context
                    .source_semantics
                    .is_none_or(|facts| !facts.bindings_incomplete)
                && !semantic::blocks_direct_call_export(provider_owner, symbol.kind)
                && (semantic::is_export_candidate(provider_owner, graph, symbol_index)
                    || explicit_export_names
                        .as_ref()
                        .is_some_and(|names| names.contains_key(&symbol_index))) =>
            {
                let names = match explicit_export_names
                    .as_ref()
                    .and_then(|names| names.get(&symbol_index))
                {
                    Some(names) => names
                        .iter()
                        .map(|name| std::borrow::Cow::Borrowed(*name))
                        .collect::<Vec<_>>(),
                    None => vec![if provider_owner == SemanticProviderOwner::EcmaScript {
                        semantic::ecmascript_identifier_value(&symbol.name).map_or(
                            std::borrow::Cow::Borrowed(symbol.name.as_str()),
                            std::borrow::Cow::Owned,
                        )
                    } else {
                        std::borrow::Cow::Borrowed(symbol.name.as_str())
                    }],
                };
                for name in names {
                    let identity = match GraphIdentityText::new(name.into_owned()) {
                        Ok(identity) => identity,
                        Err(error) => {
                            remember_contract_failure(
                                &mut contract_failures,
                                ResolutionProjectionFact::Symbol(symbol_index),
                                error,
                            );
                            continue;
                        }
                    };
                    let mut scopes = Vec::new();
                    if let Some(parent) = symbol.parent.as_deref() {
                        scopes.extend(
                            source_scopes.iter().map(|scope| {
                                normalize_repository_scope(&format!("{scope}/{parent}"))
                            }),
                        );
                        scopes.push(normalize_repository_scope(parent));
                    } else {
                        scopes.extend(source_scopes.iter().cloned());
                        scopes.push(String::new());
                    }
                    scopes.sort();
                    scopes.dedup();
                    for scope in &scopes {
                        let scope = if scope.is_empty() {
                            None
                        } else {
                            match GraphIdentityText::new(scope.clone()) {
                                Ok(scope) => Some(scope),
                                Err(error) => {
                                    remember_contract_failure(
                                        &mut contract_failures,
                                        ResolutionProjectionFact::Symbol(symbol_index),
                                        error,
                                    );
                                    continue;
                                }
                            }
                        };
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
                    let configured_scopes = configured_source_scopes.iter().map(|scope| {
                        symbol.parent.as_deref().map_or_else(
                            || scope.clone(),
                            |parent| normalize_repository_scope(&format!("{scope}/{parent}")),
                        )
                    });
                    for scope in configured_scopes {
                        let scope = match GraphIdentityText::new(scope) {
                            Ok(scope) => scope,
                            Err(error) => {
                                remember_contract_failure(
                                    &mut contract_failures,
                                    ResolutionProjectionFact::Symbol(symbol_index),
                                    error,
                                );
                                continue;
                            }
                        };
                        keys.push(CanonicalResolutionKey::new(
                            project,
                            ResolutionKeyDomain::Declaration,
                            &provider,
                            &language,
                            None,
                            Some(&scope),
                            Some(GraphRelationKind::from_legacy(RelationKind::Calls)),
                            &identity,
                        ));
                    }
                    if keys.len() > MAX_RESOLUTION_KEYS_PER_FACT {
                        keys = bounded_keys(keys, "symbol", symbol_index)?;
                    }
                }
            }
            _ => {}
        }
        let keys = bounded_keys(keys, "symbol", symbol_index)?;
        if !keys.is_empty() {
            symbol_keys.push(SymbolResolutionKeys { symbol_index, keys });
        }
    }

    let mut export_keys = Vec::new();
    if matches!(
        provider_owner,
        SemanticProviderOwner::EcmaScript | SemanticProviderOwner::Rust
    ) && let Some(facts) = context
        .source_semantics
        .filter(|facts| facts.exports_complete())
    {
        for (export_index, export) in facts.exports().iter().enumerate() {
            if export.namespace != crate::SourceExportNamespace::Value
                || !matches!(
                    export.binding,
                    crate::SourceExportBinding::ReExport(_)
                        | crate::SourceExportBinding::RustReExport(_)
                        | crate::SourceExportBinding::UnresolvedDeclarations(_)
                        | crate::SourceExportBinding::Unresolved
                )
            {
                continue;
            }
            let fact = ResolutionProjectionFact::Export(export_index);
            let mut keys = Vec::new();
            let rust_module = match &export.binding {
                crate::SourceExportBinding::RustReExport(binding) => binding.module.as_ref(),
                _ => &[],
            };
            for scope in source_scopes.iter().map(|scope| {
                if rust_module.is_empty() {
                    scope.clone()
                } else {
                    format!("{scope}/{}", rust_module.join("/"))
                }
            }) {
                if let Some(key) = retain_derived_key(
                    canonical_key(
                        project,
                        ResolutionKeyDomain::Declaration,
                        &provider,
                        &language,
                        package.as_ref(),
                        Some(&scope),
                        RelationKind::Calls,
                        &export.name,
                    ),
                    fact,
                    &mut contract_failures,
                ) {
                    keys.push(key);
                }
            }
            for scope in &configured_source_scopes {
                if let Some(key) = retain_derived_key(
                    canonical_key(
                        project,
                        ResolutionKeyDomain::Declaration,
                        &provider,
                        &language,
                        None,
                        Some(scope),
                        RelationKind::Calls,
                        &export.name,
                    ),
                    fact,
                    &mut contract_failures,
                ) {
                    keys.push(key);
                }
            }
            let keys = bounded_keys(keys, "export", export_index)?;
            if !keys.is_empty() {
                export_keys.push(ExportResolutionKeys { export_index, keys });
            }
        }
    }

    let mut parsed_imports = if context.source_semantics.is_some() {
        vec![Vec::new(); graph.relations.len()]
    } else {
        graph
            .relations
            .iter()
            .map(|relation| {
                if relation.kind == RelationKind::Imports {
                    semantic::parse_imports(provider_owner, &relation.target_name)
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>()
    };
    if let Some(source) = context.source_semantics {
        for references in &mut parsed_imports {
            references.clear();
        }
        for import in source.imports() {
            parsed_imports[import.relation].clone_from(&import.references);
        }
    }
    let mut aliases: BTreeMap<&str, Vec<&ImportReference>> = BTreeMap::new();
    for references in &parsed_imports {
        for reference in references {
            aliases
                .entry(reference.local())
                .or_default()
                .push(reference);
        }
    }
    let configured_source = context
        .configured_modules
        .map(|configured| configured.for_source(&graph.path));
    let mut import_scopes = build_import_scope_cache(
        provider_owner,
        graph,
        &parsed_imports,
        configured_source.as_ref(),
    );
    let rust_calls = context
        .source_semantics
        .map(|facts| {
            facts
                .calls()
                .iter()
                .filter_map(|call| {
                    if let SourceCallBinding::RustPath(path) = &call.binding {
                        path.reference().map(|reference| (call.relation, reference))
                    } else {
                        None
                    }
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    for reference in rust_calls.values() {
        import_scopes
            .entry(graph.path.clone())
            .or_default()
            .entry(reference.module().to_string())
            .or_insert_with(|| {
                semantic::import_scopes(
                    provider_owner,
                    &graph.path,
                    reference,
                    configured_source.as_ref(),
                )
            });
    }
    if let Some(facts) = context.source_semantics {
        for call in facts.calls() {
            if let SourceCallBinding::Qualified(reference) = &call.binding {
                import_scopes
                    .entry(graph.path.clone())
                    .or_default()
                    .entry(reference.module().to_string())
                    .or_insert_with(|| {
                        semantic::import_scopes(
                            provider_owner,
                            &graph.path,
                            reference,
                            configured_source.as_ref(),
                        )
                    });
            }
        }
    }

    let mut relation_keys = Vec::new();
    let source_import_index = context.source_semantics.map(|facts| {
        facts
            .imports()
            .iter()
            .map(|fact| (fact.relation, fact))
            .collect::<BTreeMap<_, _>>()
    });
    let source_call_index = source_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|fact| (fact.relation, fact))
            .collect::<BTreeMap<_, _>>()
    });
    for (relation_index, relation) in graph.relations.iter().enumerate() {
        let mut keys = match relation.kind {
            RelationKind::Imports if semantic::supports_source_dependencies(provider_owner) => {
                if provider_owner == SemanticProviderOwner::EcmaScript
                    && let Some(module) = source_import_index
                        .as_ref()
                        .and_then(|imports| imports.get(&relation_index))
                        .and_then(|fact| fact.module.as_deref())
                {
                    semantic::ecmascript_module_scopes(
                        &relation.path,
                        module,
                        configured_source.as_ref(),
                    )
                    .iter()
                    .filter_map(|scope| {
                        retain_derived_key(
                            canonical_key(
                                project,
                                ResolutionKeyDomain::Module,
                                &provider,
                                &language,
                                dependency_package(provider_owner, module, package.as_ref()),
                                None,
                                RelationKind::Imports,
                                scope,
                            ),
                            ResolutionProjectionFact::Relation(relation_index),
                            &mut contract_failures,
                        )
                    })
                    .collect()
                } else {
                    import_dependency_keys(
                        project,
                        provider_owner,
                        &provider,
                        &language,
                        package.as_ref(),
                        &relation.path,
                        &parsed_imports[relation_index],
                        &import_scopes,
                        relation_index,
                        &mut contract_failures,
                    )
                }
            }
            RelationKind::Calls if semantic::supports_source_dependencies(provider_owner) => {
                if let Some(calls) = &source_call_index {
                    if let Some(SourceCallFact {
                        binding:
                            SourceCallBinding::Imported {
                                import,
                                reference,
                                member,
                                ..
                            },
                        ..
                    }) = calls.get(&relation_index).copied()
                    {
                        let reference = &context
                            .source_semantics
                            .ok_or(ResolutionProjectionError::SourceEvidenceMismatch)?
                            .imports()[*import]
                            .references[*reference];
                        imported_reference_call_keys(
                            project,
                            provider_owner,
                            &provider,
                            &language,
                            package.as_ref(),
                            &relation.path,
                            reference,
                            member.as_deref(),
                            &import_scopes,
                            relation_index,
                            &mut contract_failures,
                        )
                    } else if let Some(SourceCallFact {
                        binding: SourceCallBinding::Qualified(reference),
                        ..
                    }) = calls.get(&relation_index).copied()
                    {
                        imported_reference_call_keys(
                            project,
                            provider_owner,
                            &provider,
                            &language,
                            package.as_ref(),
                            &relation.path,
                            reference,
                            None,
                            &import_scopes,
                            relation_index,
                            &mut contract_failures,
                        )
                    } else if let Some(reference) = rust_calls.get(&relation_index) {
                        imported_reference_call_keys(
                            project,
                            provider_owner,
                            &provider,
                            &language,
                            package.as_ref(),
                            &relation.path,
                            reference,
                            None,
                            &import_scopes,
                            relation_index,
                            &mut contract_failures,
                        )
                    } else {
                        Vec::new()
                    }
                } else {
                    call_dependency_keys(
                        project,
                        provider_owner,
                        &provider,
                        &language,
                        package.as_ref(),
                        &relation.path,
                        &relation.target_name,
                        &aliases,
                        &import_scopes,
                        relation_index,
                        &mut contract_failures,
                    )
                }
            }
            RelationKind::DependsOn if semantic::supports_package_dependencies(provider_owner) => {
                retain_derived_key(
                    canonical_key(
                        project,
                        ResolutionKeyDomain::Package,
                        &provider,
                        &language,
                        None,
                        None,
                        RelationKind::DependsOn,
                        &relation.target_name,
                    ),
                    ResolutionProjectionFact::Relation(relation_index),
                    &mut contract_failures,
                )
                .into_iter()
                .collect()
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

    let projection = ResolutionKeyProjection {
        source: source_keys,
        symbols: symbol_keys,
        exports: export_keys,
        relations: relation_keys,
        source_calls,
        source_spans,
        rust_toolchain_imports,
    };
    if contract_failures.failures.is_empty() {
        Ok(projection)
    } else {
        Err(ResolutionProjectionError::Contract(Box::new(
            ResolutionProjectionFailure {
                failures: contract_failures.failures,
                rejected_count: contract_failures.rejected_count,
                projection,
            },
        )))
    }
}

/// Expand each unique import scope once for the complete source graph.
fn build_import_scope_cache(
    provider_owner: SemanticProviderOwner,
    graph: &SymbolGraph,
    parsed_imports: &[Vec<ImportReference>],
    configured_source: Option<&ConfiguredModuleSource<'_>>,
) -> ImportScopeCache {
    let mut cache = ImportScopeCache::new();
    for (relation, references) in graph.relations.iter().zip(parsed_imports) {
        let caller_path = &relation.path;
        let modules = cache.entry(caller_path.clone()).or_default();
        for reference in references {
            modules
                .entry(reference.module().to_string())
                .or_insert_with(|| {
                    semantic::import_scopes(
                        provider_owner,
                        caller_path,
                        reference,
                        configured_source,
                    )
                });
        }
    }
    cache
}

/// Borrow one pre-expanded import scope list without allocating lookup keys.
fn cached_import_scopes<'a>(
    cache: &'a ImportScopeCache,
    caller_path: &str,
    reference: &ImportReference,
) -> &'a [String] {
    cache
        .get(caller_path)
        .and_then(|modules| modules.get(reference.module()))
        .map_or(&[], Vec::as_slice)
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
    import_scopes: &ImportScopeCache,
    relation_index: usize,
    contract_failures: &mut ContractFailures,
) -> Vec<CanonicalResolutionKey> {
    let mut keys = Vec::new();
    for reference in references {
        let scopes = cached_import_scopes(import_scopes, caller_path, reference);
        let package = dependency_package(provider_owner, reference.module(), package);
        for scope in scopes {
            if let Some(key) = retain_derived_key(
                canonical_key(
                    project,
                    ResolutionKeyDomain::Module,
                    provider,
                    language,
                    package,
                    None,
                    RelationKind::Imports,
                    scope,
                ),
                ResolutionProjectionFact::Relation(relation_index),
                contract_failures,
            ) {
                keys.push(key);
            }
        }
    }
    keys
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
    import_scopes: &ImportScopeCache,
    relation_index: usize,
    contract_failures: &mut ContractFailures,
) -> Vec<CanonicalResolutionKey> {
    let (prefix, remainder) = split_qualified_target(target);
    let alias = prefix.unwrap_or(target).trim();
    if let Some(references) = aliases.get(alias) {
        let mut keys = Vec::new();
        for reference in references {
            keys.extend(imported_reference_call_keys(
                project,
                provider_owner,
                provider,
                language,
                package,
                caller_path,
                reference,
                remainder,
                import_scopes,
                relation_index,
                contract_failures,
            ));
        }
        return keys;
    }

    let (scope, identity) = split_qualified_target(target);
    if scope.is_some() {
        return Vec::new();
    }
    let Some(identity) = identity.or_else(|| (!target.trim().is_empty()).then_some(target.trim()))
    else {
        return Vec::new();
    };
    retain_derived_key(
        canonical_key(
            project,
            ResolutionKeyDomain::Declaration,
            provider,
            language,
            package,
            None,
            RelationKind::Calls,
            identity,
        ),
        ResolutionProjectionFact::Relation(relation_index),
        contract_failures,
    )
    .into_iter()
    .collect()
}

/// Derive one call from its selected source import, without a file-wide alias guess.
fn imported_reference_call_keys(
    project: ProjectInstanceId,
    provider_owner: SemanticProviderOwner,
    provider: &GraphIdentityText,
    language: &GraphIdentityText,
    package: Option<&GraphIdentityText>,
    caller_path: &str,
    reference: &ImportReference,
    member: Option<&str>,
    import_scopes: &ImportScopeCache,
    relation_index: usize,
    contract_failures: &mut ContractFailures,
) -> Vec<CanonicalResolutionKey> {
    let mut keys = Vec::new();
    let (member_parent, identity) = match member {
        Some(member) if provider_owner == SemanticProviderOwner::Rust => member
            .rsplit_once("::")
            .map_or((None, Some(member)), |(parent, name)| {
                (Some(parent), Some(name))
            }),
        _ => (None, member.or_else(|| reference.imported())),
    };
    let Some(identity) = identity else {
        return Vec::new();
    };
    let scopes = cached_import_scopes(import_scopes, caller_path, reference);
    let package = dependency_package(provider_owner, reference.module(), package);
    for scope in scopes {
        let mut scope = std::borrow::Cow::Borrowed(scope.as_str());
        if member.is_some()
            && let Some(imported_parent) = reference.imported()
        {
            scope = std::borrow::Cow::Owned(format!("{scope}/{imported_parent}"));
        }
        if let Some(parent) = member_parent {
            scope =
                std::borrow::Cow::Owned(format!("{scope}/{}", normalize_repository_scope(parent)));
        }
        if let Some(key) = retain_derived_key(
            canonical_key(
                project,
                ResolutionKeyDomain::Declaration,
                provider,
                language,
                package,
                Some(scope.as_ref()),
                RelationKind::Calls,
                identity,
            ),
            ResolutionProjectionFact::Relation(relation_index),
            contract_failures,
        ) {
            keys.push(key);
        }
    }
    keys
}

/// Configured ECMAScript targets are repository paths, so caller Cargo
/// ownership must not prevent them from matching a sibling package export.
fn dependency_package<'a>(
    provider_owner: SemanticProviderOwner,
    module: &str,
    package: Option<&'a GraphIdentityText>,
) -> Option<&'a GraphIdentityText> {
    if provider_owner == SemanticProviderOwner::EcmaScript
        && !(module.starts_with("./") || module.starts_with("../"))
    {
        None
    } else {
        package
    }
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

/// Retain one valid derived key while remembering the first rejected fact.
fn retain_derived_key(
    result: Result<CanonicalResolutionKey, GraphContractError>,
    fact: ResolutionProjectionFact,
    contract_failures: &mut ContractFailures,
) -> Option<CanonicalResolutionKey> {
    match result {
        Ok(key) => Some(key),
        Err(error) => {
            remember_contract_failure(contract_failures, fact, error);
            None
        }
    }
}

/// Keep one deterministic contract failure without retaining rejected identity text.
fn remember_contract_failure(
    contract_failures: &mut ContractFailures,
    fact: ResolutionProjectionFact,
    error: GraphContractError,
) {
    contract_failures.remember(fact, error);
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
fn canonical_source_scopes(path: &str, provider: SemanticProviderOwner) -> Vec<String> {
    if provider == SemanticProviderOwner::EcmaScript {
        return canonical_configured_source_scopes(path);
    }
    let mut scopes = source_stems_for_path(path);
    scopes.extend(module_aliases_for_path(path));
    normalize_scopes(scopes, normalize_repository_scope)
}

/// Return only repository-exact scopes used by configured ECMAScript targets.
fn canonical_configured_source_scopes(path: &str) -> Vec<String> {
    let mut scopes = source_stems_for_path(path)
        .into_iter()
        .filter(|scope| !scope.is_empty())
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    ecmascript_source_scopes(path, scopes)
}

/// Frame extensionless aliases separately from explicit file identities.
///
/// Keep only the established JavaScript-output source substitutions. These keys
/// represent candidate sets, not compiler search order; competing files remain
/// ambiguous. In particular `.mjs`, `.cjs` and embedded hosts never share them.
fn ecmascript_source_scopes(path: &str, module_scopes: Vec<String>) -> Vec<String> {
    let mut scopes = module_scopes
        .into_iter()
        .map(|scope| format!("{ECMA_MODULE_SCOPE}{scope}"))
        .collect::<Vec<_>>();
    scopes.push(format!("{ECMA_FILE_SCOPE}{path}"));
    let lower = path.to_ascii_lowercase();
    let substitution = [".d.ts", ".tsx", ".ts", ".jsx"]
        .into_iter()
        .find(|extension| lower.ends_with(extension));
    if let Some(extension) = substitution {
        let stem = &path[..path.len() - extension.len()];
        scopes.push(format!("{ECMA_FILE_SCOPE}{stem}.js"));
        if matches!(extension, ".d.ts" | ".tsx") {
            scopes.push(format!("{ECMA_FILE_SCOPE}{stem}.jsx"));
        }
    }
    scopes.sort();
    scopes.dedup();
    scopes
}

/// Preserve explicit terminal suffixes independently of automatic language
/// detection; directory dots do not make an extensionless module a file lookup.
pub(super) fn ecmascript_module_scope(path: &str) -> String {
    let explicit = path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'));
    let namespace = if explicit {
        ECMA_FILE_SCOPE
    } else {
        ECMA_MODULE_SCOPE
    };
    format!("{namespace}{path}")
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
        ImportReference, ImportSyntax, ResolutionProjectionContext, ResolutionProjectionError,
        SEMANTIC_RESOLUTION_CONTRACT_VERSION, build_import_scope_cache, derive_resolution_keys,
        derive_resolution_keys_with_context, parse_import_references, resolve_relative_import_path,
        semantic_resolution_contract_digest,
    };
    use crate::{
        ConfiguredModuleResolution, EcmaScriptConfigKind, EcmaScriptModuleConfig,
        EcmaScriptPathMapping, extract_symbol_graph,
    };
    use projectatlas_core::graph::{CanonicalResolutionKey, ProjectInstanceId};
    use projectatlas_core::language::SemanticProviderOwner;
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use std::error::Error;
    use std::io;

    fn reference(
        syntax: ImportSyntax,
        module: &str,
        imported: Option<&str>,
        local: &str,
    ) -> ImportReference {
        ImportReference::new(syntax, module, imported, local)
    }

    #[test]
    fn explicit_forwarding_slots_have_separate_file_owned_lookup_budgets()
    -> Result<(), Box<dyn Error>> {
        use std::fmt::Write;
        let project = project_id(91)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let mut source = String::new();
        for index in 0..80 {
            writeln!(
                source,
                "export {{ work as slot{index} }} from './worker.js';"
            )?;
        }
        source.push_str("export type { Item as typed } from './types'; export * from './wild';");
        let extracted = crate::extract_symbol_graph_with_semantics_controlled(
            "src/barrel.ts",
            Some("typescript"),
            &source,
            &control,
        )?;
        let projection = derive_resolution_keys_with_context(
            project,
            None,
            extracted.graph(),
            ResolutionProjectionContext::default().with_source_semantics(extracted.semantics()),
        )?;
        require(
            projection.export_keys().len() == 80,
            "forwarding slots used an aggregate file fanout budget or admitted a type/wildcard slot",
        )?;
        require(
            projection.symbol_keys().is_empty(),
            "forwarding slot synthesized declaration keys",
        )?;
        require(
            projection.export_lookup_vector_bytes() > 0,
            "slot vector capacity not accounted",
        )?;
        for export in projection.export_keys() {
            require(
                !export.keys().is_empty()
                    && export.keys().len() <= super::MAX_RESOLUTION_KEYS_PER_FACT,
                "per-slot key bound",
            )?;
            require(
                matches!(
                    extracted.semantics().exports()[export.export_index()].binding,
                    crate::SourceExportBinding::ReExport(_)
                ),
                "lookup keys lost source association",
            )?;
        }
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/caller.ts",
            Some("typescript"),
            "import {slot79} from './barrel'; function caller() {slot79();}",
            &control,
        )?;
        let caller_projection = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        require(
            projection.export_keys().iter().any(|slot| {
                keys_intersect(
                    relation_keys(caller.graph(), &caller_projection, "slot79"),
                    slot.keys(),
                )
            }),
            "consumer cannot find file-owned forwarding slot",
        )?;
        Ok(())
    }

    #[test]
    fn invalid_forwarding_identity_retains_separately_valid_export_slots()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(92)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let extracted = crate::extract_symbol_graph_with_semantics_controlled(
            "src/barrel.js",
            Some("javascript"),
            "export { work as '' } from './worker';\nexport {work as good} from './worker';",
            &control,
        )?;
        let Err(ResolutionProjectionError::Contract(failure)) = derive_resolution_keys_with_context(
            project,
            None,
            extracted.graph(),
            ResolutionProjectionContext::default().with_source_semantics(extracted.semantics()),
        ) else {
            return Err("invalid export lookup identity did not reach admission".into());
        };
        require(
            failure.rejected_count() == 1
                && failure.failures()[0].fact() == super::ResolutionProjectionFact::Export(0),
            "export rejection did not retain exact source ordinal",
        )?;
        require(
            failure.projection().export_keys().len() == 1
                && failure.projection().export_keys()[0].export_index() == 1,
            "invalid slot erased valid sibling",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_default_slots_never_narrow_to_one_surviving_declaration()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(87)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for (language, path) in [
            ("javascript", "target.js"),
            ("typescript", "target.ts"),
            ("tsx", "target.tsx"),
            ("vue", "target.vue"),
            ("svelte", "target.svelte"),
            ("html", "target.html"),
        ] {
            for second in [
                "export default class Second {}",
                "export default function () {}",
                "export default {};",
                "export default function First() {}",
                "export { secondary as default };",
                "export { secondary as 'default' };",
                "export { default } from './other';",
                "export * as default from './other';",
            ] {
                let script = format!(
                    "export default function First() {{}}\n{second}\nexport function secondary() {{}}"
                );
                let content = if matches!(language, "vue" | "svelte" | "html") {
                    let role = match language {
                        "html" => "type=module",
                        "svelte" => "module",
                        _ => "",
                    };
                    format!("<script {role}>{script}</script>")
                } else {
                    script
                };
                let extracted = crate::extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &control,
                )?;
                let projection = derive_resolution_keys_with_context(
                    project,
                    None,
                    extracted.graph(),
                    ResolutionProjectionContext::default()
                        .with_source_semantics(extracted.semantics()),
                )?;
                if !symbol_keys(extracted.graph(), &projection, "First").is_empty() {
                    return Err(
                        format!("duplicate default gained a target: {language}: {second}").into(),
                    );
                }
                if symbol_keys(extracted.graph(), &projection, "secondary").is_empty() {
                    return Err(format!(
                        "invalid default removed independent named export: {language}: {second}"
                    )
                    .into());
                }
                let (_, mut graph, mut facts) = extracted.into_parts();
                let symbols = graph
                    .symbols
                    .iter()
                    .map(|symbol| symbol.name != "Second")
                    .collect::<Vec<_>>();
                let relations = vec![true; graph.relations.len()];
                facts.retain_graph_facts(&mut graph, &symbols, &relations)?;
                let projection = derive_resolution_keys_with_context(
                    project,
                    None,
                    &graph,
                    ResolutionProjectionContext::default().with_source_semantics(&facts),
                )?;
                require(
                    symbol_keys(&graph, &projection, "First").is_empty(),
                    "filtering a competing default narrowed export proof",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn duplicate_export_slots_do_not_erase_independent_names_for_one_declaration()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(88)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/caller.js",
            Some("javascript"),
            "import Entry, { named, duplicate, Actual } from './target'; function caller() { Entry(); named(); duplicate(); Actual(); }",
            &control,
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        for source in [
            "export default function Actual() {} export { Actual as named, Actual as duplicate }; export { missing as default, missing as duplicate };",
            "function Actual() {} export { Actual as default, Actual as named, Actual as duplicate }; export { missing as default, missing as duplicate };",
        ] {
            let target = crate::extract_symbol_graph_with_semantics_controlled(
                "src/target.js",
                Some("javascript"),
                source,
                &control,
            )?;
            let target_keys = derive_resolution_keys_with_context(
                project,
                None,
                target.graph(),
                ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
            )?;
            let actual = symbol_keys(target.graph(), &target_keys, "Actual");
            require(
                keys_intersect(relation_keys(caller.graph(), &caller_keys, "named"), actual),
                "duplicate removed independent named slot",
            )?;
            for name in ["Entry", "duplicate", "Actual"] {
                require(
                    !keys_intersect(relation_keys(caller.graph(), &caller_keys, name), actual),
                    "duplicate or absent slot fabricated a target",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn python_rebound_module_names_withhold_only_their_export_keys() -> Result<(), Box<dyn Error>> {
        let project = project_id(93)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for (suffix, rebound) in [
            ("def mutate():\n global run\n run = unknown\n", true),
            ("run = unknown\n", true),
            (
                "def invalid():\n run()\n global run\n run = unknown\n",
                false,
            ),
        ] {
            let source =
                format!("def run(): pass\ndef safe(): pass\ndef _private(): pass\n{suffix}");
            let extracted = crate::extract_symbol_graph_with_semantics_controlled(
                "worker.py",
                Some("python"),
                &source,
                &control,
            )?;
            let keys = derive_resolution_keys_with_context(
                project,
                None,
                extracted.graph(),
                ResolutionProjectionContext::default().with_source_semantics(extracted.semantics()),
            )?;
            require(
                symbol_keys(extracted.graph(), &keys, "run").is_empty() == rebound,
                "Python export authority disagreed with valid writer evidence",
            )?;
            require(
                symbol_keys(extracted.graph(), &keys, "_private").is_empty(),
                "source export roles exposed a private Python declaration",
            )?;
            require(
                !symbol_keys(extracted.graph(), &keys, "safe").is_empty(),
                "unrelated Python export lost its callable keys",
            )?;
        }
        Ok(())
    }

    #[test]
    fn type_and_ordinary_value_exports_do_not_supply_callable_keys() -> Result<(), Box<dyn Error>> {
        let project = project_id(90)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for (language, extension, source) in [
            (
                "typescript",
                "ts",
                "export type Alias = () => void; export interface Shape {} export class Runtime {} export enum Values { One } export const scalar = 1; export function run() {}",
            ),
            (
                "tsx",
                "tsx",
                "export type Alias = () => void; export interface Shape {} export class Runtime {} export enum Values { One } export const scalar = 1; export function run() {}",
            ),
            (
                "rust",
                "rs",
                "pub type Alias = fn(); pub trait Shape {} pub struct Runtime(); pub enum Values { One } pub const scalar: i32 = 1; pub fn run() {}",
            ),
            (
                "rust-build-script",
                "rs",
                "pub type Alias = fn(); pub trait Shape {} pub struct Runtime(); pub enum Values { One } pub const scalar: i32 = 1; pub fn run() {}",
            ),
        ] {
            let target = crate::extract_symbol_graph_with_semantics_controlled(
                &format!("src/worker.{extension}"),
                Some(language),
                source,
                &control,
            )?;
            let keys = derive_resolution_keys_with_context(
                project,
                None,
                target.graph(),
                ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
            )?;
            for name in ["Alias", "Shape", "scalar"] {
                require(
                    symbol_keys(target.graph(), &keys, name).is_empty(),
                    "type-only declaration supplied call keys",
                )?;
            }
            require(
                !symbol_keys(target.graph(), &keys, "run").is_empty(),
                "function positive lost callable keys",
            )?;
            require(
                !symbol_keys(target.graph(), &keys, "Runtime").is_empty(),
                "runtime constructor positive lost callable keys",
            )?;
            require(
                !symbol_keys(target.graph(), &keys, "Values").is_empty(),
                "runtime enum export compatibility lost keys",
            )?;
        }
        Ok(())
    }

    #[test]
    fn type_only_export_names_do_not_suppress_runtime_callable_exports()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(89)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/caller.ts",
            Some("typescript"),
            "import { named } from './target'; function caller() { named(); }",
            &control,
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        let target = crate::extract_symbol_graph_with_semantics_controlled(
            "src/target.ts",
            Some("typescript"),
            "type Item = unknown; function run() {} export type {Item as named}; export {type Item as typed, run as named};",
            &control,
        )?;
        let target_keys = derive_resolution_keys_with_context(
            project,
            None,
            target.graph(),
            ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
        )?;
        require(
            keys_intersect(
                relation_keys(caller.graph(), &caller_keys, "named"),
                symbol_keys(target.graph(), &target_keys, "run"),
            ),
            "type-only slot suppressed runtime export",
        )?;
        require(
            symbol_keys(target.graph(), &target_keys, "Item").is_empty(),
            "type-only export became callable",
        )?;
        Ok(())
    }

    #[test]
    fn source_default_slots_target_named_declarations_without_named_export_guessing()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([81; 16])?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for (language, extension) in [("javascript", "js"), ("typescript", "ts"), ("tsx", "tsx")] {
            let caller = crate::extract_symbol_graph_with_semantics_controlled(
                &format!("src/caller.{extension}"),
                Some(language),
                "import Entry, { secondary as named } from './target'; function caller() { Entry(); named(); }",
                &control,
            )?;
            let caller_keys = derive_resolution_keys_with_context(
                project,
                None,
                caller.graph(),
                ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
            )?;
            let leaked = crate::extract_symbol_graph_with_semantics_controlled(
                &format!("src/leaked.{extension}"),
                Some(language),
                "import { Actual as leaked } from './target'; function caller() { leaked(); }",
                &control,
            )?;
            let leaked_keys = derive_resolution_keys_with_context(
                project,
                None,
                leaked.graph(),
                ResolutionProjectionContext::default().with_source_semantics(leaked.semantics()),
            )?;
            for declaration in ["function Actual() {}", "class Actual {}"] {
                let target = crate::extract_symbol_graph_with_semantics_controlled(
                    &format!("src/target.{extension}"),
                    Some(language),
                    &format!("export default {declaration}\nexport function secondary() {{}}"),
                    &control,
                )?;
                let target_keys = derive_resolution_keys_with_context(
                    project,
                    None,
                    target.graph(),
                    ResolutionProjectionContext::default()
                        .with_source_semantics(target.semantics()),
                )?;
                let actual = symbol_keys(target.graph(), &target_keys, "Actual");
                require(
                    keys_intersect(relation_keys(caller.graph(), &caller_keys, "Entry"), actual),
                    "default slot did not identify actual declaration",
                )?;
                require(
                    !keys_intersect(
                        relation_keys(leaked.graph(), &leaked_keys, "leaked"),
                        actual,
                    ),
                    "default declaration fabricated a named export",
                )?;
                require(
                    keys_intersect(
                        relation_keys(caller.graph(), &caller_keys, "named"),
                        symbol_keys(target.graph(), &target_keys, "secondary"),
                    ),
                    "default slot lost named sibling",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn default_export_roles_survive_hosts_and_never_narrow_coalesced_declarations()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([82; 16])?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/caller.js",
            Some("javascript"),
            "import Entry, { Actual as leaked } from './target'; function caller() { Entry(); leaked(); }",
            &control,
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        for (language, attributes, admits_default) in [
            ("vue", "", true),
            ("vue", "setup", false),
            ("svelte", "module", false),
            ("svelte", "", false),
            ("html", "type=module", true),
            ("html", "", false),
        ] {
            for (source, valid) in [
                ("export default function Actual() {}", true),
                (
                    "export default function Actual() {} export default function Actual() {}",
                    false,
                ),
            ] {
                let target = crate::extract_symbol_graph_with_semantics_controlled(
                    &format!("src/target.{language}"),
                    Some(language),
                    &format!("<script {attributes}>{source}</script>"),
                    &control,
                )?;
                let keys = derive_resolution_keys_with_context(
                    project,
                    None,
                    target.graph(),
                    ResolutionProjectionContext::default()
                        .with_source_semantics(target.semantics()),
                )?;
                let actual = symbol_keys(target.graph(), &keys, "Actual");
                require(
                    keys_intersect(relation_keys(caller.graph(), &caller_keys, "Entry"), actual)
                        == (valid && admits_default),
                    "host default target correspondence changed",
                )?;
                require(
                    !keys_intersect(
                        relation_keys(caller.graph(), &caller_keys, "leaked"),
                        actual,
                    ),
                    "host coalescing fabricated the default declaration as a named export",
                )?;
                require(
                    !keys.source_keys().is_empty(),
                    "default ambiguity discarded admitted module evidence",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn component_export_targets_require_unambiguous_script_roles() -> Result<(), Box<dyn Error>> {
        let project = project_id(90)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "caller.js",
            Some("javascript"),
            "import {run} from './target'; function caller(){run();}",
            &control,
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        let verify = |language: &str, source: &str, valid: bool| -> Result<(), Box<dyn Error>> {
            let target = crate::extract_symbol_graph_with_semantics_controlled(
                &format!("target.{language}"),
                Some(language),
                source,
                &control,
            )?;
            let keys = derive_resolution_keys_with_context(
                project,
                None,
                target.graph(),
                ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
            )?;
            require(
                keys_intersect(
                    relation_keys(caller.graph(), &caller_keys, "run"),
                    symbol_keys(target.graph(), &keys, "run"),
                ) == valid,
                "host role changed its exported target authority",
            )
        };
        for (language, attributes, valid) in [
            ("svelte", "context=instance context=module", false),
            ("svelte", "context=module context=module", false),
            ("svelte", "module module", false),
            ("svelte", "module context=instance", false),
            ("svelte", "module='false'", false),
            ("vue", "setup setup", false),
            ("vue", "", true),
            ("svelte", "module", true),
            ("svelte", "context=module", true),
            ("svelte", "module context=module", true),
        ] {
            verify(
                language,
                &format!("<script {attributes}>export function run(){{}}</script>"),
                valid,
            )?;
        }
        for (language, source) in [
            (
                "vue",
                "<script>export function run(){}</script><script>function other(){}</script>",
            ),
            (
                "svelte",
                "<script module>export function run(){}</script><script module>function other(){}</script>",
            ),
        ] {
            verify(language, source, false)?;
        }

        Ok(())
    }

    #[test]
    fn default_expressions_and_anonymous_exports_do_not_select_a_named_sibling()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([83; 16])?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/caller.js",
            Some("javascript"),
            "import Entry from './target'; function caller() { Entry(); }",
            &control,
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            None,
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        for source in [
            "export function Only() {} export default Only;",
            "export function Only() {} export default function() {}",
            "export function Only() {} export default { run() {} };",
        ] {
            let target = crate::extract_symbol_graph_with_semantics_controlled(
                "src/target.js",
                Some("javascript"),
                source,
                &control,
            )?;
            let keys = derive_resolution_keys_with_context(
                project,
                None,
                target.graph(),
                ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
            )?;
            for symbol in keys.symbol_keys() {
                require(
                    !keys_intersect(
                        relation_keys(caller.graph(), &caller_keys, "Entry"),
                        symbol.keys(),
                    ),
                    "unsupported default expression guessed an unrelated declaration",
                )?;
            }
            require(
                !keys.source_keys().is_empty(),
                "unsupported default lost module dependency evidence",
            )?;
        }
        Ok(())
    }

    #[test]
    fn semantic_resolution_contract_digest_is_bounded_and_deterministic() {
        const PRE_ARROW_CONTEXT_DIGEST: &str =
            "66cab1855f1689308cdc5bb858a63e8d3ed42e825b073d62cada7347383731a7";
        const PRE_SOURCE_CONTEXT_DIGEST: &str =
            "784435da80445735c78b112bebab6352f74c660d550399a9617dae87d2df0424";
        const PRE_GENERATOR_DECLARATION_DIGEST: &str =
            "4542cd9a22a1df432d3a3d212f2ee5b6c52067411c3f54ca77221566a8f2ab7d";
        const PRE_MODULE_CALLBACK_DIGEST: &str =
            "487625adf2f9ec76f98034d4ef5667e707960b6b8afd280b213021cb64a0f10f";
        let first = semantic_resolution_contract_digest();
        assert_eq!(SEMANTIC_RESOLUTION_CONTRACT_VERSION, 17);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(first, semantic_resolution_contract_digest());
        assert_ne!(first, PRE_MODULE_CALLBACK_DIGEST);
        assert_ne!(first, PRE_GENERATOR_DECLARATION_DIGEST);
        assert_ne!(first, PRE_SOURCE_CONTEXT_DIGEST);
        assert_ne!(first, PRE_ARROW_CONTEXT_DIGEST);
    }

    #[test]
    fn semantic_resolution_contract_tracks_identifier_data_version() {
        let before = super::semantic_resolution_contract_digest_for_identifier_profile((17, 0, 0));
        let after = super::semantic_resolution_contract_digest_for_identifier_profile((18, 0, 0));
        assert_ne!(before, after);
        assert_eq!(
            semantic_resolution_contract_digest(),
            super::semantic_resolution_contract_digest_for_identifier_profile(
                unicode_ident::UNICODE_VERSION
            )
        );
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
            "import defaultValue, { read } unexpected from './reader';",
            "import { read, nested { broken } } from './reader';",
            "import { read as load as fetch } from './reader';",
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
    fn normalizes_flat_parenthesized_python_imports_without_guessing() {
        let expected = vec![
            reference(
                ImportSyntax::Python,
                "package.reader",
                Some("close"),
                "close",
            ),
            reference(ImportSyntax::Python, "package.reader", Some("read"), "load"),
        ];
        for statement in [
            "from package.reader import (read as load, close)",
            "from package.reader import (read as load, close,)",
            "from package.reader import (\n    read as load,\n    close,\n)",
        ] {
            assert_eq!(parse_import_references(statement), expected, "{statement}");
        }
        assert_eq!(
            parse_import_references("from .reader import (read,)"),
            vec![reference(
                ImportSyntax::Python,
                ".reader",
                Some("read"),
                "read"
            )]
        );
        assert_eq!(
            parse_import_references("from match.case import (type as match, case)"),
            vec![
                reference(ImportSyntax::Python, "match.case", Some("case"), "case"),
                reference(ImportSyntax::Python, "match.case", Some("type"), "match"),
            ]
        );
        for statement in [
            "from package.reader import ()",
            "from package.reader import (read",
            "from package.reader import read)",
            "from package.reader import ((read))",
            "from package.reader import (read),",
            "from package.reader import (read,,close)",
            "from package.reader import (,read)",
            "from package.reader import (read,,)",
            "from package.reader import (read as)",
            "from package.reader import (read as load as other)",
            "from package.reader import (read.attr)",
            "from package.reader import (read as load.attr)",
            "from package.reader import (class)",
            "from package.reader import (read as None)",
            "from package.class import (read)",
            "from package.reader import (read())",
            "from package.reader import ([read])",
            "from package.reader import ({read})",
            "from package.reader import (*)",
            "from package.reader import (read, # note\n close)",
        ] {
            assert!(parse_import_references(statement).is_empty(), "{statement}");
        }
    }

    #[test]
    fn preserves_unicode_python_import_names_and_aliases() {
        let name = "a\u{301}";
        let alias = "r\u{301}";
        let module = "helper\u{301}.reader";
        assert_eq!(
            parse_import_references(&format!("from {module} import (run)")),
            vec![reference(ImportSyntax::Python, module, Some("run"), "run")]
        );
        for list in [
            format!("{name}, run"),
            format!("({name}, run,)"),
            format!("(\n{name},\nrun,\n)"),
        ] {
            assert_eq!(
                parse_import_references(&format!("from helper import {list}")),
                vec![
                    reference(ImportSyntax::Python, "helper", Some(name), name),
                    reference(ImportSyntax::Python, "helper", Some("run"), "run"),
                ]
            );
        }
        for list in [
            format!("run as {alias}, {name}"),
            format!("(run as {alias}, {name},)"),
        ] {
            assert_eq!(
                parse_import_references(&format!("from helper import {list}")),
                vec![
                    reference(ImportSyntax::Python, "helper", Some(name), name),
                    reference(ImportSyntax::Python, "helper", Some("run"), alias),
                ]
            );
        }
        for invalid in ["\u{301}a", "a\u{2022}", "a\u{b2}", "a\u{200c}", "a\u{200d}"] {
            assert!(
                parse_import_references(&format!("from helper import ({invalid}, run)")).is_empty()
            );
            assert!(
                parse_import_references(&format!("from helper import (run as {invalid})"))
                    .is_empty()
            );
            assert!(
                parse_import_references(&format!("from {invalid}.reader import run")).is_empty()
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
    fn explicit_ecmascript_suffixes_do_not_merge_module_or_callable_targets()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(94)?;
        let configured = ConfiguredModuleResolution::new(vec![EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            Some("src".to_string()),
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/*".to_string()],
            )?],
        )?])?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let targets = [
            ("js", "javascript", "export function run(){}"),
            ("jsx", "javascript", "export function run(){}"),
            ("ts", "typescript", "export function run(){}"),
            ("tsx", "tsx", "export function run(){}"),
            ("d.ts", "typescript", "export declare function run(): void;"),
            ("mjs", "javascript", "export function run(){}"),
            ("cjs", "javascript", "function run(){}"),
            ("vue", "vue", "<script>export function run(){}</script>"),
            (
                "svelte",
                "svelte",
                "<script module>export function run(){}</script>",
            ),
            (
                "html",
                "html",
                "<script type='module'>export function run(){}</script>",
            ),
            (
                "htm",
                "html",
                "<script type='module'>export function run(){}</script>",
            ),
            ("js.ts", "typescript", "export function run(){}"),
            ("json.ts", "typescript", "export function run(){}"),
            ("widget", "typescript", "export function run(){}"),
        ];
        for (specifier, allowed) in [
            ("target.svelte", &["svelte"][..]),
            ("target.html", &["html"][..]),
            ("target.htm", &["htm"][..]),
            ("target.vue", &["vue"][..]),
            ("target.mjs", &["mjs"][..]),
            ("target.cjs", &["cjs"][..]),
            ("target.ts", &["ts"][..]),
            ("target.tsx", &["tsx"][..]),
            ("target.d.ts", &["d.ts"][..]),
            ("target.js", &["js", "jsx", "ts", "tsx", "d.ts"][..]),
            ("target.jsx", &["jsx", "tsx", "d.ts"][..]),
            ("target.json", &[][..]),
            ("target.widget", &["widget"][..]),
            ("target/index.js", &[][..]),
            ("../target", &[][..]),
        ] {
            for prefix in ["./", "@/", ""] {
                let source = format!(
                    "import {{run}} from '{prefix}{specifier}'; function caller(){{run();}}"
                );
                let caller = crate::extract_symbol_graph_with_semantics_controlled(
                    "src/caller.ts",
                    Some("typescript"),
                    &source,
                    &control,
                )?;
                let caller_keys = derive_resolution_keys_with_context(
                    project,
                    Some("app"),
                    caller.graph(),
                    ResolutionProjectionContext::with_configured_modules(&configured)
                        .with_source_semantics(caller.semantics()),
                )?;
                let dependencies =
                    relation_keys_of_kind(caller.graph(), &caller_keys, RelationKind::Imports);
                for (suffix, language, source) in targets {
                    let path = format!("src/target.{suffix}");
                    let target = crate::extract_symbol_graph_with_semantics_controlled(
                        &path,
                        Some(language),
                        source,
                        &control,
                    )?;
                    let target_keys = derive_resolution_keys_with_context(
                        project,
                        Some("app"),
                        target.graph(),
                        ResolutionProjectionContext::default()
                            .with_source_semantics(target.semantics()),
                    )?;
                    let expected = allowed.contains(&suffix);
                    require(
                        dependencies
                            .iter()
                            .any(|keys| keys_intersect(keys, target_keys.source_keys()))
                            == expected,
                        &format!("module: {prefix}{specifier} -> {path}"),
                    )?;
                    let calls = relation_keys(caller.graph(), &caller_keys, "run");
                    let exports = symbol_keys(target.graph(), &target_keys, "run");
                    require(
                        keys_intersect(calls, exports)
                            == (expected && !matches!(suffix, "cjs" | "d.ts")),
                        &format!("call: {prefix}{specifier} -> {path}"),
                    )?;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn ecmascript_module_paths_preserve_entry_aliases_and_literal_file_names()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(95)?;
        for (caller_path, module, target_path, expected) in [
            ("src/main.ts", "./tools", "src/tools/index.ts", true),
            (
                "src/main.ts",
                "./tools/index.ts",
                "src/tools/index.ts",
                true,
            ),
            ("src/main.ts", "./tools.js", "src/tools/index.ts", false),
            (
                "src/foo.bar/main.ts",
                "./tools",
                "src/foo.bar/tools.ts",
                true,
            ),
            ("src/main.ts", "./tools", "packages/other/tools.ts", false),
            ("main.ts", "./target  name.js", "target  name.js", true),
            ("main.ts", "./target::name.js", "target::name.js", true),
        ] {
            let caller = extract_symbol_graph(
                caller_path,
                Some("typescript"),
                &format!("import {{run}} from '{module}'; function caller(){{run();}}"),
            );
            let target =
                extract_symbol_graph(target_path, Some("typescript"), "export function run(){}");
            let caller_keys = derive_resolution_keys(project, None, &caller)?;
            let target_keys = derive_resolution_keys(project, None, &target)?;
            require(
                relation_keys_of_kind(&caller, &caller_keys, RelationKind::Imports)
                    .iter()
                    .any(|keys| keys_intersect(keys, target_keys.source_keys()))
                    == expected,
                &format!("module: {caller_path}: {module} -> {target_path}"),
            )?;
            require(
                keys_intersect(
                    relation_keys(&caller, &caller_keys, "run"),
                    symbol_keys(&target, &target_keys, "run"),
                ) == expected,
                &format!("call: {caller_path}: {module} -> {target_path}"),
            )?;
        }
        Ok(())
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
        let rust_module_target = extract_symbol_graph(
            "src/config.rs",
            Some("rust"),
            "pub fn load_timeout_millis() -> u64 { 250 }\n",
        );
        let rust_module_caller = extract_symbol_graph(
            "src/handler.rs",
            Some("rust"),
            "use crate::config;\npub fn health_response() { config::load_timeout_millis(); }\n",
        );
        assert_alias_resolution(
            project,
            Some("atlas"),
            &rust_module_target,
            "load_timeout_millis",
            &rust_module_caller,
            &["config::load_timeout_millis"],
        )?;
        let rust_callback_caller = extract_symbol_graph(
            "src/router.rs",
            Some("rust"),
            "use crate::handler;\nfn dispatch(path: &str) -> Option<()> { (path == \"/health\").then(handler::health_response) }\n",
        );
        assert_alias_resolution(
            project,
            Some("atlas"),
            &rust_module_caller,
            "health_response",
            &rust_callback_caller,
            &["handler::health_response"],
        )?;
        let user_defined_then = extract_symbol_graph(
            "src/scheduler.rs",
            Some("rust"),
            "use crate::handler;\nstruct Scheduler;\nimpl Scheduler { fn then(self, callback: fn()) { let _ = callback; } }\nfn schedule(scheduler: Scheduler) { scheduler.then(handler::health_response); }\n",
        );
        if user_defined_then.relations.iter().any(|relation| {
            relation.kind == RelationKind::Calls
                && relation.target_name == "handler::health_response"
        }) {
            return Err(io::Error::other(
                "a user-defined then method fabricated a callback call edge",
            )
            .into());
        }

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
    fn configured_aliases_share_file_and_declaration_keys_across_ecmascript_hosts()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(21)?;
        let configured = ConfiguredModuleResolution::new(vec![
            EcmaScriptModuleConfig::new(
                "tsconfig.json",
                EcmaScriptConfigKind::TypeScript,
                Some("src".to_string()),
                vec![
                    EcmaScriptPathMapping::new("@/*", vec!["src/*".to_string()])?,
                    EcmaScriptPathMapping::new("@index/*", vec!["src/*/index.ts".to_string()])?,
                ],
            )?,
            EcmaScriptModuleConfig::new(
                "jsconfig.json",
                EcmaScriptConfigKind::JavaScript,
                Some("src".to_string()),
                vec![EcmaScriptPathMapping::new(
                    "@/*",
                    vec!["src/*".to_string()],
                )?],
            )?,
        ])?;
        let context = ResolutionProjectionContext::with_configured_modules(&configured);

        for (target_path, language, target_source, caller_path, caller_source) in [
            (
                "src/controller.ts",
                "typescript",
                "export function useController(): string { return 'ok'; }\n",
                "src/page.ts",
                "import { useController } from '@/controller';\nexport const value = useController();\n",
            ),
            (
                "src/controller.ts",
                "typescript",
                "export function useController(): string { return 'ok'; }\n",
                "src/page.tsx",
                "import { useController } from '@/controller';\nexport function Page() { useController(); return <div />; }\n",
            ),
            (
                "src/controller.js",
                "javascript",
                "export function useController() { return 'ok'; }\n",
                "src/page.jsx",
                "import { useController } from '@/controller';\nexport function Page() { useController(); return <div />; }\n",
            ),
            (
                "src/controller.ts",
                "typescript",
                "export function useController(): string { return 'ok'; }\n",
                "src/Page.vue",
                "<script setup lang=\"ts\">\nimport { useController } from '@/controller';\nconst value = useController();\n</script>\n<template><div>{{ value }}</div></template>\n",
            ),
        ] {
            let target = extract_symbol_graph(target_path, Some(language), target_source);
            let caller_extension = std::path::Path::new(caller_path)
                .extension()
                .and_then(std::ffi::OsStr::to_str);
            let caller_language = match caller_extension {
                Some(extension) if extension.eq_ignore_ascii_case("vue") => "vue",
                Some(extension)
                    if extension.eq_ignore_ascii_case("js")
                        || extension.eq_ignore_ascii_case("jsx") =>
                {
                    "javascript"
                }
                Some(extension) if extension.eq_ignore_ascii_case("tsx") => "tsx",
                _ => "typescript",
            };
            let caller = extract_symbol_graph(caller_path, Some(caller_language), caller_source);
            let target_projection = derive_resolution_keys(project, Some("shared"), &target)?;
            let caller_projection =
                derive_resolution_keys_with_context(project, Some("app"), &caller, context)?;
            let imports = relation_keys_of_kind(&caller, &caller_projection, RelationKind::Imports);
            require(
                imports
                    .iter()
                    .any(|keys| keys_intersect(keys, target_projection.source_keys())),
                "configured import did not share the target file key",
            )?;
            require(
                keys_intersect(
                    relation_keys(&caller, &caller_projection, "useController"),
                    symbol_keys(&target, &target_projection, "useController"),
                ),
                "configured call did not share the target declaration key",
            )?;
        }

        let index_target = extract_symbol_graph(
            "src/tools/index.ts",
            Some("typescript"),
            "export function useTool() { return 'ok'; }\n",
        );
        let index_caller = extract_symbol_graph(
            "src/index-page.ts",
            Some("typescript"),
            "import { useTool } from '@index/tools';\nexport const value = useTool();\n",
        );
        let index_target_projection = derive_resolution_keys(project, Some("app"), &index_target)?;
        let index_caller_projection =
            derive_resolution_keys_with_context(project, Some("app"), &index_caller, context)?;
        require(
            relation_keys_of_kind(
                &index_caller,
                &index_caller_projection,
                RelationKind::Imports,
            )
            .iter()
            .any(|keys| keys_intersect(keys, index_target_projection.source_keys())),
            "configured index target did not reuse package-entry source keys",
        )?;
        require(
            keys_intersect(
                relation_keys(&index_caller, &index_caller_projection, "useTool"),
                symbol_keys(&index_target, &index_target_projection, "useTool"),
            ),
            "configured index target did not reuse declaration keys",
        )?;

        let extension_caller = extract_symbol_graph(
            "src/extension-page.ts",
            Some("typescript"),
            "import { useController } from '@/controller.ts';\nexport const value = useController();\n",
        );
        let extension_target = extract_symbol_graph(
            "src/controller.ts",
            Some("typescript"),
            "export function useController(): string { return 'ok'; }\n",
        );
        let extension_caller_projection =
            derive_resolution_keys_with_context(project, Some("app"), &extension_caller, context)?;
        let extension_target_projection =
            derive_resolution_keys(project, Some("app"), &extension_target)?;
        require(
            relation_keys_of_kind(
                &extension_caller,
                &extension_caller_projection,
                RelationKind::Imports,
            )
            .iter()
            .any(|keys| keys_intersect(keys, extension_target_projection.source_keys())),
            "configured extension target did not reuse extensionless source keys",
        )?;

        let root_configured = ConfiguredModuleResolution::new(vec![EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            Some(String::new()),
            Vec::new(),
        )?])?;
        let root_context = ResolutionProjectionContext::with_configured_modules(&root_configured);
        let root_target = extract_symbol_graph(
            "controller.ts",
            Some("typescript"),
            "export function useController() { return 'root'; }\n",
        );
        let nested_same_name = extract_symbol_graph(
            "packages/app/src/controller.ts",
            Some("typescript"),
            "export function useController() { return 'nested'; }\n",
        );
        let root_caller = extract_symbol_graph(
            "page.ts",
            Some("typescript"),
            "import { useController } from 'controller';\nexport const value = useController();\n",
        );
        let root_target_projection = derive_resolution_keys(project, Some("shared"), &root_target)?;
        let nested_projection = derive_resolution_keys(project, Some("nested"), &nested_same_name)?;
        let root_caller_projection =
            derive_resolution_keys_with_context(project, Some("app"), &root_caller, root_context)?;
        let configured_imports =
            relation_keys_of_kind(&root_caller, &root_caller_projection, RelationKind::Imports);
        require(
            configured_imports
                .iter()
                .any(|keys| keys_intersect(keys, root_target_projection.source_keys())),
            "root baseUrl import did not resolve its repository-exact target",
        )?;
        require(
            configured_imports
                .iter()
                .all(|keys| !keys_intersect(keys, nested_projection.source_keys())),
            "root baseUrl import also matched a nested basename alias",
        )?;
        let configured_call = relation_keys(&root_caller, &root_caller_projection, "useController");
        require(
            keys_intersect(
                configured_call,
                symbol_keys(&root_target, &root_target_projection, "useController"),
            ),
            "root baseUrl call did not resolve its repository-exact declaration",
        )?;
        require(
            !keys_intersect(
                configured_call,
                symbol_keys(&nested_same_name, &nested_projection, "useController"),
            ),
            "root baseUrl call also matched a nested basename alias",
        )?;
        Ok(())
    }

    #[test]
    fn configured_import_scopes_are_expanded_once_per_source_graph() -> Result<(), Box<dyn Error>> {
        let configured = ConfiguredModuleResolution::new(vec![EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            Some("src".to_string()),
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/*".to_string()],
            )?],
        )?])?;
        let graph = extract_symbol_graph(
            "src/page.ts",
            Some("typescript"),
            "import { useController } from '@/controller';\n\
             export const first = useController();\n\
             export const second = useController();\n",
        );
        let parsed_imports = graph
            .relations
            .iter()
            .map(|relation| {
                if relation.kind == RelationKind::Imports {
                    crate::semantic::parse_imports(
                        SemanticProviderOwner::EcmaScript,
                        &relation.target_name,
                    )
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>();
        let configured_source = configured.for_source(&graph.path);
        let cache = build_import_scope_cache(
            SemanticProviderOwner::EcmaScript,
            &graph,
            &parsed_imports,
            Some(&configured_source),
        );
        let modules = cache
            .get("src/page.ts")
            .ok_or_else(|| io::Error::other("source graph omitted its import-scope cache"))?;
        if modules.len() != 1
            || modules.get("@/controller") != Some(&vec!["module:src/controller".into()])
        {
            return Err(io::Error::other(
                "repeated imported calls did not reuse one configured scope expansion",
            )
            .into());
        }
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
                "use crate::worker::{run, broken as };\nfn main() { run(); }\n",
            ),
            (
                "src/main.ts",
                "typescript",
                "import defaultValue, { read } unexpected from './reader';\ndefaultValue();\n",
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
    fn syntax_trivia_and_literals_keep_dependency_and_aliased_call_keys()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(15)?;
        for (language, target_path, target_source, caller_path, caller_source) in [
            (
                "rust",
                "src/worker.rs",
                "pub fn run() {}\npub fn stop() {}\n",
                "src/main.rs",
                "use crate::worker::{run as launch, /* separator */ stop};\nfn caller() { launch(); }\n",
            ),
            (
                "typescript",
                "src/worker.ts",
                "export function run() {}\nexport function stop() {}\n",
                "src/main.ts",
                "import { run as launch, /* separator */ stop } from './worker';\nexport function caller() { launch(); }\n",
            ),
            (
                "python",
                "package/worker.py",
                "def run():\n    pass\ndef stop():\n    pass\n",
                "main.py",
                "from package.worker import (run as launch, # separator\n stop)\ndef caller():\n    launch()\n",
            ),
            (
                "javascript",
                "src/dep  space.js",
                "export function run() {}\n",
                "src/main.js",
                "import { run as launch } from './dep  space.js';\nexport function caller() { launch(); }\n",
            ),
            (
                "javascript",
                "src/odd'name.js",
                "export function run() {}\n",
                "src/main.js",
                "import { run as launch } from './odd\\'name.js';\nexport function caller() { launch(); }\n",
            ),
            (
                "javascript",
                "src/🚀.js",
                "export function run() {}\n",
                "src/main.js",
                "import { run as launch } from './\\uD83D\\uDE80.js';\nexport function caller() { launch(); }\n",
            ),
            (
                "python",
                "package/worker.py",
                "def run():\n    pass\n",
                "main.py",
                "from package.worker import run \\\n    as launch\ndef caller():\n    launch()\n",
            ),
        ] {
            let target = extract_symbol_graph(target_path, Some(language), target_source);
            let caller = extract_symbol_graph(caller_path, Some(language), caller_source);
            let projection = derive_resolution_keys(project, Some("app"), &caller)?;
            require(
                relation_keys_of_kind(&caller, &projection, RelationKind::Imports)
                    .iter()
                    .any(|keys| !keys.is_empty()),
                "syntax-preserving import lost dependency keys",
            )?;
            assert_alias_resolution(project, Some("app"), &target, "run", &caller, &["launch"])?;
        }
        Ok(())
    }

    #[test]
    fn rust_namespace_aliases_preserve_nested_member_ownership() -> Result<(), Box<dyn Error>> {
        let project = project_id(86)?;
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let target = crate::extract_symbol_graph_with_semantics_controlled(
            "src/fake.rs",
            Some("rust"),
            "pub fn read(_: &str) {}\npub mod fs { pub fn read(_: &str) {} }",
            &control,
        )?;
        let caller = crate::extract_symbol_graph_with_semantics_controlled(
            "src/main.rs",
            Some("rust"),
            "use crate::fake as std; fn root_call(std: u8) { std::read(\"x\"); } fn nested_call() { std::fs::read(\"x\"); }",
            &control,
        )?;
        let target_keys = derive_resolution_keys_with_context(
            project,
            Some("app"),
            target.graph(),
            ResolutionProjectionContext::default().with_source_semantics(target.semantics()),
        )?;
        let caller_keys = derive_resolution_keys_with_context(
            project,
            Some("app"),
            caller.graph(),
            ResolutionProjectionContext::default().with_source_semantics(caller.semantics()),
        )?;
        for (reference, parent) in [("std::read", None), ("std::fs::read", Some("fs"))] {
            let requests = relation_keys(caller.graph(), &caller_keys, reference);
            let matches = target_keys
                .symbol_keys()
                .iter()
                .filter(|keys| keys_intersect(requests, keys.keys()))
                .map(|keys| &target.graph().symbols[keys.symbol_index()])
                .collect::<Vec<_>>();
            require(
                matches.len() == 1,
                "qualified member did not resolve uniquely",
            )?;
            require(matches[0].name == "read", "qualified member lost leaf name")?;
            require(
                matches[0].parent.as_deref() == parent,
                "qualified member escaped owning namespace",
            )?;
        }
        Ok(())
    }

    #[test]
    fn nested_rust_imports_keep_distinct_dependencies_and_alias_callers()
    -> Result<(), Box<dyn Error>> {
        let project = project_id(15)?;
        let worker = extract_symbol_graph("src/worker.rs", Some("rust"), "pub fn run() {}\n");
        let nested =
            extract_symbol_graph("src/worker/nested.rs", Some("rust"), "pub fn tail() {}\n");
        let caller = extract_symbol_graph(
            "src/main.rs",
            Some("rust"),
            "use crate::worker::{run as launch, nested::{self as tools, tail as last}, *};\nfn caller() { launch(); last(); tools::tail(); }\n",
        );
        let projection = derive_resolution_keys(project, Some("app"), &caller)?;
        let worker_projection = derive_resolution_keys(project, Some("app"), &worker)?;
        let nested_projection = derive_resolution_keys(project, Some("app"), &nested)?;
        let imports = relation_keys_of_kind(&caller, &projection, RelationKind::Imports);
        for expected in [
            worker_projection.source_keys(),
            nested_projection.source_keys(),
        ] {
            require(
                imports.iter().any(|keys| keys_intersect(keys, expected)),
                "nested use lost a distinct module dependency",
            )?;
        }
        assert_alias_resolution(project, Some("app"), &worker, "run", &caller, &["launch"])?;
        assert_alias_resolution(
            project,
            Some("app"),
            &nested,
            "tail",
            &caller,
            &["last", "tools::tail"],
        )?;
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
        let identity =
            projectatlas_core::graph::GraphIdentityText::new("module:src/shared/reader")?;
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
    fn contract_failure_retains_valid_resolution_projection_siblings() -> Result<(), Box<dyn Error>>
    {
        let path = format!("pkg/{0}/{0}/{0}/{0}/page.rs", "d".repeat(100));
        let imported = "i".repeat(3_800);
        let mut graph = extract_symbol_graph(
            &path,
            Some("rust"),
            "pub fn caller() { helper(); }\npub fn helper() {}\n",
        );
        graph
            .relations
            .retain(|relation| relation.kind == RelationKind::Calls);
        graph.relations.push(SymbolRelation {
            path,
            source_name: "<module>".to_string(),
            target_name: format!("use self::{{{imported}}};"),
            kind: RelationKind::Imports,
            line: 1,
            context: "partial projection fixture".to_string(),
            parser: ParserKind::TreeSitter,
        });
        graph.relations.push(SymbolRelation {
            path: graph.path.clone(),
            source_name: "caller".to_string(),
            target_name: format!("{imported}.member"),
            kind: RelationKind::Calls,
            line: 3,
            context: "partial projection fixture".to_string(),
            parser: ParserKind::TreeSitter,
        });

        let result = derive_resolution_keys(project_id(23)?, None, &graph);
        let Err(ResolutionProjectionError::Contract(failure)) = result else {
            return Err(io::Error::other(
                "invalid derived resolution key did not retain a typed contract failure",
            )
            .into());
        };
        require(
            !failure.projection().source_keys().is_empty(),
            "contract failure discarded valid source resolution keys",
        )?;
        require(
            !failure.projection().symbol_keys().is_empty(),
            "contract failure discarded valid symbol resolution keys",
        )?;
        require(
            failure
                .projection()
                .relation_keys()
                .iter()
                .any(|relation| relation.relation_index() == 0 && !relation.keys().is_empty()),
            "contract failure discarded a valid sibling relation key",
        )?;
        require(
            failure
                .failures()
                .iter()
                .all(|failure| !format!("{:?}", failure.error()).contains(&imported)),
            "contract failure retained rejected raw identity text",
        )?;
        Ok(())
    }

    #[test]
    fn contract_failure_deduplicates_repeated_facts_after_detail_ceiling()
    -> Result<(), Box<dyn Error>> {
        let mut graph = SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::with_capacity(super::MAX_RESOLUTION_PROJECTION_FAILURES + 1),
            relations: Vec::new(),
        };
        for index in 0..super::MAX_RESOLUTION_PROJECTION_FAILURES {
            graph.symbols.push(CodeSymbol {
                path: graph.path.clone(),
                language: graph.language.clone(),
                name: format!("invalid\0{index}"),
                kind: SymbolKind::Function,
                signature: "fn invalid()".to_string(),
                exported: true,
                documentation: None,
                line_start: index + 1,
                line_end: index + 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: None,
            });
        }
        graph.symbols.push(CodeSymbol {
            path: graph.path.clone(),
            language: graph.language.clone(),
            name: "valid_tail".to_string(),
            kind: SymbolKind::Function,
            signature: "fn valid_tail()".to_string(),
            exported: true,
            documentation: None,
            line_start: graph.symbols.len() + 1,
            line_end: graph.symbols.len() + 1,
            source_selector: None,
            parent: Some("\0invalid-parent".to_string()),
            parser: ParserKind::TreeSitter,
            detail: None,
        });

        let Err(ResolutionProjectionError::Contract(failure)) =
            derive_resolution_keys(project_id(24)?, None, &graph)
        else {
            return Err(
                io::Error::other("repeated post-ceiling failures were not reported").into(),
            );
        };
        require(
            failure.failures().len() == super::MAX_RESOLUTION_PROJECTION_FAILURES,
            "detail ceiling retained repeated fact attempts",
        )?;
        require(
            failure.rejected_count() == super::MAX_RESOLUTION_PROJECTION_FAILURES + 1,
            "repeated post-ceiling fact inflated rejected count",
        )?;
        Ok(())
    }

    #[test]
    fn supported_typescript_parser_retains_control_bearing_import_identities()
    -> Result<(), Box<dyn Error>> {
        let graph = extract_symbol_graph(
            "src/page.ts",
            Some("typescript"),
            "import * as first from './bad\0one';\nimport * as second from './bad\0two';\nexport function caller() { first(); second(); }\n",
        );
        require(
            graph.parser == ParserKind::TreeSitter,
            "supported TypeScript fixture did not use the semantic parser",
        )?;
        require(
            graph.relations.iter().any(|relation| {
                relation.kind == RelationKind::Imports && relation.target_name.contains('\0')
            }),
            "supported TypeScript parser did not retain the control-bearing import",
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

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
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
