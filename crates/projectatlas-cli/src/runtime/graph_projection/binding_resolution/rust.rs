//! Cargo-root and module ownership for Rust static bindings.
use super::{
    BindingResolver, BindingSource, CallBindingResolution, CallBindings, CanonicalResolutionKey,
    CauseWorkspace, CliError, EntitySelector, GraphEntity, IndexWorkStage, MAX_FORWARDING_DEPTH,
    MAX_INCREMENTAL_RESOLUTION_ITEMS, ResolverScratch, STAGED_GRAPH_ROW_BYTES, SourceCallBinding,
    SourceGraph, SourceSemanticFacts, document_file_resolution_key, invalid_graph_contract,
    resolution_key_limit_failure,
};
use projectatlas_core::graph::{ExternalSelector, RepositoryFilePath};
use projectatlas_symbols::{
    SourceCargoTargetKind as TargetKind, SourceRustAnchor as Anchor, SourceRustBinding,
    SourceRustPath, SourceRustUseRole as UseRole, SourceRustVisibility as Visibility,
    cargo_target_inventory_key,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Filesystem placement is independent from a module's logical crate address.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Module {
    /// Actual target source, selected by Cargo evidence.
    root: String,
    /// Admitted file containing the module.
    file: String,
    /// Inline source module owner.
    inline: Arc<[String]>,
    /// Crate-relative module path.
    logical: Vec<String>,
    /// Default directory for external children.
    directory: String,
    /// Visibility of this module from its declaring parent.
    visibility: Visibility,
}

/// Conservative owned module payload, including shared inline-path capacity.
fn module_bytes(module: &Module) -> u64 {
    [
        module.root.capacity(),
        module.file.capacity(),
        module.directory.capacity(),
    ]
    .into_iter()
    .fold(STAGED_GRAPH_ROW_BYTES, |bytes, capacity| {
        bytes.saturating_add(capacity as u64)
    })
    .saturating_add((module.logical.capacity() * std::mem::size_of::<String>()) as u64)
    .saturating_add(
        module
            .logical
            .iter()
            .map(|part| part.capacity() as u64)
            .sum::<u64>(),
    )
    .saturating_add(std::mem::size_of_val(module.inline.as_ref()) as u64)
    .saturating_add(
        module
            .inline
            .iter()
            .map(|part| part.capacity() as u64)
            .sum::<u64>(),
    )
}

/// Cached membership retains causes even when ownership is unproved.
#[derive(Default)]
struct Membership {
    /// Unique actual Cargo root and module address.
    owner: Option<Module>,
    /// Manifest, root, module and missing-path causes.
    causes: BTreeSet<CanonicalResolutionKey>,
}

/// Preindexed source slots avoid a whole-file scan for every call.
#[derive(Default)]
struct Slots {
    /// Value declaration ordinals by actual inline owner and name.
    values: BTreeMap<Arc<[String]>, BTreeMap<String, Vec<usize>>>,
    /// Module ordinals by actual inline owner and name.
    modules: BTreeMap<Arc<[String]>, BTreeMap<String, Vec<usize>>>,
    /// Shadow-checked toolchain identity indexed by the shared use occurrence.
    toolchain: BTreeMap<*const projectatlas_symbols::SourceRustUse, String>,
    /// Inherent method ordinals grouped by actual module, type and method name.
    associated: BTreeMap<(Arc<[String]>, String, String), Vec<usize>>,
}

/// Rust value and module namespaces can share a spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Namespace {
    /// Callable values.
    Value,
    /// Declared or explicitly imported module namespaces.
    Module,
}

/// Actual selected declaration or source module, never a callable file marker.
enum Selection {
    /// Source proves the requested namespace is absent, independently from unknown lookup.
    Absent,
    /// Actual admitted callable declaration.
    Value(GraphEntity, Visibility, Vec<String>),
    /// Source-proven namespace address.
    Module(Module),
    /// Type namespace owner proved by a native declaration, never a direct callee.
    Type {
        /// Actual source module declaring this type.
        owner: Module,
        /// Source-proven type identifier.
        name: String,
        /// Type namespace visibility, independent from constructor visibility.
        visibility: Visibility,
    },
}

/// Cache key retains source syntax, inline owner and call/import meaning.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Request {
    /// Source that owns the syntax.
    source: String,
    /// Inline source module of the reference.
    module: Arc<[String]>,
    /// Complete AST path including any static call member suffix.
    path: SourceRustPath,
    /// A grouped self or wildcard import selects only a namespace.
    namespace: bool,
    /// Imports target files; calls target declarations.
    import: bool,
    /// The exact source import proves an unshadowed static toolchain root.
    toolchain: bool,
}

/// Bounded query caches reuse the existing source capture and entity hydration.
struct RustResolver<'r, 'a, 'g, 'n> {
    /// Shared operation authority and budgets.
    shared: &'r mut BindingResolver<'a, 'g, 'n>,
    /// Each source membership is checked once.
    memberships: BTreeMap<String, Arc<Membership>>,
    /// Actual manifest target candidates are collected once.
    roots: BTreeMap<String, Arc<[(String, bool)]>>,
    /// Module/value slots indexed once per required source.
    slots: BTreeMap<String, Slots>,
    /// Identical calls share one complete result and cause set.
    queries: BTreeMap<Request, Arc<CallBindingResolution>>,
    /// Competing slots observed in the current complete root query.
    ambiguous: u32,
    /// Repeated local import groups share merged namespace outcomes.
    candidate_queries: BTreeMap<
        (
            String,
            *const [(usize, usize)],
            *const [usize],
            Option<String>,
        ),
        Arc<CallBindingResolution>,
    >,
}

/// Resolve native Rust calls and dependencies with actual Cargo ownership evidence.
pub(super) fn resolve_rust_bindings<'g>(
    shared: &mut BindingResolver<'_, 'g, '_>,
    graphs: &[&SourceGraph<'g>],
    bindings: &mut CallBindings,
) -> Result<(), CliError> {
    let mut resolver = RustResolver {
        shared,
        memberships: BTreeMap::new(),
        roots: BTreeMap::new(),
        slots: BTreeMap::new(),
        queries: BTreeMap::new(),
        ambiguous: 0,
        candidate_queries: BTreeMap::new(),
    };
    for graph in graphs {
        resolver.shared.check_budget()?;
        let raw = graph.graph();
        let Some(facts) = graph
            .semantics()
            .filter(|facts| facts.rust().is_some_and(|rust| rust.complete))
        else {
            continue;
        };
        if resolver.shared.packages.package_owner(&raw.path).is_none() {
            continue;
        }
        resolver.with_scratch(|resolver, scratch| {
            let candidate_index_bytes = (facts.rust().map_or(0, |rust| rust.candidate_calls.len())
                as u64)
                .saturating_mul(STAGED_GRAPH_ROW_BYTES);
            scratch.reserve(resolver.shared, candidate_index_bytes)?;
            let candidate_calls = facts
                .rust()
                .into_iter()
                .flat_map(|rust| &rust.candidate_calls)
                .map(|call| (call.relation, call))
                .collect::<BTreeMap<_, _>>();
            for call in facts.calls() {
                resolver.shared.check_budget()?;
                if let Some(candidates) = candidate_calls.get(&call.relation) {
                    let outcome = resolver.resolve_candidates(&raw.path, facts, candidates)?;
                    resolver
                        .charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
                    bindings
                        .entry(raw.path.clone())
                        .or_default()
                        .insert(call.relation, outcome);
                    continue;
                }
                let (module, path, namespace) = match &call.binding {
                    SourceCallBinding::Imported {
                        import,
                        member,
                        rust_toolchain: None,
                        ..
                    } => {
                        let Some(binding) = facts
                            .imports()
                            .get(*import)
                            .and_then(|import| import.rust.as_deref())
                        else {
                            continue;
                        };
                        if !binding.supported || binding.role == UseRole::Wildcard {
                            continue;
                        }
                        let mut path = binding.path.clone();
                        if let Some(member) = member {
                            path.segments
                                .extend(member.split("::").map(ToString::to_string));
                        }
                        (
                            Arc::clone(&binding.module),
                            path,
                            binding.role == UseRole::SelfNamespace && member.is_none(),
                        )
                    }
                    SourceCallBinding::RustPath(call) => {
                        (Arc::clone(&call.module), call.path.clone(), false)
                    }
                    _ => continue,
                };
                // Preserve complete paths; no packed key or display-name reinterpretation.
                let outcome = resolver.resolve(Request {
                    source: raw.path.clone(),
                    module,
                    path,
                    namespace,
                    import: false,
                    toolchain: false,
                })?;
                resolver.charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
                bindings
                    .entry(raw.path.clone())
                    .or_default()
                    .insert(call.relation, outcome);
            }
            for import in facts.imports() {
                resolver.shared.check_budget()?;
                let Some(binding) = import.rust.as_deref() else {
                    continue;
                };
                if !binding.supported || import.rust_toolchain.is_some() {
                    continue;
                }
                let outcome = resolver.resolve(Request {
                    source: raw.path.clone(),
                    module: Arc::clone(&binding.module),
                    path: binding.path.clone(),
                    namespace: binding.role != UseRole::Item,
                    import: true,
                    toolchain: false,
                })?;
                resolver.charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
                bindings
                    .entry(raw.path.clone())
                    .or_default()
                    .insert(import.relation, outcome);
            }
            Ok(((), 0))
        })?;
    }
    Ok(())
}

