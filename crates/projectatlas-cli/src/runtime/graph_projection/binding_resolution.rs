//! Bounded source-proved forwarding, resolved before canonical graph publication.

mod rust;

use super::{
    CliError, EntityProjection, MAX_INCREMENTAL_RESOLUTION_ITEMS, PackageIndex,
    ProjectResolutionRegistry, ResolutionCandidateKind, STAGED_GRAPH_ROW_BYTES, SourceBindingCache,
    SourceGraph, append_persisted_resolution_candidates, dependency_closure_limit,
    document_file_resolution_key, enforce_resolution_registry_budget_with_limit,
    entity_retained_bytes, invalid_graph_contract, registry_resolution_matches, relation_reference,
    resolution_key_limit_failure, resolution_retained_bytes, source_symbol_entity,
};
use projectatlas_core::graph::{
    CanonicalResolutionKey, EntitySelector, GraphEntity, GraphIdentityText, GraphLimits,
    ProjectInstanceId, RelationResolution,
};
use projectatlas_core::language::{SemanticProviderOwner, language_capability};
use projectatlas_core::symbols::{SymbolGraph, SymbolKind, SymbolRelation};
use projectatlas_core::{IndexGeneration, IndexWorkControl, IndexWorkStage};
use projectatlas_db::AtlasStore;
use projectatlas_symbols::{
    ConfiguredModuleResolution, ResolutionKeyProjection, ResolutionProjectionError,
    SourceCallBinding, SourceSemanticFacts, derive_resolution_keys_with_context,
};
use projectatlas_symbols::{ImportReference, SourceExportBinding, SourceExportNamespace};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Maximum explicit forwarding steps; longer chains remain unresolved.
const MAX_FORWARDING_DEPTH: usize = 64;

/// One immutable call outcome, shared by identical source binding queries.
#[derive(Default)]
pub(super) struct CallBindingResolution {
    /// Actual admitted declaration, never an exporting file marker.
    target: Option<GraphEntity>,
    /// Source-owned competing slots retain an honest bounded candidate count.
    ambiguous: Option<std::num::NonZeroU32>,
    /// Source proves this binding does not occupy the requested namespace.
    absent: bool,
    /// An explicit export slot was followed, including unsupported or cyclic slots.
    followed_export: bool,
    /// Invalidation causes are independent from the declaration target.
    dependencies: BTreeSet<CanonicalResolutionKey>,
}

impl CallBindingResolution {
    /// Construct a canonical outcome without reinterpreting lookup markers as targets.
    pub(super) fn resolution(
        &self,
        relation: &SymbolRelation,
    ) -> Result<RelationResolution, CliError> {
        if let Some(candidates) = self.ambiguous {
            return Ok(RelationResolution::Ambiguous {
                reference: GraphIdentityText::new(relation_reference(relation))
                    .map_err(invalid_graph_contract)?,
                candidates,
            });
        }
        self.target.as_ref().map_or_else(
            || {
                Ok(RelationResolution::Unresolved {
                    reference: GraphIdentityText::new(relation_reference(relation))
                        .map_err(invalid_graph_contract)?,
                })
            },
            |target| {
                if matches!(target.selector(), EntitySelector::External { .. }) {
                    RelationResolution::external(target).map_err(invalid_graph_contract)
                } else {
                    RelationResolution::resolved(target).map_err(invalid_graph_contract)
                }
            },
        )
    }

    /// Borrow an actual external entity that publication must stage with its relation.
    pub(super) fn external_target(&self) -> Option<&GraphEntity> {
        self.target
            .as_ref()
            .filter(|target| matches!(target.selector(), EntitySelector::External { .. }))
    }

    /// Borrow causes retained even when forwarding cannot establish a target.
    pub(super) fn dependencies(&self) -> &BTreeSet<CanonicalResolutionKey> {
        &self.dependencies
    }

    /// Account this uniquely cached outcome and its owned payloads.
    fn retained_bytes(&self) -> u64 {
        self.dependencies
            .iter()
            .fold(STAGED_GRAPH_ROW_BYTES, |bytes, key| {
                bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(key.canonical_identity().len() as u64)
            })
            .saturating_add(self.target.as_ref().map_or(0, entity_retained_bytes))
    }
}

/// Bytes temporarily reserved while constructing one retained resolver value.
#[derive(Default)]
struct ResolverScratch {
    /// Reservations released or transferred when the operation returns.
    retained_bytes: u64,
}

impl ResolverScratch {
    /// Refuse growth before retaining another owned entry.
    fn reserve(
        &mut self,
        shared: &mut BindingResolver<'_, '_, '_>,
        bytes: u64,
    ) -> Result<(), CliError> {
        shared.reserve_scratch(bytes)?;
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
        Ok(())
    }
}

/// A query-owned cause set whose unique key copies are reserved before insertion.
struct CauseWorkspace<'a> {
    /// Exact deduplicated invalidation keys being constructed.
    keys: BTreeSet<CanonicalResolutionKey>,
    /// Construction owner that transfers these bytes into the completed value.
    scratch: &'a mut ResolverScratch,
}

