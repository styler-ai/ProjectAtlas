//! Cargo-root and module ownership for Rust static bindings.
use super::{
    BindingResolver, BindingSource, CallBindingResolution, CallBindings, CanonicalResolutionKey,
    CliError, EntitySelector, GraphEntity, GraphLimits, IndexWorkStage, MAX_FORWARDING_DEPTH,
    MAX_INCREMENTAL_RESOLUTION_ITEMS, STAGED_GRAPH_ROW_BYTES, SourceCallBinding, SourceGraph,
    SourceSemanticFacts, document_file_resolution_key, invalid_graph_contract,
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
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
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
        let candidate_calls = facts
            .rust()
            .into_iter()
            .flat_map(|rust| &rust.candidate_calls)
            .map(|call| (call.relation, call))
            .collect::<BTreeMap<_, _>>();
        let candidate_index_bytes =
            (candidate_calls.len() as u64).saturating_mul(STAGED_GRAPH_ROW_BYTES);
        resolver.charge(candidate_index_bytes)?;
        for call in facts.calls() {
            resolver.shared.check_budget()?;
            if let Some(candidates) = candidate_calls.get(&call.relation) {
                let outcome = resolver.resolve_candidates(&raw.path, facts, candidates)?;
                bindings
                    .entry(raw.path.clone())
                    .or_default()
                    .insert(call.relation, outcome);
                resolver.charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
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
            bindings
                .entry(raw.path.clone())
                .or_default()
                .insert(call.relation, outcome);
            resolver.charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
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
            bindings
                .entry(raw.path.clone())
                .or_default()
                .insert(import.relation, outcome);
            resolver.charge(STAGED_GRAPH_ROW_BYTES.saturating_add(raw.path.len() as u64))?;
        }
        resolver.shared.retained_bytes = resolver
            .shared
            .retained_bytes
            .saturating_sub(candidate_index_bytes);
    }
    Ok(())
}