impl<'g> RustResolver<'_, '_, 'g, '_> {
    /// Account all additional caches under the operation's existing memory boundary.
    fn charge(&mut self, bytes: u64) -> Result<(), CliError> {
        self.shared.reserve_scratch(bytes)
    }

    /// Release temporary construction on every exit and charge the completed owner once.
    fn with_scratch<T>(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut ResolverScratch) -> Result<(T, u64), CliError>,
    ) -> Result<T, CliError> {
        let mut scratch = ResolverScratch::default();
        let result = operation(self, &mut scratch);
        self.shared.release_scratch(scratch.retained_bytes)?;
        let (value, retained_bytes) = result?;
        self.shared.reserve_scratch(retained_bytes)?;
        Ok(value)
    }

    /// Use the shared bounded cause owner for Rust-specific source traversal.
    fn insert_cause(
        &mut self,
        causes: &mut CauseWorkspace<'_>,
        key: &CanonicalResolutionKey,
    ) -> Result<(), CliError> {
        self.shared.insert_cause(causes, key)
    }

    /// Missing paths remain invalidation causes, without attempting to read them.
    fn cause(&mut self, path: &str, causes: &mut CauseWorkspace<'_>) -> Result<bool, CliError> {
        let key = document_file_resolution_key(self.shared.project, path)?;
        self.insert_cause(causes, &key)?;
        Ok(self.shared.sources.nodes.contains_key(path))
    }

    /// Preserve root certainty while reserving each new owned path before insertion.
    fn insert_root(
        &mut self,
        scratch: &mut ResolverScratch,
        roots: &mut BTreeMap<String, bool>,
        path: String,
        certain: bool,
    ) -> Result<(), CliError> {
        self.shared.check_budget()?;
        if let Some(existing) = roots.get_mut(&path) {
            *existing &= certain;
            return Ok(());
        }
        if roots.len() >= MAX_INCREMENTAL_RESOLUTION_ITEMS as usize {
            return Err(resolution_key_limit_failure(roots.len().saturating_add(1)));
        }
        scratch.reserve(
            self.shared,
            STAGED_GRAPH_ROW_BYTES.saturating_add(path.capacity() as u64),
        )?;
        roots.insert(path, certain);
        Ok(())
    }

    /// Index complete native ownership facts once, sharing source/module payloads.
    fn source(&mut self, path: &str) -> Result<Option<Arc<BindingSource<'g>>>, CliError> {
        let source = self.shared.source(path)?;
        let Some(facts) = source
            .source
            .semantics()
            .and_then(SourceSemanticFacts::rust)
            .filter(|facts| facts.complete)
        else {
            return Ok(None);
        };
        if !self.slots.contains_key(path) {
            let slots = self.with_scratch(|this, scratch| {
                let mut slots = Slots::default();
                let mut bytes = STAGED_GRAPH_ROW_BYTES.saturating_add(path.len() as u64);
                scratch.reserve(this.shared, bytes)?;
                for (ordinal, declaration) in facts.declarations.iter().enumerate() {
                    this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                    scratch.reserve(
                        this.shared,
                        STAGED_GRAPH_ROW_BYTES
                            .saturating_mul(4)
                            .saturating_add(declaration.name.len() as u64),
                    )?;
                    slots
                        .values
                        .entry(Arc::clone(&declaration.module))
                        .or_default()
                        .entry(declaration.name.clone())
                        .or_default()
                        .push(ordinal);
                    bytes = bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES * 4)
                        .saturating_add(declaration.name.len() as u64);
                }
                for (ordinal, module) in facts.modules.iter().enumerate() {
                    this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                    scratch.reserve(
                        this.shared,
                        STAGED_GRAPH_ROW_BYTES
                            .saturating_mul(4)
                            .saturating_add(module.name.len() as u64),
                    )?;
                    slots
                        .modules
                        .entry(Arc::clone(&module.parent))
                        .or_default()
                        .entry(module.name.clone())
                        .or_default()
                        .push(ordinal);
                    bytes = bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES * 4)
                        .saturating_add(module.name.len() as u64);
                }
                for (ordinal, method) in facts.associated.iter().enumerate() {
                    this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                    scratch.reserve(
                        this.shared,
                        STAGED_GRAPH_ROW_BYTES
                            .saturating_mul(4)
                            .saturating_add(method.owner.len() as u64)
                            .saturating_add(method.name.len() as u64),
                    )?;
                    slots
                        .associated
                        .entry((
                            Arc::clone(&method.module),
                            method.owner.clone(),
                            method.name.clone(),
                        ))
                        .or_default()
                        .push(ordinal);
                    bytes = bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES * 4)
                        .saturating_add(method.owner.len() as u64)
                        .saturating_add(method.name.len() as u64);
                }
                if let Some(facts) = source.source.semantics() {
                    for import in facts.imports() {
                        this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                        if let (Some(binding), Some(reference)) =
                            (&import.rust, &import.rust_toolchain)
                            && let Some(identity) = super::super::rust_import_identity(reference)
                        {
                            scratch.reserve(
                                this.shared,
                                STAGED_GRAPH_ROW_BYTES
                                    .saturating_mul(4)
                                    .saturating_add(identity.capacity() as u64),
                            )?;
                            bytes = bytes
                                .saturating_add(STAGED_GRAPH_ROW_BYTES * 4)
                                .saturating_add(identity.capacity() as u64);
                            slots.toolchain.insert(Arc::as_ptr(binding), identity);
                        }
                    }
                }
                Ok((slots, bytes))
            })?;
            self.slots.insert(path.to_string(), slots);
        }
        Ok(Some(source))
    }

    /// Gather roots from the owning manifest and admitted default target inventory.
    fn roots(
        &mut self,
        path: &str,
        causes: &mut CauseWorkspace<'_>,
    ) -> Result<Arc<[(String, bool)]>, CliError> {
        let Some(package) = self.shared.packages.package_owner(path) else {
            return Ok(Arc::from([]));
        };
        let manifest = package.manifest.clone();
        let directory = package.root.clone();
        let package_name = package.name.clone();
        if !self.cause(&manifest, causes)? {
            return Ok(Arc::from([]));
        }
        let source = self.shared.source(&manifest)?;
        let Some(targets) = source
            .source
            .semantics()
            .and_then(SourceSemanticFacts::cargo)
            .filter(|targets| targets.complete)
        else {
            return Ok(Arc::from([]));
        };
        for auto in &targets.automatic {
            if let Some(relative) = auto.kind.conventional_root() {
                self.cause(&join(&directory, relative), causes)?;
            }
            if let Some(relative) = auto.kind.automatic_directory() {
                let target_directory = join(&directory, relative);
                let prefix = format!("{target_directory}/");
                if let Some(relative) = path.strip_prefix(&prefix) {
                    let key = cargo_target_inventory_key(
                        self.shared.project,
                        &package_name,
                        &manifest,
                        &target_directory,
                    )
                    .map_err(invalid_graph_contract)?;
                    self.insert_cause(causes, &key)?;
                    if let Some(component) = relative
                        .split('/')
                        .next()
                        .filter(|part| part.strip_suffix(".rs").is_none())
                    {
                        self.cause(&format!("{target_directory}/{component}/main.rs"), causes)?;
                    }
                }
            }
        }
        if let Some(roots) = self.roots.get(&manifest) {
            return Ok(Arc::clone(roots));
        }
        let roots = self.with_scratch(|this, scratch| {
            scratch.reserve(
                this.shared,
                (targets.targets.len() as u64).saturating_mul(STAGED_GRAPH_ROW_BYTES),
            )?;
            let overrides = targets
                .targets
                .iter()
                .filter_map(|target| {
                    target
                        .name
                        .as_ref()
                        .map(|name| (target.kind, name.as_str()))
                })
                .collect::<BTreeSet<_>>();
            let mut roots = BTreeMap::<String, bool>::new();
            for target in &targets.targets {
                this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                if let Some(path) = repository_join(&directory, &target.path) {
                    this.insert_root(scratch, &mut roots, path, true)?;
                }
            }
            for auto in &targets.automatic {
                this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                if let Some(path) = auto.kind.conventional_root() {
                    let overridden = auto.kind == TargetKind::Binary
                        && overrides.contains(&(auto.kind, package_name.as_str()));
                    if !overridden {
                        this.insert_root(
                            scratch,
                            &mut roots,
                            join(&directory, path),
                            auto.certain,
                        )?;
                    }
                }
                let Some(relative) = auto.kind.automatic_directory() else {
                    continue;
                };
                let prefix = format!("{}/", join(&directory, relative));
                let nodes = Arc::clone(&this.shared.sources.nodes);
                for (path, _) in nodes.range(prefix.as_str()..) {
                    this.shared.control.check(IndexWorkStage::SymbolParsing)?;
                    let Some(relative) = path.strip_prefix(&prefix) else {
                        break;
                    };
                    let name = relative
                        .strip_suffix(".rs")
                        .filter(|name| !name.contains('/'))
                        .or_else(|| {
                            relative
                                .strip_suffix("/main.rs")
                                .filter(|name| !name.contains('/'))
                        });
                    let Some(name) = name.filter(|name| !name.is_empty()) else {
                        continue;
                    };
                    if overrides.contains(&(auto.kind, name)) {
                        continue;
                    }
                    this.insert_root(scratch, &mut roots, (*path).to_string(), auto.certain)?;
                }
            }
            let roots: Arc<[(String, bool)]> = roots.into_iter().collect::<Vec<_>>().into();
            let retained_bytes = roots.iter().fold(
                STAGED_GRAPH_ROW_BYTES.saturating_add(manifest.capacity() as u64),
                |bytes, (path, _)| {
                    bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES)
                        .saturating_add(path.capacity() as u64)
                },
            );
            Ok((roots, retained_bytes))
        })?;
        self.roots.insert(manifest, Arc::clone(&roots));
        Ok(roots)
    }

    /// Prove membership through actual module declarations along a narrowed path.
    fn membership(&mut self, path: &str) -> Result<Arc<Membership>, CliError> {
        if let Some(result) = self.memberships.get(path) {
            return Ok(Arc::clone(result));
        }
        let result = self.with_scratch(|this, scratch| {
            let mut causes = CauseWorkspace::new(scratch);
            let mut selected_owner = None;
            let roots = this.roots(path, &mut causes)?;
            let mut owner = None;
            let mut count = 0;
            for (root, certain) in roots.iter() {
                this.shared.check_budget()?;
                let directory = parent(root).to_string();
                let Some(parts) = candidate_path(&directory, path) else {
                    continue;
                };
                if !this.cause(root, &mut causes)? {
                    continue;
                }
                let mut module = Module {
                    root: root.clone(),
                    file: root.clone(),
                    inline: Arc::from([]),
                    logical: Vec::new(),
                    directory,
                    visibility: Visibility::Public,
                };
                if module.file != path {
                    for part in parts {
                        let Some(child) = this.declared_module(&module, &part, &mut causes)? else {
                            break;
                        };
                        module = child;
                        if module.file == path {
                            break;
                        }
                    }
                }
                if module.file == path && module.inline.is_empty() {
                    count += 1;
                    if *certain {
                        owner = Some(module);
                    }
                }
            }
            if count == 1 {
                selected_owner = owner;
            }
            let result = Membership {
                owner: selected_owner,
                causes: causes.keys,
            };
            let cause_bytes = result.causes.iter().fold(0_u64, |bytes, key| {
                bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(key.canonical_identity().len() as u64)
            });
            let retained_bytes = STAGED_GRAPH_ROW_BYTES
                .saturating_add(path.len() as u64)
                .saturating_add(cause_bytes)
                .saturating_add(result.owner.as_ref().map_or(0, module_bytes));
            Ok((result, retained_bytes))
        })?;
        let result = Arc::new(result);
        self.memberships
            .insert(path.to_string(), Arc::clone(&result));
        Ok(result)
    }

    /// An external module requires one declaration and exactly one admitted layout.
    fn declared_module(
        &mut self,
        owner: &Module,
        name: &str,
        causes: &mut CauseWorkspace<'_>,
    ) -> Result<Option<Module>, CliError> {
        if !self.cause(&owner.file, causes)? {
            return Ok(None);
        }
        let Some(source) = self.source(&owner.file)? else {
            return Ok(None);
        };
        let Some(indices) = self
            .slots
            .get(&owner.file)
            .and_then(|slots| slots.modules.get(&owner.inline))
            .and_then(|slots| slots.get(name))
        else {
            return Ok(None);
        };
        if indices.len() != 1 {
            return Ok(None);
        }
        let Some(module) = source
            .source
            .semantics()
            .and_then(SourceSemanticFacts::rust)
            .and_then(|facts| facts.modules.get(indices[0]))
        else {
            return Ok(None);
        };
        if !module.supported {
            return Ok(None);
        }
        let mut child = owner.clone();
        child.visibility = module.visibility;
        child.logical.push(name.to_string());
        child.directory = join(&owner.directory, name);
        if module.body.is_some() {
            let mut inline = owner.inline.to_vec();
            inline.push(name.to_string());
            child.inline = inline.into();
        } else {
            let direct = join(&owner.directory, &format!("{name}.rs"));
            let nested = join(&owner.directory, &format!("{name}/mod.rs"));
            child.file = match (self.cause(&direct, causes)?, self.cause(&nested, causes)?) {
                (true, false) => direct,
                (false, true) => nested,
                _ => return Ok(None),
            };
            child.inline = Arc::from([]);
        }
        Ok(Some(child))
    }

    /// Recover an actual containing module from its target root.
    fn address(
        &mut self,
        owner: &Module,
        logical: &[String],
        causes: &mut CauseWorkspace<'_>,
    ) -> Result<Option<Module>, CliError> {
        let mut module = Module {
            root: owner.root.clone(),
            file: owner.root.clone(),
            inline: Arc::from([]),
            logical: Vec::new(),
            directory: parent(&owner.root).to_string(),
            visibility: Visibility::Public,
        };
        for name in logical {
            let Some(child) = self.declared_module(&module, name, causes)? else {
                return Ok(None);
            };
            module = child;
        }
        Ok(Some(module))
    }

    /// Merge only candidates from the actual winning import-only lexical group.
    fn resolve_candidates(
        &mut self,
        source: &str,
        facts: &SourceSemanticFacts,
        call: &projectatlas_symbols::SourceRustCallImports,
    ) -> Result<Arc<CallBindingResolution>, CliError> {
        let key = (
            source.to_string(),
            Arc::as_ptr(&call.bindings),
            Arc::as_ptr(&call.declarations),
            call.member.clone(),
        );
        if let Some(result) = self.candidate_queries.get(&key) {
            return Ok(Arc::clone(result));
        }
        let result = self.with_scratch(|this, scratch| {
            let mut causes = CauseWorkspace::new(scratch);
            let mut result = CallBindingResolution {
                followed_export: true,
                ..CallBindingResolution::default()
            };
            let mut count = 0_u32;
            if call.member.is_none() && !call.declarations.is_empty() {
                let owner = this.shared.source(source)?;
                for ordinal in call.declarations.iter() {
                    this.shared.check_budget()?;
                    count += 1;
                    result.target = this.shared.target(&owner, *ordinal)?;
                }
            }
            for (import, reference) in call.bindings.iter() {
                this.shared.check_budget()?;
                let Some(import) = facts
                    .imports()
                    .get(*import)
                    .filter(|import| import.references.get(*reference).is_some())
                else {
                    count += 1;
                    continue;
                };
                let Some(binding) = import
                    .rust
                    .as_deref()
                    .filter(|binding| binding.supported && binding.role != UseRole::Wildcard)
                else {
                    count += 1;
                    continue;
                };
                let mut path = binding.path.clone();
                if let Some(member) = &call.member {
                    path.segments
                        .extend(member.split("::").map(ToString::to_string));
                }
                let outcome = this.resolve(Request {
                    source: source.to_string(),
                    module: Arc::clone(&binding.module),
                    path,
                    namespace: binding.role == UseRole::SelfNamespace && call.member.is_none(),
                    import: false,
                    toolchain: import.rust_toolchain.is_some(),
                })?;
                for key in &outcome.dependencies {
                    this.insert_cause(&mut causes, key)?;
                }
                if outcome.absent {
                    continue;
                }
                count += 1;
                result.target.clone_from(&outcome.target);
                result.ambiguous = result.ambiguous.max(outcome.ambiguous);
            }
            let source_key = document_file_resolution_key(this.shared.project, source)?;
            this.insert_cause(&mut causes, &source_key)?;
            if count > 1 {
                result.ambiguous = result.ambiguous.max(std::num::NonZeroU32::new(count));
                result.target = None;
            }
            result.absent = count == 0;
            result.dependencies = causes.keys;
            let retained_bytes = result
                .retained_bytes()
                .saturating_add(STAGED_GRAPH_ROW_BYTES * 2)
                .saturating_add(key.0.capacity() as u64)
                .saturating_add(key.3.as_ref().map_or(0, String::capacity) as u64);
            Ok((result, retained_bytes))
        })?;
        let result = Arc::new(result);
        self.candidate_queries.insert(key, Arc::clone(&result));
        Ok(result)
    }

    /// Resolve one complete call/import request and retain all failed-branch causes.
    fn resolve(&mut self, request: Request) -> Result<Arc<CallBindingResolution>, CliError> {
        if let Some(result) = self.queries.get(&request) {
            return Ok(Arc::clone(result));
        }
        self.ambiguous = 0;
        let membership = if request.toolchain {
            None
        } else {
            Some(self.membership(&request.source)?)
        };
        let result = self.with_scratch(|this, scratch| {
            let mut causes = CauseWorkspace::new(scratch);
            let mut result = CallBindingResolution {
                followed_export: true,
                absent: !request.toolchain && request.namespace && !request.import,
                ..CallBindingResolution::default()
            };
            if request.toolchain {
                result.target = Some(this.external(&request.path.segments.join("::"))?);
                let key = document_file_resolution_key(this.shared.project, &request.source)?;
                this.insert_cause(&mut causes, &key)?;
            }
            if let Some(membership) = &membership {
                for key in &membership.causes {
                    this.insert_cause(&mut causes, key)?;
                }
            }
            if let Some(owner) = membership
                .as_ref()
                .and_then(|membership| membership.owner.as_ref())
            {
                let mut logical = owner.logical.clone();
                logical.extend(request.module.iter().cloned());
                if let Some(module) = this.address(owner, &logical, &mut causes)? {
                    let mut visited = BTreeSet::new();
                    if request.import {
                        let namespace = this.path(
                            &module,
                            &request.path,
                            Namespace::Module,
                            &logical,
                            &mut causes,
                            &mut visited,
                            0,
                        )?;
                        let value = if request.namespace {
                            None
                        } else {
                            this.path(
                                &module,
                                &request.path,
                                Namespace::Value,
                                &logical,
                                &mut causes,
                                &mut visited,
                                0,
                            )?
                        };
                        result.target = this.import_target(namespace, value)?;
                    } else if !request.namespace {
                        match this.path(
                            &module,
                            &request.path,
                            Namespace::Value,
                            &logical,
                            &mut causes,
                            &mut visited,
                            0,
                        )? {
                            Some(Selection::Value(entity, _, _)) => result.target = Some(entity),
                            Some(Selection::Absent) => result.absent = true,
                            _ => {}
                        }
                    }
                }
            }
            if result.target.is_none() && this.ambiguous > 1 {
                result.ambiguous = std::num::NonZeroU32::new(this.ambiguous);
            }
            result.dependencies = causes.keys;
            let retained_bytes = result
                .retained_bytes()
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(request.source.capacity() as u64)
                .saturating_add(
                    (request.path.segments.capacity() * std::mem::size_of::<String>()) as u64,
                )
                .saturating_add(request.path.segments.iter().fold(0_u64, |bytes, part| {
                    bytes.saturating_add(part.capacity() as u64)
                }));
            Ok((result, retained_bytes))
        })?;
        let result = Arc::new(result);
        self.queries.insert(request, Arc::clone(&result));
        Ok(result)
    }

    /// Preserve external identities and exact dependency file owners independently.
    fn import_target(
        &mut self,
        namespace: Option<Selection>,
        value: Option<Selection>,
    ) -> Result<Option<GraphEntity>, CliError> {
        let target = |selection: Selection| -> Result<Option<GraphEntity>, CliError> {
            let file = match selection {
                Selection::Absent => return Ok(None),
                Selection::Module(module) | Selection::Type { owner: module, .. } => module.file,
                Selection::Value(entity, _, _) => match entity.selector() {
                    EntitySelector::External { .. } => return Ok(Some(entity)),
                    EntitySelector::Symbol { symbol } => symbol.file.as_str().to_string(),
                    _ => return Ok(None),
                },
            };
            Ok(Some(
                GraphEntity::new(
                    self.shared.project,
                    EntitySelector::File {
                        path: RepositoryFilePath::new(std::path::Path::new(&file))
                            .map_err(invalid_graph_contract)?,
                    },
                    self.shared.generation,
                )
                .map_err(invalid_graph_contract)?,
            ))
        };
        let namespace = namespace.map(target).transpose()?.flatten();
        let value = value.map(target).transpose()?.flatten();
        match (namespace, value) {
            (Some(left), Some(right)) if left.key() != right.key() => {
                self.ambiguous = self.ambiguous.max(2);
                Ok(None)
            }
            (Some(target), _) | (_, Some(target)) => Ok(Some(target)),
            (None, None) => Ok(None),
        }
    }

    /// Create a content-free toolchain identity only from source or target-edition proof.
    fn external(&self, identity: &str) -> Result<GraphEntity, CliError> {
        GraphEntity::new(
            self.shared.project,
            EntitySelector::External {
                external: ExternalSelector {
                    system: super::GraphIdentityText::new(super::super::RUST_TOOLCHAIN_SYSTEM)
                        .map_err(invalid_graph_contract)?,
                    identity: super::GraphIdentityText::new(identity)
                        .map_err(invalid_graph_contract)?,
                },
            },
            self.shared.generation,
        )
        .map_err(invalid_graph_contract)
    }

    /// Resolve an explicit anchor before walking namespace members.
    #[allow(clippy::too_many_arguments)]
    fn path(
        &mut self,
        owner: &Module,
        path: &SourceRustPath,
        namespace: Namespace,
        requester: &[String],
        causes: &mut CauseWorkspace<'_>,
        visited: &mut BTreeSet<(Module, Vec<String>, Namespace)>,
        depth: usize,
    ) -> Result<Option<Selection>, CliError> {
        self.shared.check_budget()?;
        if depth >= MAX_FORWARDING_DEPTH {
            return Ok(None);
        }
        let absolute_edition = if matches!(path.anchor, Anchor::Absolute | Anchor::Unqualified) {
            let manifest = self
                .shared
                .packages
                .package_owner(&owner.file)
                .map(|package| package.manifest.clone());
            if let Some(manifest) = manifest {
                self.cause(&manifest, causes)?;
                self.shared
                    .source(&manifest)?
                    .source
                    .semantics()
                    .and_then(SourceSemanticFacts::cargo)
                    .and_then(|targets| targets.edition)
            } else {
                None
            }
        } else {
            None
        };
        if path.anchor == Anchor::Absolute
            && absolute_edition.is_some_and(|edition| edition >= 2018)
        {
            if path
                .segments
                .first()
                .is_some_and(|root| matches!(root.as_str(), "std" | "core" | "alloc"))
            {
                return Ok(Some(Selection::Value(
                    self.external(&path.segments.join("::"))?,
                    Visibility::Public,
                    Vec::new(),
                )));
            }
            return Ok(None);
        }
        let logical = match path.anchor {
            Anchor::Crate => &[][..],
            Anchor::SelfModule => owner.logical.as_slice(),
            Anchor::Unqualified | Anchor::Absolute if absolute_edition == Some(2015) => &[][..],
            Anchor::Unqualified
                if absolute_edition.is_some_and(|edition| edition >= 2018)
                    || owner.logical.is_empty() =>
            {
                owner.logical.as_slice()
            }
            Anchor::Super(parents) if parents <= owner.logical.len() => {
                &owner.logical[..owner.logical.len() - parents]
            }
            Anchor::Unqualified | Anchor::Super(_) | Anchor::Absolute => return Ok(None),
        };
        let Some(module) = self.address(owner, logical, causes)? else {
            return Ok(None);
        };
        self.select(
            &module,
            &path.segments,
            namespace,
            requester,
            causes,
            visited,
            depth,
        )
    }

    /// Preserve duplicate slots and terminate cycles without narrowing partial proof.
    #[allow(clippy::too_many_arguments)]
    fn select(
        &mut self,
        owner: &Module,
        parts: &[String],
        namespace: Namespace,
        requester: &[String],
        causes: &mut CauseWorkspace<'_>,
        visited: &mut BTreeSet<(Module, Vec<String>, Namespace)>,
        depth: usize,
    ) -> Result<Option<Selection>, CliError> {
        self.shared.check_budget()?;
        if depth >= MAX_FORWARDING_DEPTH {
            return Ok(None);
        }
        // Reserve both owned query copies and their set node before cloning.
        let frame_bytes = STAGED_GRAPH_ROW_BYTES.saturating_mul(2).saturating_add(
            module_bytes(owner)
                .saturating_mul(2)
                .saturating_add(std::mem::size_of_val(parts) as u64 * 2)
                .saturating_add(parts.iter().fold(0_u64, |bytes, part| {
                    bytes.saturating_add(part.len() as u64 * 2)
                })),
        );
        self.shared.reserve_scratch(frame_bytes)?;
        let query = (owner.clone(), parts.to_vec(), namespace);
        if !visited.insert(query.clone()) {
            self.shared.release_scratch(frame_bytes)?;
            return Ok(None);
        }
        let result = self.select_inner(owner, parts, namespace, requester, causes, visited, depth);
        visited.remove(&query);
        self.shared.release_scratch(frame_bytes)?;
        result
    }

    /// One namespace step checks visibility and source-owned competing bindings.
    #[allow(clippy::too_many_arguments)]
    fn select_inner(
        &mut self,
        owner: &Module,
        parts: &[String],
        namespace: Namespace,
        requester: &[String],
        causes: &mut CauseWorkspace<'_>,
        visited: &mut BTreeSet<(Module, Vec<String>, Namespace)>,
        depth: usize,
    ) -> Result<Option<Selection>, CliError> {
        if !self.cause(&owner.file, causes)? {
            return Ok(None);
        }
        let Some(source) = self.source(&owner.file)? else {
            return Ok(None);
        };
        let Some((name, rest)) = parts.split_first() else {
            return Ok((namespace == Namespace::Module).then(|| Selection::Module(owner.clone())));
        };
        let wanted = if rest.is_empty() {
            namespace
        } else {
            Namespace::Module
        };
        let Some(facts) = source
            .source
            .semantics()
            .and_then(SourceSemanticFacts::rust)
        else {
            return Ok(None);
        };
        let slots = &self.slots[&owner.file];
        let values = slots
            .values
            .get(&owner.inline)
            .and_then(|slots| slots.get(name))
            .cloned()
            .unwrap_or_default();
        let modules = slots
            .modules
            .get(&owner.inline)
            .and_then(|slots| slots.get(name))
            .cloned()
            .unwrap_or_default();
        let mut count = 0_usize;
        let mut selected = None;
        let mut opposite = false;
        if wanted == Namespace::Module {
            for index in &modules {
                self.shared.control.check(IndexWorkStage::SymbolParsing)?;
                let module = &facts.modules[*index];
                count += 1;
                if modules.len() == 1 && visible(module.visibility, &owner.logical, requester) {
                    selected = self
                        .declared_module(owner, name, causes)?
                        .map(Selection::Module);
                }
            }
        } else {
            opposite = !modules.is_empty();
        }
        for index in values {
            self.shared.control.check(IndexWorkStage::SymbolParsing)?;
            let declaration = &facts.declarations[index];
            let candidate = match (&declaration.binding, wanted) {
                (
                    SourceRustBinding::TypeDeclaration(_) | SourceRustBinding::UnresolvedType,
                    Namespace::Value,
                )
                | (
                    SourceRustBinding::Declaration(_) | SourceRustBinding::Unresolved,
                    Namespace::Module,
                ) => {
                    opposite = true;
                    continue;
                }
                (SourceRustBinding::Use(binding), Namespace::Value)
                    if binding.role == UseRole::SelfNamespace =>
                {
                    opposite = true;
                    continue;
                }
                _ if !visible(declaration.visibility, &owner.logical, requester) => None,
                (SourceRustBinding::TypeDeclaration(_), Namespace::Module) => {
                    Some(Selection::Type {
                        owner: owner.clone(),
                        name: name.clone(),
                        visibility: declaration.visibility,
                    })
                }
                (SourceRustBinding::Declaration(index), Namespace::Value) => {
                    self.shared.target(&source, *index)?.map(|entity| {
                        Selection::Value(entity, declaration.visibility, owner.logical.clone())
                    })
                }
                (SourceRustBinding::Use(binding), _)
                    if binding.supported && binding.role != UseRole::Wildcard =>
                {
                    if let Some(identity) =
                        self.slots[&owner.file].toolchain.get(&Arc::as_ptr(binding))
                    {
                        Some(Selection::Value(
                            self.external(identity)?,
                            Visibility::Public,
                            Vec::new(),
                        ))
                    } else {
                        self.path(
                            owner,
                            &binding.path,
                            wanted,
                            &owner.logical,
                            causes,
                            visited,
                            depth + 1,
                        )?
                        .filter(|selection| selection_visible(selection, requester))
                    }
                }
                _ => None,
            };
            if matches!(candidate, Some(Selection::Absent)) {
                opposite = true;
                continue;
            }
            count += 1;
            selected = candidate;
        }
        if count != 1 {
            self.ambiguous = self.ambiguous.max(u32::try_from(count).map_err(|error| {
                CliError::InvalidInput(format!("Rust slot count exceeds graph range: {error}"))
            })?);
            return Ok((count == 0 && opposite).then_some(Selection::Absent));
        }
        if rest.is_empty() {
            return Ok(selected);
        }
        if matches!(selected, Some(Selection::Absent)) {
            return Ok(selected);
        }
        if let Some(Selection::Value(entity, _, _)) = &selected
            && let EntitySelector::External { external } = entity.selector()
        {
            let identity = format!("{}::{}", external.identity.as_str(), rest.join("::"));
            return Ok(Some(Selection::Value(
                self.external(&identity)?,
                Visibility::Public,
                Vec::new(),
            )));
        }
        if let Some(Selection::Type { owner, name, .. }) = &selected {
            if rest.len() != 1 || namespace != Namespace::Value {
                return Ok(None);
            }
            let Some(indices) = self.slots.get(&owner.file).and_then(|slots| {
                slots
                    .associated
                    .get(&(Arc::clone(&owner.inline), name.clone(), rest[0].clone()))
            }) else {
                return Ok(None);
            };
            if indices.len() != 1 {
                self.ambiguous =
                    self.ambiguous
                        .max(u32::try_from(indices.len()).map_err(|error| {
                            CliError::InvalidInput(format!(
                                "Rust method count exceeds graph range: {error}"
                            ))
                        })?);
                return Ok(None);
            }
            let ordinal = indices[0];
            let Some(source) = self.source(&owner.file)? else {
                return Ok(None);
            };
            let Some(method) = source
                .source
                .semantics()
                .and_then(SourceSemanticFacts::rust)
                .and_then(|facts| facts.associated.get(ordinal))
            else {
                return Ok(None);
            };
            if !method.supported || !visible(method.visibility, &owner.logical, requester) {
                return Ok(None);
            }
            return method.declaration.map_or_else(
                || Ok(None),
                |ordinal| {
                    self.shared.target(&source, ordinal).map(|entity| {
                        entity.map(|entity| {
                            Selection::Value(entity, method.visibility, owner.logical.clone())
                        })
                    })
                },
            );
        }
        let Some(Selection::Module(module)) = selected else {
            return Ok(None);
        };
        self.select(
            &module,
            rest,
            namespace,
            requester,
            causes,
            visited,
            depth + 1,
        )
    }
}