impl<'a> CauseWorkspace<'a> {
    /// Start without allocating or borrowing a cached dependency set.
    fn new(scratch: &'a mut ResolverScratch) -> Self {
        Self {
            keys: BTreeSet::new(),
            scratch,
        }
    }
}

/// Per-graph, per-occurrence forwarding outcomes.
pub(super) type CallBindings = BTreeMap<String, BTreeMap<usize, Arc<CallBindingResolution>>>;

/// Canonical module value identity, valid only within one binding resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ModuleId(usize);

/// Intern module values once per source allocation, retaining witnesses until drop.
#[derive(Default)]
struct ModuleInterner {
    /// The retained Arc prevents a remembered address from being reused.
    allocations: BTreeMap<(usize, usize), (Arc<str>, ModuleId)>,
    /// Standard value equality handles equal allocations and hash collisions exactly.
    values: HashMap<Arc<str>, ModuleId>,
}

impl ModuleInterner {
    /// Admit metadata before insertion; hot allocations never hash module bytes again.
    fn intern<E>(
        &mut self,
        module: Arc<str>,
        admit: impl FnOnce(u64) -> Result<(), E>,
    ) -> Result<ModuleId, E> {
        let allocation = (module.as_ptr() as usize, module.len());
        if let Some((_, id)) = self.allocations.get(&allocation) {
            return Ok(*id);
        }
        let existing = self.values.get(module.as_ref()).copied();
        let entries = if existing.is_some() { 1 } else { 2 };
        admit(STAGED_GRAPH_ROW_BYTES.saturating_mul(entries))?;
        let id = if let Some(id) = existing {
            id
        } else {
            let id = ModuleId(self.values.len());
            self.values.insert(Arc::clone(&module), id);
            id
        };
        self.allocations.insert(allocation, (module, id));
        Ok(id)
    }
}

/// A typed module/export/member request; canonical key witnesses remain opaque.
#[derive(Clone)]
struct BindingQuery {
    /// Source whose import syntax owns this module reference.
    source: String,
    /// Cooked module allocation retained and accounted by its source evidence.
    module: Arc<str>,
    /// Exact value identity from the same resolver used by both query/visited maps.
    module_id: ModuleId,
    /// Explicit imported slot, absent for a namespace binding.
    imported: Option<String>,
    /// Remaining statically named namespace member.
    member: Option<String>,
}

impl BindingQuery {
    /// Preserve binding roles and exact module values through one resolver's interner.
    fn new<E>(
        source: &str,
        reference: &ImportReference,
        member: Option<&str>,
        modules: &mut ModuleInterner,
        admit: impl FnOnce(u64) -> Result<(), E>,
    ) -> Result<Self, E> {
        let module = reference.shared_module();
        let module_id = modules.intern(Arc::clone(&module), admit)?;
        Ok(Self {
            source: source.to_string(),
            module,
            module_id,
            imported: reference.imported().map(ToString::to_string),
            member: member.map(ToString::to_string),
        })
    }

    /// Compare short fields and one canonical ID; text stays available for resolution.
    fn identity(&self) -> (&str, Option<&str>, Option<&str>, ModuleId) {
        (
            &self.source,
            self.imported.as_deref(),
            self.member.as_deref(),
            self.module_id,
        )
    }

    /// Count query-owned text and headers; source owners already charge shared module bytes.
    fn retained_bytes(&self) -> u64 {
        STAGED_GRAPH_ROW_BYTES
            .saturating_add(self.source.capacity() as u64)
            .saturating_add(self.imported.as_ref().map_or(0, String::capacity) as u64)
            .saturating_add(self.member.as_ref().map_or(0, String::capacity) as u64)
    }
}

impl PartialEq for BindingQuery {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for BindingQuery {}

impl PartialOrd for BindingQuery {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BindingQuery {
    fn cmp(&self, other: &Self) -> Ordering {
        self.identity().cmp(&other.identity())
    }
}

/// Existing staged projection or an extra context source's admitted key projection.
enum SourceProjection<'g> {
    /// Reuse already validated keys without cloning them.
    Staged(&'g ResolutionKeyProjection),
    /// Project an explicitly required unchanged context once.
    Captured(ResolutionKeyProjection),
}

impl SourceProjection<'_> {
    /// Borrow the common key projection.
    fn get(&self) -> &ResolutionKeyProjection {
        match self {
            Self::Staged(keys) => keys,
            Self::Captured(keys) => keys,
        }
    }
}

/// One exact context source and precomputed bounded lookup indexes.
struct BindingSource<'g> {
    /// Fresh borrowed evidence or the existing exact source cache's shared capture.
    source: SourceGraph<'g>,
    /// Actual source export ordinals grouped only in the runtime value namespace.
    slots: BTreeMap<String, Vec<usize>>,
    /// Static module dependencies indexed once rather than scanned per call.
    modules: BTreeMap<String, Vec<CanonicalResolutionKey>>,
    /// The same qualified parent selectors used by ordinary entity publication.
    parents: Vec<Option<GraphIdentityText>>,
}