impl<'g> RustResolver<'_, '_, 'g, '_> {
    /// Account all additional caches under the operation's existing memory boundary.
    fn charge(&mut self, bytes: u64) -> Result<(), CliError> {
        self.shared.retained_bytes = self.shared.retained_bytes.saturating_add(bytes);
        self.shared.check_budget()
    }

    /// Missing paths remain invalidation causes, without attempting to read them.
    fn cause(
        &mut self,
        path: &str,
        causes: &mut BTreeSet<CanonicalResolutionKey>,
    ) -> Result<bool, CliError> {
        self.shared.check_budget()?;
        causes.insert(document_file_resolution_key(self.shared.project, path)?);
        if causes.len() > GraphLimits::MAX_ROWS as usize {
            return Err(resolution_key_limit_failure(causes.len()));
        }
        Ok(self.shared.sources.nodes.contains_key(path))
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
            let mut slots = Slots::default();
            let mut bytes = STAGED_GRAPH_ROW_BYTES.saturating_add(path.len() as u64);
            for (ordinal, declaration) in facts.declarations.iter().enumerate() {
                self.shared.control.check(IndexWorkStage::SymbolParsing)?;
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
                self.shared.control.check(IndexWorkStage::SymbolParsing)?;
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
                self.shared.control.check(IndexWorkStage::SymbolParsing)?;
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
                    self.shared.control.check(IndexWorkStage::SymbolParsing)?;
                    if let (Some(binding), Some(reference)) = (&import.rust, &import.rust_toolchain)
                        && let Some(identity) = super::super::rust_import_identity(reference)
                    {
                        bytes = bytes
                            .saturating_add(STAGED_GRAPH_ROW_BYTES * 4)
                            .saturating_add(identity.capacity() as u64);
                        slots.toolchain.insert(Arc::as_ptr(binding), identity);
                    }
                }
            }
            self.slots.insert(path.to_string(), slots);
            self.charge(bytes)?;
        }
        Ok(Some(source))
    }

    /// Gather roots from the owning manifest and admitted default target inventory.
    fn roots(
        &mut self,
        path: &str,
        causes: &mut BTreeSet<CanonicalResolutionKey>,
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
                    causes.insert(
                        cargo_target_inventory_key(
                            self.shared.project,
                            &package_name,
                            &manifest,
                            &target_directory,
                        )
                        .map_err(invalid_graph_contract)?,
                    );
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
        let override_bytes = (overrides.len() as u64).saturating_mul(STAGED_GRAPH_ROW_BYTES);
        self.charge(override_bytes)?;
        let mut roots = BTreeMap::<String, bool>::new();
        for target in &targets.targets {
            self.shared.control.check(IndexWorkStage::SymbolParsing)?;
            if let Some(path) = repository_join(&directory, &target.path) {
                roots.insert(path, true);
            }
        }
        for auto in &targets.automatic {
            self.shared.control.check(IndexWorkStage::SymbolParsing)?;
            if let Some(path) = auto.kind.conventional_root() {
                let overridden = auto.kind == TargetKind::Binary
                    && overrides.contains(&(auto.kind, package_name.as_str()));
                if !overridden {
                    roots
                        .entry(join(&directory, path))
                        .and_modify(|certain| *certain &= auto.certain)
                        .or_insert(auto.certain);
                }
            }
            let Some(relative) = auto.kind.automatic_directory() else {
                continue;
            };
            let prefix = format!("{}/", join(&directory, relative));
            for (path, _) in self.shared.sources.nodes.range(prefix.as_str()..) {
                self.shared.control.check(IndexWorkStage::SymbolParsing)?;
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
                roots
                    .entry((*path).to_string())
                    .and_modify(|certain| *certain &= auto.certain)
                    .or_insert(auto.certain);
                if roots.len() > MAX_INCREMENTAL_RESOLUTION_ITEMS as usize {
                    return Err(resolution_key_limit_failure(roots.len()));
                }
            }
        }
        let roots: Arc<[(String, bool)]> = roots.into_iter().collect::<Vec<_>>().into();
        self.charge(
            roots
                .iter()
                .fold(STAGED_GRAPH_ROW_BYTES, |bytes, (path, _)| {
                    bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES)
                        .saturating_add(path.capacity() as u64)
                }),
        )?;
        self.shared.retained_bytes = self.shared.retained_bytes.saturating_sub(override_bytes);
        self.roots.insert(manifest, Arc::clone(&roots));
        Ok(roots)
    }

    /// Prove membership through actual module declarations along a narrowed path.
    fn membership(&mut self, path: &str) -> Result<Arc<Membership>, CliError> {
        if let Some(result) = self.memberships.get(path) {
            return Ok(Arc::clone(result));
        }
        let mut result = Membership::default();
        let roots = self.roots(path, &mut result.causes)?;
        let mut owner = None;
        let mut count = 0;
        for (root, certain) in roots.iter() {
            self.shared.check_budget()?;
            let directory = parent(root).to_string();
            let Some(parts) = candidate_path(&directory, path) else {
                continue;
            };
            if !self.cause(root, &mut result.causes)? {
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
                    let Some(child) = self.declared_module(&module, &part, &mut result.causes)?
                    else {
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
            result.owner = owner;
        }
        let cause_bytes = result.causes.iter().fold(0_u64, |bytes, key| {
            bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(key.canonical_identity().len() as u64)
        });
        self.charge(
            STAGED_GRAPH_ROW_BYTES
                .saturating_add(path.len() as u64)
                .saturating_add(cause_bytes)
                .saturating_add(result.owner.as_ref().map_or(0, module_bytes)),
        )?;
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
        causes: &mut BTreeSet<CanonicalResolutionKey>,
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
        causes: &mut BTreeSet<CanonicalResolutionKey>,
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
        let mut result = CallBindingResolution {
            followed_export: true,
            ..CallBindingResolution::default()
        };
        let mut count = 0_u32;
        if call.member.is_none() && !call.declarations.is_empty() {
            let owner = self.shared.source(source)?;
            for ordinal in call.declarations.iter() {
                self.shared.check_budget()?;
                count += 1;
                result.target = self.shared.target(&owner, *ordinal)?;
            }
        }
        for (import, reference) in call.bindings.iter() {
            self.shared.check_budget()?;
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
            let outcome = self.resolve(Request {
                source: source.to_string(),
                module: Arc::clone(&binding.module),
                path,
                namespace: binding.role == UseRole::SelfNamespace && call.member.is_none(),
                import: false,
                toolchain: import.rust_toolchain.is_some(),
            })?;
            result
                .dependencies
                .extend(outcome.dependencies.iter().cloned());
            if outcome.absent {
                continue;
            }
            count += 1;
            result.target.clone_from(&outcome.target);
            result.ambiguous = result.ambiguous.max(outcome.ambiguous);
        }
        result
            .dependencies
            .insert(document_file_resolution_key(self.shared.project, source)?);
        if result.dependencies.len() > GraphLimits::MAX_ROWS as usize {
            return Err(resolution_key_limit_failure(result.dependencies.len()));
        }
        if count > 1 {
            result.ambiguous = result.ambiguous.max(std::num::NonZeroU32::new(count));
            result.target = None;
        }
        result.absent = count == 0;
        self.charge(
            result
                .retained_bytes()
                .saturating_add(STAGED_GRAPH_ROW_BYTES * 2)
                .saturating_add(key.0.capacity() as u64)
                .saturating_add(key.3.as_ref().map_or(0, String::capacity) as u64),
        )?;
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
        if request.toolchain {
            let result = Arc::new(CallBindingResolution {
                target: Some(self.external(&request.path.segments.join("::"))?),
                followed_export: true,
                dependencies: [document_file_resolution_key(
                    self.shared.project,
                    &request.source,
                )?]
                .into(),
                ..CallBindingResolution::default()
            });
            self.charge(
                result
                    .retained_bytes()
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(request.source.capacity() as u64),
            )?;
            self.queries.insert(request, Arc::clone(&result));
            return Ok(result);
        }
        let membership = self.membership(&request.source)?;
        let mut result = CallBindingResolution {
            followed_export: true,
            absent: request.namespace && !request.import,
            dependencies: membership.causes.clone(),
            ..CallBindingResolution::default()
        };
        if let Some(owner) = &membership.owner {
            let mut logical = owner.logical.clone();
            logical.extend(request.module.iter().cloned());
            if let Some(module) = self.address(owner, &logical, &mut result.dependencies)? {
                let mut visited = BTreeSet::new();
                if request.import {
                    let namespace = self.path(
                        &module,
                        &request.path,
                        Namespace::Module,
                        &logical,
                        &mut result.dependencies,
                        &mut visited,
                        0,
                    )?;
                    let value = if request.namespace {
                        None
                    } else {
                        self.path(
                            &module,
                            &request.path,
                            Namespace::Value,
                            &logical,
                            &mut result.dependencies,
                            &mut visited,
                            0,
                        )?
                    };
                    result.target = self.import_target(namespace, value)?;
                } else if !request.namespace {
                    match self.path(
                        &module,
                        &request.path,
                        Namespace::Value,
                        &logical,
                        &mut result.dependencies,
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
        if result.target.is_none() && self.ambiguous > 1 {
            result.ambiguous = std::num::NonZeroU32::new(self.ambiguous);
        }
        self.charge(
            result
                .retained_bytes()
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(request.source.len() as u64)
                .saturating_add(
                    request
                        .path
                        .segments
                        .iter()
                        .map(|part| part.capacity() as u64 + std::mem::size_of::<String>() as u64)
                        .sum::<u64>(),
                ),
        )?;
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
        causes: &mut BTreeSet<CanonicalResolutionKey>,
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
        causes: &mut BTreeSet<CanonicalResolutionKey>,
        visited: &mut BTreeSet<(Module, Vec<String>, Namespace)>,
        depth: usize,
    ) -> Result<Option<Selection>, CliError> {
        self.shared.check_budget()?;
        if depth >= MAX_FORWARDING_DEPTH {
            return Ok(None);
        }
        let query = (owner.clone(), parts.to_vec(), namespace);
        let frame_bytes = module_bytes(owner)
            .saturating_mul(2)
            .saturating_add((query.1.capacity() * std::mem::size_of::<String>()) as u64 * 2)
            .saturating_add(
                query
                    .1
                    .iter()
                    .map(|part| part.capacity() as u64 * 2)
                    .sum::<u64>(),
            );
        if !visited.insert(query.clone()) {
            return Ok(None);
        }
        let result =
            self.select_inner(owner, parts, namespace, requester, causes, visited, depth)?;
        visited.remove(&query);
        self.shared.retained_bytes = self.shared.retained_bytes.saturating_sub(frame_bytes);
        Ok(result)
    }

    /// One namespace step checks visibility and source-owned competing bindings.
    #[allow(clippy::too_many_arguments)]
    fn select_inner(
        &mut self,
        owner: &Module,
        parts: &[String],
        namespace: Namespace,
        requester: &[String],
        causes: &mut BTreeSet<CanonicalResolutionKey>,
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