/// A public alias cannot expose an item whose own visibility excludes the caller.
fn selection_visible(selection: &Selection, requester: &[String]) -> bool {
    match selection {
        Selection::Absent => true,
        Selection::Value(_, visibility, owner) => visible(*visibility, owner, requester),
        Selection::Type {
            owner, visibility, ..
        } => visible(*visibility, &owner.logical, requester),
        Selection::Module(owner) => visible(
            owner.visibility,
            &owner.logical[..owner.logical.len().saturating_sub(1)],
            requester,
        ),
    }
}

/// Private values remain visible to descendants, including child root-function imports.
fn visible(visibility: Visibility, owner: &[String], requester: &[String]) -> bool {
    match visibility {
        Visibility::Public | Visibility::Crate => true,
        Visibility::Private => requester.starts_with(owner),
        Visibility::Unsupported => false,
    }
}

/// Borrow the directory of a canonical repository path.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Join canonical relative components without inventing a leading separator.
fn join(directory: &str, path: &str) -> String {
    if directory.is_empty() {
        path.to_string()
    } else {
        format!("{directory}/{path}")
    }
}

/// Admit manifest-relative paths only inside this repository.
fn repository_join(directory: &str, path: &str) -> Option<String> {
    let mut parts = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for component in std::path::Path::new(path).components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop()?;
            }
            std::path::Component::Normal(part) => parts.push(part.to_str()?),
        }
    }
    let path = parts.join("/");
    RepositoryFilePath::new(std::path::Path::new(&path))
        .ok()
        .map(|_| path)
}