/// Find an associated parser ordinal without assuming projection row ordinals are raw ordinals.
fn relation_keys(
    projection: &ResolutionKeyProjection,
    relation: usize,
) -> &[CanonicalResolutionKey] {
    projection
        .relation_keys()
        .binary_search_by_key(
            &relation,
            projectatlas_symbols::RelationResolutionKeys::relation_index,
        )
        .map_or(&[], |index| projection.relation_keys()[index].keys())
}

/// Preserve the ordinary declaration fast path, including already ambiguous targets.
fn needs_forwarding(
    keys: &[CanonicalResolutionKey],
    candidates: &ProjectResolutionRegistry,
) -> bool {
    keys.iter().any(|key| {
        candidates
            .forwarding_key_digests
            .contains(&key.digest_bytes())
    }) || !keys
        .iter()
        .any(|key| candidates.candidate_digests_by_key.contains_key(key))
}

/// One operation's source/query caches and existing repository resolution registry.
struct BindingResolver<'a, 'g, 'n> {
    /// Existing read authority; no source parsing occurs inside a writer transaction.
    store: &'a AtlasStore,
    /// Selected project and pending generation.
    project: ProjectInstanceId,
    /// Pending publication generation.
    generation: IndexGeneration,
    /// Exact graph entity generation available before this publication.
    base_generation: Option<IndexGeneration>,
    /// Current source graphs paired with their own parser evidence.
    graphs: BTreeMap<&'g str, (&'g SymbolGraph, Option<&'g SourceSemanticFacts>)>,
    /// Existing admitted entities and projection keys.
    entities: &'g EntityProjection,
    /// Existing package and configured-module owners.
    packages: &'a PackageIndex,
    /// One validated configuration snapshot shared with ordinary resolution.
    configured: &'a ConfiguredModuleResolution,
    /// Existing hash-checked, accounted source recovery owner.
    sources: &'a mut SourceBindingCache<'n>,
    /// Existing candidate lookup owner, extended only for explicitly requested keys.
    candidates: &'a mut ProjectResolutionRegistry,
    /// Paths replaced by the pending publication must not re-enter through old rows.
    replaced: &'a BTreeSet<String>,
    /// Complete candidate keys already fetched in an incremental operation.
    loaded: Option<BTreeSet<CanonicalResolutionKey>>,
    /// Captured source views, including borrowed fresh evidence.
    context: BTreeMap<String, Arc<BindingSource<'g>>>,
    /// Exact target hydration cached by stable identity, including absent owners.
    hydrated: BTreeMap<String, Option<GraphEntity>>,
    /// Canonical IDs and retained allocation witnesses for this resolver only.
    modules: ModuleInterner,
    /// Finished outcomes shared by repeated identical binding queries.
    queries: BTreeMap<BindingQuery, BTreeMap<usize, Arc<CallBindingResolution>>>,
    /// Conservative bytes owned by this resolver's additional indexes and outcomes.
    retained_bytes: u64,
    /// Existing staging ceiling, injectable per owner for deterministic boundary tests.
    retained_limit: u64,
    /// Shared indexing deadline, cancellation and work budget.
    control: &'a IndexWorkControl,
}

/// Resolve only calls whose ordinary exact declaration lookup lacks complete proof.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_source_bindings<'g>(
    store: &AtlasStore,
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[&'g SourceGraph<'g>],
    packages: &PackageIndex,
    configured: &ConfiguredModuleResolution,
    sources: &mut SourceBindingCache<'_>,
    entities: &'g EntityProjection,
    candidates: &mut ProjectResolutionRegistry,
    loaded: Option<BTreeSet<CanonicalResolutionKey>>,
    replaced: &BTreeSet<String>,
    control: &IndexWorkControl,
    staging_limit: u64,
) -> Result<(CallBindings, u64), CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let rust_needed = graphs.iter().any(|graph| {
        graph
            .semantics()
            .is_some_and(|facts| facts.rust().is_some())
            && packages.package_owner(&graph.graph().path).is_some()
    });
    let mut needed = rust_needed;
    'sources: for graph in graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        if graph
            .graph()
            .language
            .as_deref()
            .and_then(language_capability)
            .and_then(|capability| capability.effective_semantic_provider())
            != Some(SemanticProviderOwner::EcmaScript)
        {
            continue;
        }
        let Some(facts) = graph.semantics() else {
            continue;
        };
        let Some(projection) = entities.keys_by_graph.get(&graph.graph().path) else {
            continue;
        };
        for call in facts.calls() {
            control.check(IndexWorkStage::SymbolParsing)?;
            if matches!(call.binding, SourceCallBinding::Imported { .. })
                && needs_forwarding(relation_keys(projection, call.relation), candidates)
            {
                needed = true;
                break 'sources;
            }
        }
    }
    if !needed {
        return Ok((CallBindings::new(), 0));
    }
    let loaded_bytes = loaded.as_ref().map_or(0, |keys| {
        keys.iter()
            .map(super::resolution_workspace_key_bytes)
            .fold(0_u64, u64::saturating_add)
    });
    let mut resolver = BindingResolver {
        store,
        project,
        generation,
        base_generation: store.repository_graph_generation()?,
        graphs: graphs
            .iter()
            .map(|graph| {
                (
                    graph.graph().path.as_str(),
                    (graph.graph(), graph.semantics()),
                )
            })
            .collect(),
        entities,
        packages,
        configured,
        sources,
        candidates,
        replaced,
        loaded,
        context: BTreeMap::new(),
        modules: ModuleInterner::default(),
        queries: BTreeMap::new(),
        hydrated: BTreeMap::new(),
        retained_bytes: (graphs.len() as u64)
            .saturating_mul(STAGED_GRAPH_ROW_BYTES)
            .saturating_add(std::mem::size_of::<ModuleInterner>() as u64)
            .saturating_add(loaded_bytes),
        control,
        retained_limit: staging_limit,
    };
    resolver.check_budget()?;
    let mut bindings = CallBindings::new();
    for graph in graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        let raw = graph.graph();
        if raw
            .language
            .as_deref()
            .and_then(language_capability)
            .and_then(|capability| capability.effective_semantic_provider())
            != Some(SemanticProviderOwner::EcmaScript)
        {
            continue;
        }
        let Some(facts) = graph.semantics() else {
            continue;
        };
        let Some(projection) = entities.keys_by_graph.get(&raw.path) else {
            continue;
        };
        for call in facts.calls() {
            control.check(IndexWorkStage::SymbolParsing)?;
            let SourceCallBinding::Imported {
                import,
                reference,
                member,
                ..
            } = &call.binding
            else {
                continue;
            };
            let reference = &facts.imports()[*import].references[*reference];
            if reference.imported().is_none() && member.is_none() {
                continue;
            }
            let ordinary = relation_keys(projection, call.relation);
            if !needs_forwarding(ordinary, resolver.candidates) {
                continue;
            }
            let query = resolver.query(&raw.path, reference, member.as_deref())?;
            let outcome = resolver.resolve(query, &mut BTreeMap::new(), 0)?;
            if !outcome.followed_export {
                continue;
            }
            bindings
                .entry(raw.path.clone())
                .or_default()
                .insert(call.relation, outcome);
            resolver.retained_bytes = resolver
                .retained_bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(raw.path.len() as u64);
            resolver.check_budget()?;
        }
    }
    rust::resolve_rust_bindings(&mut resolver, graphs, &mut bindings)?;
    // Only unique outcomes referenced by the occurrence map survive this step.
    let mut shared = BTreeSet::new();
    let retained = bindings.iter().fold(0_u64, |bytes, (path, calls)| {
        calls.values().fold(
            bytes
                .saturating_add(path.len() as u64)
                .saturating_add((calls.len() as u64).saturating_mul(STAGED_GRAPH_ROW_BYTES)),
            |bytes, outcome| {
                if shared.insert(Arc::as_ptr(outcome)) {
                    bytes.saturating_add(outcome.retained_bytes())
                } else {
                    bytes
                }
            },
        )
    });
    Ok((bindings, retained))
}

impl<'g> BindingResolver<'_, 'g, '_> {
    /// Bind every query to this resolver's IDs and reserve retained metadata atomically.
    fn query(
        &mut self,
        source: &str,
        reference: &ImportReference,
        member: Option<&str>,
    ) -> Result<BindingQuery, CliError> {
        self.check_budget()?;
        let before = self.retained_staging_bytes();
        let limit = self.retained_limit;
        let control = self.control;
        let retained = &mut self.retained_bytes;
        BindingQuery::new(source, reference, member, &mut self.modules, |bytes| {
            control.check(IndexWorkStage::SymbolParsing)?;
            enforce_resolution_registry_budget_with_limit(before.saturating_add(bytes), limit)?;
            *retained = retained.saturating_add(bytes);
            Ok(())
        })
    }

    /// Include every simultaneously live owner in the staging memory boundary.
    fn check_budget(&self) -> Result<(), CliError> {
        self.control.check(IndexWorkStage::SymbolParsing)?;
        enforce_resolution_registry_budget_with_limit(
            self.retained_staging_bytes(),
            self.retained_limit,
        )
    }

    /// Sum accounted owners without changing their reservation state.
    fn retained_staging_bytes(&self) -> u64 {
        self.entities
            .retained_bytes
            .saturating_add(self.candidates.retained_bytes)
            .saturating_add(self.sources.retained_bytes)
            .saturating_add(self.retained_bytes)
    }

    /// Admit a temporary owner before allocating, retaining prior accounting on refusal.
    fn reserve_scratch(&mut self, bytes: u64) -> Result<(), CliError> {
        let before = self.retained_bytes;
        self.retained_bytes = before.saturating_add(bytes);
        if let Err(error) = self.check_budget() {
            self.retained_bytes = before;
            return Err(error);
        }
        Ok(())
    }

    /// Release exactly one admitted temporary owner without erasing persistent state.
    fn release_scratch(&mut self, bytes: u64) -> Result<(), CliError> {
        self.retained_bytes = self.retained_bytes.checked_sub(bytes).ok_or_else(|| {
            CliError::InvalidInput("resolution workspace accounting is unbalanced".to_string())
        })?;
        Ok(())
    }