/// Layout only narrows candidates; actual module declarations establish membership.
fn candidate_path(directory: &str, path: &str) -> Option<Vec<String>> {
    let relative = if directory.is_empty() {
        path
    } else {
        path.strip_prefix(directory)?.strip_prefix('/')?
    };
    let relative = relative.strip_suffix(".rs")?;
    let mut parts = relative
        .split('/')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if parts.last().is_some_and(|part| part == "mod") {
        parts.pop();
    }
    (parts.len() <= MAX_FORWARDING_DEPTH).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        PackageIndex, ProjectResolutionRegistry, SourceBindingCache, build_entity_projection,
    };
    use super::super::BindingResolver;
    use super::{Module, Namespace, RustResolver, Visibility};
    use projectatlas_core::IndexGeneration;
    use projectatlas_core::{IndexCancellation, IndexWorkControl, IndexWorkFailure};
    use projectatlas_db::AtlasStore;
    use projectatlas_symbols::ConfiguredModuleResolution;
    use std::collections::{BTreeMap, BTreeSet};
    use std::error::Error;
    use std::io;
    use std::sync::Arc;

    /// Report a failed boundary proof through the test's fallible result.
    fn require(condition: bool, message: &str) -> Result<(), super::CliError> {
        if !condition {
            return Err(super::CliError::InvalidInput(message.to_string()));
        }
        Ok(())
    }

    /// Preserve actual and expected values without panicking in a fallible test.
    fn require_eq<T: std::fmt::Debug + PartialEq>(
        actual: &T,
        expected: &T,
    ) -> Result<(), super::CliError> {
        if actual != expected {
            return Err(super::CliError::InvalidInput(format!(
                "actual {actual:?}, expected {expected:?}"
            )));
        }
        Ok(())
    }

    /// Use the real resolver and empty captured inventory with an isolated store.
    fn with_empty_resolver(
        check: impl FnOnce(&mut RustResolver<'_, '_, '_, '_>, &Module) -> Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let store = AtlasStore::open_for_project(&root.join("atlas.db"), root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("test project identity missing"))?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let generation = IndexGeneration::new(1);
        let graphs: &[&projectatlas_core::symbols::SymbolGraph] = &[];
        let packages = PackageIndex::from_graphs(graphs)?;
        let configured = ConfiguredModuleResolution::default();
        let entities =
            build_entity_projection(project, generation, &[], graphs, &packages, true, &control)?;
        let mut sources = SourceBindingCache::new(root, &[], None);
        let mut candidates = ProjectResolutionRegistry::default();
        let replaced = BTreeSet::new();
        let mut shared = BindingResolver {
            store: &store,
            project,
            generation,
            base_generation: None,
            graphs: BTreeMap::new(),
            entities: &entities,
            packages: &packages,
            configured: &configured,
            sources: &mut sources,
            candidates: &mut candidates,
            replaced: &replaced,
            loaded: None,
            context: BTreeMap::new(),
            hydrated: BTreeMap::new(),
            queries: BTreeMap::new(),
            retained_bytes: 128,
            retained_limit: 64 * 1_024,
            control: &control,
        };
        let mut resolver = RustResolver {
            shared: &mut shared,
            memberships: BTreeMap::new(),
            roots: BTreeMap::new(),
            slots: BTreeMap::new(),
            queries: BTreeMap::new(),
            ambiguous: 0,
            candidate_queries: BTreeMap::new(),
        };
        let module = Module {
            root: "src/lib.rs".to_string(),
            file: "missing.rs".to_string(),
            inline: Arc::from([]),
            logical: Vec::new(),
            directory: "src".to_string(),
            visibility: Visibility::Public,
        };
        check(&mut resolver, &module)
    }

    #[test]
    fn resolver_frames_refuse_before_erasing_retained_state() -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, module| {
            let before = resolver.shared.retained_bytes;
            resolver.with_scratch(|this, scratch| {
                let mut causes = super::CauseWorkspace::new(scratch);
                // The output cause is already owned: the refusal must be at frame admission.
                let key = super::document_file_resolution_key(this.shared.project, &module.file)?;
                this.insert_cause(&mut causes, &key)?;
                this.shared.retained_limit = this.shared.retained_staging_bytes() + 1;
                this.shared.check_budget()?;
                let before_frame = this.shared.retained_bytes;
                let mut visited = BTreeSet::new();
                let result = this.select(
                    module,
                    &["absent".to_string()],
                    Namespace::Value,
                    &[],
                    &mut causes,
                    &mut visited,
                    0,
                );
                require(
                    matches!(
                        result,
                        Err(super::CliError::IndexWork(
                            IndexWorkFailure::ResourceLimitExceeded { .. }
                        ))
                    ),
                    "resolver boundary proof failed",
                )?;
                require_eq(&(this.shared.retained_bytes), &(before_frame))?;
                require(visited.is_empty(), "resolver boundary proof failed")?;
                require_eq(&(causes.keys), &(BTreeSet::from([key])))?;
                Ok(((), 0))
            })?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            Ok(())
        })
    }

    #[test]
    fn resolver_frames_release_cycle_and_inner_error_ownership() -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, module| {
            let before = resolver.shared.retained_bytes;
            resolver.with_scratch(|this, scratch| {
                let parts = vec!["absent".to_string()];
                let ancestor = (module.clone(), parts.clone(), Namespace::Value);
                let mut visited = BTreeSet::from([ancestor.clone()]);
                let mut causes = super::CauseWorkspace::new(scratch);
                let result = this.select(
                    module,
                    &parts,
                    Namespace::Value,
                    &[],
                    &mut causes,
                    &mut visited,
                    0,
                )?;
                require(result.is_none(), "resolver boundary proof failed")?;
                require_eq(&(visited), &(BTreeSet::from([ancestor.clone()])))?;
                require_eq(&(this.shared.retained_bytes), &(before))?;
                let mut invalid = module.clone();
                invalid.file = "x".repeat(projectatlas_core::graph::MAX_GRAPH_IDENTITY_BYTES + 1);
                let result = this.select(
                    &invalid,
                    &parts,
                    Namespace::Value,
                    &[],
                    &mut causes,
                    &mut visited,
                    0,
                );
                require(
                    matches!(result, Err(super::CliError::InvalidInput(_))),
                    "resolver boundary proof failed",
                )?;
                require_eq(&(visited), &(BTreeSet::from([ancestor])))?;
                require_eq(&(this.shared.retained_bytes), &(before))?;
                Ok(((), 0))
            })?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            Ok(())
        })
    }

    #[test]
    fn cause_workspace_deduplicates_and_refuses_before_copying() -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let before = resolver.shared.retained_bytes;
            resolver.with_scratch(|this, scratch| {
                let mut causes = super::CauseWorkspace::new(scratch);
                let first = super::document_file_resolution_key(this.shared.project, "first.rs")?;
                let next = super::document_file_resolution_key(this.shared.project, "next.rs")?;
                this.insert_cause(&mut causes, &first)?;
                let retained = this.shared.retained_bytes;
                this.insert_cause(&mut causes, &first)?;
                require_eq(&(this.shared.retained_bytes), &(retained))?;
                require_eq(&(causes.keys.len()), &(1))?;
                this.shared.retained_limit = this.shared.retained_staging_bytes();
                this.shared.check_budget()?;
                let result = this.insert_cause(&mut causes, &next);
                require(
                    matches!(
                        result,
                        Err(super::CliError::IndexWork(
                            IndexWorkFailure::ResourceLimitExceeded { .. }
                        ))
                    ),
                    "resolver boundary proof failed",
                )?;
                require_eq(&(this.shared.retained_bytes), &(retained))?;
                require_eq(&(causes.keys), &(BTreeSet::from([first])))?;
                Ok(((), 0))
            })?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            Ok(())
        })
    }

    #[test]
    fn cause_workspace_bounds_rows_and_releases_on_cancellation() -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            resolver.shared.retained_limit =
                super::super::super::super::MAX_PUBLICATION_STAGING_BYTES;
            let before = resolver.shared.retained_bytes;
            resolver.with_scratch(|this, scratch| {
                let mut causes = super::CauseWorkspace::new(scratch);
                for index in 0..super::super::GraphLimits::MAX_ROWS {
                    let key = super::document_file_resolution_key(
                        this.shared.project,
                        &format!("file_{index}.rs"),
                    )?;
                    this.insert_cause(&mut causes, &key)?;
                }
                let retained = this.shared.retained_bytes;
                let excess = super::document_file_resolution_key(this.shared.project, "excess.rs")?;
                require(
                    this.insert_cause(&mut causes, &excess).is_err(),
                    "resolver boundary proof failed",
                )?;
                require_eq(
                    &(causes.keys.len()),
                    &(super::super::GraphLimits::MAX_ROWS as usize),
                )?;
                require_eq(&(this.shared.retained_bytes), &(retained))?;
                let duplicate =
                    super::document_file_resolution_key(this.shared.project, "file_0.rs")?;
                this.insert_cause(&mut causes, &duplicate)?;
                require_eq(&(this.shared.retained_bytes), &(retained))?;
                Ok(((), 0))
            })?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            let cancelled = resolver.with_scratch(|this, scratch| {
                let mut causes = super::CauseWorkspace::new(scratch);
                let key = super::document_file_resolution_key(this.shared.project, "cancel.rs")?;
                this.insert_cause(&mut causes, &key)?;
                this.shared.control.cancel();
                this.insert_cause(&mut causes, &key)?;
                Ok(((), 0))
            });
            require(
                matches!(
                    cancelled,
                    Err(super::CliError::IndexWork(
                        IndexWorkFailure::Cancelled { .. }
                    ))
                ),
                "resolver boundary proof failed",
            )?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            Ok(())
        })
    }

    #[test]
    fn root_workspace_refuses_growth_and_preserves_duplicate_certainty()
    -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let before = resolver.shared.retained_bytes;
            resolver.with_scratch(|this, scratch| {
                let mut roots = BTreeMap::new();
                this.insert_root(scratch, &mut roots, "src/lib.rs".to_string(), true)?;
                let retained = this.shared.retained_bytes;
                this.shared.retained_limit = this.shared.retained_staging_bytes();
                this.shared.check_budget()?;
                this.insert_root(scratch, &mut roots, "src/lib.rs".to_string(), false)?;
                require_eq(&this.shared.retained_bytes, &retained)?;
                require_eq(&roots, &BTreeMap::from([("src/lib.rs".to_string(), false)]))?;
                let refusal =
                    this.insert_root(scratch, &mut roots, "src/main.rs".to_string(), true);
                require(
                    matches!(
                        refusal,
                        Err(super::CliError::IndexWork(
                            IndexWorkFailure::ResourceLimitExceeded { .. }
                        ))
                    ),
                    "root growth was not refused",
                )?;
                require_eq(&this.shared.retained_bytes, &retained)?;
                require_eq(&roots.len(), &1)?;
                Ok(((), 0))
            })?;
            require_eq(&resolver.shared.retained_bytes, &before)?;
            Ok(())
        })
    }

    #[test]
    fn alias_workspace_refusal_cleans_frames_and_preserves_source_cache()
    -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let shared = &mut *resolver.shared;
            let query = super::super::BindingQuery {
                source: "main.js".to_string(),
                module: "./worker.js".into(),
                imported: Some("run".to_string()),
                member: None,
            };
            let graph = projectatlas_symbols::extract_symbol_graph(
                "main.js",
                Some("javascript"),
                "import {run} from './worker.js'; run();",
            );
            let keys = ["first.js", "second.js"]
                .into_iter()
                .map(|path| super::document_file_resolution_key(shared.project, path))
                .collect::<Result<Vec<_>, _>>()?;
            let source = Arc::new(super::BindingSource {
                source: super::SourceGraph::Persisted(graph),
                slots: BTreeMap::new(),
                modules: BTreeMap::from([(query.module.to_string(), keys.clone())]),
                parents: Vec::new(),
            });
            shared.reserve_scratch(super::STAGED_GRAPH_ROW_BYTES * 8)?;
            shared
                .context
                .insert(query.source.clone(), Arc::clone(&source));
            let before = shared.retained_bytes;
            shared.retained_limit = shared.retained_staging_bytes()
                + query.retained_bytes().saturating_mul(2)
                + super::STAGED_GRAPH_ROW_BYTES
                + keys[0].canonical_identity().len() as u64;
            shared.check_budget()?;
            let mut visited = BTreeMap::new();
            let result = shared.resolve(query.clone(), &mut visited, 0);
            require(
                matches!(
                    result,
                    Err(super::CliError::IndexWork(
                        IndexWorkFailure::ResourceLimitExceeded { .. }
                    ))
                ),
                "alias workspace growth was not refused",
            )?;
            require_eq(&shared.retained_bytes, &before)?;
            require(
                visited.is_empty() && shared.queries.is_empty(),
                "alias refusal retained a frame or failed cache entry",
            )?;
            require(
                Arc::ptr_eq(
                    shared
                        .context
                        .get(&query.source)
                        .ok_or_else(|| io::Error::other("source cache missing"))?,
                    &source,
                ),
                "alias refusal replaced captured context",
            )?;
            shared.retained_limit = 64 * 1_024;
            let outcome = shared.resolve(query.clone(), &mut visited, 0)?;
            let mut expected = keys.into_iter().collect::<BTreeSet<_>>();
            expected.insert(super::document_file_resolution_key(
                shared.project,
                &query.source,
            )?);
            require_eq(&outcome.dependencies, &expected)?;
            require(visited.is_empty(), "alias success retained a frame")?;
            let retained = shared.retained_bytes;
            let cached = shared.resolve(query, &mut visited, 0)?;
            require(
                Arc::ptr_eq(&outcome, &cached),
                "alias query did not reuse its completed outcome",
            )?;
            require_eq(&shared.retained_bytes, &retained)?;
            Ok(())
        })
    }

    #[test]
    fn alias_workspace_cycles_retain_only_completed_owners_and_errors_release_frames()
    -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let shared = &mut *resolver.shared;
            let query = super::super::BindingQuery {
                source: "self.js".to_string(),
                module: "./self.js".into(),
                imported: Some("run".to_string()),
                member: None,
            };
            let (_, graph, facts) =
                projectatlas_symbols::extract_symbol_graph_with_semantics_controlled(
                    "self.js",
                    Some("javascript"),
                    "export {run} from './self.js';",
                    shared.control,
                )?
                .into_parts();
            let slot = facts
                .exports()
                .iter()
                .position(|export| export.name == "run")
                .ok_or_else(|| io::Error::other("native reexport slot missing"))?;
            let key = super::document_file_resolution_key(shared.project, "self.js")?;
            let entity = super::GraphEntity::new(
                shared.project,
                super::EntitySelector::File {
                    path: projectatlas_core::graph::RepositoryFilePath::new(std::path::Path::new(
                        "self.js",
                    ))?,
                },
                shared.generation,
            )?;
            shared.candidates.insert_candidate(&key, &entity)?;
            let source = Arc::new(super::BindingSource {
                source: super::SourceGraph::Captured(Arc::new(
                    super::super::super::CapturedSourceGraph {
                        graph,
                        semantics: facts,
                        admission: super::super::super::GraphIdentityAdmission::default(),
                    },
                )),
                slots: BTreeMap::from([("run".to_string(), vec![slot])]),
                modules: BTreeMap::from([(query.module.to_string(), vec![key.clone()])]),
                parents: Vec::new(),
            });
            shared.reserve_scratch(super::STAGED_GRAPH_ROW_BYTES * 8)?;
            shared.context.insert(query.source.clone(), source);
            let before = shared.retained_bytes;
            let frame_query = super::super::BindingQuery {
                source: "unindexed.js".to_string(),
                ..query.clone()
            };
            shared.retained_limit = shared.retained_staging_bytes() + frame_query.retained_bytes();
            shared.check_budget()?;
            let mut visited = BTreeMap::new();
            let refusal = shared.resolve(frame_query, &mut visited, 0);
            require(
                matches!(
                    refusal,
                    Err(super::CliError::IndexWork(
                        IndexWorkFailure::ResourceLimitExceeded { .. }
                    ))
                ),
                "second query copy was not admitted before cloning",
            )?;
            require_eq(&shared.retained_bytes, &before)?;
            require(
                visited.is_empty() && shared.queries.is_empty(),
                "frame refusal retained owned state",
            )?;
            shared.retained_limit = 64 * 1_024;
            let outcome = shared.resolve(query.clone(), &mut visited, 0)?;
            require(
                outcome.followed_export && outcome.target.is_none(),
                "self reexport did not abstain",
            )?;
            require_eq(&outcome.dependencies, &BTreeSet::from([key]))?;
            require_eq(
                &shared.retained_bytes,
                &(before + outcome.retained_bytes() + query.retained_bytes()),
            )?;
            require(visited.is_empty(), "cycle retained an ancestor frame")?;
            let retained = shared.retained_bytes;
            let cached = shared.resolve(query, &mut visited, 0)?;
            require(
                Arc::ptr_eq(&outcome, &cached),
                "cycle did not reuse completed root outcome",
            )?;
            require_eq(&shared.retained_bytes, &retained)?;
            let missing = super::super::BindingQuery {
                source: "missing.js".to_string(),
                module: "./absent.js".into(),
                imported: Some("run".to_string()),
                member: None,
            };
            require(
                shared.resolve(missing, &mut visited, 0).is_err(),
                "missing source unexpectedly resolved",
            )?;
            require(visited.is_empty(), "alias error retained a frame")?;
            require_eq(&shared.retained_bytes, &retained)?;
            Ok(())
        })
    }

    #[test]
    fn loaded_lookup_keys_reserve_before_copy_and_reuse_completed_admission()
    -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let shared = &mut *resolver.shared;
            let key = super::document_file_resolution_key(shared.project, "worker.js")?;
            shared.loaded = Some(BTreeSet::new());
            let before = shared.retained_bytes;
            shared.retained_limit = shared.retained_staging_bytes() + 1;
            shared.check_budget()?;
            let refusal = shared.module_file(std::slice::from_ref(&key));
            require(
                matches!(
                    refusal,
                    Err(super::CliError::IndexWork(
                        IndexWorkFailure::ResourceLimitExceeded { .. }
                    ))
                ),
                "lookup copy was not refused before read",
            )?;
            require_eq(&shared.retained_bytes, &before)?;
            require(
                shared.loaded.as_ref().is_some_and(BTreeSet::is_empty),
                "refused lookup was marked loaded",
            )?;
            shared.retained_limit = 64 * 1_024;
            require_eq(&shared.module_file(std::slice::from_ref(&key))?, &None)?;
            require(
                shared
                    .loaded
                    .as_ref()
                    .is_some_and(|loaded| loaded.contains(&key)),
                "completed lookup was not retained",
            )?;
            let retained = shared.retained_bytes;
            shared.module_file(std::slice::from_ref(&key))?;
            require_eq(&shared.retained_bytes, &retained)?;
            Ok(())
        })
    }

    /// Seed real parsed import leaves and completed child outcomes for aggregate-only proof.
    fn cached_candidates(
        resolver: &mut RustResolver<'_, '_, '_, '_>,
    ) -> Result<
        (
            super::SourceSemanticFacts,
            projectatlas_symbols::SourceRustCallImports,
        ),
        Box<dyn Error>,
    > {
        const SOURCE: &str = "src/main.rs";
        let extracted = projectatlas_symbols::extract_symbol_graph_with_semantics_controlled(
            SOURCE,
            Some("rust"),
            "use crate::one::run; use crate::two::run; fn caller(){run();}",
            resolver.shared.control,
        )?;
        let (_, _, facts) = extracted.into_parts();
        require_eq(&(facts.imports().len()), &(2))?;
        for (index, import) in facts.imports().iter().enumerate() {
            let binding = import
                .rust
                .as_ref()
                .ok_or_else(|| io::Error::other("native import binding missing"))?;
            let request = super::Request {
                source: SOURCE.to_string(),
                module: Arc::clone(&binding.module),
                path: binding.path.clone(),
                namespace: false,
                import: false,
                toolchain: false,
            };
            let key = super::document_file_resolution_key(
                resolver.shared.project,
                &format!("dependency_{index}.rs"),
            )?;
            let outcome = Arc::new(super::CallBindingResolution {
                absent: true,
                dependencies: BTreeSet::from([key]),
                ..super::CallBindingResolution::default()
            });
            resolver.charge(
                outcome
                    .retained_bytes()
                    .saturating_add(super::STAGED_GRAPH_ROW_BYTES * 4),
            )?;
            resolver.queries.insert(request, outcome);
        }
        let call = projectatlas_symbols::SourceRustCallImports {
            relation: 0,
            bindings: Arc::from([(0, 0), (1, 0)]),
            declarations: Arc::from([]),
            member: None,
        };
        Ok((facts, call))
    }

    #[test]
    fn candidate_union_refusal_preserves_cached_children() -> Result<(), Box<dyn Error>> {
        with_empty_resolver(|resolver, _| {
            let (facts, call) = cached_candidates(resolver)?;
            let before = resolver.shared.retained_bytes;
            resolver.shared.retained_limit =
                resolver.shared.retained_staging_bytes() + super::STAGED_GRAPH_ROW_BYTES;
            resolver.shared.check_budget()?;
            let result = resolver.resolve_candidates("src/main.rs", &facts, &call);
            require(
                matches!(
                    result,
                    Err(super::CliError::IndexWork(
                        IndexWorkFailure::ResourceLimitExceeded { .. }
                    ))
                ),
                "resolver boundary proof failed",
            )?;
            require_eq(&(resolver.shared.retained_bytes), &(before))?;
            require_eq(&(resolver.queries.len()), &(call.bindings.len()))?;
            require(
                resolver.candidate_queries.is_empty(),
                "resolver boundary proof failed",
            )?;
            Ok(())
        })
    }

    #[test]
    fn candidate_union_transfers_unique_causes_once_and_reuses_cache() -> Result<(), Box<dyn Error>>
    {
        with_empty_resolver(|resolver, _| {
            let (facts, call) = cached_candidates(resolver)?;
            let result = resolver.resolve_candidates("src/main.rs", &facts, &call)?;
            require(result.absent, "resolver boundary proof failed")?;
            require(result.target.is_none(), "resolver boundary proof failed")?;
            let expected = ["src/main.rs", "dependency_0.rs", "dependency_1.rs"]
                .into_iter()
                .map(|path| super::document_file_resolution_key(resolver.shared.project, path))
                .collect::<Result<BTreeSet<_>, _>>()?;
            require_eq(&(result.dependencies), &(expected))?;
            let retained = resolver.shared.retained_bytes;
            let repeated = resolver.resolve_candidates("src/main.rs", &facts, &call)?;
            require(
                Arc::ptr_eq(&result, &repeated),
                "resolver boundary proof failed",
            )?;
            require_eq(&(resolver.shared.retained_bytes), &(retained))?;
            Ok(())
        })
    }
}