    /// Release temporary construction on every exit and retain the completed owner once.
    fn with_scratch<T>(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut ResolverScratch) -> Result<(T, u64), CliError>,
    ) -> Result<T, CliError> {
        let mut scratch = ResolverScratch::default();
        let result = operation(self, &mut scratch);
        self.release_scratch(scratch.retained_bytes)?;
        let (value, retained_bytes) = result?;
        self.reserve_scratch(retained_bytes)?;
        Ok(value)
    }

    /// Retain only a new key, checking both row and byte admission before cloning.
    fn insert_cause(
        &mut self,
        causes: &mut CauseWorkspace<'_>,
        key: &CanonicalResolutionKey,
    ) -> Result<(), CliError> {
        self.check_budget()?;
        if causes.keys.contains(key) {
            return Ok(());
        }
        if causes.keys.len() >= GraphLimits::MAX_ROWS as usize {
            return Err(resolution_key_limit_failure(
                causes.keys.len().saturating_add(1),
            ));
        }
        causes.scratch.reserve(
            self,
            STAGED_GRAPH_ROW_BYTES.saturating_add(key.canonical_identity().len() as u64),
        )?;
        causes.keys.insert(key.clone());
        Ok(())
    }

    /// Load one explicitly needed source, reusing fresh facts and captured bytes.
    fn source(&mut self, path: &str) -> Result<Arc<BindingSource<'g>>, CliError> {
        if let Some(source) = self.context.get(path) {
            return Ok(Arc::clone(source));
        }
        self.check_budget()?;
        let mut projection_bytes = 0;
        let (source, projection) = if let Some((graph, Some(semantics))) =
            self.graphs.get(path).copied()
        {
            let keys = self.entities.keys_by_graph.get(path).ok_or_else(|| {
                CliError::InvalidInput("binding source has no admitted key projection".to_string())
            })?;
            (
                SourceGraph::Staged {
                    graph,
                    semantics: Some(semantics),
                },
                SourceProjection::Staged(keys),
            )
        } else {
            let source_limit = self
                .retained_limit
                .saturating_sub(self.entities.retained_bytes)
                .saturating_sub(self.candidates.retained_bytes)
                .saturating_sub(self.retained_bytes);
            self.sources.set_retained_limit(source_limit)?;
            let captured = self.sources.capture(path, self.control)?;
            self.check_budget()?;
            let keys = match derive_resolution_keys_with_context(
                self.project,
                self.packages.package_name(path),
                &captured.graph,
                super::resolution_projection_context(path, self.packages, self.configured)
                    .with_source_semantics(&captured.semantics)
                    .with_retained_bytes_limit(
                        self.retained_limit
                            .saturating_sub(self.retained_staging_bytes()),
                    ),
            ) {
                Ok(keys) => keys,
                Err(ResolutionProjectionError::Contract(failure)) => failure.into_parts().2,
                Err(ResolutionProjectionError::KeyLimit { requested, .. }) => {
                    return Err(resolution_key_limit_failure(requested));
                }
                Err(ResolutionProjectionError::MemoryLimit { limit, requested }) => {
                    return Err(super::resolution_projection_memory_failure(
                        limit, requested,
                    ));
                }
                Err(ResolutionProjectionError::SourceEvidenceMismatch) => {
                    return Err(CliError::InvalidInput(
                        "captured binding source changed its ordinal proof".to_string(),
                    ));
                }
            };
            projection_bytes = resolution_retained_bytes(&keys);
            self.retained_bytes = self.retained_bytes.saturating_add(projection_bytes);
            (
                SourceGraph::Captured(captured),
                SourceProjection::Captured(keys),
            )
        };
        let mut slots = BTreeMap::<String, Vec<usize>>::new();
        let mut modules = BTreeMap::<String, BTreeSet<CanonicalResolutionKey>>::new();
        if let Some(facts) = source.semantics() {
            if facts.exports_complete() {
                for (index, export) in facts.exports().iter().enumerate() {
                    self.control.check(IndexWorkStage::SymbolParsing)?;
                    if export.namespace == SourceExportNamespace::Value {
                        let bytes = (4 * std::mem::size_of::<usize>()) as u64
                            + if slots.contains_key(&export.name) {
                                0
                            } else {
                                STAGED_GRAPH_ROW_BYTES.saturating_add(export.name.len() as u64)
                            };
                        self.reserve_scratch(bytes)?;
                        slots.entry(export.name.clone()).or_default().push(index);
                    }
                }
            }
            for import in facts.imports() {
                self.control.check(IndexWorkStage::SymbolParsing)?;
                if let Some(module) = &import.module {
                    if !modules.contains_key(module) {
                        self.reserve_scratch(
                            STAGED_GRAPH_ROW_BYTES.saturating_add(module.len() as u64),
                        )?;
                        modules.insert(module.clone(), BTreeSet::new());
                    }
                    for key in relation_keys(projection.get(), import.relation) {
                        if !modules.get(module).is_some_and(|keys| keys.contains(key)) {
                            self.reserve_scratch(
                                STAGED_GRAPH_ROW_BYTES
                                    .saturating_add(key.canonical_identity().len() as u64),
                            )?;
                            if let Some(keys) = modules.get_mut(module) {
                                keys.insert(key.clone());
                            }
                        }
                    }
                }
            }
        }
        let vector_rows = modules.values().map(BTreeSet::len).sum::<usize>();
        self.reserve_scratch(
            (vector_rows as u64)
                .saturating_mul(std::mem::size_of::<CanonicalResolutionKey>() as u64),
        )?;
        let modules = modules
            .into_iter()
            .map(|(name, keys)| (name, keys.into_iter().collect::<Vec<_>>()))
            .collect::<BTreeMap<_, _>>();
        let parents = projectatlas_symbols::declaration_identity::qualified_symbol_parents_checked(
            source.graph(),
            &mut |bytes| self.reserve_scratch(bytes),
        )?;
        self.reserve_scratch(STAGED_GRAPH_ROW_BYTES.saturating_add(path.len() as u64))?;
        drop(projection);
        self.retained_bytes = self.retained_bytes.saturating_sub(projection_bytes);
        let source = Arc::new(BindingSource {
            source,
            slots,
            modules,
            parents,
        });
        self.context.insert(path.to_string(), Arc::clone(&source));
        self.check_budget()?;
        Ok(source)
    }

    /// Fetch each additional canonical lookup once under the existing bounded API.
    fn module_file(&mut self, keys: &[CanonicalResolutionKey]) -> Result<Option<String>, CliError> {
        if self.loaded.is_some() {
            let needed = self.with_scratch(|this, scratch| {
                let mut needed = CauseWorkspace::new(scratch);
                for key in keys {
                    if !this
                        .loaded
                        .as_ref()
                        .is_some_and(|loaded| loaded.contains(key))
                    {
                        this.insert_cause(&mut needed, key)?;
                    }
                }
                let needed = needed.keys.into_iter().collect::<Vec<_>>();
                if !needed.is_empty() {
                    this.control.check(IndexWorkStage::SymbolParsing)?;
                    let candidates = this
                        .store
                        .repository_resolution_candidates_for_keys_with_byte_limit(
                            this.project,
                            &needed,
                            MAX_INCREMENTAL_RESOLUTION_ITEMS,
                            this.retained_limit
                                .saturating_sub(this.retained_staging_bytes()),
                        )
                        .map_err(super::resolution_read_error)?;
                    if candidates.page.truncated {
                        return Err(dependency_closure_limit(
                            this.sources.root,
                            std::iter::empty(),
                            GraphLimits::MAX_ROWS as usize + 1,
                        ));
                    }
                    this.candidates.retained_limit = this
                        .retained_limit
                        .saturating_sub(this.entities.retained_bytes)
                        .saturating_sub(this.sources.retained_bytes)
                        .saturating_sub(this.retained_bytes);
                    append_persisted_resolution_candidates(
                        this.candidates,
                        this.project,
                        this.generation,
                        candidates,
                        this.replaced,
                        this.control,
                    )?;
                    this.check_budget()?;
                }
                let bytes = scratch.retained_bytes;
                Ok((needed, bytes))
            })?;
            if let Some(loaded) = &mut self.loaded {
                loaded.extend(needed);
            }
        }
        let matches = registry_resolution_matches(
            keys,
            ResolutionCandidateKind::Any,
            self.candidates,
            &self.entities.entity_by_digest,
            self.control,
        )?;
        let path = if matches.count == 1 {
            matches.first.and_then(|entity| match entity.selector() {
                EntitySelector::File { path } => Some(path.as_str().to_string()),
                _ => None,
            })
        } else {
            None
        };
        self.check_budget()?;
        Ok(path)
    }

    /// Follow explicit module slots; a cycle, ambiguity or unsupported value abstains.
    fn resolve(
        &mut self,
        query: BindingQuery,
        visited: &mut BTreeMap<BindingQuery, usize>,
        depth: usize,
    ) -> Result<Arc<CallBindingResolution>, CliError> {
        self.check_budget()?;
        if let Some(outcome) = self
            .queries
            .get(&query)
            .and_then(|depths| depths.get(&depth))
        {
            return Ok(Arc::clone(outcome));
        }
        if depth >= MAX_FORWARDING_DEPTH {
            return Ok(Arc::new(CallBindingResolution::default()));
        }
        let outcome = self.with_scratch(|this, scratch| {
            let mut outcome = CallBindingResolution::default();
            let mut causes = CauseWorkspace::new(scratch);
            causes
                .scratch
                .reserve(this, query.retained_bytes().saturating_mul(2))?;
            visited.insert(query.clone(), depth);
            let result: Result<(), CliError> = (|| {
                let source = this.source(&query.source)?;
                let keys = source
                    .modules
                    .get(query.module.as_ref())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for key in keys {
                    this.insert_cause(&mut causes, key)?;
                }
                let key = document_file_resolution_key(this.project, &query.source)?;
                this.insert_cause(&mut causes, &key)?;
                if let Some(path) = this.module_file(keys)? {
                    let path = path.as_str();
                    let key = document_file_resolution_key(this.project, path)?;
                    this.insert_cause(&mut causes, &key)?;
                    let target = this.source(path)?;
                    let selected = if let Some(name) = &query.imported {
                        Some((name.as_str(), query.member.as_deref()))
                    } else {
                        query.member.as_deref().map(|member| {
                            member
                                .split_once('.')
                                .map_or((member, None), |(name, member)| (name, Some(member)))
                        })
                    };
                    if let Some((name, member)) = selected
                        && let Some(indices) = target.slots.get(name)
                        && indices.len() == 1
                        && let Some(fact) = target
                            .source
                            .semantics()
                            .and_then(|facts| facts.exports().get(indices[0]))
                    {
                        outcome.followed_export = true;
                        match &fact.binding {
                            SourceExportBinding::LocalDeclaration(index) if member.is_none() => {
                                outcome.target = this.target(&target, *index)?;
                            }
                            SourceExportBinding::ReExport(reference) => {
                                let forwarded_query = this.query(path, reference, member)?;
                                if let Some(first) = visited.get(&forwarded_query).copied() {
                                    // The parent owns cycle causes directly; no uncached set escapes
                                    // with a charge that outlives its temporary result.
                                    for (cycle, _) in
                                        visited.iter().filter(|(_, position)| **position >= first)
                                    {
                                        let key = document_file_resolution_key(
                                            this.project,
                                            &cycle.source,
                                        )?;
                                        this.insert_cause(&mut causes, &key)?;
                                    }
                                } else {
                                    let forwarded =
                                        this.resolve(forwarded_query, visited, depth + 1)?;
                                    outcome.target.clone_from(&forwarded.target);
                                    for key in &forwarded.dependencies {
                                        this.insert_cause(&mut causes, key)?;
                                    }
                                }
                            }
                            SourceExportBinding::LocalDeclaration(_)
                            | SourceExportBinding::RustReExport(_)
                            | SourceExportBinding::AmbiguousDeclarations(_)
                            | SourceExportBinding::UnresolvedDeclarations(_)
                            | SourceExportBinding::Unresolved => {}
                        }
                    }
                }
                Ok(())
            })();
            visited.remove(&query);
            result?;
            outcome.dependencies = causes.keys;
            let retained_bytes = outcome
                .retained_bytes()
                .saturating_add(query.retained_bytes());
            Ok((outcome, retained_bytes))
        })?;
        let outcome = Arc::new(outcome);
        self.queries
            .entry(query)
            .or_default()
            .insert(depth, Arc::clone(&outcome));
        Ok(outcome)
    }

    /// Hydrate the exact existing canonical declaration selected by admitted source ordinals.
    fn target(
        &mut self,
        source: &BindingSource<'_>,
        index: usize,
    ) -> Result<Option<GraphEntity>, CliError> {
        let graph = source.source.graph();
        let Some(symbol) = graph.symbols.get(index) else {
            return Ok(None);
        };
        if matches!(
            symbol.kind,
            SymbolKind::Type
                | SymbolKind::Interface
                | SymbolKind::Trait
                | SymbolKind::Import
                | SymbolKind::Module
        ) {
            return Ok(None);
        }
        let parent = source.parents.get(index).cloned().flatten();
        let expected = source_symbol_entity(self.project, self.generation, graph, symbol, parent)?;
        if let Some(entity) = self.entities.entity_by_digest.get(expected.key().digest()) {
            return Ok(Some(entity.clone()));
        }
        if let Some(entity) = self
            .candidates
            .supplemental_entities_by_digest
            .get(expected.key().digest())
        {
            return Ok(Some(entity.clone()));
        }
        if let Some(entity) = self.hydrated.get(expected.key().digest()) {
            return Ok(entity.clone());
        }
        let Some(base_generation) = self.base_generation else {
            return Ok(None);
        };
        self.control.check(IndexWorkStage::SymbolParsing)?;
        let budget = projectatlas_db::RepositoryGraphReadBudget::new(
            1,
            1,
            projectatlas_db::RepositoryGraphReadBudget::MAX_DECODED_BYTES,
            1,
            1,
        )
        .map_err(invalid_graph_contract)?;
        let digest = expected.key().digest().to_string();
        let stored = self.store.repository_graph_entity_bounded(
            expected.key(),
            base_generation,
            budget,
            Some(self.control),
        )?;
        self.control.check(IndexWorkStage::SymbolParsing)?;
        let entity = stored
            .rows
            .into_iter()
            .next()
            .filter(|entity| entity.selector() == expected.selector())
            .map(|_stored| expected);
        self.retained_bytes = self
            .retained_bytes
            .saturating_add(STAGED_GRAPH_ROW_BYTES)
            .saturating_add(digest.len() as u64)
            .saturating_add(entity.as_ref().map_or(0, entity_retained_bytes));
        self.hydrated.insert(digest, entity.clone());
        self.check_budget()?;
        Ok(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::{BindingQuery, ModuleInterner, STAGED_GRAPH_ROW_BYTES};
    use projectatlas_core::{IndexCancellation, IndexWorkControl};
    use projectatlas_symbols::SourceExportBinding;
    use std::error::Error;

    #[test]
    fn forwarding_query_module_identity_uses_values_across_allocations()
    -> Result<(), Box<dyn Error>> {
        let mut modules = ModuleInterner::default();
        let first =
            projectatlas_symbols::parse_import_references("import {run} from './target.js';");
        let second =
            projectatlas_symbols::parse_import_references("import {run} from './target.js';");
        let first_reference = first.first().ok_or("first native import missing")?;
        let second_reference = second.first().ok_or("second native import missing")?;
        if std::sync::Arc::ptr_eq(
            &first_reference.shared_module(),
            &second_reference.shared_module(),
        ) {
            return Err("independent imports unexpectedly share one allocation".into());
        }
        let first = BindingQuery::new("barrel.js", first_reference, None, &mut modules, |_| {
            Ok::<(), std::convert::Infallible>(())
        })?;
        let second = BindingQuery::new("barrel.js", second_reference, None, &mut modules, |_| {
            Ok::<(), std::convert::Infallible>(())
        })?;
        if first != second || first.cmp(&second) != std::cmp::Ordering::Equal {
            return Err("equal module values did not retain exact query identity".into());
        }
        let mut cache = std::collections::BTreeMap::new();
        cache.insert(first.clone(), 7);
        if cache.get(&second) != Some(&7) {
            return Err("equal allocation missed query cache".into());
        }
        let mut visited = std::collections::BTreeMap::new();
        visited.insert(first, 3);
        if visited.get(&second) != Some(&3)
            || modules.values.len() != 1
            || modules.allocations.len() != 2
        {
            return Err("visited identity or retained module witnesses changed".into());
        }
        Ok(())
    }

    #[test]
    fn module_identity_admission_preserves_exact_values_and_owned_allocations()
    -> Result<(), Box<dyn Error>> {
        let mut modules = ModuleInterner::default();
        let first: std::sync::Arc<str> = format!("./{}a", "x".repeat(262_144)).into();
        let rejected = modules.intern(std::sync::Arc::clone(&first), |bytes| {
            super::enforce_resolution_registry_budget_with_limit(bytes, STAGED_GRAPH_ROW_BYTES)
        });
        if rejected.is_ok() || !modules.values.is_empty() || !modules.allocations.is_empty() {
            return Err("refused module admission retained metadata".into());
        }
        let mut admitted = Vec::new();
        let mut admit = |bytes| {
            admitted.push(bytes);
            Ok::<(), std::convert::Infallible>(())
        };
        let id = modules.intern(std::sync::Arc::clone(&first), &mut admit)?;
        if modules.intern(std::sync::Arc::clone(&first), &mut admit)? != id {
            return Err("repeated allocation changed module identity".into());
        }
        let equal: std::sync::Arc<str> = std::sync::Arc::from(first.as_ref());
        let retained = std::sync::Arc::downgrade(&equal);
        if modules.intern(equal, &mut admit)? != id || retained.upgrade().is_none() {
            return Err("equal value or retained allocation witness changed".into());
        }
        let different: std::sync::Arc<str> = format!("./{}b", "x".repeat(262_144)).into();
        if modules.intern(different, &mut admit)? == id {
            return Err("different module values acquired one identity".into());
        }
        if admitted
            != [
                STAGED_GRAPH_ROW_BYTES * 2,
                STAGED_GRAPH_ROW_BYTES,
                STAGED_GRAPH_ROW_BYTES * 2,
            ]
            || modules.values.len() != 2
            || modules.allocations.len() != 3
        {
            return Err("module metadata admission changed".into());
        }
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        let result = modules.intern(std::sync::Arc::from("./canceled.js"), |_| {
            control.check(projectatlas_core::IndexWorkStage::SymbolParsing)
        });
        if result.is_ok() || modules.values.len() != 2 || modules.allocations.len() != 3 {
            return Err("canceled module admission retained metadata".into());
        }
        Ok(())
    }

    #[test]
    fn forwarding_queries_share_source_modules_before_cache_lookup() -> Result<(), Box<dyn Error>> {
        const SOURCE: &str = "barrel.js";
        let module = format!("./{}", "x".repeat(262_144));
        let slots = (0..32)
            .map(|index| format!("run as e{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let content = format!("export {{{slots}}} from \"{module}\";");
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (_, _, facts) = projectatlas_symbols::extract_symbol_graph_with_semantics_controlled(
            SOURCE,
            Some("javascript"),
            &content,
            &control,
        )?
        .into_parts();
        if facts.exports().len() != 32 {
            return Err("native reexport slots missing".into());
        }
        let mut modules = ModuleInterner::default();
        for export in facts.exports() {
            let SourceExportBinding::ReExport(reference) = &export.binding else {
                return Err("native reexport evidence missing".into());
            };
            let query = BindingQuery::new(SOURCE, reference, None, &mut modules, |_| {
                Ok::<(), std::convert::Infallible>(())
            })?;
            if query.module.as_ptr() != reference.module().as_ptr() {
                return Err("query copied source-owned module before cache lookup".into());
            }
            let copied = query.clone();
            if copied.module.as_ptr() != query.module.as_ptr() {
                return Err("resolver frame copied shared query module".into());
            }
        }
        Ok(())
    }
}
