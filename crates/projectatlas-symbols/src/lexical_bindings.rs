//! Lexical call ownership collected from the already parsed native syntax tree.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use projectatlas_core::language::{TreeSitterGrammar, tree_sitter_grammar};
use projectatlas_core::symbols::{SymbolGraph, SymbolKind};
use tree_sitter::Node;

use crate::semantic::ecmascript_context;
use crate::semantic::embedded_source::EmbeddedScriptRole;
use crate::semantic::{EcmascriptNameContext, EcmascriptNameRole};
use crate::{SourceCallBinding, SourceSemanticFacts, declaration_kind, node_name};

mod container_scopes;
mod declaration_collisions;
mod native_identifiers;

/// One actual parsed program, borrowing its tree until binding has completed.
pub(super) struct SourceSyntaxRoot<'tree> {
    /// Native program with original source offsets.
    pub(super) node: Node<'tree>,
    /// Declared host role; absent for a standalone source file.
    pub(super) role: Option<EmbeddedScriptRole>,
}

/// Cached declaration admission cannot be reconstructed later from its spelling.
#[derive(Clone, Default)]
enum DeclarationBindingName {
    /// The actual declaration has not yet been visited.
    #[default]
    Unseen,
    /// One exact cooked name and its original lexical owner.
    Admitted {
        /// Source-valid cooked identifier value.
        name: String,
        /// Original lexical/hoisted binding environment.
        scope: usize,
    },
    /// Invalid or conflicting source context cannot grant target authority.
    Rejected,
}

/// Bound environments even when syntax produces no capped symbols or relations.
const MAX_LEXICAL_SCOPES: usize = 16_384;
/// Bound transitions in every lexical namespace, including pending writes.
const MAX_LEXICAL_BINDINGS: usize = 16_384;

/// Binding meaning, independent of a presentation name or a source line guess.
#[derive(Clone, Eq, PartialEq)]
enum BindingTarget {
    /// Ordinal of an actual emitted declaration.
    Declaration(usize),
    /// One immutable overload set shared across calls in the same environment.
    Declarations(Arc<[usize]>),
    /// An actual module declaration supplies a namespace in this source module.
    RustModule(Arc<[String]>),
    /// Hoisted module/import competitors need namespace target proof at publication.
    RustNamespaceLookup(Arc<[String]>),
    /// Hoisted function/import competitors need value target proof at publication.
    RustValueLookup(Arc<[String]>),
    /// One independently admitted canonical import leaf.
    Import {
        /// Import evidence ordinal.
        import: usize,
        /// Reference ordinal within that import.
        reference: usize,
    },
    /// Every tied candidate is an import; namespace ownership remains undecided.
    Imports {
        /// Actual winning-scope import/reference ordinals.
        bindings: Arc<[(usize, usize)]>,
        /// Actual declarations in that same scope, without unknown blockers.
        declarations: Arc<[usize]>,
    },
    /// Python global/nonlocal lookup redirects to its actual enclosing environment.
    PythonOwner(usize),
    /// A value whose runtime callable identity is unknown.
    Shadow,
    /// Competing binding categories do not establish one static meaning.
    Unresolved,
}

/// The three closed lexical namespaces share the same retention admission.
#[derive(Clone, Copy)]
enum BindingNamespace {
    /// Ordinary expression values and imported names.
    Value,
    /// Rust type/module and C++ constructor names.
    Namespace,
    /// Java implicit methods, separate from expression values.
    Method,
}

/// Python directives select an environment rather than declaring a local value.
#[derive(Clone, Copy, Eq, PartialEq)]
enum PythonDirective {
    /// The selected module environment, skipping enclosing function locals.
    Global,
    /// The nearest actual enclosing function binding, skipping class namespaces.
    Nonlocal,
    /// Conflicting directives cannot establish a valid binding owner.
    Invalid,
}

/// Declaration reservation and assignment act on different ECMAScript owners.
#[derive(Clone, Copy)]
enum EcmaPatternEffect {
    /// Hoist the name without assigning a value.
    Reserve,
    /// Link a value write at its actual evaluation boundary.
    Write(usize),
}

/// Language-owned type and construction environments without runtime inference.
#[derive(Clone, Copy, Default)]
enum NativeContainerScope {
    /// Lexical, callable or singleton-object environment without an instance boundary.
    #[default]
    Ordinary,
    /// Instance members require an outer receiver across inner-class boundaries.
    KotlinInstance {
        /// Constructor parameter view used by property expressions and init blocks.
        initialization: Option<usize>,
    },
    /// C# type members use a separate primary-parameter initialization view.
    CSharpType {
        /// Shared priority view, including partial parts before the primary declaration.
        initialization: usize,
    },
}

/// A binding's visibility point and its pre-initialization shadow policy.
struct Binding {
    /// First source offset at which this target is initialized.
    from: usize,
    /// Statically established meaning after initialization.
    target: BindingTarget,
}

/// A precomputed visibility timeline with constant-time reservation evidence.
#[derive(Default)]
struct NameBindings {
    /// Whether any declaration reserves this name before initialization.
    reserves_scope: bool,
    /// One grouped target for each ordered visibility transition.
    transitions: Vec<Binding>,
    /// Earliest transition sharing the final target; earlier suffixes are ambiguous.
    stable_tail: usize,
    /// A nested callable can write this captured binding at an unknown runtime phase.
    runtime_mutated: bool,
    /// A sole Python directive writer may read its prior value before its own write.
    python_writer: Option<usize>,
    /// An annotation declares a local but cannot be combined with a scope directive.
    python_annotated: bool,
}

impl NameBindings {
    /// Group tied visibility points once, not separately for every call.
    fn finish<E>(&mut self, check: &mut impl FnMut() -> Result<(), E>) -> Result<(), E> {
        check()?;
        self.transitions.sort_by_key(|binding| binding.from);
        let mut entries = std::mem::take(&mut self.transitions).into_iter().peekable();
        while let Some(mut binding) = entries.next() {
            check()?;
            if entries.peek().is_some_and(|next| next.from == binding.from) {
                let mut declarations = Vec::new();
                let mut imports = Vec::new();
                let mut count = 0;
                let mut shadow = false;
                let mut only_declarations = true;
                let mut target = binding.target;
                let mut rust_module = None;
                let mut only_rust_namespace = true;
                loop {
                    check()?;
                    count += 1;
                    match &target {
                        BindingTarget::RustModule(owner) => rust_module = Some(Arc::clone(owner)),
                        BindingTarget::Import { .. } => {}
                        _ => only_rust_namespace = false,
                    }
                    match target {
                        BindingTarget::Declaration(symbol) => declarations.push(symbol),
                        BindingTarget::Shadow => {
                            shadow = true;
                            only_declarations = false;
                        }
                        BindingTarget::Import { import, reference } => {
                            imports.push((import, reference));
                            only_declarations = false;
                        }
                        BindingTarget::Imports { .. }
                        | BindingTarget::RustModule(_)
                        | BindingTarget::RustNamespaceLookup(_)
                        | BindingTarget::RustValueLookup(_)
                        | BindingTarget::Declarations(_)
                        | BindingTarget::PythonOwner(_)
                        | BindingTarget::Unresolved => only_declarations = false,
                    }
                    if entries.peek().is_none_or(|next| next.from != binding.from) {
                        break;
                    }
                    let Some(next) = entries.next() else {
                        break;
                    };
                    target = next.target;
                }
                binding.target = if shadow {
                    BindingTarget::Shadow
                } else if only_rust_namespace && rust_module.is_some() {
                    rust_module.map_or(
                        BindingTarget::Unresolved,
                        BindingTarget::RustNamespaceLookup,
                    )
                } else if !imports.is_empty() && imports.len() + declarations.len() == count {
                    BindingTarget::Imports {
                        bindings: imports.into(),
                        declarations: declarations.into(),
                    }
                } else if only_declarations {
                    declarations.sort_unstable();
                    declarations.dedup();
                    if declarations.len() == 1 {
                        BindingTarget::Declaration(declarations[0])
                    } else {
                        BindingTarget::Declarations(declarations.into())
                    }
                } else {
                    BindingTarget::Unresolved
                };
            }
            self.transitions.push(binding);
        }
        self.stable_tail = self.transitions.len().saturating_sub(1);
        while self.stable_tail > 0 {
            check()?;
            if self.transitions[self.stable_tail - 1].target
                != self.transitions[self.stable_tail].target
            {
                break;
            }
            self.stable_tail -= 1;
        }
        check()
    }
}

/// Whether Rust items belong to an actual module body or a local environment.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum RustItemOwner {
    /// A function-local module has no supported crate-level module address.
    UnsupportedModule,
    /// File or inline module body.
    Module,
    /// Function, impl, block or another language's environment.
    #[default]
    Local,
}

/// Exact declaration ordinals, refining only row/name collisions by canonical metadata.
enum DeclarationOrdinals<'a> {
    /// Preserve the ordinary unique declaration fast path.
    Unique(usize),
    /// Parent/signature matches distinguish identities; duplicate metadata stays absent.
    Identities {
        /// Every original owner retained for conservative export ambiguity markers.
        ordinals: Vec<usize>,
        /// Exact parent and signature evidence, with duplicate identity blockers.
        metadata: BTreeMap<(Option<&'a str>, &'a str), Option<usize>>,
    },
}

impl<'a> DeclarationOrdinals<'a> {
    /// Borrow every original ordinal without narrowing an ambiguous export owner.
    fn as_slice(&self) -> &[usize] {
        match self {
            Self::Unique(ordinal) => std::slice::from_ref(ordinal),
            Self::Identities { ordinals, .. } => ordinals,
        }
    }

    /// Stable marker identity after the declaration index has finished construction.
    fn as_ptr(&self) -> *const usize {
        self.as_slice().as_ptr()
    }

    /// Add a graph row without allowing duplicate metadata to become exact.
    fn insert(&mut self, ordinal: usize, graph: &'a SymbolGraph) {
        let metadata_for = |index: usize| {
            let symbol = &graph.symbols[index];
            (symbol.parent.as_deref(), symbol.signature.as_str())
        };
        match self {
            Self::Unique(original) => {
                let mut identities = BTreeMap::from([(metadata_for(*original), Some(*original))]);
                identities
                    .entry(metadata_for(ordinal))
                    .and_modify(|existing| *existing = None)
                    .or_insert(Some(ordinal));
                *self = Self::Identities {
                    ordinals: vec![*original, ordinal],
                    metadata: identities,
                };
            }
            Self::Identities { ordinals, metadata } => {
                ordinals.push(ordinal);
                metadata
                    .entry(metadata_for(ordinal))
                    .and_modify(|existing| *existing = None)
                    .or_insert(Some(ordinal));
            }
        }
    }
}

/// Whether ECMAScript var declarations belong here or in an enclosing body.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum EcmascriptVarEnvironment {
    /// Blocks and parameters do not own function-body var declarations.
    #[default]
    Enclosing,
    /// Function, static-initializer and namespace bodies own their var declarations.
    Local,
}

/// A grammar-owned lexical environment, including parameter and block owners.
#[derive(Default)]
struct Scope {
    /// Lexical lookup parent, independent from class presentation ownership.
    parent: Option<usize>,
    /// Current physical body that reuses this logical container directly.
    container_body: Option<usize>,
    /// TypeScript exports shared with other parts; private lookup stays lexical.
    namespace_exports: Option<usize>,
    /// C++ implicitly makes its single anonymous child visible here.
    anonymous_namespace: Option<usize>,
    /// Source grammar context, separate from lexical lookup parentage.
    ecma_context: EcmascriptNameContext,
    /// File/region environment that owns this scope's host role.
    ecma_root: usize,
    /// Declared host role on root environments only.
    ecma_role: Option<EmbeddedScriptRole>,
    /// Whether lookup crosses a callable boundary; also Python's local owner.
    function: bool,
    /// ECMAScript body/static/namespace vars stay separate from parameter evaluation.
    ecma_var_environment: EcmascriptVarEnvironment,
    /// Whether Python function lookup must skip this class body.
    class: bool,
    /// Kotlin instance ownership and constructor-only lookup view.
    native_container: NativeContainerScope,
    /// Python iteration variables stay inside their comprehension environment.
    comprehension: bool,
    /// Earliest outer lookup phase for deferred code; zero keeps immediate order.
    enclosing_visibility_floor: usize,
    /// Exact enclosing callable declaration, independent of its display name.
    owner: Option<usize>,
    /// Names mapped to ordered visibility transitions.
    bindings: BTreeMap<String, NameBindings>,
    /// Native global/nonlocal declarations and their latest source boundary.
    python_directives: BTreeMap<String, (PythonDirective, usize)>,
    /// Earliest value read per name, for Python's use-before-directive early error.
    python_reads: BTreeMap<String, usize>,
    /// Java method names occupy a separate namespace from expression names.
    methods: BTreeMap<String, NameBindings>,
    /// Mandatory Java methods with no source declaration, indexed by fixed arity.
    java_implicit_methods: BTreeMap<String, usize>,
    /// Rust module names and C++ constructor types are separate from value names.
    namespaces: BTreeMap<String, NameBindings>,
    /// Actual Rust inline module path, independent from callable display parents.
    rust_module: Arc<[String]>,
    /// The scope is the file or an actual inline module declaration body.
    rust_item_owner: RustItemOwner,
}

/// Exact source occurrence awaiting lookup after all hoisted bindings are known.
struct CallSite {
    /// Captured call evidence ordinal.
    fact: usize,
    /// Innermost source environment.
    scope: usize,
    /// Actual enclosing callable declaration captured during the AST walk.
    owner: Option<usize>,
    /// Actual call source offset.
    offset: usize,
    /// Cooked identifier root of the callee expression.
    name: Option<String>,
    /// Explicit static member, excluding computed/dynamic property access.
    member: Option<String>,
    /// A Java simple method invocation is not a callable-value expression.
    method_name: bool,
    /// Source argument count distinguishes disjoint implicit/explicit Java overloads.
    argument_count: Option<usize>,
    /// Static path proof independent from the call's presentation text.
    qualified: Option<crate::ImportReference>,
    /// Exact callee or callback occurrence, rather than the enclosing invocation.
    span: crate::SourceByteSpan,
    /// Actual inline module containing this call's source reference.
    rust_module: Arc<[String]>,
}

/// One parse's bounded environment and ordinal indexes; discarded after binding.
struct LexicalCollector<'a> {
    /// Registry-selected native language.
    language: &'a str,
    /// Current visit's syntax profile, restored around recursive visits.
    ecma_context: EcmascriptNameContext,
    /// The current program's lexical owner, retained for export-slot decisions.
    ecma_root: usize,
    /// Captured source backing the existing syntax tree.
    content: &'a str,
    /// Final augmented graph whose ordinals are being proved.
    graph: &'a SymbolGraph,
    /// Immutable source occurrences while collecting lexical decisions.
    facts: &'a SourceSemanticFacts,
    /// Source-owned environment tree, addressed by ordinal.
    scopes: Vec<Scope>,
    /// Exact same-file identities, bounded by their admitted logical environments.
    container_scopes: BTreeMap<container_scopes::ContainerScopeKey, usize>,
    /// Only an explicit namespace export redirects its owning block's bindings.
    namespace_export_binding: Option<(usize, usize)>,
    /// Admitted transitions, pending writes and retained container identity components.
    binding_count: usize,
    /// Only names actually declared by Python global/nonlocal need read-order evidence.
    python_directive_names: BTreeSet<String>,
    /// Exhaustion suppresses all binding authority, including earlier decisions.
    incomplete: bool,
    /// ECMAScript writes linked to their actual lexical owner after declarations exist.
    mutations: Vec<(usize, String, usize)>,
    /// Actual import extents mapped to independent import evidence.
    imports: BTreeMap<(usize, usize), Vec<usize>>,
    /// Actual call extents mapped to admitted call evidence.
    calls: BTreeMap<(usize, usize), Vec<usize>>,
    /// Exact emitted declaration identities; collisions stay ambiguous.
    declarations: BTreeMap<(usize, usize, &'a str, &'a str), DeclarationOrdinals<'a>>,
    /// At most one bounded name admission per emitted ECMAScript declaration.
    declaration_names: Vec<DeclarationBindingName>,
    /// Logical lexical owner captured before entering each declaration body.
    declaration_scopes: Vec<Option<usize>>,
    /// Admitted calls awaiting visibility lookup after collection.
    sites: Vec<CallSite>,
    /// Explicit export slots collected from the same native tree.
    exports: Vec<crate::SourceExportFact>,
    /// Lexical owner corresponding to each retained export occurrence.
    export_scopes: Vec<usize>,
    /// Exhausted export capacity invalidates all target proof, including earlier slots.
    exports_incomplete: bool,
    /// Local export clauses linked after the complete module environment is known.
    local_exports: Vec<(usize, String)>,
    /// Ambiguous declaration groups already represented by one export marker.
    export_ambiguities: BTreeSet<*const usize>,
    /// Shared raw declaration groups whose export role must block metadata fallback.
    export_declaration_groups: BTreeMap<*const usize, Arc<[usize]>>,
    /// Actual visibility points of C++ free function declarations.
    cpp_function_visibility: BTreeMap<usize, usize>,
    /// Shared overload groups' bounds, computed once rather than for each call.
    cpp_group_visibility: BTreeMap<*const usize, (usize, usize)>,
    /// Rust module/value ownership collected in this same AST walk.
    rust: Option<crate::SourceRustFacts>,
    /// Lexical declaration environment for each Rust import occurrence.
    rust_import_scopes: Vec<Option<usize>>,
    /// Imports whose actual source root proves toolchain ownership.
    toolchain_imports: BTreeSet<(usize, usize)>,
}

/// Resolve captured calls without another parse or a graph-wide scan per call.
pub(super) fn bind_calls<E>(
    roots: &[SourceSyntaxRoot<'_>],
    content: &str,
    graph: &SymbolGraph,
    facts: &mut SourceSemanticFacts,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let Some(grammar) = graph.language.as_deref().and_then(tree_sitter_grammar) else {
        return Ok(());
    };
    // PHP owns namespace rules separately. Optional/fallback parsers do not gain
    // cross-file providers or invented native binding proof here.
    let language = match grammar {
        TreeSitterGrammar::Rust => "rust",
        TreeSitterGrammar::Python => "python",
        TreeSitterGrammar::JavaScript => "javascript",
        TreeSitterGrammar::TypeScript => "typescript",
        TreeSitterGrammar::Tsx => "tsx",
        TreeSitterGrammar::Java => "java",
        TreeSitterGrammar::Kotlin => "kotlin",
        TreeSitterGrammar::CSharp => "csharp",
        TreeSitterGrammar::Go => "go",
        TreeSitterGrammar::ObjectiveC => "objective-c",
        TreeSitterGrammar::Zig => "zig",
        TreeSitterGrammar::C => "c",
        TreeSitterGrammar::Cpp => "cpp",
        TreeSitterGrammar::Php => return Ok(()),
    };
    let bindings_incomplete = facts.bindings_incomplete;
    let mut collector = LexicalCollector {
        language,
        ecma_context: EcmascriptNameContext::default(),
        ecma_root: 0,
        content,
        graph,
        facts,
        scopes: vec![Scope {
            rust_item_owner: if language == "rust" {
                RustItemOwner::Module
            } else {
                RustItemOwner::Local
            },
            ..Scope::default()
        }],
        binding_count: 0,
        container_scopes: BTreeMap::new(),
        namespace_export_binding: None,
        python_directive_names: BTreeSet::new(),
        incomplete: false,
        mutations: Vec::new(),
        imports: BTreeMap::new(),
        calls: BTreeMap::new(),
        declarations: BTreeMap::new(),
        declaration_names: if matches!(language, "javascript" | "typescript" | "tsx") {
            vec![DeclarationBindingName::Unseen; graph.symbols.len()]
        } else {
            Vec::new()
        },
        declaration_scopes: vec![None; graph.symbols.len()],
        sites: Vec::new(),
        exports: Vec::new(),
        export_scopes: Vec::new(),
        exports_incomplete: false,
        local_exports: Vec::new(),
        export_ambiguities: BTreeSet::new(),
        export_declaration_groups: BTreeMap::new(),
        cpp_function_visibility: BTreeMap::new(),
        cpp_group_visibility: BTreeMap::new(),
        rust: (language == "rust").then(|| crate::SourceRustFacts {
            complete: !bindings_incomplete,
            ..crate::SourceRustFacts::default()
        }),
        rust_import_scopes: if language == "rust" {
            vec![None; facts.imports.len()]
        } else {
            Vec::new()
        },
        toolchain_imports: BTreeSet::new(),
    };
    for (index, fact) in facts.imports.iter().enumerate() {
        check()?;
        collector
            .imports
            .entry((fact.span.start, fact.span.end))
            .or_default()
            .push(index);
    }
    for (index, fact) in facts.calls.iter().enumerate() {
        check()?;
        collector
            .calls
            .entry((fact.span.start, fact.span.end))
            .or_default()
            .push(index);
    }
    for (index, symbol) in graph.symbols.iter().enumerate() {
        check()?;
        collector
            .declarations
            .entry((
                symbol.line_start,
                symbol.line_end,
                symbol.name.as_str(),
                symbol.detail.as_deref().unwrap_or_default(),
            ))
            .and_modify(|ordinals| ordinals.insert(index, graph))
            .or_insert(DeclarationOrdinals::Unique(index));
    }
    for root in roots {
        check()?;
        collector.collect_python_directive_names(root.node, check)?;
        let scope = collector.source_root_scope(root, check)?;
        collector.ecma_root = scope;
        collector.visit(root.node, scope, check)?;
    }
    collector.link_python_directives(check)?;
    collector.apply_mutations(check)?;
    if collector.incomplete {
        if let Some(rust) = &mut collector.rust {
            rust.complete = false;
        }
        let rust = collector.rust.take();
        let sites = std::mem::take(&mut collector.sites);
        drop(collector);
        facts.bindings_incomplete = true;
        facts.exports_incomplete = true;
        facts.exports.clear();
        facts.rust = rust;
        for call in &mut facts.calls {
            check()?;
            call.binding = SourceCallBinding::Unresolved;
        }
        for site in sites {
            check()?;
            facts.calls[site.fact].owner = site.owner;
            facts.calls[site.fact].span = site.span;
        }
        return Ok(());
    }
    let mut unsupported_imports = Vec::new();
    for scope in &mut collector.scopes {
        check()?;
        for bindings in scope
            .bindings
            .values_mut()
            .chain(scope.methods.values_mut())
            .chain(scope.namespaces.values_mut())
        {
            bindings.finish(check)?;
        }
    }
    if language == "rust" {
        let mut mixed = BTreeMap::<(Arc<[String]>, String), (usize, bool)>::new();
        if let Some(rust) = &collector.rust {
            for declaration in &rust.declarations {
                check()?;
                let kinds = mixed
                    .entry((Arc::clone(&declaration.module), declaration.name.clone()))
                    .or_default();
                match declaration.binding {
                    crate::SourceRustBinding::Use(_) => kinds.0 += 1,
                    crate::SourceRustBinding::Declaration(_)
                    | crate::SourceRustBinding::TypeDeclaration(_) => kinds.1 = true,
                    _ => {}
                }
            }
        }
        for scope in &mut collector.scopes {
            if scope.rust_item_owner != RustItemOwner::Module {
                continue;
            }
            for (namespace, names) in [(false, &mut scope.bindings), (true, &mut scope.namespaces)]
            {
                for (name, bindings) in names {
                    check()?;
                    if mixed
                        .get(&(Arc::clone(&scope.rust_module), name.clone()))
                        .is_some_and(|(imports, declaration)| {
                            *imports > 1 || *imports > 0 && *declaration
                        })
                    {
                        for binding in &mut bindings.transitions {
                            if matches!(binding.target, BindingTarget::Unresolved) {
                                binding.target = if namespace {
                                    BindingTarget::RustNamespaceLookup(Arc::clone(
                                        &scope.rust_module,
                                    ))
                                } else {
                                    BindingTarget::RustValueLookup(Arc::clone(&scope.rust_module))
                                };
                            }
                        }
                    }
                }
            }
        }
        for import in 0..collector.facts.imports.len() {
            if !collector.unqualified_import_has_module_owner(import) {
                unsupported_imports.push(import);
            }
        }
        for (import, fact) in collector.facts.imports.iter().enumerate() {
            for reference in 0..fact.references.len() {
                if collector.toolchain_import_is_unshadowed(
                    import,
                    reference,
                    &mut BTreeSet::new(),
                    check,
                )? {
                    collector.toolchain_imports.insert((import, reference));
                }
            }
        }
    }
    collector.link_local_exports(check)?;
    if collector.language == "cpp" {
        for scope in &collector.scopes {
            for bindings in scope.bindings.values() {
                for binding in &bindings.transitions {
                    check()?;
                    if let BindingTarget::Declarations(symbols) = &binding.target {
                        let mut bounds: Option<(usize, usize)> = None;
                        for symbol in symbols.iter() {
                            check()?;
                            if let Some(from) = collector.cpp_function_visibility.get(symbol) {
                                bounds = Some(bounds.map_or((*from, *from), |(first, last)| {
                                    (first.min(*from), last.max(*from))
                                }));
                            }
                        }
                        if let Some(bounds) = bounds {
                            collector
                                .cpp_group_visibility
                                .insert(symbols.as_ptr(), bounds);
                        }
                    }
                }
            }
        }
    }
    let collisions = collector.declaration_identity_collisions(check)?;
    if collisions.iter().any(|collision| *collision) {
        declaration_collisions::guard_scope_bindings(&mut collector.scopes, &collisions, check)?;
    }
    let mut decisions = Vec::with_capacity(collector.sites.len());
    for site in &collector.sites {
        check()?;
        decisions.push((
            site.fact,
            site.owner,
            site.span,
            if site.owner.is_some_and(|owner| collisions[owner]) {
                SourceCallBinding::Unresolved
            } else {
                collector.resolve(site, check)?
            },
        ));
    }
    let mut candidate_calls = Vec::new();
    if language == "rust"
        && !bindings_incomplete
        && collector.rust.as_ref().is_some_and(|rust| rust.complete)
    {
        for site in &collector.sites {
            check()?;
            if site.owner.is_some_and(|owner| collisions[owner]) {
                continue;
            }
            if let Some(bindings) = collector.rust_import_candidates(site, check)? {
                // Colliding targets were withdrawn as whole winning groups
                // before either ordinary or import-candidate lookup.
                candidate_calls.push(bindings);
            }
        }
    }
    if let Some(rust) = &mut collector.rust {
        rust.candidate_calls = candidate_calls;
        declaration_collisions::guard_rust_declarations(rust, &collisions, check)?;
    }
    declaration_collisions::guard_exports(&mut collector.exports, &collisions, check)?;
    let exports = std::mem::take(&mut collector.exports);
    let exports_incomplete = collector.exports_incomplete;
    let rust = collector.rust.take();
    let toolchain_imports = std::mem::take(&mut collector.toolchain_imports);
    drop(collector);
    facts.exports_incomplete = exports_incomplete;
    facts.exports = exports;
    let call_evidence_complete =
        !bindings_incomplete && rust.as_ref().is_none_or(|rust| rust.complete);
    facts.rust = rust;
    for import in &unsupported_imports {
        if let Some(binding) = &mut facts.imports[*import].rust {
            Arc::make_mut(binding).supported = false;
        }
    }
    for (import, reference) in toolchain_imports {
        if !call_evidence_complete || unsupported_imports.binary_search(&import).is_ok() {
            continue;
        }
        facts.imports[import].rust_toolchain = Some(Box::new(
            facts.imports[import].references[reference].clone(),
        ));
    }
    for (fact, owner, span, binding) in decisions {
        facts.calls[fact].owner = owner;
        facts.calls[fact].span = span;
        let import_supported = !matches!(&binding, SourceCallBinding::Imported {import, ..} if unsupported_imports.binary_search(import).is_ok());
        facts.calls[fact].binding = if call_evidence_complete && import_supported {
            binding
        } else {
            SourceCallBinding::Unresolved
        };
    }
    Ok(())
}

impl LexicalCollector<'_> {
    /// Allocate only the lexical environment established by this source program.
    fn source_root_scope<E>(
        &mut self,
        root: &SourceSyntaxRoot<'_>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<usize, E> {
        if !self.ecmascript() {
            return Ok(0);
        }
        let context = ecmascript_context::source_context(
            root.node,
            &self.graph.path,
            root.role,
            self.content,
            check,
        )?;
        let separate = matches!(
            root.role,
            Some(
                EmbeddedScriptRole::HtmlModule
                    | EmbeddedScriptRole::VueSetup
                    | EmbeddedScriptRole::SvelteInstance
            )
        );
        if !separate {
            self.scopes[0].ecma_context = context;
            self.scopes[0].ecma_role = root.role;
            return Ok(0);
        }
        if !self.admit_scope() {
            return Ok(0);
        }
        if matches!(
            root.role,
            Some(EmbeddedScriptRole::VueSetup | EmbeddedScriptRole::SvelteInstance)
        ) {
            self.scopes[0].ecma_context = EcmascriptNameContext::module();
        }
        let scope = self.scopes.len();
        let instance = root.role != Some(EmbeddedScriptRole::HtmlModule);
        self.scopes.push(Scope {
            parent: Some(0),
            ecma_context: context,
            ecma_root: scope,
            ecma_role: root.role,
            ecma_var_environment: EcmascriptVarEnvironment::Local,
            function: instance,
            enclosing_visibility_floor: if instance { root.node.end_byte() } else { 0 },
            ..Scope::default()
        });
        Ok(scope)
    }

    /// Retain the original declaration's admitted name before entering its body.
    fn admit_declaration_name<E>(
        &mut self,
        node: Node<'_>,
        symbol: usize,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let mut context = self.ecma_context;
        if ecmascript_context::is_class(node) {
            context = context.with_strict(true);
        }
        if ecmascript_context::is_callable(node) {
            context =
                ecmascript_context::callable_contexts(node, context, self.content, check)?.name;
        }
        let invalid_initializer = if crate::declaration_has_direct_callable_initializer(node)
            && let Some(initializer) = crate::first_declaration_initializer(node)
            && ecmascript_context::is_callable(initializer)
            && let Some(name) = initializer.child_by_field_name("name")
        {
            let profile = ecmascript_context::callable_contexts(
                initializer,
                self.ecma_context,
                self.content,
                check,
            )?;
            self.content.get(name.byte_range()).is_none_or(|name| {
                crate::semantic::ecmascript_contextual_name(
                    name,
                    profile.name,
                    EcmascriptNameRole::Binding,
                )
                .is_none()
            })
        } else {
            false
        };
        let scope = if node.kind() == "variable_declaration" {
            self.value_declaration_scope(scope)
        } else {
            scope
        };
        let role = if node.kind() == "lexical_declaration"
            || node
                .parent()
                .is_some_and(|parent| parent.kind() == "lexical_declaration")
        {
            EcmascriptNameRole::LexicalBinding
        } else {
            EcmascriptNameRole::Binding
        };
        let name = crate::semantic::ecmascript_contextual_name(
            &self.graph.symbols[symbol].name,
            context,
            role,
        );
        let next = match name {
            Some(name) if !invalid_initializer => DeclarationBindingName::Admitted { name, scope },
            Some(_) | None => DeclarationBindingName::Rejected,
        };
        let existing = &mut self.declaration_names[symbol];
        match (&*existing, &next) {
            (DeclarationBindingName::Unseen, _) => *existing = next,
            (
                DeclarationBindingName::Admitted { name, scope },
                DeclarationBindingName::Admitted {
                    name: other_name,
                    scope: other_scope,
                },
            ) if name == other_name && scope == other_scope => {}
            _ => *existing = DeclarationBindingName::Rejected,
        }
        Ok(())
    }

    /// Derive syntax parameters at the source boundary that actually owns them.
    fn child_ecma_context<E>(
        &self,
        node: Node<'_>,
        function: bool,
        class: bool,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<EcmascriptNameContext, E> {
        let mut context = self.ecma_context;
        if !self.ecmascript() {
            return Ok(context);
        }
        if class {
            context = context.with_strict(true);
        }
        if function && ecmascript_context::is_callable(node) {
            context =
                ecmascript_context::callable_contexts(node, context, self.content, check)?.body;
        }
        if node.kind() == "class_static_block" {
            context = context.with_strict(true);
            context.await_keyword = true;
            context.yield_keyword = false;
            context.arguments_forbidden = true;
        }
        Ok(context)
    }

    /// Select directive names once from the captured AST, never from literal matches alone.
    fn collect_python_directive_names<E>(
        &mut self,
        root: Node<'_>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.language != "python"
            || !(self.content.contains("global") || self.content.contains("nonlocal"))
        {
            return Ok(());
        }
        let mut cursor = root.walk();
        loop {
            check()?;
            let node = cursor.node();
            if matches!(node.kind(), "global_statement" | "nonlocal_statement") {
                let mut owner = node.parent();
                while let Some(parent) = owner {
                    check()?;
                    if matches!(parent.kind(), "function_definition" | "class_definition") {
                        break;
                    }
                    owner = parent.parent();
                }
                // Module-global declarations are no-ops and need no local read state.
                if owner.is_some() {
                    let mut names = node.walk();
                    for child in node.named_children(&mut names) {
                        check()?;
                        if let Some(name) = self.identifier(child)
                            && !self.python_directive_names.contains(&name)
                        {
                            if !self.admit_binding() {
                                return Ok(());
                            }
                            self.python_directive_names.insert(name);
                        }
                    }
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Ok(());
                }
            }
        }
    }

    /// Link directives after all enclosing declarations exist, without another AST walk.
    fn link_python_directives<E>(
        &mut self,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.language != "python" {
            return Ok(());
        }
        for scope in 1..self.scopes.len() {
            for (name, (directive, at)) in std::mem::take(&mut self.scopes[scope].python_directives)
            {
                check()?;
                if self.incomplete {
                    return Ok(());
                }
                let mut owner = None;
                if directive == PythonDirective::Global {
                    owner = Some(0);
                } else if directive == PythonDirective::Nonlocal {
                    let mut parent = self.scopes[scope].parent;
                    while let Some(enclosing) = parent {
                        check()?;
                        if self.scopes[enclosing].function
                            && let Some(bindings) = self.scopes[enclosing].bindings.get(&name)
                        {
                            // A global declaration blocks nonlocal lookup through this function.
                            match bindings.transitions.first().map(|binding| &binding.target) {
                                Some(BindingTarget::PythonOwner(0)) => break,
                                Some(BindingTarget::PythonOwner(target)) => {
                                    owner = Some(*target);
                                    break;
                                }
                                Some(_) => {
                                    owner = Some(enclosing);
                                    break;
                                }
                                None => {}
                            }
                        }
                        parent = self.scopes[enclosing].parent;
                    }
                }
                let names = &mut self.scopes[scope].bindings;
                let writes = names
                    .get(&name)
                    .is_some_and(|bindings| !bindings.transitions.is_empty());
                let invalid = names.get(&name).is_some_and(|bindings| {
                    bindings.python_annotated
                        || bindings.transitions.iter().any(|binding| binding.from < at)
                }) || self.scopes[scope]
                    .python_reads
                    .get(&name)
                    .is_some_and(|read| *read < at);
                let names = &mut self.scopes[scope].bindings;
                if let Some(bindings) = names.get_mut(&name) {
                    bindings.reserves_scope = false;
                    if invalid || owner.is_none() {
                        for binding in &mut bindings.transitions {
                            binding.target = BindingTarget::Unresolved;
                        }
                    }
                }
                let target = if invalid {
                    BindingTarget::Unresolved
                } else {
                    owner.map_or(BindingTarget::Unresolved, BindingTarget::PythonOwner)
                };
                // Insert at the front before finishing, so child directives see the owner.
                if !self.admit_binding() {
                    return Ok(());
                }
                self.scopes[scope]
                    .bindings
                    .entry(name.clone())
                    .or_default()
                    .transitions
                    .insert(0, Binding { from: 0, target });
                if !invalid
                    && writes
                    && let Some(owner) = owner
                {
                    if !self.scopes[owner].bindings.contains_key(&name) && !self.admit_binding() {
                        return Ok(());
                    }
                    let binding = self.scopes[owner].bindings.entry(name).or_default();
                    binding.python_writer = if binding.runtime_mutated {
                        binding.python_writer.filter(|writer| *writer == scope)
                    } else {
                        Some(scope)
                    };
                    binding.runtime_mutated = true;
                }
            }
        }
        Ok(())
    }

    /// Charge retained lexical names and pending writes before inserting them.
    fn admit_binding(&mut self) -> bool {
        if self.incomplete || self.binding_count >= MAX_LEXICAL_BINDINGS {
            self.incomplete = true;
            return false;
        }
        self.binding_count += 1;
        true
    }

    /// Resolve writes without inventing function-local declarations for captured variables.
    fn apply_mutations<E>(&mut self, check: &mut impl FnMut() -> Result<(), E>) -> Result<(), E> {
        for (mut scope, name, from) in std::mem::take(&mut self.mutations) {
            check()?;
            let mut captured = false;
            while !self.scopes[scope].bindings.contains_key(&name) {
                check()?;
                if let Some(shared) = self.scopes[scope].namespace_exports
                    && self.scopes[shared].bindings.contains_key(&name)
                {
                    scope = shared;
                    break;
                }
                let Some(parent) = self.scopes[scope].parent else {
                    break;
                };
                captured |= self.scopes[scope].function
                    || self.scopes[scope].enclosing_visibility_floor != 0;
                scope = parent;
            }
            let binding = self.scopes[scope].bindings.entry(name).or_default();
            if captured {
                binding.runtime_mutated = true;
            } else {
                binding.transitions.push(Binding {
                    from,
                    target: BindingTarget::Shadow,
                });
            }
        }
        Ok(())
    }

    /// Capture only assignment targets; member writes do not rebind their object.
    fn mutation_names<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        from: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        self.ecmascript_pattern_bindings(node, scope, EcmaPatternEffect::Write(from), check)
    }

    /// Reuse the same admitted pattern leaves for hoisting and lexical writes.
    fn ecmascript_pattern_bindings<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        effect: EcmaPatternEffect,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if self.incomplete {
            return Ok(());
        }
        if matches!(
            node.kind(),
            "identifier" | "shorthand_property_identifier_pattern"
        ) {
            if let Some(name) = self.binding_name(node)
                && self.admit_binding()
            {
                match effect {
                    EcmaPatternEffect::Reserve => {
                        let scope = self.binding_scope(scope);
                        self.scopes[scope]
                            .bindings
                            .entry(name)
                            .or_default()
                            .reserves_scope = true;
                    }
                    EcmaPatternEffect::Write(from) => self.mutations.push((scope, name, from)),
                }
            }
        } else if matches!(
            node.kind(),
            "assignment_pattern" | "object_assignment_pattern" | "pair_pattern" | "rest_pattern"
        ) {
            if let Some(target) = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("value"))
                .or_else(|| crate::first_named_child(node))
            {
                self.ecmascript_pattern_bindings(target, scope, effect, check)?;
            }
        } else if matches!(node.kind(), "object_pattern" | "array_pattern") {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.ecmascript_pattern_bindings(child, scope, effect, check)?;
            }
        }
        Ok(())
    }

    /// Admit an environment before retaining its maps or assigning its ordinal.
    fn admit_scope(&mut self) -> bool {
        if self.incomplete || self.scopes.len() >= MAX_LEXICAL_SCOPES {
            self.incomplete = true;
            return false;
        }
        true
    }

    /// ECMAScript's module and lexical environments share canonical cooked names.
    fn ecmascript(&self) -> bool {
        matches!(self.language, "javascript" | "typescript" | "tsx")
    }

    /// Decode only grammar-owned identifier leaves, never an arbitrary snippet.
    fn identifier(&self, node: Node<'_>) -> Option<String> {
        if !matches!(
            node.kind(),
            "identifier"
                | "type_identifier"
                | "simple_identifier"
                | "field_identifier"
                | "property_identifier"
                | "shorthand_property_identifier_pattern"
                | "shorthand_field_identifier"
        ) {
            return None;
        }
        let text = self.content.get(node.byte_range())?;
        if self.ecmascript() {
            crate::semantic::ecmascript_identifier_value(text)
        } else if matches!(self.language, "csharp" | "c_sharp") {
            native_identifiers::csharp(text)
        } else if self.language == "cpp" {
            native_identifiers::cpp(text)
        } else {
            Some(text.to_string())
        }
    }

    /// Find a declaration by the exact emitted AST identity; ambiguity abstains.
    fn declaration(&self, node: Node<'_>) -> Option<usize> {
        if !crate::should_emit_declaration_symbol(node, self.content) {
            return None;
        }
        let name = node_name(node, self.content)?;
        let candidates = self.declarations.get(&(
            node.start_position().row + 1,
            node.end_position().row + 1,
            name.as_str(),
            node.kind(),
        ))?;
        match candidates {
            DeclarationOrdinals::Unique(ordinal) => Some(*ordinal),
            DeclarationOrdinals::Identities { metadata, .. } => {
                // Reuse the existing producer's AST normalization, not a signature parser.
                let parent = crate::symbol_parent(node, self.content, None)
                    .and_then(|parent| crate::compact_symbol_identity(&parent));
                let signature = crate::truncate_chars_at_boundary(
                    &crate::compact_text(&crate::declaration_signature(node, self.content)),
                    crate::MAX_SNIPPET_CHARS,
                );
                metadata
                    .get(&(parent.as_deref(), signature.as_str()))
                    .copied()
                    .flatten()
            }
        }
    }

    /// Keep graph display spelling while using canonical ECMAScript binding values.
    fn declaration_binding_name(&self, symbol: usize) -> Option<String> {
        let name = &self.graph.symbols[symbol].name;
        if self.ecmascript() {
            match self.declaration_names.get(symbol)? {
                DeclarationBindingName::Admitted { name, .. } => Some(name.clone()),
                DeclarationBindingName::Unseen | DeclarationBindingName::Rejected => None,
            }
        } else if matches!(self.language, "csharp" | "c_sharp") {
            native_identifiers::csharp(name)
        } else if self.language == "cpp" {
            native_identifiers::cpp(name)
        } else {
            Some(name.clone())
        }
    }

    /// Apply binding-role early errors without rejecting reserved member properties.
    fn binding_name(&self, node: Node<'_>) -> Option<String> {
        let name = self.identifier(node)?;
        if self.ecmascript() {
            crate::semantic::ecmascript_contextual_name(
                &name,
                self.ecma_context,
                EcmascriptNameRole::Binding,
            )
        } else {
            Some(name)
        }
    }

    /// References retain their own early errors, including legal strict eval reads.
    fn reference_name(&self, node: Node<'_>) -> Option<String> {
        let name = self.identifier(node)?;
        if self.ecmascript() {
            crate::semantic::ecmascript_contextual_name(
                &name,
                self.ecma_context,
                EcmascriptNameRole::Reference,
            )
        } else {
            Some(name)
        }
    }

    /// Member names enter ordinary lexical values only where the language allows it.
    fn declaration_is_lexical_value(&self, node: Node<'_>, kind: SymbolKind) -> bool {
        if self.csharp() && csharp_explicit_interface_member(node) {
            return false;
        }
        if self.language == "rust" && is_rust_associated_member(node) {
            return false;
        }
        !(kind == SymbolKind::Method
            && (self.ecmascript() || matches!(self.language, "go" | "objective-c")))
    }

    /// Append one owning binding in source order before one final visibility sort.
    fn bind(
        &mut self,
        scope: usize,
        name: String,
        from: usize,
        reserves_scope: bool,
        target: BindingTarget,
    ) {
        self.bind_in(
            scope,
            name,
            from,
            reserves_scope,
            target,
            BindingNamespace::Value,
        );
    }

    /// Admit all value, method and namespace transitions through one bounded owner.
    fn bind_in(
        &mut self,
        scope: usize,
        name: String,
        from: usize,
        reserves_scope: bool,
        target: BindingTarget,
        namespace: BindingNamespace,
    ) {
        if !self.admit_binding() {
            return;
        }
        let scope = self.binding_scope(scope);
        let scope = &mut self.scopes[scope];
        let names = match namespace {
            BindingNamespace::Value => &mut scope.bindings,
            BindingNamespace::Namespace => &mut scope.namespaces,
            BindingNamespace::Method => &mut scope.methods,
        };
        let bindings = names.entry(name).or_default();
        bindings.reserves_scope |= reserves_scope;
        bindings.transitions.push(Binding { from, target });
    }

    /// Find the owning local environment without exposing body vars to parameters.
    fn value_declaration_scope(&self, mut scope: usize) -> usize {
        while !((if self.ecmascript() {
            self.scopes[scope].ecma_var_environment == EcmascriptVarEnvironment::Local
        } else {
            self.scopes[scope].function
        }) || self.language == "python" && self.scopes[scope].class)
        {
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        scope
    }

    /// Python class namespaces are not closure environments for nested code.
    fn python_closure_parent(&self, mut scope: usize) -> usize {
        while self.scopes[scope].class {
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        scope
    }

    /// Visit each node once; retain only names, scopes and admitted call occurrences.
    fn visit<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let previous = self.ecma_context;
        let previous_export = self.namespace_export_binding;
        if self.ecmascript()
            && node.kind() == "export_statement"
            && let Some(shared) = self.scopes[scope].namespace_exports
        {
            self.namespace_export_binding = Some((scope, shared));
        }
        self.ecma_context = self.scopes[scope].ecma_context;
        let result = self.visit_node(node, scope, check);
        self.ecma_context = previous;
        self.namespace_export_binding = previous_export;
        result
    }

    /// Visit under an already selected syntax and lexical environment.
    fn visit_node<E>(
        &mut self,
        node: Node<'_>,
        mut scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if self.incomplete {
            return Ok(());
        }
        if self.language == "python"
            && matches!(node.kind(), "global_statement" | "nonlocal_statement")
        {
            if scope == 0 {
                return Ok(());
            }
            let directive = if node.kind() == "global_statement" {
                PythonDirective::Global
            } else {
                PythonDirective::Nonlocal
            };
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                check()?;
                if let Some(name) = self.identifier(child) {
                    if let Some((existing, at)) =
                        self.scopes[scope].python_directives.get_mut(&name)
                    {
                        if *existing != directive {
                            *existing = PythonDirective::Invalid;
                        }
                        *at = (*at).max(node.end_byte());
                    } else if self.admit_binding() {
                        self.scopes[scope]
                            .python_directives
                            .insert(name, (directive, node.end_byte()));
                    }
                }
            }
            return Ok(());
        }
        if self.language == "python"
            && !self.python_directive_names.is_empty()
            && node.kind() == "identifier"
            && let Some(name) = self.identifier(node)
            && self.python_directive_names.contains(&name)
            && python_identifier_is_read(node)
            && !self.scopes[scope].python_reads.contains_key(&name)
            && self.admit_binding()
        {
            self.scopes[scope]
                .python_reads
                .insert(name, node.start_byte());
        }
        if self.ecmascript() && scope == self.ecma_root && node.kind() == "export_statement" {
            self.direct_export(node, check)?;
        }
        let enclosing_scope = scope;
        if self.language == "python"
            && matches!(
                node.kind(),
                "list_comprehension"
                    | "set_comprehension"
                    | "dictionary_comprehension"
                    | "generator_expression"
            )
        {
            return self.python_comprehension_scope(node, scope, check);
        }
        if self.language == "rust"
            && matches!(
                node.kind(),
                "if_expression" | "while_expression" | "for_expression"
            )
        {
            return self.rust_control_scope(node, scope, check);
        }
        if self.language == "zig"
            && matches!(
                node.kind(),
                "catch_expression"
                    | "else_clause"
                    | "errdefer_statement"
                    | "for_expression"
                    | "for_statement"
                    | "if_expression"
                    | "if_statement"
                    | "if_type_expression"
                    | "switch_case"
                    | "while_expression"
                    | "while_statement"
            )
        {
            return self.zig_payload_scope(node, scope, check);
        }
        let source_context = self.ecma_context;
        let callable_contexts = if self.ecmascript() && ecmascript_context::is_callable(node) {
            Some(ecmascript_context::callable_contexts(
                node,
                source_context,
                self.content,
                check,
            )?)
        } else {
            None
        };
        let function = is_function(node.kind())
            || self.language == "kotlin"
                && matches!(node.kind(), "secondary_constructor" | "getter" | "setter")
            || self.csharp()
                && matches!(
                    node.kind(),
                    "accessor_declaration"
                        | "indexer_declaration"
                        | "local_function_statement"
                        | "anonymous_method_expression"
                        | "operator_declaration"
                        | "conversion_operator_declaration"
                );
        // Java bodies also own interface/enum/record and anonymous members.
        // Select the body rather than adding a second scope to named classes.
        let class = if self.language == "java" {
            matches!(
                node.kind(),
                "class_body" | "interface_body" | "enum_body" | "annotation_type_body"
            )
        } else if self.language == "kotlin" {
            is_class(node.kind())
                || matches!(
                    node.kind(),
                    "class_body" | "enum_class_body" | "object_declaration" | "companion_object"
                )
        } else if self.csharp() {
            is_class(node.kind())
                || matches!(node.kind(), "record_declaration" | "interface_declaration")
        } else {
            is_class(node.kind())
        };
        let class = class
            || self.language == "zig"
                && matches!(
                    node.kind(),
                    "enum_declaration" | "union_declaration" | "opaque_declaration"
                )
            || self.language == "rust"
                && matches!(
                    node.kind(),
                    "enum_item" | "trait_item" | "union_item" | "type_item"
                );
        let declaration = crate::source_declaration_kind(node.kind(), Some(self.language))
            .and_then(|_| self.declaration(node));
        if self.ecmascript()
            && let Some(symbol) = declaration
        {
            self.admit_declaration_name(node, symbol, scope, check)?;
        }
        if let Some(symbol) = declaration {
            let owner = if self.ecmascript() {
                match self.declaration_names.get(symbol) {
                    Some(DeclarationBindingName::Admitted { scope, .. }) => *scope,
                    _ => scope,
                }
            } else {
                scope
            };
            let owner = self.binding_scope(owner);
            self.declaration_scopes[symbol] = Some(owner);
        }
        if self.language == "rust"
            && self.scopes[scope].rust_item_owner != RustItemOwner::UnsupportedModule
        {
            if crate::rust_binding_source::is_module_item(node) {
                self.rust_module_item(node, scope, declaration);
            } else if node.kind() == "function_item" {
                self.rust_associated_item(node, scope, declaration);
            }
        }
        if self.language == "cpp"
            && declaration.is_none()
            && matches!(
                declaration_kind(node.kind()),
                Some(SymbolKind::Class | SymbolKind::Struct)
            )
            && crate::should_emit_declaration_symbol(node, self.content)
            && let Some(name) = node_name(node, self.content)
        {
            self.bind_in(
                scope,
                name,
                Self::cpp_declaration_visibility(node),
                false,
                BindingTarget::Unresolved,
                BindingNamespace::Namespace,
            );
        }
        if self.language == "cpp"
            && declaration.is_none()
            && declaration_kind(node.kind()) == Some(SymbolKind::Function)
            && crate::should_emit_declaration_symbol(node, self.content)
            && let Some(name) = node_name(node, self.content)
        {
            self.bind(
                scope,
                name,
                Self::cpp_declaration_visibility(node),
                false,
                BindingTarget::Unresolved,
            );
        }
        if self.language == "rust"
            && matches!(
                node.kind(),
                "mod_item"
                    | "struct_item"
                    | "enum_item"
                    | "trait_item"
                    | "union_item"
                    | "type_item"
            )
            && let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| self.identifier(name))
        {
            let target = if !node.has_error()
                && crate::rust_binding_source::is_module_item(node)
                && !crate::rust_binding_source::has_unsupported_attributes(node, self.content)
            {
                if node.kind() == "mod_item" {
                    BindingTarget::RustModule(Arc::clone(&self.scopes[scope].rust_module))
                } else {
                    BindingTarget::RustNamespaceLookup(Arc::clone(&self.scopes[scope].rust_module))
                }
            } else {
                BindingTarget::Unresolved
            };
            self.bind_in(scope, name, 0, false, target, BindingNamespace::Namespace);
        }
        let callable_owner = declaration.or_else(|| {
            if !function {
                return None;
            }
            let parent = node.parent()?.parent()?;
            (crate::declaration_has_direct_callable_initializer(parent)
                && crate::first_declaration_initializer(parent)
                    .is_some_and(|initializer| initializer.id() == node.id()))
            .then(|| self.declaration(parent))
            .flatten()
        });
        if let Some(symbol) = declaration {
            if self.language == "python"
                && scope == 0
                && !self.graph.symbols[symbol].name.starts_with('_')
            {
                self.export_slot(
                    self.graph.symbols[symbol].name.clone(),
                    node,
                    crate::SourceExportBinding::LocalDeclaration(symbol),
                    crate::SourceExportNamespace::Value,
                );
            }
            let fact = &self.graph.symbols[symbol];
            if self.language == "java" && fact.kind == SymbolKind::Method {
                self.bind_in(
                    scope,
                    fact.name.clone(),
                    0,
                    false,
                    BindingTarget::Declaration(symbol),
                    BindingNamespace::Method,
                );
            }
            if matches!(
                fact.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Module
                    | SymbolKind::Type
            ) && self.declaration_is_lexical_value(node, fact.kind)
                && !(self.language == "java" && fact.kind == SymbolKind::Method)
                && !(matches!(self.language, "c" | "objective-c")
                    && fact.kind == SymbolKind::Struct)
                && !(self.ecmascript() && fact.kind == SymbolKind::Type)
                && !(self.language == "rust"
                    && (matches!(fact.kind, SymbolKind::Module | SymbolKind::Type)
                        || fact.kind == SymbolKind::Struct
                            && node
                                .child_by_field_name("body")
                                .is_some_and(|body| body.kind() == "field_declaration_list")))
                && !crate::declaration_has_direct_callable_initializer(node)
            {
                let hoisted = self.language != "python";
                if let Some(name) = self.declaration_binding_name(symbol) {
                    if self.language == "cpp"
                        && matches!(fact.kind, SymbolKind::Class | SymbolKind::Struct)
                    {
                        let from = Self::cpp_declaration_visibility(node);
                        self.bind_in(
                            scope,
                            name,
                            from,
                            false,
                            BindingTarget::Declaration(symbol),
                            BindingNamespace::Namespace,
                        );
                    } else {
                        if self.language == "cpp" && fact.kind == SymbolKind::Function {
                            self.cpp_function_visibility
                                .insert(symbol, Self::cpp_declaration_visibility(node));
                        }
                        self.bind(
                            scope,
                            name,
                            if hoisted { 0 } else { node.end_byte() },
                            self.language == "python" && self.scopes[scope].function,
                            BindingTarget::Declaration(symbol),
                        );
                    }
                } else if self.ecmascript()
                    && let Some(name) = crate::semantic::ecmascript_identifier_value(&fact.name)
                {
                    self.bind(scope, name, 0, false, BindingTarget::Shadow);
                }
            }
        }
        let block = self.scopes[scope].container_body != Some(node.id())
            && ((self.language != "python"
                && matches!(
                    node.kind(),
                    "block"
                        | "statement_block"
                        | "compound_statement"
                        | "for_statement"
                        | "for_in_statement"
                        | "catch_clause"
                        | "switch_body"
                        | "declaration_list"
                ))
                || (self.language == "rust" && node.kind() == "match_arm")
                || (self.language == "go"
                    && matches!(
                        node.kind(),
                        "if_statement"
                            | "expression_switch_statement"
                            | "type_switch_statement"
                            | "communication_case"
                    ))
                || (self.language == "cpp"
                    && matches!(
                        node.kind(),
                        "if_statement" | "switch_statement" | "while_statement" | "for_range_loop"
                    ))
                || (matches!(self.language, "csharp" | "c_sharp")
                    && matches!(
                        node.kind(),
                        "foreach_statement"
                            | "using_statement"
                            | "switch_section"
                            | "switch_expression_arm"
                    ))
                || (self.language == "kotlin" && node.kind() == "catch_block"));
        let ecma_initializer = self.ecmascript()
            && matches!(
                node.kind(),
                "field_definition" | "public_field_definition" | "class_static_block"
            );
        let container = self.container_scope(node, scope, check)?;
        if function || class || block || ecma_initializer || container.is_some() {
            if let Some(container) = container {
                scope = container;
            } else {
                if !self.admit_scope() {
                    return Ok(());
                }
                let mut parent = scope;
                if self.language == "kotlin"
                    && matches!(node.kind(), "class_body" | "enum_class_body")
                    && node.parent().is_some_and(|owner| {
                        matches!(
                            owner.kind(),
                            "class_declaration" | "object_declaration" | "companion_object"
                        )
                    })
                {
                    // Member bodies exclude the primary-constructor header environment.
                    parent = self.scopes[parent].parent.unwrap_or(parent);
                }
                if self.language == "kotlin"
                    && matches!(
                        node.kind(),
                        "class_declaration" | "object_declaration" | "companion_object"
                    )
                    && !kotlin_inner_classifier(node, self.content)
                {
                    while matches!(
                        self.scopes[parent].native_container,
                        NativeContainerScope::KotlinInstance { .. }
                    ) {
                        check()?;
                        let Some(outer) = self.scopes[parent].parent else {
                            break;
                        };
                        parent = outer;
                    }
                }
                if (function || class) && self.language == "python" {
                    parent = self.python_closure_parent(parent);
                }
                scope = self.scopes.len();
                let mut rust_item_owner =
                    if self.scopes[parent].rust_item_owner == RustItemOwner::UnsupportedModule {
                        RustItemOwner::UnsupportedModule
                    } else {
                        RustItemOwner::Local
                    };
                let rust_module = if self.language == "rust"
                    && node.kind() == "declaration_list"
                    && node
                        .parent()
                        .is_some_and(|owner| owner.kind() == "mod_item")
                {
                    match crate::rust_binding_source::module_owner(node, self.content) {
                        Some(module) if rust_item_owner != RustItemOwner::UnsupportedModule => {
                            rust_item_owner = RustItemOwner::Module;
                            module
                        }
                        _ => {
                            rust_item_owner = RustItemOwner::UnsupportedModule;
                            Arc::clone(&self.scopes[parent].rust_module)
                        }
                    }
                } else {
                    Arc::clone(&self.scopes[parent].rust_module)
                };
                if self.language == "rust"
                    && node.kind() == "mod_item"
                    && !crate::rust_binding_source::is_module_item(node)
                {
                    rust_item_owner = RustItemOwner::UnsupportedModule;
                }
                self.scopes.push(Scope {
                    parent: Some(parent),
                    ecma_context: self.child_ecma_context(node, function, class, check)?,
                    ecma_root: self.scopes[parent].ecma_root,
                    function,
                    ecma_var_environment: if self.ecmascript()
                        && node.parent().is_some_and(|owner| {
                            (is_function(owner.kind())
                                || matches!(
                                    owner.kind(),
                                    "class_static_block" | "internal_module" | "module"
                                ))
                                && owner
                                    .child_by_field_name("body")
                                    .is_some_and(|body| body.id() == node.id())
                        }) {
                        EcmascriptVarEnvironment::Local
                    } else {
                        EcmascriptVarEnvironment::Enclosing
                    },
                    class,
                    native_container: if self.language == "kotlin"
                        && matches!(node.kind(), "class_body" | "enum_class_body")
                        && node
                            .parent()
                            .is_some_and(|owner| owner.kind() == "class_declaration")
                    {
                        NativeContainerScope::KotlinInstance {
                            initialization: None,
                        }
                    } else {
                        NativeContainerScope::Ordinary
                    },
                    enclosing_visibility_floor: if (function
                        && (self.language == "python" || self.ecmascript())
                        && !immediately_invoked_expression(node))
                        || (self.ecmascript() && deferred_instance_field(node))
                    {
                        node.end_byte()
                    } else {
                        0
                    },
                    owner: if function {
                        callable_owner
                    } else {
                        self.scopes[parent].owner
                    },
                    rust_module,
                    rust_item_owner,
                    ..Scope::default()
                });
            }
            self.ecma_context = self.scopes[scope].ecma_context;
            if self.ecmascript()
                && node.kind() == "class"
                && let Some(name) = node
                    .child_by_field_name("name")
                    .and_then(|name| self.shadow_name(name))
            {
                // A class expression's private name is not its outer namesake.
                self.bind(scope, name, 0, true, BindingTarget::Shadow);
            }
            if function {
                if self.ecmascript() && node.kind() != "arrow_function" {
                    self.bind(
                        scope,
                        "arguments".to_string(),
                        0,
                        false,
                        BindingTarget::Shadow,
                    );
                }
                if self.ecmascript()
                    && matches!(
                        node.kind(),
                        "function_expression" | "function" | "generator_function"
                    )
                    && let Some(symbol) = callable_owner
                    && let Some(name) = node
                        .child_by_field_name("name")
                        .and_then(|name| self.binding_name(name))
                {
                    self.bind(scope, name, 0, false, BindingTarget::Declaration(symbol));
                }
                let body_context = self.ecma_context;
                if let Some(contexts) = callable_contexts {
                    self.ecma_context = contexts.formals;
                }
                self.parameters(node, scope, check)?;
                self.ecma_context = body_context;
            }
        }
        if self.csharp()
            && matches!(
                node.kind(),
                "class_declaration"
                    | "struct_declaration"
                    | "record_declaration"
                    | "interface_declaration"
            )
        {
            self.csharp_type_scope(node, scope, check)?;
        }
        if self.language == "java" && class {
            self.java_implicit_method_blockers(node, scope, check)?;
        }
        if self.language == "kotlin" && matches!(node.kind(), "getter" | "setter") {
            self.bind(scope, "field".to_string(), 0, true, BindingTarget::Shadow);
        }
        if self.language == "kotlin" {
            if node.kind() == "class_declaration" {
                self.kotlin_constructor_bindings(node, scope, false, true, check)?;
            } else if matches!(node.kind(), "class_body" | "enum_class_body")
                && let Some(owner) = node
                    .parent()
                    .filter(|owner| owner.kind() == "class_declaration")
            {
                self.kotlin_constructor_bindings(owner, scope, true, false, check)?;
                if owner
                    .named_children(&mut owner.walk())
                    .any(|child| child.kind() == "primary_constructor")
                    && self.admit_scope()
                {
                    let initialization = self.scopes.len();
                    self.scopes.push(Scope {
                        parent: Some(scope),
                        owner: self.scopes[scope].owner,
                        ..Scope::default()
                    });
                    self.kotlin_constructor_bindings(owner, initialization, false, false, check)?;
                    self.scopes[scope].native_container = NativeContainerScope::KotlinInstance {
                        initialization: Some(initialization),
                    };
                }
            }
        }
        if self.language == "cpp" && node.kind() == "catch_clause" {
            self.parameters(node, scope, check)?;
        }
        if self.ecmascript() && node.kind() == "import_statement" {
            let imports_admitted = self.ecma_context.is_module()
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "program");
            if !imports_admitted && node.has_error() {
                self.incomplete = true;
            }
            for name in crate::semantic::ecmascript::invalid_local_bindings(
                node,
                self.content,
                imports_admitted,
                check,
            )? {
                self.bind(scope, name, 0, true, BindingTarget::Shadow);
            }
        }
        if crate::owns_import_occurrence(node, self.graph)
            && let Some(imports) = self
                .imports
                .get(&(node.start_byte(), node.end_byte()))
                .cloned()
        {
            for import in imports {
                if self.language == "rust" {
                    self.rust_import_scopes[import] = Some(scope);
                }
                if self.language == "rust"
                    && self.scopes[scope].rust_item_owner != RustItemOwner::UnsupportedModule
                    && crate::rust_binding_source::is_module_item(node)
                    && let Some(binding) = self.facts.imports[import].rust.clone()
                    && let Some(name) = binding.local.clone()
                {
                    if let Some(rust) = &mut self.rust {
                        if rust.declarations.len().saturating_add(rust.modules.len())
                            >= crate::MAX_RELATIONS_PER_FILE
                        {
                            rust.complete = false;
                        } else {
                            rust.declarations.push(crate::SourceRustDeclaration {
                                module: Arc::clone(&binding.module),
                                name: name.clone(),
                                visibility: crate::rust_binding_source::visibility(node),
                                binding: crate::SourceRustBinding::Use(Arc::clone(&binding)),
                            });
                        }
                    }
                    if binding.reexport {
                        self.export_slot(
                            name,
                            node,
                            crate::SourceExportBinding::RustReExport(binding),
                            crate::SourceExportNamespace::Value,
                        );
                    }
                }
                for (reference, binding) in self.facts.imports[import].references.iter().enumerate()
                {
                    check()?;
                    let python = self.language == "python";
                    let conditional = python && has_conditional_owner(node)
                        || self.language == "rust"
                            && self.facts.imports[import]
                                .rust
                                .as_ref()
                                .is_none_or(|binding| !binding.supported);
                    if self.language == "rust" {
                        self.bind_in(
                            scope,
                            binding.local().to_string(),
                            0,
                            false,
                            if conditional {
                                BindingTarget::Shadow
                            } else {
                                BindingTarget::Import { import, reference }
                            },
                            BindingNamespace::Namespace,
                        );
                    }
                    let import_scope = if self.ecmascript()
                        && scope == self.ecma_root
                        && matches!(
                            self.scopes[scope].ecma_role,
                            Some(EmbeddedScriptRole::VueSetup | EmbeddedScriptRole::SvelteInstance)
                        ) {
                        0
                    } else {
                        scope
                    };
                    self.bind(
                        import_scope,
                        binding.local().to_string(),
                        if python { node.end_byte() } else { 0 },
                        python && self.scopes[scope].function,
                        if conditional {
                            BindingTarget::Shadow
                        } else {
                            BindingTarget::Import { import, reference }
                        },
                    );
                }
            }
        }
        if self.language == "rust"
            && node.kind() == "type_parameter"
            && let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| self.identifier(name))
        {
            self.bind_in(
                scope,
                name,
                0,
                false,
                BindingTarget::Shadow,
                BindingNamespace::Namespace,
            );
        }
        self.local_bindings(node, scope, check)?;
        if self.language == "python"
            && node.kind() == "case_pattern"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "case_clause")
        {
            self.python_match_bindings(node, self.value_declaration_scope(scope), check)?;
        }
        if crate::is_call_node(node.kind())
            && let Some(calls) = self
                .calls
                .get(&(node.start_byte(), node.end_byte()))
                .cloned()
        {
            for fact in calls {
                let target = call_target(node)
                    .filter(|target| {
                        self.content
                            .get(target.byte_range())
                            .map(crate::compact_text)
                            .as_deref()
                            == Some(
                                self.graph.relations[self.facts.calls[fact].relation]
                                    .target_name
                                    .as_str(),
                            )
                    })
                    .or_else(|| {
                        (self.language == "rust"
                            && call_target(node).is_some_and(|target| {
                                crate::rust_target_invokes_function_item(target, self.content)
                            }))
                        .then(|| {
                            node.child_by_field_name("arguments")
                                .and_then(crate::first_named_child)
                        })
                        .flatten()
                        .filter(|target| target.kind() == "scoped_identifier")
                        .filter(|target| {
                            self.content
                                .get(target.byte_range())
                                .map(crate::compact_text)
                                .as_deref()
                                == Some(
                                    self.graph.relations[self.facts.calls[fact].relation]
                                        .target_name
                                        .as_str(),
                                )
                        })
                    });
                let name = if self.language == "java" {
                    self.call_name(node)
                } else {
                    target.and_then(|target| self.target_name(target))
                };
                let qualified = target.and_then(|target| self.rust_qualified_reference(target));
                let name = name.or_else(|| {
                    qualified.as_ref().map(|reference| {
                        (
                            reference.local().to_string(),
                            reference.imported().map(ToString::to_string),
                        )
                    })
                });
                self.sites.push(CallSite {
                    fact,
                    scope,
                    owner: self.scopes[scope].owner,
                    offset: node.start_byte(),
                    name: name.as_ref().map(|(name, _)| name.clone()),
                    member: name.and_then(|(_, member)| member),
                    method_name: self.language == "java"
                        && node.kind() == "method_invocation"
                        && node.child_by_field_name("object").is_none(),
                    argument_count: if self.language == "java" {
                        node.child_by_field_name("arguments").map(|arguments| {
                            arguments
                                .named_children(&mut arguments.walk())
                                .filter(|argument| !argument.is_extra())
                                .count()
                        })
                    } else {
                        None
                    },
                    qualified,
                    span: target
                        .filter(|target| {
                            call_target(node).is_some_and(|callee| callee.id() != target.id())
                        })
                        .map_or(self.facts.calls[fact].span, |target| {
                            crate::SourceByteSpan {
                                start: target.start_byte(),
                                end: target.end_byte(),
                            }
                        }),
                    rust_module: Arc::clone(&self.scopes[scope].rust_module),
                });
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let saved_context = self.scopes[scope].ecma_context;
            if self.ecmascript() {
                self.scopes[scope].ecma_context = ecmascript_context::child_context(
                    node,
                    child,
                    source_context,
                    callable_contexts,
                );
            }
            let computed_method_key = self.ecmascript()
                && node.kind() == "method_definition"
                && node
                    .child_by_field_name("name")
                    .is_some_and(|name| name.id() == child.id());
            let immediate_field_syntax = self.ecmascript()
                && matches!(node.kind(), "field_definition" | "public_field_definition")
                && node
                    .child_by_field_name("value")
                    .is_none_or(|value| value.id() != child.id());
            let python_definition_header = self.language == "python"
                && (function || class)
                && node
                    .child_by_field_name("body")
                    .is_none_or(|body| body.id() != child.id());
            let initialization = match self.scopes[scope].native_container {
                NativeContainerScope::KotlinInstance { initialization } => initialization,
                NativeContainerScope::CSharpType { initialization } => Some(initialization),
                NativeContainerScope::Ordinary => None,
            };
            let csharp_initialization = initialization
                .filter(|_| self.csharp() && csharp_initializer_child(node, child, self.content));
            let kotlin_initialization = initialization.filter(|_| {
                self.language == "kotlin"
                    && (child.kind() == "anonymous_initializer"
                        || node.kind() == "property_declaration"
                            && !matches!(
                                child.kind(),
                                "variable_declaration"
                                    | "multi_variable_declaration"
                                    | "getter"
                                    | "setter"
                                    | "modifiers"
                                    | "type_parameters"
                                    | "type_constraints"
                            ))
            });
            self.visit(
                child,
                if let Some(initialization) = kotlin_initialization.or(csharp_initialization) {
                    initialization
                } else if python_definition_header || computed_method_key || immediate_field_syntax
                {
                    enclosing_scope
                } else {
                    scope
                },
                check,
            )?;
            if self.ecmascript() {
                self.scopes[scope].ecma_context = saved_context;
            }
        }
        Ok(())
    }

    /// Known but unrepresented Java members cannot authorize an unrelated outer method.
    fn java_implicit_method_blockers<E>(
        &mut self,
        body: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let Some(owner) = body.parent() else {
            return Ok(());
        };
        if body.kind() == "enum_body" || owner.kind() == "enum_constant" {
            for (name, arity) in [("values", 0), ("valueOf", 1)] {
                check()?;
                if !self.admit_binding() {
                    return Ok(());
                }
                self.scopes[scope]
                    .java_implicit_methods
                    .insert(name.to_string(), arity);
            }
        }
        if owner.kind() != "record_declaration" {
            return Ok(());
        }
        let Some(parameters) = owner.child_by_field_name("parameters") else {
            return Ok(());
        };
        if parameters.named_child_count() == 0 {
            return Ok(());
        }
        let graph = self.graph;
        let mut accessors = BTreeSet::new();
        for method in body.named_children(&mut body.walk()) {
            check()?;
            if method.kind() == "method_declaration"
                && !method.has_error()
                && method
                    .child_by_field_name("parameters")
                    .is_some_and(|parameters| {
                        parameters
                            .named_children(&mut parameters.walk())
                            .all(|parameter| parameter.kind() == "receiver_parameter")
                    })
                && let Some(symbol) = self.declaration(method)
            {
                let name = graph.symbols[symbol].name.as_str();
                if !accessors.contains(name) {
                    if !self.admit_binding() {
                        return Ok(());
                    }
                    accessors.insert(name);
                }
            }
        }
        for parameter in parameters.named_children(&mut parameters.walk()) {
            check()?;
            let name = parameter.child_by_field_name("name").or_else(|| {
                parameter
                    .named_children(&mut parameter.walk())
                    .find(|child| child.kind() == "variable_declarator")
                    .and_then(|declarator| declarator.child_by_field_name("name"))
            });
            if let Some(name) = name.and_then(|name| self.identifier(name))
                && !accessors.contains(name.as_str())
            {
                if !self.admit_binding() {
                    return Ok(());
                }
                self.scopes[scope].java_implicit_methods.insert(name, 0);
            }
        }
        Ok(())
    }

    /// The grammar registry already normalizes both C# spellings to this owner.
    fn csharp(&self) -> bool {
        matches!(self.language, "csharp" | "c_sharp")
    }

    /// Retain one shared member view and phase-specific primary-parameter precedence.
    fn csharp_type_scope<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let initialization = if let NativeContainerScope::CSharpType { initialization } =
            self.scopes[scope].native_container
        {
            initialization
        } else {
            if !self.admit_scope() {
                return Ok(());
            }
            let initialization = self.scopes.len();
            self.scopes.push(Scope {
                parent: Some(scope),
                owner: self.scopes[scope].owner,
                ..Scope::default()
            });
            self.scopes[scope].native_container =
                NativeContainerScope::CSharpType { initialization };
            initialization
        };
        self.scopes[scope].container_body = node.child_by_field_name("body").map(|body| body.id());
        if node
            .named_children(&mut node.walk())
            .any(|child| child.kind() == "parameter_list")
        {
            if !self.admit_scope() {
                return Ok(());
            }
            let parameters = self.scopes.len();
            self.scopes.push(Scope {
                parent: self.scopes[scope].parent,
                owner: self.scopes[scope].owner,
                ..Scope::default()
            });
            self.scopes[scope].parent = Some(parameters);
            self.parameters(node, parameters, check)?;
            self.parameters(node, initialization, check)?;
        }
        Ok(())
    }

    /// Primary-constructor names belong to header/initialization, or to property members.
    fn kotlin_constructor_bindings<E>(
        &mut self,
        owner: Node<'_>,
        scope: usize,
        property_only: bool,
        ordered: bool,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = owner.walk();
        let Some(constructor) = owner
            .named_children(&mut cursor)
            .find(|child| child.kind() == "primary_constructor")
        else {
            return Ok(());
        };
        let mut cursor = constructor.walk();
        let Some(parameters) = constructor
            .named_children(&mut cursor)
            .find(|child| child.kind() == "class_parameters")
        else {
            return Ok(());
        };
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            check()?;
            if parameter.kind() != "class_parameter" {
                continue;
            }
            let mut parameter_cursor = parameter.walk();
            if property_only
                && !parameter
                    .children(&mut parameter_cursor)
                    .any(|child| matches!(child.kind(), "val" | "var"))
            {
                continue;
            }
            if let Some(name) = parameter
                .named_children(&mut parameter_cursor)
                .find(|child| child.kind() == "identifier")
                .and_then(|name| self.identifier(name))
            {
                self.bind(
                    scope,
                    name,
                    if ordered { parameter.end_byte() } else { 0 },
                    false,
                    BindingTarget::Shadow,
                );
            }
        }
        Ok(())
    }

    /// Retain module-owned values and module declarations without display-string inference.
    fn rust_module_item(&mut self, node: Node<'_>, scope: usize, declaration: Option<usize>) {
        let Some(rust) = &mut self.rust else {
            return;
        };
        if rust
            .modules
            .len()
            .saturating_add(rust.declarations.len())
            .saturating_add(rust.associated.len())
            >= crate::MAX_RELATIONS_PER_FILE
        {
            rust.complete = false;
            return;
        }
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(self.content.as_bytes()).ok())
        else {
            return;
        };
        let module = Arc::clone(&self.scopes[scope].rust_module);
        let visibility = if node.kind() == "struct_item" {
            crate::rust_binding_source::constructor_visibility(node)
        } else {
            crate::rust_binding_source::visibility(node)
        };
        let supported = !node.has_error()
            && !crate::rust_binding_source::has_unsupported_attributes(node, self.content);
        if matches!(
            node.kind(),
            "struct_item" | "enum_item" | "union_item" | "type_item" | "trait_item"
        ) {
            rust.declarations.push(crate::SourceRustDeclaration {
                module: Arc::clone(&module),
                name: name.to_string(),
                visibility: crate::rust_binding_source::visibility(node),
                binding: if supported {
                    declaration.map_or(
                        crate::SourceRustBinding::UnresolvedType,
                        crate::SourceRustBinding::TypeDeclaration,
                    )
                } else {
                    crate::SourceRustBinding::UnresolvedType
                },
            });
        }
        if node.kind() == "mod_item" {
            rust.modules.push(crate::SourceRustModule {
                parent: module,
                name: name.to_string(),
                span: crate::SourceByteSpan {
                    start: node.start_byte(),
                    end: node.end_byte(),
                },
                body: node
                    .child_by_field_name("body")
                    .map(|body| crate::SourceByteSpan {
                        start: body.start_byte(),
                        end: body.end_byte(),
                    }),
                supported,
                visibility,
            });
        } else if matches!(node.kind(), "function_item" | "const_item" | "static_item")
            || node.kind() == "struct_item"
                && node
                    .child_by_field_name("body")
                    .is_none_or(|body| body.kind() == "ordered_field_declaration_list")
        {
            let callable = supported && matches!(node.kind(), "function_item" | "struct_item");
            rust.declarations.push(crate::SourceRustDeclaration {
                module,
                name: name.to_string(),
                visibility,
                binding: if callable {
                    declaration.map_or(
                        crate::SourceRustBinding::Unresolved,
                        crate::SourceRustBinding::Declaration,
                    )
                } else {
                    crate::SourceRustBinding::Unresolved
                },
            });
        }
    }

    /// Retain actual inherent impl ownership without inferring receiver types.
    fn rust_associated_item(&mut self, node: Node<'_>, scope: usize, declaration: Option<usize>) {
        let Some(implementation) = node
            .parent()
            .and_then(|body| body.parent())
            .filter(|owner| {
                owner.kind() == "impl_item" && owner.child_by_field_name("trait").is_none()
            })
        else {
            return;
        };
        if !crate::rust_binding_source::is_module_item(implementation) {
            return;
        }
        let Some(mut owner) = implementation.child_by_field_name("type") else {
            return;
        };
        if owner.kind() == "generic_type" {
            let Some(inner) = owner.child_by_field_name("type") else {
                return;
            };
            owner = inner;
        }
        if owner.kind() != "type_identifier" {
            return;
        }
        let Some(owner) = owner.utf8_text(self.content.as_bytes()).ok() else {
            return;
        };
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(self.content.as_bytes()).ok())
        else {
            return;
        };
        let Some(rust) = &mut self.rust else {
            return;
        };
        if rust
            .modules
            .len()
            .saturating_add(rust.declarations.len())
            .saturating_add(rust.associated.len())
            >= crate::MAX_RELATIONS_PER_FILE
        {
            rust.complete = false;
            return;
        }
        rust.associated.push(crate::SourceRustAssociated {
            module: Arc::clone(&self.scopes[scope].rust_module),
            owner: owner.to_string(),
            name: name.to_string(),
            declaration,
            visibility: crate::rust_binding_source::visibility(node),
            supported: !node.has_error()
                && !implementation.has_error()
                && !crate::rust_binding_source::has_unsupported_attributes(node, self.content)
                && !crate::rust_binding_source::has_unsupported_attributes(
                    implementation,
                    self.content,
                ),
        });
    }

    /// A local alias or type cannot be reinterpreted as a same-name crate module.
    fn unqualified_import_has_module_owner(&self, import: usize) -> bool {
        let Some(binding) = self.facts.imports[import].rust.as_deref() else {
            return false;
        };
        if binding.path.anchor != crate::SourceRustAnchor::Unqualified {
            return true;
        }
        let Some(root) = binding.path.segments.first() else {
            return false;
        };
        let mut current = self.rust_import_scopes[import];
        while let Some(scope) = current {
            if let Some(target) = self.scopes[scope]
                .namespaces
                .get(root)
                .and_then(|names| names.transitions.last())
            {
                if let BindingTarget::Import {
                    import: other,
                    reference,
                } = target.target
                {
                    if other == import { /* This declaration cannot shadow its own path root. */
                    } else if self.scopes[scope].rust_item_owner == RustItemOwner::Module {
                        return true;
                    } else {
                        let reference = &self.facts.imports[other].references[reference];
                        return reference.module() == root && reference.imported().is_none();
                    }
                } else {
                    return self.scopes[scope].rust_item_owner == RustItemOwner::Module;
                }
            }
            if matches!(
                self.scopes[scope].rust_item_owner,
                RustItemOwner::Module | RustItemOwner::UnsupportedModule
            ) {
                break;
            }
            current = self.scopes[scope].parent;
        }
        true
    }

    /// Prove an import's toolchain root through its declaration environment, not its alias.
    fn toolchain_import_is_unshadowed<E>(
        &self,
        import: usize,
        reference: usize,
        visited: &mut BTreeSet<(usize, usize)>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        check()?;
        if visited.len() >= 64 || !visited.insert((import, reference)) {
            return Ok(false);
        }
        let Some(binding) = self.facts.imports[import].rust.as_ref() else {
            return Ok(false);
        };
        if !binding.supported {
            return Ok(false);
        }
        let Some(root) = binding.path.segments.first() else {
            return Ok(false);
        };
        if !matches!(root.as_str(), "std" | "core" | "alloc") {
            return Ok(false);
        }
        match binding.path.anchor {
            crate::SourceRustAnchor::Unqualified => {}
            crate::SourceRustAnchor::Absolute
            | crate::SourceRustAnchor::Crate
            | crate::SourceRustAnchor::SelfModule
            | crate::SourceRustAnchor::Super(_) => return Ok(false),
        }
        let mut current = self.rust_import_scopes[import];
        while let Some(scope) = current {
            check()?;
            if let Some(target) = self.scopes[scope]
                .namespaces
                .get(root)
                .and_then(|bindings| bindings.transitions.last())
            {
                match target.target {
                    BindingTarget::Import {
                        import: other,
                        reference: other_reference,
                    } if (other, other_reference) == (import, reference) => {}
                    BindingTarget::Import {
                        import: other,
                        reference: other_reference,
                    } => {
                        let other_target = &self.facts.imports[other].references[other_reference];
                        if other_target.module() != root || other_target.imported().is_some() {
                            return Ok(false);
                        }
                        return self.toolchain_import_is_unshadowed(
                            other,
                            other_reference,
                            visited,
                            check,
                        );
                    }
                    _ => return Ok(false),
                }
            }
            if self.language == "rust"
                && matches!(
                    self.scopes[scope].rust_item_owner,
                    RustItemOwner::Module | RustItemOwner::UnsupportedModule
                )
            {
                break;
            }
            current = self.scopes[scope].parent;
        }
        Ok(true)
    }

    /// Carry an imported toolchain call's actual path through lexical alias selection.
    fn toolchain_call_reference(
        &self,
        import: usize,
        reference: usize,
        member: Option<&str>,
    ) -> Option<Box<crate::ImportReference>> {
        if !self.toolchain_imports.contains(&(import, reference)) {
            return None;
        }
        let reference = &self.facts.imports[import].references[reference];
        let mut identity = reference.module().to_string();
        for component in reference.imported().into_iter().chain(member) {
            identity.push_str("::");
            identity.push_str(component);
        }
        let root = reference.module().split("::").next()?;
        let (module, imported) = identity
            .rsplit_once("::")
            .map_or((identity.as_str(), None), |(module, imported)| {
                (module, Some(imported))
            });
        Some(Box::new(crate::ImportReference::new(
            crate::ImportSyntax::Rust,
            module,
            imported,
            root,
        )))
    }

    /// Invalid binding leaves still prevent accidental fallback to an outer value.
    fn shadow_name(&self, node: Node<'_>) -> Option<String> {
        self.binding_name(node)
            .or_else(|| self.ecmascript().then(|| self.identifier(node)).flatten())
    }

    /// Parameters are owned by their function, including destructuring leaves.
    fn parameters<E>(
        &mut self,
        function: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.csharp()
            && function.kind() == "accessor_declaration"
            && function
                .child_by_field_name("name")
                .is_some_and(|name| matches!(name.kind(), "set" | "init" | "add" | "remove"))
        {
            self.bind(scope, "value".to_string(), 0, true, BindingTarget::Shadow);
            return Ok(());
        }
        if self.language == "kotlin" && function.kind() == "setter" {
            let mut cursor = function.walk();
            if let Some(name) = function
                .named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
            {
                self.parameter_names(name, scope, check)?;
            } else {
                self.bind(scope, "value".to_string(), 0, true, BindingTarget::Shadow);
            }
            return Ok(());
        }
        if self.language == "go"
            && let Some(receiver) = function.child_by_field_name("receiver")
        {
            self.parameter_names(receiver, scope, check)?;
        }
        let mut owner = function;
        while let Some(declarator) = owner.child_by_field_name("declarator") {
            if declarator.kind() != "function_declarator" {
                break;
            }
            owner = declarator;
        }
        if let Some(parameters) = owner
            .child_by_field_name("parameters")
            .or_else(|| owner.child_by_field_name("parameter"))
        {
            self.parameter_names(parameters, scope, check)?;
        } else {
            let mut cursor = owner.walk();
            for child in owner.named_children(&mut cursor) {
                if matches!(
                    child.kind(),
                    "function_value_parameters"
                        | "lambda_parameters"
                        | "parameter_list"
                        | "formal_parameters"
                        | "parameters"
                ) {
                    self.parameter_names(child, scope, check)?;
                }
                if self.language == "objective-c"
                    && matches!(child.kind(), "method_parameter" | "keyword_declarator")
                {
                    self.parameter_names(child, scope, check)?;
                }
            }
        }
        Ok(())
    }

    /// Follow parameter binding fields, excluding type/default-expression identifiers.
    fn parameter_names<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if self.language == "objective-c" && node.kind() == "keyword_declarator" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "method_parameter" {
                    self.parameter_names(child, scope, check)?;
                }
            }
            return Ok(());
        }
        if self.language == "objective-c" && node.kind() == "method_parameter" {
            if let Some(declarator) = node.child_by_field_name("declarator") {
                return self.pattern_names(declarator, scope, 0, true, check);
            }
            let mut cursor = node.walk();
            if let Some(name) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
            {
                return self.pattern_names(name, scope, 0, true, check);
            }
            return Ok(());
        }
        if self.language == "kotlin" && node.kind() == "variable_declaration" {
            let mut cursor = node.walk();
            if let Some(name) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
            {
                return self.pattern_names(name, scope, 0, true, check);
            }
            return Ok(());
        }
        if let Some(name) = self.shadow_name(node) {
            self.bind(scope, name, 0, true, BindingTarget::Shadow);
            return Ok(());
        }
        for field in ["pattern", "name", "declarator", "left"] {
            if let Some(binding) = node.child_by_field_name(field) {
                return self.pattern_names(binding, scope, 0, true, check);
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(
                child.kind(),
                "type_annotation"
                    | "type_identifier"
                    | "predefined_type"
                    | "primitive_type"
                    | "function_type"
                    | "default_value"
                    | "assignment_expression"
            ) {
                continue;
            }
            self.parameter_names(child, scope, check)?;
        }
        Ok(())
    }

    /// Collect destructuring/pointer binding leaves without reading initializers.
    fn pattern_names<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        from: usize,
        reserves: bool,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if let Some(name) = self.shadow_name(node) {
            self.bind(scope, name, from, reserves, BindingTarget::Shadow);
            return Ok(());
        }
        if let Some(binding) = node
            .child_by_field_name("declarator")
            .or_else(|| node.child_by_field_name("pattern"))
            .or_else(|| node.child_by_field_name("left"))
            .or_else(|| node.child_by_field_name("value"))
        {
            return self.pattern_names(binding, scope, from, reserves, check);
        }
        if matches!(
            node.kind(),
            "tuple_pattern"
                | "list_pattern"
                | "pattern_list"
                | "expression_list"
                | "tuple"
                | "list"
                | "array_pattern"
                | "object_pattern"
                | "slice_pattern"
                | "tuple_struct_pattern"
                | "reference_pattern"
                | "rest_pattern"
                | "parenthesized_declarator"
                | "match_pattern"
                | "or_pattern"
                | "captured_pattern"
                | "struct_pattern"
                | "field_pattern"
                | "as_pattern_target"
                | "mut_pattern"
                | "ref_pattern"
                | "list_splat_pattern"
                | "dictionary_splat_pattern"
                | "parenthesized_variable_designation"
                | "structured_binding_declarator"
        ) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if node
                    .child_by_field_name("type")
                    .is_some_and(|kind| kind.id() == child.id())
                    || node
                        .child_by_field_name("condition")
                        .is_some_and(|condition| condition.id() == child.id())
                {
                    continue;
                }
                self.pattern_names(child, scope, from, reserves, check)?;
            }
        }
        Ok(())
    }

    /// Apply each native grammar's owning assignment/declaration fields.
    fn local_bindings<E>(
        &mut self,
        node: Node<'_>,
        mut scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.csharp()
            && matches!(node.kind(), "property_declaration" | "event_declaration")
            && csharp_explicit_interface_member(node)
        {
            return Ok(());
        }
        if self.language == "rust" && node.kind() == "const_item" && is_rust_associated_member(node)
        {
            return Ok(());
        }
        if self.ecmascript() && node.kind() == "for_in_statement" {
            if let Some(pattern) = node.child_by_field_name("left") {
                let from = node
                    .child_by_field_name("right")
                    .map_or(pattern.end_byte(), |right| right.end_byte());
                if let Some(kind) = node.child_by_field_name("kind") {
                    let owner = if kind.kind() == "var" {
                        self.value_declaration_scope(scope)
                    } else {
                        scope
                    };
                    if kind.kind() == "var" {
                        self.ecmascript_pattern_bindings(
                            pattern,
                            owner,
                            EcmaPatternEffect::Reserve,
                            check,
                        )?;
                        self.mutation_names(pattern, scope, from, check)?;
                    } else {
                        self.pattern_names(pattern, owner, from, true, check)?;
                    }
                } else {
                    self.mutation_names(pattern, scope, from, check)?;
                }
            }
            return Ok(());
        }
        if self.ecmascript()
            && matches!(
                node.kind(),
                "assignment_expression" | "augmented_assignment_expression" | "update_expression"
            )
        {
            if let Some(target) = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("argument"))
            {
                self.mutation_names(target, scope, node.end_byte(), check)?;
            }
            return Ok(());
        }
        let field = match (self.language, node.kind()) {
            ("rust", "let_declaration" | "match_arm") => Some("pattern"),
            ("python", "assignment" | "augmented_assignment" | "for_statement")
            | ("go", "short_var_declaration")
            | ("cpp", "lambda_capture_initializer")
            | ("csharp" | "c_sharp", "foreach_statement") => Some("left"),
            (
                "javascript" | "typescript" | "tsx" | "java" | "csharp" | "c_sharp",
                "variable_declarator",
            )
            | ("go", "var_spec" | "const_spec")
            | ("kotlin", "variable_declaration" | "catch_block")
            | (
                "csharp" | "c_sharp",
                "property_declaration"
                | "event_declaration"
                | "declaration_pattern"
                | "var_pattern"
                | "recursive_pattern"
                | "catch_declaration",
            )
            | ("zig", "variable_declaration")
            | ("rust", "const_item" | "static_item" | "const_parameter")
            | ("python", "named_expression") => Some("name"),
            ("c" | "cpp" | "objective-c", "declaration" | "init_declarator")
            | ("cpp", "for_range_loop") => Some("declarator"),
            ("go", "range_clause" | "receive_statement") => {
                let mut cursor = node.walk();
                if node.children(&mut cursor).any(|child| child.kind() == ":=") {
                    Some("left")
                } else {
                    None
                }
            }
            ("go", "type_switch_statement") | ("python", "except_clause" | "as_pattern") => {
                Some("alias")
            }
            ("javascript" | "typescript" | "tsx", "catch_clause") => Some("parameter"),
            _ => None,
        };
        if self.language == "python" && node.kind() == "delete_statement" {
            let binding_scope = self.value_declaration_scope(scope);
            let mut cursor = node.walk();
            for target in node.named_children(&mut cursor) {
                self.pattern_names(
                    target,
                    binding_scope,
                    node.end_byte(),
                    self.scopes[binding_scope].function,
                    check,
                )?;
            }
            return Ok(());
        }
        let Some(field) = field else {
            return Ok(());
        };
        let Some(pattern) = node.child_by_field_name(field).or_else(|| {
            ((self.language == "kotlin"
                && matches!(node.kind(), "variable_declaration" | "catch_block"))
                || (self.language == "zig" && node.kind() == "variable_declaration"))
                .then(|| {
                    let mut cursor = node.walk();
                    node.named_children(&mut cursor)
                        .find(|child| child.kind() == "identifier")
                })
                .flatten()
        }) else {
            return Ok(());
        };
        // A function prototype is a declaration, not a local function-pointer value.
        if matches!(self.language, "c" | "cpp" | "objective-c")
            && pattern.kind() == "function_declarator"
            && pattern
                .child_by_field_name("declarator")
                .is_some_and(|name| matches!(name.kind(), "identifier" | "qualified_identifier"))
        {
            return Ok(());
        }
        let evaluation_scope = scope;
        let python = self.language == "python";
        let var = self.ecmascript()
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "variable_declaration");
        if python || var {
            if python && node.kind() == "named_expression" {
                while self.scopes[scope].comprehension {
                    let Some(parent) = self.scopes[scope].parent else {
                        break;
                    };
                    scope = parent;
                }
            }
            scope = self.value_declaration_scope(scope);
        }
        if var {
            self.ecmascript_pattern_bindings(pattern, scope, EcmaPatternEffect::Reserve, check)?;
            // A valid bare declaration does not perform a lexical value write.
            if !crate::declarator_has_recovery_error(node)
                && node.child_by_field_name("value").is_none()
            {
                return Ok(());
            }
        }
        if python && node.kind() == "assignment" && node.child_by_field_name("type").is_some() {
            if let Some(name) = self.binding_name(pattern) {
                if !self.admit_binding() {
                    return Ok(());
                }
                let reserves = self.scopes[scope].function;
                let binding = self.scopes[scope].bindings.entry(name).or_default();
                binding.python_annotated = true;
                binding.reserves_scope |= reserves;
            }
            // A bare annotation does not write a value or replace an existing callable.
            if node.child_by_field_name("right").is_none() {
                return Ok(());
            }
        }
        if self.ecmascript()
            && let Some(parent) = node.parent()
            && crate::declaration_has_direct_callable_initializer(parent)
            && let Some(symbol) = self.declaration(parent)
            && let Some(name) = self.identifier(pattern)
            && self.declaration_binding_name(symbol).as_ref() == Some(&name)
        {
            let mut write_scope = self.binding_scope(if var { evaluation_scope } else { scope });
            while var && !self.scopes[write_scope].bindings.contains_key(&name) {
                let Some(parent) = self.scopes[write_scope].parent else {
                    break;
                };
                write_scope = parent;
            }
            self.bind(
                write_scope,
                name,
                node.end_byte(),
                true,
                BindingTarget::Declaration(symbol),
            );
            return Ok(());
        }
        if var {
            self.mutation_names(pattern, evaluation_scope, node.end_byte(), check)?;
            return Ok(());
        }
        let from = if self.language == "kotlin" && (scope == 0 || self.scopes[scope].class)
            || self.csharp()
                && matches!(
                    self.scopes[scope].native_container,
                    NativeContainerScope::CSharpType { .. }
                ) {
            // Kotlin declaration scopes reserve members independently of source order.
            0
        } else if matches!(self.language, "c" | "cpp" | "objective-c")
            || matches!(node.kind(), "except_clause" | "catch_block")
        {
            pattern.end_byte()
        } else if matches!(node.kind(), "foreach_statement" | "type_switch_statement")
            || (python && node.kind() == "for_statement")
        {
            node.child_by_field_name("right")
                .or_else(|| node.child_by_field_name("value"))
                .map_or(pattern.end_byte(), |value| value.end_byte())
        } else if self.language == "rust"
            && matches!(
                node.kind(),
                "match_arm" | "const_item" | "static_item" | "const_parameter"
            )
        {
            0
        } else {
            node.end_byte()
        };
        self.pattern_names(
            pattern,
            scope,
            from,
            (python && self.scopes[scope].function)
                || self.ecmascript()
                || (matches!(self.language, "csharp" | "c_sharp")
                    && node.kind() != "foreach_statement"),
            check,
        )
    }

    /// Zig payloads own the following branch, continuation and body, never a sibling.
    fn zig_payload_scope<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let mut scope = parent;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            check()?;
            if child.kind() == "else" || child.kind() == "else_clause" {
                scope = parent;
            }
            if child.kind() == "payload" {
                if !self.admit_scope() {
                    return Ok(());
                }
                scope = self.scopes.len();
                self.scopes.push(Scope {
                    parent: Some(parent),
                    owner: self.scopes[parent].owner,
                    ..Scope::default()
                });
                let mut payload_cursor = child.walk();
                for name in child.named_children(&mut payload_cursor) {
                    if let Some(name) = self.binding_name(name) {
                        self.bind(scope, name, 0, false, BindingTarget::Shadow);
                    }
                }
            } else if child.is_named() {
                self.visit(child, scope, check)?;
            }
        }
        Ok(())
    }

    /// Evaluate each iterable outside its own binder and the body inside all binders.
    fn python_comprehension_scope<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let body = node.child_by_field_name("body");
        let mut scope = parent;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            check()?;
            if body.is_some_and(|body| body.id() == child.id()) {
                continue;
            }
            if child.kind() != "for_in_clause" {
                self.visit(child, scope, check)?;
                continue;
            }
            let previous = scope;
            if !self.admit_scope() {
                return Ok(());
            }
            scope = self.scopes.len();
            let lookup_parent = self.python_closure_parent(previous);
            self.scopes.push(Scope {
                parent: Some(lookup_parent),
                function: true,
                comprehension: true,
                enclosing_visibility_floor: if node.kind() == "generator_expression" {
                    node.end_byte()
                } else {
                    0
                },
                owner: self.scopes[previous].owner,
                ..Scope::default()
            });
            if let Some(pattern) = child.child_by_field_name("left") {
                self.pattern_names(pattern, scope, 0, false, check)?;
            }
            let mut clause_cursor = child.walk();
            for expression in child.named_children(&mut clause_cursor) {
                let binding = child
                    .child_by_field_name("left")
                    .is_some_and(|left| left.id() == expression.id());
                self.visit(expression, if binding { scope } else { previous }, check)?;
            }
        }
        if let Some(body) = body {
            self.visit(body, scope, check)?;
        }
        Ok(())
    }

    /// Rust conditional/iterator patterns bind only the condition tail and body.
    fn rust_control_scope<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if !self.admit_scope() {
            return Ok(());
        }
        let scope = self.scopes.len();
        self.scopes.push(Scope {
            parent: Some(parent),
            owner: self.scopes[parent].owner,
            ..Scope::default()
        });
        if let Some(condition) = node.child_by_field_name("condition") {
            self.rust_condition_bindings(condition, scope, check)?;
        }
        if let Some(pattern) = node.child_by_field_name("pattern") {
            self.pattern_names(
                pattern,
                scope,
                node.child_by_field_name("value")
                    .map_or(node.start_byte(), |value| value.end_byte()),
                false,
                check,
            )?;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let branch = ["condition", "consequence", "body"].iter().any(|field| {
                node.child_by_field_name(field)
                    .is_some_and(|owned| owned.id() == child.id())
            });
            self.visit(child, if branch { scope } else { parent }, check)?;
        }
        Ok(())
    }

    /// Let-chain patterns become visible after their own initializer, in order.
    fn rust_condition_bindings<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if node.kind() == "let_condition"
            && let Some(pattern) = node.child_by_field_name("pattern")
        {
            return self.pattern_names(pattern, scope, node.end_byte(), false, check);
        }
        if node.kind() == "let_chain" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.rust_condition_bindings(child, scope, check)?;
            }
        }
        Ok(())
    }

    /// Python match capture roles exclude dotted constants and class/keyword names.
    fn python_match_bindings<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if node.kind() == "dotted_name" {
            let mut cursor = node.walk();
            let mut children = node.named_children(&mut cursor);
            if let Some(child) = children.next()
                && children.next().is_none()
                && let Some(name) = self.identifier(child)
                && name != "_"
            {
                self.bind(scope, name, node.end_byte(), true, BindingTarget::Shadow);
            }
            return Ok(());
        }
        if let Some(alias) = node.child_by_field_name("alias") {
            self.pattern_names(alias, scope, node.end_byte(), true, check)?;
        }
        if !matches!(
            node.kind(),
            "case_pattern"
                | "class_pattern"
                | "keyword_pattern"
                | "as_pattern"
                | "list_pattern"
                | "tuple_pattern"
                | "union_pattern"
                | "dict_pattern"
                | "splat_pattern"
        ) {
            return Ok(());
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if node.kind() == "class_pattern" && child.kind() == "dotted_name" {
                continue;
            }
            self.python_match_bindings(child, scope, check)?;
        }
        Ok(())
    }

    /// Preserve explicit declaration slots and unresolved default-slot collisions.
    fn direct_export<E>(
        &mut self,
        node: Node<'_>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = node.walk();
        let mut is_default = false;
        for child in node.children(&mut cursor) {
            check()?;
            is_default |= child.kind() == crate::semantic::DEFAULT_EXPORT_SLOT;
        }
        if !is_default {
            self.export_clauses(node, check)?;
        }
        let export = if is_default {
            self.export_slot(
                crate::semantic::DEFAULT_EXPORT_SLOT.to_string(),
                node,
                crate::SourceExportBinding::Unresolved,
                crate::SourceExportNamespace::Value,
            )
        } else {
            None
        };
        let Some(declaration) = node.child_by_field_name("declaration") else {
            return Ok(());
        };
        if !is_default
            && matches!(
                declaration.kind(),
                "lexical_declaration" | "variable_declaration"
            )
        {
            let owners = if let Some(name) = node_name(declaration, self.content) {
                if let Some(symbols) = self.declarations.get(&(
                    declaration.start_position().row + 1,
                    declaration.end_position().row + 1,
                    name.as_str(),
                    declaration.kind(),
                )) {
                    Arc::clone(
                        self.export_declaration_groups
                            .entry(symbols.as_ptr())
                            .or_insert_with(|| symbols.as_slice().to_vec().into()),
                    )
                } else {
                    Arc::<[usize]>::from([])
                }
            } else {
                Arc::<[usize]>::from([])
            };
            let mut cursor = declaration.walk();
            for declarator in declaration.named_children(&mut cursor) {
                check()?;
                if declarator.kind() == "variable_declarator"
                    && let Some(pattern) = declarator.child_by_field_name("name")
                {
                    self.export_binding_pattern(pattern, !declarator.has_error(), &owners, check)?;
                }
            }
            return Ok(());
        }
        if !matches!(
            declaration_kind(declaration.kind()),
            Some(SymbolKind::Function | SymbolKind::Class)
        ) {
            return Ok(());
        }
        let Some(name) = node_name(declaration, self.content) else {
            return Ok(());
        };
        let Some(symbols) = self.declarations.get(&(
            declaration.start_position().row + 1,
            declaration.end_position().row + 1,
            name.as_str(),
            declaration.kind(),
        )) else {
            return Ok(());
        };
        let valid_binding = declaration
            .child_by_field_name("name")
            .and_then(|name| self.binding_name(name))
            .is_some();
        let binding = if !node.has_error()
            && valid_binding
            && let Some(ordinal) = self.declaration(declaration)
        {
            crate::SourceExportBinding::LocalDeclaration(ordinal)
        } else {
            if !self.export_ambiguities.insert(symbols.as_ptr()) {
                return Ok(());
            }
            crate::SourceExportBinding::AmbiguousDeclarations(symbols.as_slice().to_vec().into())
        };
        if let Some(export) = export {
            self.exports[export].binding = binding;
        } else if !is_default
            && let Some(name) = crate::semantic::ecmascript_identifier_value(&name)
        {
            self.export_slot(name, node, binding, crate::SourceExportNamespace::Value);
        }
        Ok(())
    }

    /// Retain declared export names; the existing module binding owner proves callability.
    fn export_binding_pattern<E>(
        &mut self,
        node: Node<'_>,
        valid: bool,
        owners: &Arc<[usize]>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        if self.exports_incomplete {
            return Ok(());
        }
        let binding = self.binding_name(node);
        let binding_valid = binding.is_some();
        let name = binding.or_else(|| {
            node.utf8_text(self.content.as_bytes())
                .ok()
                .and_then(crate::semantic::ecmascript_identifier_value)
        });
        if let Some(name) = name {
            let index = self.export_slot(
                name.clone(),
                node,
                if owners.is_empty() {
                    crate::SourceExportBinding::Unresolved
                } else {
                    crate::SourceExportBinding::UnresolvedDeclarations(Arc::clone(owners))
                },
                crate::SourceExportNamespace::Value,
            );
            if valid
                && binding_valid
                && !node.has_error()
                && let Some(index) = index
            {
                self.local_exports.push((index, name));
            }
            return Ok(());
        }
        if let Some(binding) = node
            .child_by_field_name("left")
            .or_else(|| node.child_by_field_name("value"))
        {
            return self.export_binding_pattern(binding, valid, owners, check);
        }
        if matches!(
            node.kind(),
            "object_pattern" | "array_pattern" | "rest_pattern"
        ) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.export_binding_pattern(child, valid, owners, check)?;
            }
        }
        Ok(())
    }

    /// Capture explicit forwarding slots without introducing local import bindings.
    fn export_clauses<E>(
        &mut self,
        node: Node<'_>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let module =
            crate::semantic::ecmascript_source_module(node, self.content).map(Arc::<str>::from);
        let mut cursor = node.walk();
        let type_only = node
            .children(&mut cursor)
            .any(|token| !token.is_named() && token.kind() == "type");
        let mut cursor = node.walk();
        for clause in node.named_children(&mut cursor) {
            check()?;
            if clause.kind() == "namespace_export" {
                let mut names = clause.walk();
                for name in clause.children(&mut names).filter(|name| {
                    matches!(name.kind(), "identifier" | "string")
                        || name.kind() == crate::semantic::DEFAULT_EXPORT_SLOT
                }) {
                    check()?;
                    if let Some(name) = crate::semantic::ecmascript_export_name(name, self.content)
                    {
                        let binding = module
                            .as_ref()
                            .filter(|_| !type_only && !clause.has_error())
                            .map_or(crate::SourceExportBinding::Unresolved, |module| {
                                crate::SourceExportBinding::ReExport(
                                    crate::ImportReference::with_shared_module(
                                        crate::ImportSyntax::EcmaScript,
                                        Arc::clone(module),
                                        None,
                                        &name,
                                    ),
                                )
                            });
                        self.export_slot(
                            name,
                            clause,
                            binding,
                            if type_only {
                                crate::SourceExportNamespace::Type
                            } else {
                                crate::SourceExportNamespace::Value
                            },
                        );
                    }
                }
            } else if clause.kind() == "export_clause" {
                let mut items = clause.walk();
                for item in clause.named_children(&mut items) {
                    check()?;
                    if item.kind() != "export_specifier" {
                        continue;
                    }
                    let Some(source) = item.child_by_field_name("name") else {
                        continue;
                    };
                    let Some(name) = crate::semantic::ecmascript_export_name(
                        item.child_by_field_name("alias").unwrap_or(source),
                        self.content,
                    ) else {
                        continue;
                    };
                    let mut tokens = item.walk();
                    let type_only = type_only
                        || item
                            .children(&mut tokens)
                            .any(|token| !token.is_named() && token.kind() == "type");
                    let valid = !type_only && !item.has_error();
                    let binding = if let Some(module) = module.as_ref().filter(|_| valid) {
                        crate::semantic::ecmascript_export_name(source, self.content).map_or(
                            crate::SourceExportBinding::Unresolved,
                            |imported| {
                                crate::SourceExportBinding::ReExport(
                                    crate::ImportReference::with_shared_module(
                                        crate::ImportSyntax::EcmaScript,
                                        Arc::clone(module),
                                        Some(&imported),
                                        &name,
                                    ),
                                )
                            },
                        )
                    } else {
                        crate::SourceExportBinding::Unresolved
                    };
                    let index = self.export_slot(
                        name,
                        item,
                        binding,
                        if type_only {
                            crate::SourceExportNamespace::Type
                        } else {
                            crate::SourceExportNamespace::Value
                        },
                    );
                    if let Some(index) = index
                        && valid
                        && module.is_none()
                        && node.child_by_field_name("source").is_none()
                        && let Some(local) = self.binding_name(source)
                    {
                        self.local_exports.push((index, local));
                    }
                }
            }
        }
        Ok(())
    }

    /// Retain source occurrence even when its static target is unknown.
    fn export_slot(
        &mut self,
        name: String,
        node: Node<'_>,
        binding: crate::SourceExportBinding,
        namespace: crate::SourceExportNamespace,
    ) -> Option<usize> {
        if self.exports_incomplete {
            return None;
        }
        if self.exports.len() >= crate::MAX_RELATIONS_PER_FILE {
            self.exports_incomplete = true;
            self.exports.clear();
            self.local_exports.clear();
            self.export_scopes.clear();
            return None;
        }
        let index = self.exports.len();
        self.exports.push(crate::SourceExportFact {
            namespace,
            name,
            span: crate::SourceByteSpan {
                start: node.start_byte(),
                end: node.end_byte(),
            },
            line: node.start_position().row + 1,
            binding,
        });
        self.export_scopes.push(self.ecma_root);
        Some(index)
    }

    /// Local export clauses link module bindings, not runtime reads at their offset.
    fn link_local_exports<E>(
        &mut self,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        for (export, name) in &self.local_exports {
            check()?;
            let scope = self.export_scopes[*export];
            let Some(bindings) = self.scopes[scope].bindings.get(name) else {
                continue;
            };
            if bindings.runtime_mutated {
                continue;
            }
            let Some(binding) = bindings.transitions.last() else {
                continue;
            };
            self.exports[*export].binding = match &binding.target {
                BindingTarget::Declaration(symbol) => {
                    crate::SourceExportBinding::LocalDeclaration(*symbol)
                }
                BindingTarget::Declarations(symbols) => {
                    crate::SourceExportBinding::AmbiguousDeclarations(Arc::clone(symbols))
                }
                BindingTarget::Import { import, reference } => self
                    .facts
                    .imports
                    .get(*import)
                    .and_then(|fact| fact.references.get(*reference))
                    .cloned()
                    .map_or(
                        crate::SourceExportBinding::Unresolved,
                        crate::SourceExportBinding::ReExport,
                    ),
                BindingTarget::Shadow
                | BindingTarget::PythonOwner(_)
                | BindingTarget::RustModule(_)
                | BindingTarget::RustNamespaceLookup(_)
                | BindingTarget::RustValueLookup(_)
                | BindingTarget::Unresolved
                | BindingTarget::Imports { .. } => continue,
            };
        }
        for export in 0..self.exports.len() {
            check()?;
            let crate::SourceExportBinding::LocalDeclaration(symbol) = self.exports[export].binding
            else {
                continue;
            };
            let Some(name) = self.declaration_binding_name(symbol) else {
                if self.ecmascript() {
                    self.exports[export].binding =
                        crate::SourceExportBinding::UnresolvedDeclarations(vec![symbol].into());
                }
                continue;
            };
            let scope = if self.ecmascript() {
                match &self.declaration_names[symbol] {
                    DeclarationBindingName::Admitted { scope, .. } => *scope,
                    DeclarationBindingName::Unseen | DeclarationBindingName::Rejected => {
                        self.export_scopes[export]
                    }
                }
            } else {
                0
            };
            if self.scopes[scope]
                .bindings
                .get(&name)
                .is_some_and(|bindings| {
                    bindings.runtime_mutated
                        || bindings.transitions.last().is_some_and(|binding| {
                            binding.target != BindingTarget::Declaration(symbol)
                        })
                })
            {
                self.exports[export].binding =
                    crate::SourceExportBinding::UnresolvedDeclarations(vec![symbol].into());
            }
        }
        if self.ecmascript() {
            for (export, scope) in self.exports.iter_mut().zip(&self.export_scopes) {
                check()?;
                let owner = &self.scopes[*scope];
                let admitted = owner
                    .ecma_role
                    .map_or(owner.ecma_context.is_module(), |role| {
                        role.admits_export(&export.name)
                    });
                if !admitted {
                    export.binding = match std::mem::replace(
                        &mut export.binding,
                        crate::SourceExportBinding::Unresolved,
                    ) {
                        crate::SourceExportBinding::LocalDeclaration(symbol) => {
                            crate::SourceExportBinding::UnresolvedDeclarations(vec![symbol].into())
                        }
                        crate::SourceExportBinding::AmbiguousDeclarations(symbols)
                        | crate::SourceExportBinding::UnresolvedDeclarations(symbols) => {
                            crate::SourceExportBinding::UnresolvedDeclarations(symbols)
                        }
                        _ => crate::SourceExportBinding::Unresolved,
                    };
                }
            }
        }
        Ok(())
    }

    /// Extract a static root name and an optional explicit non-computed member.
    fn call_name(&self, node: Node<'_>) -> Option<(String, Option<String>)> {
        if self.language == "java" && node.kind() == "method_invocation" {
            let name = node.child_by_field_name("name")?;
            return node.child_by_field_name("object").map_or_else(
                || Some((self.identifier(name)?, None)),
                |object| Some((self.identifier(object)?, Some(self.identifier(name)?))),
            );
        }
        let target = call_target(node)?;
        self.target_name(target)
    }

    /// Inspect a callee expression or proven callback using the same grammar roles.
    fn target_name(&self, target: Node<'_>) -> Option<(String, Option<String>)> {
        if self.language == "rust" && target.kind() == "scoped_identifier" {
            let (root, members) = self.rust_path_parts(target)?;
            return Some((root, Some(members.join("::"))));
        }
        if let Some(name) = self.reference_name(target) {
            return Some((name, None));
        }
        let (object, member) = match target.kind() {
            "member_expression" => (
                target.child_by_field_name("object")?,
                target.child_by_field_name("property")?,
            ),
            "attribute" => (
                target.child_by_field_name("object")?,
                target.child_by_field_name("attribute")?,
            ),
            "field_expression" => (
                target.child_by_field_name("value")?,
                target.child_by_field_name("field")?,
            ),
            "selector_expression" => (
                target.child_by_field_name("operand")?,
                target.child_by_field_name("field")?,
            ),
            "scoped_identifier" | "qualified_identifier" => (
                target
                    .child_by_field_name("path")
                    .or_else(|| target.child_by_field_name("scope"))?,
                target.child_by_field_name("name")?,
            ),
            _ => return None,
        };
        Some((self.reference_name(object)?, Some(self.identifier(member)?)))
    }

    /// Preserve only explicit anchored paths and roots with a known toolchain identity.
    fn rust_qualified_reference(&self, target: Node<'_>) -> Option<crate::ImportReference> {
        if self.language != "rust" || target.kind() != "scoped_identifier" {
            return None;
        }
        if let Some(path) = crate::rust_use_source::rust_path(target, self.content, 0)
            && path.starts_with("::")
            && let Some((module, member)) = path.rsplit_once("::")
        {
            return Some(crate::ImportReference::new(
                crate::ImportSyntax::Rust,
                module,
                Some(member),
                "::",
            ));
        }
        let (root, mut parts) = self.rust_path_parts(target)?;
        if !matches!(
            root.as_str(),
            "crate" | "self" | "super" | "std" | "core" | "alloc"
        ) {
            return None;
        }
        let member = parts.pop()?;
        parts.insert(0, root.clone());
        Some(crate::ImportReference::new(
            crate::ImportSyntax::Rust,
            &parts.join("::"),
            Some(&member),
            &root,
        ))
    }

    /// Retain each grammar-owned component, including imported namespace members.
    fn rust_path_parts(&self, mut target: Node<'_>) -> Option<(String, Vec<String>)> {
        let mut parts = Vec::new();
        while target.kind() == "scoped_identifier" {
            if parts.len() >= 128 {
                return None;
            }
            parts.push(self.identifier(target.child_by_field_name("name")?)?);
            target = target.child_by_field_name("path")?;
        }
        let root = if matches!(target.kind(), "crate" | "self" | "super") {
            self.content.get(target.byte_range())?.to_string()
        } else {
            self.identifier(target)?
        };
        parts.reverse();
        Some((root, parts))
    }

    /// Follow the actual declarator name without reading a signature or type text.
    fn cpp_declaration_visibility(mut node: Node<'_>) -> usize {
        for _ in 0..128 {
            if matches!(
                node.kind(),
                "identifier" | "type_identifier" | "field_identifier"
            ) {
                return node.end_byte();
            }
            let Some(name) = node
                .child_by_field_name("declarator")
                .or_else(|| node.child_by_field_name("name"))
            else {
                break;
            };
            node = name;
        }
        node.end_byte()
    }

    /// Retain a whole visible overload group or abstain for a partial group.
    fn cpp_value_visibility(&self, target: &BindingTarget) -> Option<(usize, usize)> {
        match target {
            BindingTarget::Declaration(symbol) => self
                .cpp_function_visibility
                .get(symbol)
                .map(|from| (*from, *from)),
            BindingTarget::Declarations(symbols) => {
                self.cpp_group_visibility.get(&symbols.as_ptr()).copied()
            }
            _ => None,
        }
    }

    /// Retain only an import-only group in the winning lexical environment.
    fn rust_import_candidates<E>(
        &self,
        site: &CallSite,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<crate::SourceRustCallImports>, E> {
        let Some(name) = &site.name else {
            return Ok(None);
        };
        let mut current = Some(site.scope);
        let mut offset = site.offset;
        while let Some(scope) = current {
            check()?;
            let names = if site.member.is_some() {
                &self.scopes[scope].namespaces
            } else {
                &self.scopes[scope].bindings
            };
            if let Some(bindings) = names.get(name) {
                let end = bindings
                    .transitions
                    .partition_point(|binding| binding.from <= offset);
                if end > 0 {
                    return Ok(match &bindings.transitions[end - 1].target {
                        BindingTarget::Imports {
                            bindings,
                            declarations,
                        } => Some(crate::SourceRustCallImports {
                            relation: self.facts.calls[site.fact].relation,
                            bindings: Arc::clone(bindings),
                            declarations: Arc::clone(declarations),
                            member: site.member.clone(),
                        }),
                        _ => None,
                    });
                }
                if bindings.reserves_scope {
                    return Ok(None);
                }
            }
            offset = offset.max(self.scopes[scope].enclosing_visibility_floor);
            if self.language == "rust"
                && matches!(
                    self.scopes[scope].rust_item_owner,
                    RustItemOwner::Module | RustItemOwner::UnsupportedModule
                )
            {
                break;
            }
            current = self.scopes[scope].parent;
        }
        Ok(None)
    }

    /// Convert an already selected lexical target without changing its authority.
    fn call_binding(
        &self,
        target: &BindingTarget,
        site: &CallSite,
        name: &str,
    ) -> SourceCallBinding {
        match target {
            BindingTarget::Declaration(symbol) if site.member.is_none() => {
                SourceCallBinding::LocalDeclaration(*symbol)
            }
            BindingTarget::Declarations(symbols) if site.member.is_none() => {
                SourceCallBinding::LocalDeclarations(Arc::clone(symbols))
            }
            BindingTarget::Import { import, reference } => SourceCallBinding::Imported {
                import: *import,
                reference: *reference,
                member: site.member.clone(),
                rust_toolchain: self.toolchain_call_reference(
                    *import,
                    *reference,
                    site.member.as_deref(),
                ),
            },
            BindingTarget::RustModule(module) if site.member.is_some() => site
                .member
                .as_ref()
                .and_then(|member| {
                    crate::SourceRustPath::from_ast_path(&format!("self::{name}::{member}"))
                })
                .map_or(SourceCallBinding::Unresolved, |path| {
                    SourceCallBinding::RustPath(crate::SourceRustCall {
                        path,
                        module: Arc::clone(module),
                        origin: crate::SourceRustCallOrigin::DeclaredModule,
                    })
                }),
            BindingTarget::Shadow => SourceCallBinding::Shadowed,
            BindingTarget::Declaration(_)
            | BindingTarget::Declarations(_)
            | BindingTarget::PythonOwner(_)
            | BindingTarget::RustModule(_)
            | BindingTarget::RustNamespaceLookup(_)
            | BindingTarget::RustValueLookup(_)
            | BindingTarget::Unresolved
            | BindingTarget::Imports { .. } => SourceCallBinding::Unresolved,
        }
    }

    /// Nearest visible binding wins; a missing or dynamic owner cannot authorize fallback.
    fn resolve<E>(
        &self,
        site: &CallSite,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<SourceCallBinding, E> {
        if self.language == "rust"
            && self.scopes[site.scope].rust_item_owner == RustItemOwner::UnsupportedModule
        {
            return Ok(SourceCallBinding::Unresolved);
        }
        let Some(name) = site.name.as_ref() else {
            return Ok(SourceCallBinding::Unresolved);
        };
        let mut current = Some(site.scope);
        let mut offset = site.offset;
        let mut deferred = false;
        let mut python_writer = None;
        'lookup: while let Some(scope) = current {
            check()?;
            if self.language == "cpp" {
                if let Some(target) = self.cpp_namespace_target(scope, name, offset, check)? {
                    return Ok(self.call_binding(target, site, name));
                }
                current = self.scopes[scope].parent;
                continue;
            }
            if site.method_name
                && let Some(arity) = self.scopes[scope].java_implicit_methods.get(name)
                && (site.argument_count.is_none_or(|actual| actual == *arity)
                    || !self.scopes[scope].methods.contains_key(name))
            {
                return Ok(SourceCallBinding::Shadowed);
            }
            let bindings = if site.method_name {
                self.scopes[scope].methods.get(name)
            } else if self.language == "rust" && site.member.is_some() {
                self.scopes[scope].namespaces.get(name)
            } else {
                self.scopes[scope].bindings.get(name)
            };
            let exported = self.scopes[scope]
                .namespace_exports
                .and_then(|shared| self.scopes[shared].bindings.get(name));
            for bindings in bindings.into_iter().chain(exported) {
                if bindings.runtime_mutated
                    && (python_writer.is_none() || python_writer != bindings.python_writer)
                {
                    return Ok(SourceCallBinding::Unresolved);
                }
                let mut end = bindings
                    .transitions
                    .partition_point(|binding| binding.from <= offset);
                if deferred {
                    if end.saturating_sub(1) < bindings.stable_tail {
                        return Ok(SourceCallBinding::Unresolved);
                    }
                    end = bindings.transitions.len();
                }
                if end == 0 {
                    if bindings.reserves_scope {
                        return Ok(SourceCallBinding::Shadowed);
                    }
                } else {
                    let target = &bindings.transitions[end - 1].target;
                    if let BindingTarget::PythonOwner(owner) = target {
                        python_writer = Some(scope);
                        let floor = self.scopes[scope].enclosing_visibility_floor;
                        offset = offset.max(floor);
                        deferred |= floor != 0;
                        current = Some(*owner);
                        continue 'lookup;
                    }
                    if let BindingTarget::RustNamespaceLookup(module)
                    | BindingTarget::RustValueLookup(module) = target
                    {
                        let path = site.member.as_ref().map_or_else(
                            || format!("self::{name}"),
                            |member| format!("self::{name}::{member}"),
                        );
                        return Ok(crate::SourceRustPath::from_ast_path(&path).map_or(
                            SourceCallBinding::Unresolved,
                            |path| {
                                SourceCallBinding::RustPath(crate::SourceRustCall {
                                    path,
                                    module: Arc::clone(module),
                                    origin: crate::SourceRustCallOrigin::ModuleLookup,
                                })
                            },
                        ));
                    }
                    return Ok(self.call_binding(target, site, name));
                }
            }
            let floor = self.scopes[scope].enclosing_visibility_floor;
            offset = offset.max(floor);
            deferred |= floor != 0;
            if self.language == "rust"
                && matches!(
                    self.scopes[scope].rust_item_owner,
                    RustItemOwner::Module | RustItemOwner::UnsupportedModule
                )
            {
                break;
            }
            current = self.scopes[scope].parent;
        }
        Ok(site
            .qualified
            .clone()
            .map_or(SourceCallBinding::Unresolved, |reference| {
                if matches!(reference.local(), "crate" | "self" | "super" | "::") {
                    let path = reference.imported().map_or_else(
                        || reference.module().to_string(),
                        |member| format!("{}::{member}", reference.module()),
                    );
                    crate::SourceRustPath::from_ast_path(&path).map_or(
                        SourceCallBinding::Unresolved,
                        |path| {
                            SourceCallBinding::RustPath(crate::SourceRustCall {
                                path,
                                module: Arc::clone(&site.rust_module),
                                origin: crate::SourceRustCallOrigin::AnchoredPath,
                            })
                        },
                    )
                } else {
                    SourceCallBinding::Qualified(reference)
                }
            }))
    }
}

/// The actual callee field, preserving the producer's grammar-specific fallback.
fn call_target(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("function")
        .or_else(|| crate::first_named_child(node))
}

/// Identify Python value reads without treating parameter/import/member labels as uses.
/// Native traversal already bounds ancestry depth; this never parses source again.
fn python_identifier_is_read(mut node: Node<'_>) -> bool {
    let mut qualified_pattern = false;
    while let Some(parent) = node.parent() {
        let field_is = |field| {
            parent
                .child_by_field_name(field)
                .is_some_and(|child| child.id() == node.id())
        };
        match parent.kind() {
            "global_statement"
            | "nonlocal_statement"
            | "import_statement"
            | "import_from_statement"
            | "aliased_import"
            | "parameters"
            | "lambda_parameters"
            | "type" => return false,
            "function_definition"
            | "class_definition"
            | "keyword_argument"
            | "named_expression"
                if field_is("name") =>
            {
                return false;
            }
            "attribute" => return !field_is("attribute"),
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause"
                if field_is("left") =>
            {
                return false;
            }
            "default_parameter" | "typed_default_parameter" if field_is("value") => return true,
            "as_pattern" if field_is("alias") => return false,
            "class_pattern" => {
                return parent
                    .named_child(0)
                    .is_some_and(|child| child.id() == node.id());
            }
            "keyword_pattern"
                if parent
                    .named_child(0)
                    .is_some_and(|child| child.id() == node.id()) =>
            {
                return false;
            }
            "case_pattern" => return qualified_pattern,
            "dotted_name" => {
                let mut cursor = parent.walk();
                let mut names = parent.named_children(&mut cursor);
                if names.next().is_some_and(|first| first.id() != node.id()) {
                    return false;
                }
                qualified_pattern |= names.next().is_some();
            }
            "block" | "module" => return true,
            _ => {}
        }
        node = parent;
    }
    true
}

/// Instance field values execute at construction, unless their class is constructed here.
fn deferred_instance_field(node: Node<'_>) -> bool {
    if !matches!(node.kind(), "field_definition" | "public_field_definition") {
        return false;
    }
    let mut cursor = node.walk();
    if node
        .children(&mut cursor)
        .any(|child| child.kind() == "static")
    {
        return false;
    }
    node.parent()
        .filter(|body| body.kind() == "class_body")
        .and_then(|body| body.parent())
        .filter(|class| matches!(class.kind(), "class" | "class_declaration"))
        .is_some_and(|class| !immediately_invoked_expression(class))
}

/// Direct calls/constructions execute at their source position through transparent wrappers.
fn immediately_invoked_expression(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression"
            | "type_assertion" => node = parent,
            "call" | "call_expression" | "new_expression" => {
                return call_target(parent).is_some_and(|callee| callee.id() == node.id());
            }
            _ => return false,
        }
    }
    false
}

/// Function environments include anonymous functions and language-owned lambdas.
fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_definition"
            | "method_definition"
            | "method_declaration"
            | "function_declaration_with_receiver"
            | "constructor_declaration"
            | "arrow_function"
            | "function_expression"
            | "function"
            | "generator_function"
            | "lambda_expression"
            | "lambda_literal"
            | "func_literal"
            | "closure_expression"
            | "lambda"
    )
}

/// Explicit interface implementations are unavailable through ordinary class-name lookup.
fn csharp_explicit_interface_member(node: Node<'_>) -> bool {
    node.named_children(&mut node.walk())
        .any(|child| child.kind() == "explicit_interface_specifier")
}

/// C# primary parameters outrank members only in instance initialization/base arguments.
fn csharp_initializer_child(node: Node<'_>, child: Node<'_>, content: &str) -> bool {
    let static_owner = |owner: Node<'_>| {
        owner.named_children(&mut owner.walk()).any(|modifier| {
            modifier.kind() == "modifier"
                && modifier.utf8_text(content.as_bytes()).ok() == Some("static")
        })
    };
    if matches!(
        node.kind(),
        "class_declaration" | "struct_declaration" | "record_declaration"
    ) {
        return child.kind() == "base_list";
    }
    if node.kind() == "property_declaration" {
        return !static_owner(node)
            && child.kind() != "arrow_expression_clause"
            && node
                .child_by_field_name("value")
                .is_some_and(|value| value.id() == child.id());
    }
    node.kind() == "variable_declarator"
        && node
            .child_by_field_name("name")
            .is_none_or(|name| name.id() != child.id())
        && node
            .parent()
            .and_then(|declaration| declaration.parent())
            .is_some_and(|owner| {
                matches!(
                    owner.kind(),
                    "field_declaration" | "event_field_declaration"
                ) && !static_owner(owner)
            })
}

/// Only an explicit Kotlin inner modifier retains an enclosing instance environment.
fn kotlin_inner_classifier(node: Node<'_>, content: &str) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "modifiers")
        .any(|modifiers| {
            let mut cursor = modifiers.walk();
            modifiers.named_children(&mut cursor).any(|modifier| {
                modifier.kind() == "class_modifier"
                    && modifier.utf8_text(content.as_bytes()).ok() == Some("inner")
            })
        })
}

/// Direct impl/trait members require qualification rather than ordinary lexical lookup.
fn is_rust_associated_member(node: Node<'_>) -> bool {
    node.parent()
        .and_then(|body| body.parent())
        .is_some_and(|owner| matches!(owner.kind(), "impl_item" | "trait_item"))
}

/// Type/module containers prevent member bindings from leaking into outer scopes.
fn is_class(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "class"
            | "class_definition"
            | "class_specifier"
            | "struct_item"
            | "struct_specifier"
            | "union_specifier"
            | "struct_declaration"
            | "impl_item"
            | "mod_item"
            | "namespace_definition"
            | "namespace_declaration"
    )
}

/// Python conditional imports do not prove an executed binding outside that branch.
fn has_conditional_owner(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if is_function(parent.kind()) {
            return false;
        }
        if matches!(
            parent.kind(),
            "if_statement"
                | "try_statement"
                | "for_statement"
                | "while_statement"
                | "match_statement"
        ) {
            return true;
        }
        node = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::SourceCallBinding;
    use crate::extract_symbol_graph_with_semantics_controlled;
    use projectatlas_core::IndexWorkControl;

    /// Use a host role that actually establishes module-export authority.
    fn ecmascript_module_fixture(language: &str, source: &str) -> String {
        let attributes = match language {
            "html" => " type=\"module\"",
            "svelte" => " module",
            _ => "",
        };
        format!("<script{attributes}>{source}</script>")
    }

    #[test]
    fn reexport_module_storage_is_shared_across_slots_and_consumer_clones() {
        let module = format!("./{}", "a".repeat(65_536));
        let slots = (0..64)
            .map(|index| format!("work as slot{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("export {{ {slots} }} from '{module}';");
        for language in ["javascript", "typescript", "tsx", "vue", "svelte", "html"] {
            let content = if matches!(language, "vue" | "svelte" | "html") {
                ecmascript_module_fixture(language, &source)
            } else {
                source.clone()
            };
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "barrel.ts",
                Some(language),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            let facts = extracted.semantics();
            assert_eq!(facts.exports().len(), 64, "{language}");
            assert!(facts.exports_complete(), "{language}");
            let mut allocation = None;
            for export in facts.exports() {
                assert!(
                    matches!(export.binding, crate::SourceExportBinding::ReExport(_)),
                    "{language}: export lost its static module binding"
                );
                let crate::SourceExportBinding::ReExport(reference) = &export.binding else {
                    return;
                };
                assert_eq!(reference.module(), module, "{language}");
                assert_eq!(reference.imported(), Some("work"), "{language}");
                assert_eq!(reference.local(), export.name, "{language}");
                let pointer = reference.module().as_ptr();
                assert_eq!(*allocation.get_or_insert(pointer), pointer, "{language}");
                let cloned = reference.clone();
                assert_eq!(cloned.module().as_ptr(), pointer, "{language}");
            }
            assert!(
                facts.retained_bytes() < (content.len() as u64).saturating_mul(8),
                "{language}: retained {} for {} source bytes",
                facts.retained_bytes(),
                content.len()
            );
            assert_eq!(facts.clone().retained_bytes(), facts.retained_bytes());
        }
    }

    #[test]
    fn ecmascript_export_clauses_retain_static_forwarding_and_local_identity() {
        for (path, language) in [
            ("barrel.js", "javascript"),
            ("barrel.ts", "typescript"),
            ("barrel.tsx", "tsx"),
        ] {
            let content = r#"export { run as launch, run as second }; function run() {}
import { work as imported } from './worker.js'; export { imported as forwarded };
export { work as renamed, default as chosen, work as "quoted name" } from './worker.js';
export * as tools from './worker.js'; export * from './wild.js';
function caller() { renamed(); tools.work(); }
"#;
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let run = graph.symbols.iter().position(|symbol| symbol.name == "run");
            assert!(run.is_some(), "{graph:?}");
            for name in ["launch", "second"] {
                assert!(facts.exports().iter().any(|fact| fact.name == name &&
                    matches!(fact.binding, crate::SourceExportBinding::LocalDeclaration(index) if Some(index) == run)), "{language}: {name}: {facts:?}");
            }
            for (name, imported) in [
                ("forwarded", Some("work")),
                ("renamed", Some("work")),
                ("chosen", Some("default")),
                ("quoted name", Some("work")),
                ("tools", None),
            ] {
                assert!(facts.exports().iter().any(|fact| fact.name == name &&
                    matches!(&fact.binding, crate::SourceExportBinding::ReExport(reference)
                        if reference.module() == "./worker.js" && reference.imported() == imported)), "{language}: {name}: {facts:?}");
            }
            assert!(
                facts
                    .imports()
                    .iter()
                    .any(|fact| fact.module.as_deref() == Some("./wild.js")),
                "wildcard dependency: {facts:?}"
            );
            assert!(
                facts
                    .imports()
                    .iter()
                    .filter(|fact| &content[fact.span.start..fact.span.end]
                        != "import { work as imported } from './worker.js';")
                    .all(|fact| fact.references.is_empty()),
                "export-from must not introduce local imports"
            );
            assert!(
                facts
                    .calls()
                    .iter()
                    .all(|fact| matches!(fact.binding, SourceCallBinding::Unresolved)),
                "export names are not local bindings: {facts:?}"
            );
        }
    }

    #[test]
    fn malformed_export_leaf_preserves_independent_static_siblings() {
        for (language, path) in [
            ("javascript", "barrel.js"),
            ("typescript", "barrel.ts"),
            ("tsx", "barrel.tsx"),
        ] {
            for suffix in ["", " from './worker.js'"] {
                let content = format!(
                    "function run() {{}} export {{ run as good, broken as , run as second }}{suffix};"
                );
                let result = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(result.is_ok(), "{result:?}");
                let Ok(extracted) = result else { return };
                for name in ["good", "second"] {
                    assert!(extracted.semantics().exports().iter().any(|fact| fact.name == name &&
                        if suffix.is_empty() { matches!(fact.binding, crate::SourceExportBinding::LocalDeclaration(_)) }
                        else { matches!(&fact.binding, crate::SourceExportBinding::ReExport(reference) if reference.imported() == Some("run") && reference.module() == "./worker.js") }),
                        "{language}: {content}: {:?}", extracted.semantics());
                }
            }
        }
    }

    #[test]
    fn type_only_export_slots_keep_their_namespace_through_host_and_ordinal_mapping() {
        for (language, path, host) in [
            ("typescript", "target.ts", false),
            ("tsx", "target.tsx", false),
            ("vue", "target.vue", true),
            ("svelte", "target.svelte", true),
            ("html", "target.html", true),
        ] {
            let script = "type Item = unknown; function run() {} export type { Item as named }; export { type Item as inline, run as named };";
            let content = if host {
                ecmascript_module_fixture(language, script).replacen(
                    "<script",
                    "<script lang=\"ts\"",
                    1,
                )
            } else {
                script.to_string()
            };
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, mut graph, mut facts) = extracted.into_parts();
            let before = facts.exports().to_vec();
            for name in ["named", "inline"] {
                assert!(
                    before.iter().any(|fact| fact.name == name
                        && fact.namespace == crate::SourceExportNamespace::Type
                        && fact.binding == crate::SourceExportBinding::Unresolved),
                    "{language}: {before:?}"
                );
            }
            assert!(
                before.iter().any(|fact| fact.name == "named"
                    && fact.namespace == crate::SourceExportNamespace::Value
                    && matches!(
                        fact.binding,
                        crate::SourceExportBinding::LocalDeclaration(_)
                    )),
                "{language}: {before:?}"
            );
            let symbols = vec![true; graph.symbols.len()];
            let relations = vec![true; graph.relations.len()];
            assert!(
                facts
                    .retain_graph_facts(&mut graph, &symbols, &relations)
                    .is_ok()
            );
            assert_eq!(facts.exports(), before);
        }
    }

    #[test]
    fn export_capacity_never_narrows_discarded_competing_slots() {
        use std::fmt::Write;
        let mut content = String::from("function run() {} export {run as first};\n");
        for index in 0..crate::MAX_RELATIONS_PER_FILE {
            assert!(writeln!(content, "export {{run as slot{index}}};").is_ok());
        }
        let result = extract_symbol_graph_with_semantics_controlled(
            "barrel.js",
            Some("javascript"),
            &content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else { return };
        assert!(!extracted.semantics().exports_complete());
        assert!(extracted.semantics().exports().is_empty());
        assert!(extracted.semantics().matches_graph(extracted.graph()));
    }

    #[test]
    fn ecmascript_local_exports_do_not_invent_callable_values() {
        let content = "export { missing as lost, value as unknown, run as default }; const value = factory(); function run() {}";
        let result = extract_symbol_graph_with_semantics_controlled(
            "barrel.js",
            Some("javascript"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else { return };
        let (_, graph, facts) = extracted.into_parts();
        for name in ["lost", "unknown"] {
            assert!(
                facts.exports().iter().any(|fact| fact.name == name
                    && fact.binding == crate::SourceExportBinding::Unresolved),
                "{facts:?}"
            );
        }
        assert!(facts.exports().iter().any(|fact| fact.name == "default" &&
            matches!(fact.binding, crate::SourceExportBinding::LocalDeclaration(index) if graph.symbols[index].name == "run")), "{facts:?}");
    }

    #[test]
    fn rust_static_paths_and_proven_callbacks_keep_their_own_binding_evidence() {
        for language in ["rust", "rust-build-script"] {
            let content = "use crate::worker as work; fn clean() { std::fs::read(\"x\"); true.then(work::run); } fn shadow(std: Value, work: Value) { std::fs::read(\"x\"); true.then(work::run); }";
            let result = extract_symbol_graph_with_semantics_controlled(
                "src/main.rs",
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            for target in ["std::fs::read", "work::run"] {
                let calls = facts
                    .calls()
                    .iter()
                    .filter(|fact| graph.relations[fact.relation].target_name == target)
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 2, "{target}: {graph:?}");
                assert!(
                    !matches!(
                        calls[0].binding,
                        SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                    ),
                    "{target}: {:?}",
                    calls[0]
                );
                assert_eq!(
                    calls[1].binding, calls[0].binding,
                    "value parameter must not shadow Rust module path {target}"
                );
                if target == "work::run" {
                    for fact in calls {
                        assert_eq!(
                            &content[fact.span.start..fact.span.end],
                            target,
                            "callback occurrence"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn type_only_names_do_not_compete_with_callable_value_bindings() {
        let mut failures = Vec::new();
        for (path, language, content) in [
            (
                "main.c",
                "c",
                "struct run { int value; }; void run(void) {} void caller(void) { run(); }",
            ),
            (
                "main.h",
                "h",
                "struct run { int value; }; void run(void) {} void caller(void) { run(); }",
            ),
            (
                "main.m",
                "objective-c",
                "struct run { int value; }; void run(void) {} void caller(void) { run(); }",
            ),
            (
                "main.cpp",
                "cpp",
                "struct run { int value; }; void run() {} void caller() { run(); }",
            ),
            (
                "main.hpp",
                "hpp",
                "struct run { int value; }; void run() {} void caller() { run(); }",
            ),
            (
                "main.ts",
                "typescript",
                "type run = string; function run() {} function caller() { run(); }",
            ),
            (
                "main.tsx",
                "tsx",
                "type run = string; function run() {} function caller() { run(); }",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1, "{language}: {graph:?}");
            if !matches!(facts.calls()[0].binding, SourceCallBinding::LocalDeclaration(symbol) if graph.symbols[symbol].kind == projectatlas_core::symbols::SymbolKind::Function)
            {
                failures.push(format!("{language}: {facts:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn reopened_containers_preserve_member_visibility_without_sharing_private_blocks() {
        let mut failures = Vec::new();
        for (language, content, resolved) in [
            (
                "cpp",
                "namespace N { void run() {} } namespace N { void caller() { run(); } }",
                true,
            ),
            (
                "cpp",
                r"namespace \u03B1 { void run() {} } namespace α { void caller() { run(); } }",
                true,
            ),
            (
                "cpp",
                r"namespace \u03B1 { void run() {} } namespace β { void caller() { run(); } }",
                false,
            ),
            (
                "cpp",
                r"namespace N { void \u03B1() {} } namespace N { void caller() { α(); } }",
                true,
            ),
            (
                "cpp",
                r"namespace N { void α() {} } namespace N { void caller() { auto \u03B1 = unknown; α(); } }",
                false,
            ),
            (
                "cpp",
                "namespace N { void caller() { run(); } } namespace N { void run() {} }",
                false,
            ),
            (
                "cpp",
                "namespace A { namespace B { void run() {} } } namespace A::B { void caller() { run(); } }",
                true,
            ),
            (
                "cpp",
                "namespace A::B { void run() {} } namespace A { namespace B { void caller() { run(); } } }",
                true,
            ),
            (
                "cpp",
                "namespace A { namespace N { void run() {} } } namespace B { namespace N { void caller() { run(); } } }",
                false,
            ),
            (
                "cpp",
                "namespace { void run() {} } namespace { void caller() { run(); } }",
                true,
            ),
            (
                "cpp",
                "inline namespace N { void run() {} } namespace N { void caller() { run(); } }",
                true,
            ),
            (
                "cpp",
                "void run() {} namespace N { void caller() { run(); } void run() {} }",
                true,
            ),
            (
                "csharp",
                "partial class C { static void Run() {} } partial class C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "namespace N { partial class C { static void Run() {} } } namespace N { partial class C { static void Caller() { Run(); } } }",
                true,
            ),
            (
                "csharp",
                "partial class A { static void Run() {} } partial class B { static void Caller() { Run(); } }",
                false,
            ),
            (
                "typescript",
                "namespace N { export function run() {} } namespace N { export function caller() { run(); } }",
                true,
            ),
            (
                "typescript",
                "namespace N { function run() {} } namespace N { export function caller() { run(); } }",
                false,
            ),
            (
                "typescript",
                "namespace N { function run() {} export function caller() { run(); } }",
                true,
            ),
            (
                "csharp",
                "partial struct C { static void Run() {} } partial struct C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "partial record C { static void Run() {} } partial record C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "partial interface C { static void Run() {} } partial interface C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "partial class C<T> { static void Run() {} } partial class C<T> { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "namespace @N { partial class @C<@T> { static void Run() {} } } namespace N { partial class C<T> { static void Caller() { Run(); } } }",
                true,
            ),
            (
                "csharp",
                r"namespace \u004e { partial class \u0043<\u0054> { static void Run() {} } } namespace N { partial class C<T> { static void Caller() { Run(); } } }",
                true,
            ),
            (
                "csharp",
                "namespace N\u{200d} { partial class C\u{200d}<T\u{200d}> { static void Run() {} } } namespace N { partial class C<T> { static void Caller() { Run(); } } }",
                true,
            ),
            (
                "csharp",
                "partial class C { static void @Run() {} } partial class C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "partial class C { static void Run() {} } partial class C { static void Caller(System.Action @Run) { Run(); } }",
                false,
            ),
            (
                "csharp",
                "partial record struct C { static void Run() {} } partial record struct C { static void Caller() { Run(); } }",
                true,
            ),
            (
                "csharp",
                "partial class C<T> { static void Run() {} } partial class C<T,U> { static void Caller() { Run(); } }",
                false,
            ),
            (
                "csharp",
                "namespace A { partial class C { static void Run() {} } } namespace B { partial class C { static void Caller() { Run(); } } }",
                false,
            ),
            (
                "csharp",
                "namespace A.B { partial class C { static void Run() {} } } namespace A { namespace B { partial class C { static void Caller() { Run(); } } } }",
                true,
            ),
            (
                "csharp",
                "partial class O { partial class C { static void Run() {} } } partial class O { partial class C { static void Caller() { Run(); } } }",
                true,
            ),
            (
                "csharp",
                "class C { static void Run() {} } class C { static void Caller() { Run(); } }",
                false,
            ),
            (
                "csharp",
                "class C { static void Run() {} } partial class C { static void Caller() { Run(); } }",
                false,
            ),
            (
                "typescript",
                "namespace \\u004e { export function run() {} } namespace N { export function caller() { run(); } }",
                true,
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "containers.source",
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let actual = facts
                .calls()
                .iter()
                .filter(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                .count();
            if facts.calls().len() != 1 || actual != usize::from(resolved) {
                failures.push(format!("{language}: {content}: {graph:?}: {facts:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn partial_type_identity_components_share_the_lexical_retention_budget() {
        use std::fmt::Write as _;
        for (widths, complete) in [
            (vec![super::MAX_LEXICAL_BINDINGS / 2 + 1; 2], false),
            (vec![super::MAX_LEXICAL_BINDINGS + 1], false),
            (vec![super::MAX_LEXICAL_BINDINGS / 2 - 8; 2], true),
        ] {
            let mut content = String::from(
                "class Calls { static void Run() {} static void Caller() { Run(); } }\n",
            );
            for (owner, width) in widths.into_iter().enumerate() {
                let parameters = (0..width)
                    .map(|index| format!("T{index}"))
                    .collect::<Vec<_>>()
                    .join(",");
                assert!(writeln!(content, "partial class C{owner}<{parameters}> {{}}").is_ok());
            }
            let result = extract_symbol_graph_with_semantics_controlled(
                "Capacity.cs",
                Some("csharp"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.bindings_incomplete, !complete);
            assert_eq!(facts.exports_incomplete, !complete);
            assert_eq!(facts.calls().len(), 1);
            assert!(graph.symbols.iter().any(|symbol| symbol.name == "Run"));
            assert_eq!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                complete
            );
        }
    }

    #[test]
    fn namespace_exports_keep_private_closures_and_write_owners() {
        for (content, expected) in [
            (
                "namespace N { function hidden() {} export function run(){hidden();} } namespace N { export function caller(){run();} }",
                vec![true, true],
            ),
            (
                "namespace N { function hidden() {} export function run(){hidden();} } namespace N { export function caller(){hidden();} }",
                vec![true, false],
            ),
            (
                "namespace N { export var run = ()=>{}; } namespace N { export function caller(){run();} }",
                vec![true],
            ),
            (
                "namespace N { export function run(){} } namespace N { run=null; export function caller(){run();} }",
                vec![false],
            ),
            (
                "namespace N { function hidden(){} export namespace M { export function run(){hidden();} } } namespace N.M { export function caller(){run();} }",
                vec![true, true],
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "namespace.ts",
                Some("typescript"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, _, facts) = extracted.into_parts();
            let actual = facts
                .calls()
                .iter()
                .map(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{content}: {facts:?}");
        }
    }

    #[test]
    fn cpp_anonymous_namespace_lookup_preserves_visibility_and_competitors() {
        let mut failures = Vec::new();
        for (content, resolved) in [
            ("namespace { void run() {} } void caller() { run(); }", true),
            (
                "namespace N { namespace { void run() {} } void caller() { run(); } }",
                true,
            ),
            (
                "namespace { namespace { void run() {} } } void caller() { run(); }",
                true,
            ),
            (
                "namespace { void run() {} } void caller() { auto run = unknown; run(); }",
                false,
            ),
            (
                "namespace { void run() {} } void run() {} void caller() { run(); }",
                false,
            ),
            (
                "void run() {} namespace { void run() {} } void caller() { run(); }",
                false,
            ),
            (
                "namespace { void run() {} } auto run = unknown; void caller() { run(); }",
                false,
            ),
            (
                "void caller() { run(); } namespace { void run() {} }",
                false,
            ),
            (
                "namespace { void run() {} } void caller() { run(); } void run() {}",
                false,
            ),
            (
                "void run() {} void caller() { run(); } namespace { void run() {} }",
                false,
            ),
            (
                "namespace A { namespace { void run() {} } } namespace B { void caller() { run(); } }",
                false,
            ),
            (
                "namespace { void run(int) {} } void caller() { run(1); } namespace { void run(char) {} }",
                false,
            ),
        ] {
            // Keep rows distinct to exercise canonical selector collisions as
            // well as lexical visibility, independent of same-line matching.
            let content = content.replace("} ", "}\n");
            let result = extract_symbol_graph_with_semantics_controlled(
                "anonymous.cpp",
                Some("cpp"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            if facts.calls().len() != 1
                || matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ) != resolved
            {
                failures.push(format!("{content}: {graph:?}: {facts:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn cpp_function_visibility_preserves_recursion_and_whole_overload_groups() {
        for (content, expected) in [
            ("void caller() { run(); } void run() {}", 0),
            ("void run() { run(); }", 1),
            (
                "void run(int value) {}\nvoid caller() { run(1); }\nvoid run(char value) {}",
                0,
            ),
            (
                "void run(int value) {}\nvoid run(char value) {}\nvoid caller() { run(1); }",
                2,
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.cpp",
                Some("cpp"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1);
            match expected {
                0 => assert_eq!(
                    facts.calls()[0].binding,
                    SourceCallBinding::Unresolved,
                    "{content}"
                ),
                1 => assert!(
                    matches!(
                        facts.calls()[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    "{facts:?}"
                ),
                _ => assert!(
                    matches!(&facts.calls()[0].binding, SourceCallBinding::LocalDeclarations(symbols) if symbols.len() == expected),
                    "{facts:?}"
                ),
            }
        }
    }

    #[test]
    fn cpp_distinct_local_type_metadata_cannot_select_an_outer_constructor() {
        let result = extract_symbol_graph_with_semantics_controlled(
            "main.cpp",
            Some("cpp"),
            "struct Run {};\nvoid caller() { struct Run {}; Run(); } void other() { struct Run {}; Run(); }",
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else { return };
        let (_, graph, facts) = extracted.into_parts();
        assert_eq!(facts.calls().len(), 2);
        for call in facts.calls() {
            assert!(
                matches!(call.binding, SourceCallBinding::LocalDeclaration(_)),
                "{facts:?}"
            );
            let SourceCallBinding::LocalDeclaration(ordinal) = call.binding else {
                return;
            };
            let Some(owner) = call.owner.and_then(|owner| graph.symbols.get(owner)) else {
                return;
            };
            let target = &graph.symbols[ordinal];
            assert_eq!(target.kind, projectatlas_core::symbols::SymbolKind::Struct);
            assert_eq!(target.parent.as_deref(), Some(owner.name.as_str()));
        }
    }

    #[test]
    fn cpp_type_constructors_remain_available_without_a_hiding_value() {
        for (content, expected) in [
            (
                "void Run() {} void caller() { Run(); struct Run {}; }",
                Some(projectatlas_core::symbols::SymbolKind::Function),
            ),
            (
                "struct Run {}; void caller() { Run(); } void Run() {}",
                Some(projectatlas_core::symbols::SymbolKind::Struct),
            ),
            (
                "struct Run {}; void caller() { Run(); }",
                Some(projectatlas_core::symbols::SymbolKind::Struct),
            ),
            (
                "void Run() {} void caller() { struct Run {}; Run(); }",
                Some(projectatlas_core::symbols::SymbolKind::Struct),
            ),
            ("struct Run {}; void caller(void (*Run)()) { Run(); }", None),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.cpp",
                Some("cpp"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1, "{graph:?}");
            if let Some(kind) = expected {
                assert!(
                    matches!(facts.calls()[0].binding, SourceCallBinding::LocalDeclaration(symbol) if graph.symbols[symbol].kind == kind),
                    "{facts:?}"
                );
            } else {
                assert_eq!(facts.calls()[0].binding, SourceCallBinding::Shadowed);
            }
        }
    }

    #[test]
    fn rust_value_calls_ignore_type_only_names_but_respect_value_items() {
        for (declaration, call) in [("struct run;", "run()"), ("struct run(u8);", "run(1)")] {
            let content = format!("{declaration} fn caller() {{ {call}; }}");
            let result = extract_symbol_graph_with_semantics_controlled(
                "src/main.rs",
                Some("rust"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert!(
                matches!(facts.calls()[0].binding, SourceCallBinding::LocalDeclaration(symbol) if graph.symbols[symbol].kind == projectatlas_core::symbols::SymbolKind::Struct),
                "constructor: {declaration}: {facts:?}"
            );
        }
        for sibling in ["mod run {}", "type run = ();", "struct run { value: u8 }"] {
            let content = format!("{sibling} fn run() {{}} fn caller() {{ run(); }}");
            let result = extract_symbol_graph_with_semantics_controlled(
                "src/main.rs",
                Some("rust"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert!(
                matches!(facts.calls()[0].binding, SourceCallBinding::LocalDeclaration(symbol) if graph.symbols[symbol].kind == projectatlas_core::symbols::SymbolKind::Function),
                "{sibling}: {facts:?}"
            );
        }
        for item in ["const run: fn() = || {};", "static run: fn() = || {};"] {
            let content = format!("fn run() {{}} fn caller() {{ {item} run(); }}");
            let result = extract_symbol_graph_with_semantics_controlled(
                "src/main.rs",
                Some("rust"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(
                facts.calls()[0].binding,
                SourceCallBinding::Shadowed,
                "{item}"
            );
        }
    }

    #[test]
    fn rust_module_paths_keep_all_members_and_use_the_type_namespace() {
        for blocker in [
            "mod std {}",
            "struct std;",
            "enum std {}",
            "trait std {}",
            "type std = ();",
            "union std { value: u8 }",
        ] {
            let content = format!("{blocker} fn caller(std: u8) {{ std::fs::read(\"x\"); }}");
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "src/main.rs",
                Some("rust"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else { return };
            let (_, _, facts) = extracted.into_parts();
            if blocker == "mod std {}" {
                assert!(matches!(&facts.calls()[0].binding,
                    SourceCallBinding::RustPath(path)
                    if path.origin == crate::SourceRustCallOrigin::DeclaredModule
                        && path.path.anchor == crate::SourceRustAnchor::SelfModule
                        && path.path.segments == ["std", "fs", "read"]
                ));
                continue;
            }
            if matches!(&facts.calls()[0].binding, SourceCallBinding::RustPath(_)) {
                assert!(matches!(&facts.calls()[0].binding,
                    SourceCallBinding::RustPath(path)
                    if path.origin == crate::SourceRustCallOrigin::ModuleLookup
                        && path.reference().is_none()
                        && path.path.segments == ["std", "fs", "read"]
                ));
                continue;
            }
            assert!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                ),
                "{blocker}: {facts:?}"
            );
        }
        let content = "use crate::fake as std; use crate::worker::run as launch; fn caller(std: u8, launch: fn()) { std::fs::read(\"x\"); launch(); } fn generic<std>() { std::fs::read(\"x\"); }";
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "src/main.rs",
            Some("rust"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else { return };
        let (_, graph, facts) = extracted.into_parts();
        let calls = facts.calls();
        assert!(
            matches!(&calls[0].binding, SourceCallBinding::Imported { member: Some(member), .. } if member == "fs::read"),
            "{:?}",
            calls[0]
        );
        assert_eq!(graph.relations[calls[1].relation].target_name, "launch");
        assert_eq!(calls[1].binding, SourceCallBinding::Shadowed);
        assert!(
            matches!(
                calls[2].binding,
                SourceCallBinding::Shadowed | SourceCallBinding::Unresolved
            ),
            "type parameter must block imported namespace: {:?}",
            calls[2]
        );
    }

    #[test]
    fn native_parameter_shadows_callee_without_hiding_unshadowed_sibling() {
        for (path, language, content) in [
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller(run: fn()) { run(); } fn clean() { run(); }",
            ),
            (
                "main.py",
                "python",
                "def run(): pass\ndef caller(run):\n run()\ndef clean():\n run()\n",
            ),
            (
                "main.js",
                "javascript",
                "function run() {} function caller(run) { run(); } function clean() { run(); }",
            ),
            (
                "main.ts",
                "typescript",
                "function run() {} function caller(run: () => void) { run(); } function clean() { run(); }",
            ),
            (
                "main.tsx",
                "tsx",
                "function run() {} function caller(run: () => void) { run(); } function clean() { run(); }",
            ),
            (
                "main.go",
                "go",
                "package main\nfunc run() {}\nfunc caller(run func()) { run() }\nfunc clean() { run() }",
            ),
            (
                "main.c",
                "c",
                "void run(void) {} void caller(void (*run)(void)) { run(); } void clean(void) { run(); }",
            ),
            (
                "main.cpp",
                "cpp",
                "void run() {} void caller(void (*run)()) { run(); } void clean() { run(); }",
            ),
            (
                "Main.java",
                "java",
                "class Main { static void run() {} static void caller(Runnable run) { run.run(); } static void clean() { run(); } }",
            ),
            (
                "Main.cs",
                "csharp",
                "class Main { static void run() {} static void caller(System.Action run) { run(); } static void clean() { run(); } }",
            ),
            (
                "main.kt",
                "kotlin",
                "fun run() {}\nfun caller(run: () -> Unit) { run() }\nfun clean() { run() }",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let calls: Vec<_> = facts
                .calls
                .iter()
                .map(|fact| (&graph.relations[fact.relation].source_name, &fact.binding))
                .collect();
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "caller"
                        && matches!(binding, SourceCallBinding::Shadowed)),
                "{language}: {calls:?}"
            );
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "clean"
                        && matches!(binding, SourceCallBinding::LocalDeclaration(_))),
                "{language}: {calls:?}"
            );
        }
    }

    #[test]
    fn scoped_imports_do_not_leak_and_independent_function_aliases_remain_exact() {
        for (path, language, content) in [
            (
                "main.rs",
                "rust",
                "fn first() { use crate::one::run as launch; launch(); } fn second() { use crate::two::run as launch; launch(); } fn outside() { launch(); }",
            ),
            (
                "main.py",
                "python",
                "def first():\n from one import run as launch\n launch()\ndef second():\n from two import run as launch\n launch()\ndef outside():\n launch()\n",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let mut imported = Vec::new();
            for fact in &facts.calls {
                let owner = &graph.relations[fact.relation].source_name;
                if owner == "outside" {
                    assert_eq!(fact.binding, SourceCallBinding::Unresolved, "{language}");
                } else {
                    let SourceCallBinding::Imported {
                        import,
                        reference,
                        member: None,
                        ..
                    } = &fact.binding
                    else {
                        assert!(
                            matches!(fact.binding, SourceCallBinding::Imported { .. }),
                            "{language}: {owner} {:?}",
                            fact.binding
                        );
                        return;
                    };
                    imported.push(facts.imports[*import].references[*reference].module());
                }
            }
            assert_eq!(imported.len(), 2, "{language}");
            assert_ne!(imported[0], imported[1], "{language}");
        }
    }

    #[test]
    fn native_local_values_shadow_static_callees_in_their_own_scope() {
        for (path, language, content) in [
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller() { let run = unknown; run(); } fn clean() { run(); }",
            ),
            (
                "main.py",
                "python",
                "def run(): pass\ndef caller():\n run = unknown\n run()\ndef clean():\n run()\n",
            ),
            (
                "main.js",
                "javascript",
                "function run() {} function caller() { const run = unknown; run(); } function clean() { run(); }",
            ),
            (
                "main.ts",
                "typescript",
                "function run() {} function caller() { let run = unknown; run(); } function clean() { run(); }",
            ),
            (
                "main.go",
                "go",
                "package main\nfunc run() {}\nfunc caller() { run := unknown; run() }\nfunc clean() { run() }",
            ),
            (
                "main.c",
                "c",
                "void run(void) {} void caller(void) { void (*run)(void) = unknown; run(); } void clean(void) { run(); }",
            ),
            (
                "main.cpp",
                "cpp",
                "void run() {} void caller() { auto run = unknown; run(); } void clean() { run(); }",
            ),
            (
                "Main.java",
                "java",
                "class Main { static void run() {} static void caller() { Runnable run = unknown; run.run(); } static void clean() { run(); } }",
            ),
            (
                "Main.cs",
                "csharp",
                "class Main { static void run() {} static void caller() { System.Action run = unknown; run(); } static void clean() { run(); } }",
            ),
            (
                "main.kt",
                "kotlin",
                "fun run() {}\nfun caller() { val run = unknown; run() }\nfun clean() { run() }",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let calls: Vec<_> = facts
                .calls
                .iter()
                .map(|fact| (&graph.relations[fact.relation].source_name, &fact.binding))
                .collect();
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "caller"
                        && matches!(binding, SourceCallBinding::Shadowed)),
                "{language}: {calls:?}"
            );
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "clean"
                        && matches!(binding, SourceCallBinding::LocalDeclaration(_))),
                "{language}: {calls:?}"
            );
        }
    }

    #[test]
    fn temporal_shadowing_follows_python_function_and_ecmascript_block_rules() {
        for (path, language, content, expected) in [
            (
                "main.py",
                "python",
                "def run(): pass\ndef caller():\n run()\n run = unknown\n",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "main.js",
                "javascript",
                "function run() {} function caller() { run(); let run = unknown; }",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller() { run(); let run = unknown; run(); }",
                vec![
                    SourceCallBinding::LocalDeclaration(0),
                    SourceCallBinding::Shadowed,
                ],
            ),
            (
                "main.js",
                "javascript",
                "function run() {} function caller() { { let run = unknown; run(); } run(); }",
                vec![
                    SourceCallBinding::Shadowed,
                    SourceCallBinding::LocalDeclaration(0),
                ],
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            let actual: Vec<_> = facts.calls.into_iter().map(|fact| fact.binding).collect();
            assert_eq!(actual, expected, "{language}: {content}");
        }
    }

    #[test]
    fn java_member_calls_retain_method_names_without_guessing_type_targets() {
        for content in [
            "class Service { static void run() {} } class Main { void caller() { Service.run(); } }",
            "class Service { static void run() {} } class Other { static void run() {} } class Main { void caller() { Service.run(); } Other Service; }",
            "class Service { static void run(int value) {} static void run(String value) {} } class Main { void caller() { Service.run(1); } }",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "Main.java",
                Some("java"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1);
            let call = &facts.calls()[0];
            assert_eq!(graph.relations[call.relation].target_name, "Service.run");
            assert!(
                matches!(
                    call.binding,
                    SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                ),
                "qualified Java dispatch needs type/receiver proof: {call:?}"
            );
        }
    }

    #[test]
    fn java_method_names_remain_distinct_from_same_named_expression_variables() {
        let content = "class Main { static void run() {} static void caller(Runnable run) { run(); } static void local() { Runnable run = null; run(); } }";
        let result = extract_symbol_graph_with_semantics_controlled(
            "Main.java",
            Some("java"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 2);
        assert!(
            facts
                .calls
                .iter()
                .all(|fact| matches!(fact.binding, SourceCallBinding::LocalDeclaration(_)))
        );
    }

    #[test]
    fn callable_variable_exports_keep_exact_declarations_and_unknown_value_owners() {
        for (path, language, host) in [
            ("base.js", "javascript", false),
            ("base.ts", "typescript", false),
            ("base.tsx", "tsx", false),
            ("base.vue", "vue", true),
            ("base.svelte", "svelte", true),
            ("base.html", "html", true),
        ] {
            let script = "export const arrow = () => {};\nexport let expression = function () {};\nexport var generator = function* () {};\nexport const scalar = 1;\nexport const {ghost: renamed} = () => {};\nexport default () => {};";
            let content = if host {
                ecmascript_module_fixture(language, script)
            } else {
                script.to_string()
            };
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            for name in ["arrow", "expression", "generator"] {
                assert!(facts.exports().iter().any(|fact| fact.name == name && matches!(fact.binding, crate::SourceExportBinding::LocalDeclaration(index)
                    if graph.symbols[index].name == name && graph.symbols[index].kind == projectatlas_core::symbols::SymbolKind::Function)), "{language}: {name}: {facts:?}");
            }
            for name in ["scalar", "renamed", "default"] {
                assert!(
                    facts.exports().iter().any(|fact| fact.name == name
                        && matches!(
                            fact.binding,
                            crate::SourceExportBinding::Unresolved
                                | crate::SourceExportBinding::UnresolvedDeclarations(_)
                        )),
                    "{language}: {name}: {facts:?}"
                );
            }
            assert!(
                !graph.symbols.iter().any(|symbol| symbol.name == "ghost"),
                "destructuring key became a declaration: {language}: {graph:?}"
            );
            assert!(facts.matches_graph(&graph));
        }
    }

    #[test]
    fn directly_declared_ecmascript_callables_keep_static_targets_and_body_owners() {
        for language in ["javascript", "typescript", "tsx"] {
            let content = "const run = () => run(); function caller() { run(); }";
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.ts",
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 2, "{language}");
            for fact in &facts.calls {
                assert!(
                    matches!(fact.binding, SourceCallBinding::LocalDeclaration(_)),
                    "{language}: {:?}",
                    fact.binding
                );
                assert!(fact.owner.is_some(), "{language}");
            }
            let owner = facts.calls[0].owner;
            assert!(owner.is_some_and(|owner| graph.symbols[owner].name == "run"));
        }
    }

    #[test]
    fn destructuring_and_catch_binding_roles_do_not_inherit_outer_callees() {
        for (path, language, content) in [
            (
                "main.js",
                "javascript",
                "function run() {} function caller(value) { const {run} = value; run(); }",
            ),
            (
                "main.ts",
                "typescript",
                "function run() {} function caller(value: any) { const {run: invoke} = value; invoke(); }",
            ),
            (
                "main.js",
                "javascript",
                "function run() {} function caller() { try {} catch(run) { run(); } }",
            ),
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller(value: Input) { let Input {run} = value; run(); }",
            ),
            (
                "main.py",
                "python",
                "def run(): pass\ndef caller(*run):\n run()\n",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 1, "{language}");
            assert_eq!(
                facts.calls[0].binding,
                SourceCallBinding::Shadowed,
                "{language}: {content}"
            );
        }
    }

    #[test]
    fn python_recursive_body_retains_its_actual_declaration_owner() {
        for content in [
            "def walk(n):\n return walk(n - 1)\n",
            "def outer():\n def walk(n):\n  return walk(n - 1)\n return walk(3)\n",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert!(!facts.calls.is_empty());
            for fact in facts.calls {
                let SourceCallBinding::LocalDeclaration(target) = fact.binding else {
                    assert!(
                        matches!(fact.binding, SourceCallBinding::LocalDeclaration(_)),
                        "{:?}",
                        fact.binding
                    );
                    return;
                };
                assert_eq!(graph.symbols[target].name, "walk");
                assert!(fact.owner.is_some());
            }
        }
    }

    #[test]
    fn pattern_and_statement_scopes_preserve_shadows_and_outer_siblings() {
        for (path, language, content, expected) in [
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller(x: Option<fn()>) { match x { Some(run) => run(), _ => () }; run(); }",
                vec![
                    SourceCallBinding::Shadowed,
                    SourceCallBinding::LocalDeclaration(0),
                ],
            ),
            (
                "main.rs",
                "rust",
                "fn run() {} fn caller(x: Option<fn()>) { if let Some(run) = x { run(); } else { run(); } run(); }",
                vec![
                    SourceCallBinding::Shadowed,
                    SourceCallBinding::LocalDeclaration(0),
                    SourceCallBinding::LocalDeclaration(0),
                ],
            ),
            (
                "main.py",
                "python",
                "from worker import run\ndef caller(value):\n match value:\n  case run: run()\n",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "main.go",
                "go",
                "package main\nfunc run() {}\nfunc caller(other func()) { if run := other; true { run() }; run() }",
                vec![
                    SourceCallBinding::Shadowed,
                    SourceCallBinding::LocalDeclaration(1),
                ],
            ),
            (
                "main.c",
                "c",
                "void run(void) {} void caller(void) { void (*run)(void) = run(); }",
                vec![SourceCallBinding::Shadowed],
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let actual: Vec<_> = facts.calls.into_iter().map(|fact| fact.binding).collect();
            assert_eq!(
                actual, expected,
                "{language}: {content}: {:?}",
                graph.symbols
            );
        }
    }

    #[test]
    fn exception_iteration_and_declaration_patterns_own_their_callable_names() {
        for (path, language, content, shadowed) in [
            (
                "main.py",
                "python",
                "from worker import run\ndef caller():\n try: pass\n except Exception as run: run()\n",
                1,
            ),
            (
                "main.py",
                "python",
                "from worker import run\ntry: pass\nexcept Exception as run: run()\n",
                1,
            ),
            (
                "main.py",
                "python",
                "from worker import run\ndef caller(manager):\n with manager as run: run()\n",
                1,
            ),
            (
                "main.go",
                "go",
                "package main\nfunc run() {}\nfunc caller(callbacks []func()) { for _, run := range callbacks { run() }; run() }",
                1,
            ),
            (
                "Main.cs",
                "csharp",
                "class Main { static void run() {} static void caller(object value) { if (value is System.Action run) run(); } }",
                1,
            ),
            (
                "Main.cs",
                "csharp",
                "class Main { static void run() {} static void caller(System.Action[] callbacks) { foreach (var run in callbacks) run(); run(); } }",
                1,
            ),
            (
                "main.cpp",
                "cpp",
                "void run() {} void caller() { if (auto run = unknown; true) { run(); } run(); }",
                1,
            ),
            (
                "main.kt",
                "kotlin",
                "fun run() {}\nfun caller() { try {} catch (run: Exception) { run() }; run() }",
                1,
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert!(!facts.calls.is_empty(), "{language}");
            let shadows = facts
                .calls
                .iter()
                .filter(|fact| matches!(fact.binding, SourceCallBinding::Shadowed))
                .count();
            assert_eq!(
                shadows, shadowed,
                "{language}: {content}: {:?}",
                facts.calls
            );
            if matches!(language, "go" | "cpp" | "kotlin") || content.contains("foreach") {
                assert!(
                    facts.calls.last().is_some_and(|fact| matches!(
                        fact.binding,
                        SourceCallBinding::LocalDeclaration(_)
                    )),
                    "{language}: {:?}",
                    facts.calls
                );
            }
        }
    }

    #[test]
    fn python_comprehension_binders_do_not_capture_their_iterable_or_escape() {
        for expression in [
            "[run() for run in (run(),)]",
            "{run() for run in (run(),)}",
            "{0: run() for run in (run(),)}",
            "(run() for run in (run(),))",
        ] {
            let content = format!("from worker import run\ndef caller():\n {expression}\n run()\n");
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 3, "{expression}");
            assert_eq!(
                facts.calls[0].binding,
                SourceCallBinding::Shadowed,
                "{expression}"
            );
            assert!(
                facts.calls[1..]
                    .iter()
                    .all(|fact| matches!(fact.binding, SourceCallBinding::Imported { .. })),
                "{expression}: {:?}",
                facts.calls
            );
        }
    }

    #[test]
    fn python_class_values_do_not_replace_module_imports() {
        let content = "from worker import run\nclass Holder:\n run = None\ndef caller():\n run()\n";
        let result = extract_symbol_graph_with_semantics_controlled(
            "main.py",
            Some("python"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 1);
        assert!(matches!(
            facts.calls[0].binding,
            SourceCallBinding::Imported { .. }
        ));
    }

    #[test]
    fn python_nested_classes_skip_class_namespaces_but_retain_function_closures() {
        for (content, expected_parent) in [
            (
                "def launch(): pass\nclass Outer:\n def launch(self): pass\n class Inner:\n  def call(self): launch()\n",
                None,
            ),
            (
                "def outer():\n def launch(): pass\n class Outer:\n  def launch(self): pass\n  class Inner:\n   def call(self): launch()\n",
                Some("outer"),
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 1);
            let SourceCallBinding::LocalDeclaration(target) = facts.calls[0].binding else {
                assert!(
                    matches!(
                        facts.calls[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    "{:?}",
                    facts.calls
                );
                return;
            };
            assert_eq!(graph.symbols[target].parent.as_deref(), expected_parent);
        }
    }

    #[test]
    fn python_definition_headers_use_the_enclosing_binding_and_caller_owner() {
        for content in [
            "from worker import run\ndef caller(run=run()): pass\n",
            "from worker import run\n@run()\ndef caller(run): pass\n",
            "from worker import run\ndef caller(run: run()) -> run(): pass\n",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert!(!facts.calls.is_empty());
            for fact in &facts.calls {
                assert!(
                    matches!(fact.binding, SourceCallBinding::Imported { .. }),
                    "{:?}",
                    facts.calls
                );
                assert_eq!(fact.owner, None);
            }
        }
    }

    #[test]
    fn python_iteration_and_deletion_invalidate_prior_binding_at_their_real_transition() {
        for (content, expected) in [
            (
                "def run(): pass\nfor run in callbacks:\n run()\n",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "def run(): pass\ndef caller():\n del run\n run()\n",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "def run(): pass\ndel run\nrun()\n",
                vec![SourceCallBinding::Shadowed],
            ),
            (
                "def run(): pass\nfor run in (run(),):\n run()\n",
                vec![
                    SourceCallBinding::LocalDeclaration(0),
                    SourceCallBinding::Shadowed,
                ],
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(
                facts
                    .calls
                    .into_iter()
                    .map(|fact| fact.binding)
                    .collect::<Vec<_>>(),
                expected,
                "{content}"
            );
        }
    }

    #[test]
    fn python_scope_directives_preserve_read_before_write_owners() {
        for statement in [
            "run = run()\nrun()",
            "run += run()\nrun()",
            "for run in run():\n run()",
            "with run() as run:\n run()",
            "run()\ndel run\nrun()",
            "try:\n run()\nexcept Exception as run:\n run()",
            "(run := run())\nrun()",
            "run()\nmatch value:\n case run:\n  run()",
            "[(run := run()) for _ in [1]]\nrun()",
        ] {
            for (header, indent, directive) in [
                ("def run(): pass\ndef caller():\n", " ", "global"),
                (
                    "def outer():\n def run(): pass\n def caller():\n",
                    "  ",
                    "nonlocal",
                ),
            ] {
                let content = format!(
                    "{header}{indent}{directive} run\n{indent}{}\n",
                    statement.replace('\n', &format!("\n{indent}"))
                );
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "main.py",
                    Some("python"),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{extracted:?}");
                let Ok(extracted) = extracted else {
                    return;
                };
                let (_, graph, facts) = extracted.into_parts();
                let calls = facts
                    .calls
                    .iter()
                    .filter(|call| graph.relations[call.relation].target_name == "run")
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 2, "{content}: {:?}", facts.calls);
                assert!(
                    matches!(calls[0].binding, SourceCallBinding::LocalDeclaration(_)),
                    "{content}: {:?}",
                    facts.calls
                );
                if let SourceCallBinding::LocalDeclaration(target) = calls[0].binding {
                    assert_eq!(graph.symbols[target].name, "run");
                }
                assert!(
                    matches!(
                        calls[1].binding,
                        SourceCallBinding::Shadowed | SourceCallBinding::Unresolved
                    ),
                    "{content}: {:?}",
                    facts.calls
                );
            }
        }
    }

    #[test]
    fn python_directives_keep_actual_owners_and_captured_writes_conservative() {
        for (content, expected_lines) in [
            (
                "def run(): pass\ndef invalid():\n run()\n global run\n run = unknown\ndef valid(): run()\n",
                vec![None, Some(1)],
            ),
            (
                "def outer():\n def run(): pass\n def invalid():\n  run()\n  nonlocal run\n  run = unknown\n def valid(): run()\n",
                vec![None, Some(2)],
            ),
            (
                "def run(): pass\ndef caller():\n def nested(value: run): pass\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n def nested(value: run = 1): pass\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n def nested() -> run(): pass\n global run\n run()\n",
                vec![Some(1), Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n value: run\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n def nested(run): pass\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n from worker import run as other\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n value = obj.run\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef caller():\n other(run=1)\n global run\n run()\n",
                vec![Some(1)],
            ),
            (
                "def run(): pass\ndef outer():\n def run(): pass\n def caller():\n  global run\n  run()\n",
                vec![Some(1)],
            ),
            (
                "def outer():\n def run(): pass\n def middle():\n  global run\n  def caller():\n   nonlocal run\n   run()\n",
                vec![None],
            ),
            (
                "def outer():\n def caller():\n  nonlocal run\n  run()\n def run(): pass\n",
                vec![Some(5)],
            ),
            (
                "def run(): pass\ndef caller():\n global run\n from worker import run\n run()\n",
                vec![None],
            ),
            (
                "def run(): pass\ndef mutate():\n global run\n run = unknown\ndef caller(): run()\n",
                vec![None],
            ),
            (
                "def outer():\n def run(): pass\n def mutate():\n  nonlocal run\n  run = unknown\n def caller(): run()\n",
                vec![None],
            ),
            (
                "def run(): pass\ndef first():\n global run\n run = unknown\ndef second():\n global run\n run = run()\n",
                vec![None],
            ),
            (
                "def run(): pass\ndef caller():\n global run\n def later(): return run()\n run = unknown\n",
                vec![None],
            ),
            (
                "def run(): pass\ndef caller():\n (run := run())\n run()\n",
                vec![None, None],
            ),
            (
                "def run(): pass\nclass Api:\n global run\n run = run()\n run()\n",
                vec![Some(1), None],
            ),
            (
                "def run(): pass\ndef caller():\n global run\n def run(): pass\n run()\n",
                vec![Some(4)],
            ),
            (
                "def outer():\n def run(): pass\n def caller():\n  nonlocal run\n  def run(): pass\n  run()\n",
                vec![Some(5)],
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let actual = facts
                .calls
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == "run")
                .map(|call| match call.binding {
                    SourceCallBinding::LocalDeclaration(target) => {
                        Some(graph.symbols[target].line_start)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected_lines, "{content}: {:?}", facts.calls);
            if content.contains(" from worker import run\n") {
                assert!(matches!(
                    facts.calls[0].binding,
                    SourceCallBinding::Imported { .. }
                ));
            }
        }
    }

    #[test]
    fn python_reads_before_directives_cannot_authorize_calls() {
        for (prefix, directive) in [
            ("def run(): pass\ndef caller():\n", "global"),
            (
                "def outer():\n def run(): pass\n def caller():\n",
                "nonlocal",
            ),
        ] {
            let indent = if directive == "global" { " " } else { "  " };
            let repeated = format!("{directive} run\nrun()");
            for read in [
                "run()",
                "saved = run",
                "saved = run.value",
                "def nested(value=run): pass",
                repeated.as_str(),
            ] {
                let content = format!(
                    "{prefix}{indent}{}\n{indent}{directive} run\n{indent}run()\n",
                    read.replace('\n', &format!("\n{indent}"))
                );
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "main.py",
                    Some("python"),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{extracted:?}");
                let Ok(extracted) = extracted else {
                    return;
                };
                let (_, graph, facts) = extracted.into_parts();
                assert!(
                    facts
                        .calls
                        .iter()
                        .filter(|call| graph.relations[call.relation].target_name == "run")
                        .all(|call| matches!(
                            call.binding,
                            SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                        )),
                    "{content}: {:?}",
                    facts.calls
                );
            }
        }
    }

    #[test]
    fn python_annotations_preserve_values_and_function_local_reservations() {
        for (content, expected) in [
            ("def run(): pass\nrun: object\nrun()\n", vec![Some(1)]),
            (
                "def caller():\n def run(): pass\n run: object\n run()\n",
                vec![Some(2)],
            ),
            (
                "def run(): pass\ndef caller():\n run: object\n run()\n",
                vec![None],
            ),
            (
                "def run(): pass\ndef caller():\n global run\n run: object\n run()\n",
                vec![None],
            ),
            (
                "def outer():\n def run(): pass\n def caller():\n  nonlocal run\n  run: object\n  run()\n",
                vec![None],
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let actual = facts
                .calls
                .iter()
                .map(|call| match call.binding {
                    SourceCallBinding::LocalDeclaration(target) => {
                        Some(graph.symbols[target].line_start)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{content}: {:?}", facts.calls);
        }
    }

    #[test]
    fn python_files_without_directives_do_not_retain_read_order_state() {
        use std::fmt::Write as _;
        for comment in ["", "# global nonlocal in comments are not directives\n"] {
            let mut content = format!("{comment}def run(): pass\n");
            for index in 0..=super::MAX_LEXICAL_BINDINGS {
                assert!(writeln!(content, "value_{index}").is_ok());
            }
            content.push_str("run()\n");
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.py",
                Some("python"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert!(!facts.bindings_incomplete);
            assert_eq!(facts.calls.len(), 1);
            assert!(matches!(
                facts.calls[0].binding,
                SourceCallBinding::LocalDeclaration(_)
            ));
        }
    }

    #[test]
    fn python_module_global_directives_do_not_consume_local_binding_state() {
        use std::fmt::Write as _;
        let mut content = String::from("def run(): pass\n");
        for index in 0..=super::MAX_LEXICAL_BINDINGS {
            assert!(writeln!(content, "global value_{index}").is_ok());
        }
        content.push_str("run()\n");
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.py",
            Some("python"),
            &content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert!(!facts.bindings_incomplete);
        assert_eq!(facts.calls.len(), 1);
        assert!(matches!(
            facts.calls[0].binding,
            SourceCallBinding::LocalDeclaration(_)
        ));
    }

    #[test]
    fn python_directives_share_the_lexical_retention_limit() {
        use std::fmt::Write as _;
        let mut content = String::from("def caller():\n");
        for index in 0..=super::MAX_LEXICAL_BINDINGS {
            assert!(writeln!(content, " global value_{index}").is_ok());
        }
        content.push_str(" run()\n");
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.py",
            Some("python"),
            &content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert!(facts.bindings_incomplete);
        assert!(
            facts
                .calls
                .iter()
                .all(|call| matches!(call.binding, SourceCallBinding::Unresolved))
        );
    }

    #[test]
    fn overloaded_callees_share_one_immutable_candidate_set() {
        let content =
            "void run(int value) {}\nvoid run(double value) {}\nvoid caller() { run(0); run(1); }";
        let result = extract_symbol_graph_with_semantics_controlled(
            "main.cpp",
            Some("cpp"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 2);
        let (
            SourceCallBinding::LocalDeclarations(first),
            SourceCallBinding::LocalDeclarations(second),
        ) = (&facts.calls[0].binding, &facts.calls[1].binding)
        else {
            assert!(
                facts
                    .calls
                    .iter()
                    .all(|fact| matches!(fact.binding, SourceCallBinding::LocalDeclarations(_))),
                "{:?}",
                facts.calls
            );
            return;
        };
        assert_eq!(first.len(), 2);
        assert!(std::sync::Arc::ptr_eq(first, second));
    }

    #[test]
    fn escaped_ecmascript_declaration_spelling_keeps_its_cooked_lexical_identity() {
        for content in [
            r"function l\u0061unch() {} function caller() { launch(); }",
            r"const l\u0061unch = () => launch(); function caller() { launch(); }",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.js",
                Some("javascript"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert!(!facts.calls.is_empty());
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name.contains(r"\u0061"))
            );
            assert!(
                facts
                    .calls
                    .iter()
                    .all(|fact| matches!(fact.binding, SourceCallBinding::LocalDeclaration(_))),
                "{content}: {:?}",
                facts.calls
            );
        }
    }

    #[test]
    fn grouping_many_tied_bindings_remains_cooperatively_cancellable() {
        let mut bindings = super::NameBindings {
            transitions: (0..4_000)
                .map(|symbol| super::Binding {
                    from: 0,
                    target: super::BindingTarget::Declaration(symbol),
                })
                .collect(),
            ..super::NameBindings::default()
        };
        let mut checks = 0;
        let result = bindings.finish(&mut || {
            checks += 1;
            if checks == 100 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err("cancelled"));
        assert_eq!(checks, 100);
    }

    #[test]
    fn java_member_bodies_isolate_homonymous_method_bindings() {
        for source in [
            "class Left { void run() {} void callerLeft(){run();} } class Right { void run() {} void callerRight(){run();} }",
            "record Left() {\n void run() {}\n void callerLeft(){run();}\n}\nrecord Right() {\n void run() {}\n void callerRight(){run();}\n}",
            "class Outer { Object left=new Object(){\n void run() {}\n void callerLeft(){run();}\n}; Object right=new Object(){\n void run() {}\n void callerRight(){run();}\n}; }",
            "enum Both { LEFT {\n void run() {}\n void callerLeft(){run();}\n}, RIGHT {\n void run() {}\n void callerRight(){run();}\n}; abstract void run(); }",
            "interface Left { default void run() {} default void callerLeft() { run(); } } interface Right { default void run() {} default void callerRight() { run(); } }",
            "enum Left { VALUE; void run() {} void callerLeft(){run();} } enum Right { VALUE; void run() {} void callerRight(){run();} }",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "Members.java",
                Some("java"),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let calls = facts
                .calls()
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == "run")
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 2, "{source}: {facts:?}");
            let targets = graph
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, symbol)| {
                    symbol.name == "run"
                        && symbol.kind == projectatlas_core::symbols::SymbolKind::Method
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert!(targets.len() >= 2, "{source}: {graph:?}");
            for call in calls {
                if source.starts_with("enum Both") {
                    // Constant-specific members share raw enum parent metadata;
                    // their distinct body scopes cannot grant one graph target.
                    assert_eq!(call.binding, SourceCallBinding::Unresolved, "{call:?}");
                    continue;
                }
                assert!(
                    matches!(call.binding, SourceCallBinding::LocalDeclaration(_)),
                    "{source}: {call:?}"
                );
                let SourceCallBinding::LocalDeclaration(target) = call.binding else {
                    return;
                };
                assert!(call.owner.is_some(), "missing call owner");
                let Some(owner) = call.owner else { return };
                assert_eq!(
                    graph.symbols[target].parent, graph.symbols[owner].parent,
                    "{source}: {graph:?}"
                );
                assert_eq!(graph.symbols[target].name, "run");
                let expected = if graph.symbols[owner].name == "callerLeft" {
                    targets[0]
                } else {
                    targets[1]
                };
                assert_eq!(target, expected, "{source}: {graph:?}");
            }
        }
    }

    #[test]
    fn rust_associated_constants_do_not_shadow_lexical_free_calls() {
        for source in [
            "fn run() {} struct Api; impl Api { const run: u8 = 0; fn caller() { run(); } }",
            "fn run() {} trait Api { const run: u8; fn caller() { run(); } }",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.rs",
                Some("rust"),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1, "{source}: {facts:?}");
            assert!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                "{source}: {:?}",
                facts.calls()
            );
            let SourceCallBinding::LocalDeclaration(target) = facts.calls()[0].binding else {
                return;
            };
            assert_eq!(graph.symbols[target].name, "run");
            assert!(graph.symbols[target].parent.is_none());
        }
    }

    #[test]
    fn rust_nonassociated_constants_and_const_parameters_remain_lexical_blockers() {
        for source in [
            "fn run(){} mod inner {const run:fn()=||{}; fn caller(){run();}}",
            "fn run(){} struct Api; impl Api {fn caller(){const run:fn()=||{}; run();}}",
            "fn run(){} fn caller(){ {const run:fn()=||{}; run();} }",
            "fn run(){} fn caller<const run:bool>(){run();}",
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "main.rs",
                Some("rust"),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            assert_eq!(extracted.semantics().calls().len(), 1, "{source}");
            assert!(
                !matches!(
                    extracted.semantics().calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                "{source}: {:?}",
                extracted.semantics()
            );
        }
    }

    #[test]
    fn native_member_containers_do_not_capture_outer_free_calls() {
        let mut failures = Vec::new();
        for (language, source) in [
            (
                "cpp",
                "void run(){}\nunion Owner {\n void run(){}\n};\nvoid outside(){run();}",
            ),
            (
                "kotlin",
                "fun run()=1\nobject Owner {\n fun run()=2\n}\nfun outside(){run()}",
            ),
            (
                "kotlin",
                "fun run()=1\nclass Owner {\ncompanion object {\nfun run()=2\n}\n}\nfun outside(){run()}",
            ),
            (
                "kotlin",
                "fun run()=1\nval owner=object {\nfun run()=2\n}\nfun outside(){run()}",
            ),
            (
                "kotlin",
                "fun run()=1\nenum class Owner {\nOne {\nfun run()=2\n};\n}\nfun outside(){run()}",
            ),
            (
                "zig",
                "fn run() void {}\nconst Owner=enum {value, pub fn run() void {}};\npub fn outside() void {run();}",
            ),
            (
                "zig",
                "fn run() void {}\nconst Owner=union {value:u8, pub fn run() void {}};\npub fn outside() void {run();}",
            ),
            (
                "zig",
                "fn run() void {}\nconst Owner=opaque {pub fn run() void {}};\npub fn outside() void {run();}",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1, "{language}: {source}: {facts:?}");
            let actual = match facts.calls()[0].binding {
                SourceCallBinding::LocalDeclaration(target) => {
                    Some(graph.symbols[target].line_start)
                }
                _ => None,
            };
            if actual != Some(1) {
                failures.push(format!("{language}: {source}: {:?}", facts.calls()));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn kotlin_classifier_headers_and_constructor_names_have_distinct_visibility() {
        let mut failures = Vec::new();
        for (source, expected_line) in [
            (
                "fun run()=1\nopen class Base(val value:Int)\nclass Api:Base(run()){fun run()=2}",
                Some(1),
            ),
            (
                "fun run()=1\nopen class Base(val value:Int)\nobject Api:Base(run()){fun run()=2}",
                Some(1),
            ),
            (
                "fun run(){}\nclass Api(val run:()->Unit){fun caller(){run()}}",
                None,
            ),
            (
                "fun run(){}\nclass Api(run:()->Unit){fun caller(){run()}}",
                Some(1),
            ),
            (
                "fun run(){}\nclass Api(run:()->Unit){val value=run()}",
                None,
            ),
            (
                "fun run(){}\nclass Api {constructor(run:()->Unit){run()}}",
                None,
            ),
            (
                "fun run(){}\nclass Api {\nvar callback:()->Unit = {}\nset(run){run()}\n}",
                None,
            ),
            (
                "fun run(){}\nclass Api(run:()->Unit){val value get()=run()}",
                Some(1),
            ),
            ("fun run(){}\nclass Api(run:()->Unit){init {run()}}", None),
            (
                "fun run(){}\nclass Api(run:()->Unit){val value={run()}}",
                None,
            ),
            ("fun run(){}\nclass Api(run:()->Unit = {run()})", Some(1)),
            (
                "fun run()=1\nopen class Base(val value:Int)\nclass Api(run:()->Int):Base(run())",
                None,
            ),
            (
                "fun run(){}\nclass Outer {\nfun run(){}\nclass Nested {fun caller(){run()}}\n}",
                Some(1),
            ),
            (
                "fun run(){}\nclass Outer {\nfun run(){}\nobject Nested {fun caller(){run()}}\n}",
                Some(1),
            ),
            (
                "fun run(){}\nclass Outer {\nfun run(){}\ninner class Nested {fun caller(){run()}}\n}",
                Some(3),
            ),
            (
                "fun run(){}\nclass Outer {\nfun run(){}\nval nested=object {fun caller(){run()}}\n}",
                Some(3),
            ),
            (
                "fun run(){}\nclass Api {\nfun caller(){run()}\nval run:()->Unit = {}\n}",
                None,
            ),
            (
                "fun run(){}\nobject Outer {\nfun run(){}\nclass Nested {fun caller(){run()}}\n}",
                Some(3),
            ),
            (
                "fun run(){}\nfun outer(){\nfun run(){}\nclass Local {fun caller(){run()}}\n}",
                Some(3),
            ),
            (
                "fun run(){}\nobject Outer {\nfun run(){}\nobject Nested {fun caller(){run()}}\n}",
                Some(3),
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.kt",
                Some("kotlin"),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let calls = facts
                .calls()
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == "run")
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 1, "{source}: {facts:?}");
            assert!(!facts.bindings_incomplete, "{source}");
            if expected_line.is_none() {
                assert!(
                    matches!(calls[0].binding, SourceCallBinding::Shadowed),
                    "{source}: {:?}",
                    calls[0].binding
                );
            }
            let actual = match calls[0].binding {
                SourceCallBinding::LocalDeclaration(target) => {
                    Some(graph.symbols[target].line_start)
                }
                _ => None,
            };
            if actual != expected_line {
                failures.push(format!(
                    "{source}: expected {expected_line:?}, actual {actual:?}: {facts:?}"
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn cpp_condition_declarations_end_at_their_owning_statement() {
        let mut failures = Vec::new();
        for statement in [
            "if(auto run=next()){run();}",
            "switch(auto run=next();0){default:run();}",
            "for(auto run=next();run;){run();}",
            "while(auto run=next()){run();}",
        ] {
            let source = format!(
                "using Callback=void(*)(); Callback next(){{return nullptr;}} void run(){{}} void caller(){{{statement} run();}}"
            );
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.cpp",
                Some("cpp"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let calls = facts
                .calls()
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == "run")
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 2, "{statement}: {facts:?}");
            if !matches!(calls[0].binding, SourceCallBinding::Shadowed)
                || !matches!(calls[1].binding, SourceCallBinding::LocalDeclaration(_))
            {
                failures.push(format!("{statement}: {calls:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn cpp_union_methods_and_prototypes_retain_member_identity() {
        for member in ["void run();", "void run(){}"] {
            let source =
                format!("void run(){{}}\nunion Owner {{{member} void caller(){{run();}}}};");
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.cpp",
                Some("cpp"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 1, "{source}: {facts:?}");
            assert!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                "{source}: {:?}",
                facts.calls()
            );
            let SourceCallBinding::LocalDeclaration(target) = facts.calls()[0].binding else {
                return;
            };
            assert_eq!(
                graph.symbols[target].kind,
                projectatlas_core::symbols::SymbolKind::Method,
                "{source}: {graph:?}"
            );
        }
    }

    #[test]
    fn native_accessor_and_parameter_environments_block_false_outer_calls() {
        let mut failures = Vec::new();
        for (label, language, name, source, shadowed) in [
            (
                "Kotlin backing getter",
                "kotlin",
                "field",
                "fun field(){}\nclass Api {\nvar callback:()->Unit = {}\nget(){field();return field}\n}\nfun caller(){field()}\n",
                true,
            ),
            (
                "Kotlin backing setter",
                "kotlin",
                "field",
                "fun field(){}\nclass Api {\nvar callback:()->Unit = {}\nset(next){field();field=next}\n}\nfun caller(){field()}\n",
                true,
            ),
            (
                "CSharp setter",
                "csharp",
                "value",
                "using System;class Api {static void value(){} Action Callback {set{value();}} static void caller(){value();}}",
                true,
            ),
            (
                "CSharp arrow setter",
                "csharp",
                "value",
                "using System;class Api {static void value(){} Action Callback {set=>value();} static void caller(){value();}}",
                true,
            ),
            (
                "CSharp init",
                "csharp",
                "value",
                "using System;class Api {static void value(){} Action Callback {get=>()=>{};init{value();}} static void caller(){value();}}",
                true,
            ),
            (
                "CSharp event add",
                "csharp",
                "value",
                "using System;class Api {static void value(){} event Action Callback {add{value();}remove{}} static void caller(){value();}}",
                true,
            ),
            (
                "CSharp event remove",
                "csharp",
                "value",
                "using System;class Api {static void value(){} event Action Callback {add{}remove{value();}} static void caller(){value();}}",
                true,
            ),
            (
                "CSharp getter positive",
                "csharp",
                "value",
                "using System;class Api {static void value(){} Action Callback {get{value();return()=>{};}} static void caller(){value();}}",
                false,
            ),
            (
                "CSharp indexer",
                "csharp",
                "run",
                "using System;class Api {static void run(){} Action this[Action run] {get{run();return run;}} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp local function",
                "csharp",
                "run",
                "using System;class Api {static void run(){} void caller(){void inner(Action run){run();}run();}}",
                true,
            ),
            (
                "CSharp anonymous delegate",
                "csharp",
                "run",
                "using System;class Api {static void run(){} void caller(){Action<Action> callback=delegate(Action run){run();};run();}}",
                true,
            ),
            (
                "CSharp operator",
                "csharp",
                "run",
                "using System;class Api {static void run(){} public static Api operator+(Api self,Action run){run();return self;} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp conversion",
                "csharp",
                "run",
                "using System;class Api {static void run(){} public static implicit operator Api(Action run){run();return new Api();} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary class",
                "csharp",
                "run",
                "using System;class Outer {static void run(){} class Api(Action run){void inside(){run();}} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary struct",
                "csharp",
                "run",
                "using System;class Outer {static void run(){} struct Api(Action run){void inside(){run();}} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp positional record",
                "csharp",
                "run",
                "using System;class Outer {static void run(){} record Api(Action run){void inside(){run();}} static void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary member precedence",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){Func<int> callback=()=>run();int run()=>0;void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary property initializer",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){Func<int> Callback{get;}=()=>run();int run()=>0;void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary event initializer",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){event Func<int> Callback=()=>run();int run()=>0;void caller(){run();}}",
                true,
            ),
            (
                "CSharp primary base argument",
                "csharp",
                "run",
                "using System;class Base{public Base(int x){}}class Api(Func<int> run):Base(run()){int run()=>0;void caller(){run();}}",
                true,
            ),
            (
                "CSharp partial primary earlier initializer",
                "csharp",
                "run",
                "using System;partial class Api{Func<int> callback=()=>run();void caller(){run();}}partial class Api(Func<int> run){int run()=>0;}",
                true,
            ),
            (
                "CSharp later field",
                "csharp",
                "run",
                "using System;class Outer{static void run(){}class Api{void inside(){run();}Action run=()=>{};}static void caller(){run();}}",
                true,
            ),
            (
                "CSharp property value",
                "csharp",
                "run",
                "using System;class Outer{static void run(){}class Api{void inside(){run();}Action run{get;}=()=>{};}static void caller(){run();}}",
                true,
            ),
            (
                "CSharp later event field",
                "csharp",
                "run",
                "using System;class Outer{static void run(){}class Api{void inside(){run();}event Action run;}static void caller(){run();}}",
                true,
            ),
            (
                "CSharp static field initializer",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){static Func<int> callback=()=>run();static int run()=>0;static void caller(){run();}}",
                false,
            ),
            (
                "CSharp static property initializer",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){static Func<int> Callback{get;}=()=>run();static int run()=>0;static void caller(){run();}}",
                false,
            ),
            (
                "CSharp static event initializer",
                "csharp",
                "run",
                "using System;class Api(Func<int> run){static event Func<int> Callback=()=>run();static int run()=>0;static void caller(){run();}}",
                false,
            ),
            (
                "CSharp partial positional record without body",
                "csharp",
                "run",
                "using System;class Outer{static void run(){}partial record Api(Action run);partial record Api{void inside(){run();}}static void caller(){run();}}",
                true,
            ),
            (
                "Java record component blocker",
                "java",
                "run",
                "class Outer{static void run(){}record Api(Runnable run){void inside(){run();}}static void caller(){run();}}",
                true,
            ),
            (
                "Java explicit component accessor",
                "java",
                "run",
                "class Outer{static void run(){}record Api(Runnable run){public Runnable run(){return run;}void inside(){run();}}static void caller(){run();}}",
                false,
            ),
            (
                "Java enum values blocker",
                "java",
                "values",
                "class Outer{static void values(){}enum Api{ONE;void inside(){values();}}static void caller(){values();}}",
                true,
            ),
            (
                "Java enum valueOf blocker",
                "java",
                "valueOf",
                "class Outer{static void valueOf(String text){}enum Api{ONE;void inside(){valueOf(\"ONE\");}}static void caller(){valueOf(\"ONE\");}}",
                true,
            ),
            (
                "Java record disjoint-arity overload",
                "java",
                "value",
                "class Outer{static void value(int n){}record Api(int value){void value(int n){}void inside(){value(1);}}static void caller(){value(1);}}",
                false,
            ),
            (
                "Java enum disjoint-arity overload",
                "java",
                "values",
                "class Outer{static void values(int n){}enum Api{ONE;void values(int n){}void inside(){values(1);}}static void caller(){values(1);}}",
                false,
            ),
            (
                "Java enum unknown same-arity overload",
                "java",
                "valueOf",
                "class Outer{static void valueOf(int n){}enum Api{ONE;void valueOf(int n){}void inside(){valueOf(1);}}static void caller(){valueOf(1);}}",
                true,
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{label}: {result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let calls = facts
                .calls()
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == name)
                .collect::<Vec<_>>();
            if calls.len() != 2 {
                failures.push(format!("{label}: unexpected call admission {calls:?}"));
                continue;
            }
            if facts.bindings_incomplete
                || matches!(calls[0].binding, SourceCallBinding::Shadowed) != shadowed
                || !shadowed && !matches!(calls[0].binding, SourceCallBinding::LocalDeclaration(_))
                || !matches!(calls[1].binding, SourceCallBinding::LocalDeclaration(_))
            {
                failures.push(format!("{label}: {calls:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn native_type_and_local_function_symbols_keep_actual_declaration_owners() {
        for (language, source, expected_kind) in [
            (
                "java",
                "record Left(){void run(){}} record Right(){void run(){} void caller(){run();}}",
                projectatlas_core::symbols::SymbolKind::Class,
            ),
            (
                "csharp",
                "record Left{void run(){}} record Right{void run(){} void caller(){run();}}",
                projectatlas_core::symbols::SymbolKind::Class,
            ),
            (
                "csharp",
                "record struct Left{void run(){}} record struct Right{void run(){} void caller(){run();}}",
                projectatlas_core::symbols::SymbolKind::Struct,
            ),
            (
                "cpp",
                "union Left{void run(){}};union Right{void run(){}void caller(){run();}};",
                projectatlas_core::symbols::SymbolKind::Struct,
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                "{language}: {facts:?}"
            );
            let SourceCallBinding::LocalDeclaration(target) = facts.calls()[0].binding else {
                return;
            };
            assert_eq!(
                graph.symbols[target].parent.as_deref(),
                Some("Right"),
                "{language}: {graph:?}"
            );
            assert!(
                graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == "Right" && symbol.kind == expected_kind),
                "{language}: {graph:?}"
            );
        }
        let source = "class Api{static void run(){}static void caller(){void run(){}run();}static void outside(){run();}}";
        let result = extract_symbol_graph_with_semantics_controlled(
            "owners.cs",
            Some("csharp"),
            source,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else { return };
        let (_, graph, facts) = extracted.into_parts();
        let calls = facts.calls();
        assert_eq!(calls.len(), 2);
        assert!(
            matches!(calls[0].binding, SourceCallBinding::LocalDeclaration(_))
                && matches!(calls[1].binding, SourceCallBinding::LocalDeclaration(_)),
            "{facts:?}"
        );
        let (
            SourceCallBinding::LocalDeclaration(local),
            SourceCallBinding::LocalDeclaration(outer),
        ) = (&calls[0].binding, &calls[1].binding)
        else {
            return;
        };
        assert_eq!(
            graph.symbols[*local].kind,
            projectatlas_core::symbols::SymbolKind::Function
        );
        assert_eq!(graph.symbols[*local].parent.as_deref(), Some("caller"));
        assert_eq!(
            graph.symbols[*outer].kind,
            projectatlas_core::symbols::SymbolKind::Method
        );
        assert_eq!(graph.symbols[*outer].parent.as_deref(), Some("Api"));
    }

    #[test]
    fn invalid_identity_neighbors_preserve_valid_calls_and_collision_blockers() {
        for prefix in [
            "import './bad\0identity';\n",
            "function invalid(value='bad\0identity'){}\n",
        ] {
            for (body, expected_exact) in [
                ("function run(){}function caller(){run();}", true),
                (
                    "function caller(){\n{function run(){}\nrun();}\n{function run(){}\nrun();}\n}",
                    false,
                ),
            ] {
                let source = format!("'use strict';\n{prefix}{body}");
                let result = extract_symbol_graph_with_semantics_controlled(
                    "neighbors.js",
                    Some("javascript"),
                    &source,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(result.is_ok(), "{result:?}");
                let Ok(extracted) = result else { return };
                let facts = extracted.semantics();
                assert_eq!(facts.calls().len(), if expected_exact { 1 } else { 2 });
                for call in facts.calls() {
                    assert_eq!(
                        matches!(call.binding, SourceCallBinding::LocalDeclaration(_)),
                        expected_exact,
                        "{source:?}: {facts:?}"
                    );
                    if !expected_exact {
                        assert_eq!(call.binding, SourceCallBinding::Unresolved);
                    }
                }
            }
        }
    }

    #[test]
    fn rust_competing_imports_cannot_override_colliding_call_owners_or_targets() {
        for (body, expected_candidates) in [
            (
                "fn outer(){\n{fn caller(){use crate::a::run;fn run(){}run();}}\n{fn caller(){use crate::a::run;fn run(){}run();}}\n}",
                0,
            ),
            (
                "fn outer(){\n{fn run(){}use crate::a::run;run();}\n{fn run(){}use crate::a::run;run();}\n}",
                0,
            ),
            ("fn caller(){use crate::a::run;fn run(){}run();}", 1),
        ] {
            let source = format!("mod a{{pub struct run{{}}}}\n{body}");
            let result = extract_symbol_graph_with_semantics_controlled(
                "src/lib.rs",
                Some("rust"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let facts = extracted.semantics();
            let rust = facts.rust();
            assert!(rust.is_some(), "{facts:?}");
            let Some(rust) = rust else { return };
            assert_eq!(
                rust.candidate_calls.len(),
                expected_candidates,
                "{source}: {facts:?}"
            );
            if expected_candidates == 0 {
                assert!(
                    facts
                        .calls()
                        .iter()
                        .all(|call| call.binding == SourceCallBinding::Unresolved),
                    "{facts:?}"
                );
            }
        }
    }

    #[test]
    fn distinct_lexical_declarations_with_one_graph_identity_withdraw_call_authority() {
        for (language, source) in [
            (
                "csharp",
                "class Api { static void caller() {\n{ void run() {}\nrun(); }\n{ void run() {}\nrun(); }\n} }",
            ),
            (
                "java",
                "class Api { void caller() {\n{ record Box() { void run() { run(); } } }\n{ record Box() { void run() { run(); } } }\n} }",
            ),
            (
                "javascript",
                "'use strict'; function caller() {\n{ function run() {}\nrun(); }\n{ function run() {}\nrun(); }\n}",
            ),
            (
                "rust",
                "fn caller() {\n{ fn run() {}\nrun(); }\n{ fn run() {}\nrun(); }\n}",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "scopes.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls().len(), 2, "{language}: {graph:?}");
            assert!(
                facts
                    .calls()
                    .iter()
                    .all(|call| call.binding == SourceCallBinding::Unresolved),
                "{language}: {facts:?}"
            );
            assert_eq!(
                graph
                    .symbols
                    .iter()
                    .filter(|symbol| symbol.name == "run")
                    .count(),
                2,
                "source navigation retains both declarations"
            );
        }
        for (language, source) in [
            (
                "cpp",
                "namespace N { void run(); }\nnamespace N { void run() {} void caller(){run();} }",
            ),
            (
                "cpp",
                "class Api { public: void run(); void caller(){run();} };\nvoid Api::run() {}",
            ),
            (
                "csharp",
                "partial class Api { partial void run(); }\npartial class Api { partial void run() {} void caller(){run();} }",
            ),
            (
                "java",
                "class Left { record Box(){void run(){run();}} }\nclass Right { record Box(){void run(){run();}} }",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                "scopes.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let parents = crate::declaration_identity::qualified_symbol_parents(&graph);
            assert!(parents.is_ok(), "{parents:?}");
            let Ok(parents) = parents else { return };
            for call in facts.calls() {
                if let SourceCallBinding::LocalDeclarations(ordinals) = &call.binding {
                    let keys = ordinals
                        .iter()
                        .map(|ordinal| {
                            let symbol = &graph.symbols[*ordinal];
                            (
                                symbol.name.as_str(),
                                symbol.kind.to_string(),
                                parents[*ordinal].as_ref(),
                                symbol.signature.trim(),
                            )
                        })
                        .collect::<std::collections::BTreeSet<_>>();
                    assert_eq!(
                        keys.len(),
                        1,
                        "equivalent declarations retain one graph identity: {language}: {graph:?}"
                    );
                }
            }
            assert!(!facts.calls().is_empty(), "{language}: {facts:?}");
            assert!(
                facts.calls().iter().all(|call| matches!(
                    call.binding,
                    SourceCallBinding::LocalDeclaration(_)
                        | SourceCallBinding::LocalDeclarations(_)
                )),
                "{language}: {facts:?}"
            );
        }
    }

    #[test]
    fn csharp_explicit_interface_members_keep_ordinary_outer_calls() {
        for member in [
            "void I.run(){}",
            "Action I.run {get=>()=>{};}",
            "event Action I.run {add{}remove{}}",
        ] {
            let contract = if member.starts_with("void") {
                "void run();"
            } else if member.starts_with("event") {
                "event Action run;"
            } else {
                "Action run{get;}"
            };
            let source = format!(
                "using System;interface I{{{contract}}}class Outer{{static void run(){{}}class Api:I{{{member}void inside(){{run();}}}}static void caller(){{run();}}}}"
            );
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.cs",
                Some("csharp"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            let calls = facts
                .calls()
                .iter()
                .filter(|call| graph.relations[call.relation].target_name == "run")
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 2, "{member}: {facts:?}");
            assert!(
                matches!(calls[0].binding, SourceCallBinding::LocalDeclaration(_))
                    && matches!(calls[1].binding, SourceCallBinding::LocalDeclaration(_)),
                "{member}: {calls:?}"
            );
            let (
                SourceCallBinding::LocalDeclaration(inside),
                SourceCallBinding::LocalDeclaration(outside),
            ) = (&calls[0].binding, &calls[1].binding)
            else {
                return;
            };
            assert_eq!(inside, outside, "{member}: {calls:?}");
            assert_eq!(graph.symbols[*inside].parent.as_deref(), Some("Outer"));
        }
    }

    #[test]
    fn csharp_constructor_views_share_scope_and_binding_admission() {
        use std::fmt::Write as _;
        for (scope_stress, amount, complete) in [
            (false, super::MAX_LEXICAL_BINDINGS / 6 - 8, true),
            (false, super::MAX_LEXICAL_BINDINGS / 6 + 1, false),
            (true, 1600, true),
            (true, 1700, false),
        ] {
            let mut source =
                String::from("class Root{static void run(){}static void caller(){run();}");
            let classes = if scope_stress { amount } else { 3 };
            for index in 0..classes {
                assert!(write!(&mut source, "class Owner{index}(").is_ok());
                let parameters = if scope_stress { 1 } else { amount };
                for parameter in 0..parameters {
                    if parameter > 0 {
                        source.push(',');
                    }
                    assert!(write!(&mut source, "int p{parameter}").is_ok());
                }
                source.push_str("){");
                if scope_stress {
                    source.push_str("void work(){{}{}{}{}{}}");
                }
                source.push('}');
            }
            source.push('}');
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.cs",
                Some("csharp"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let (_, graph, facts) = extracted.into_parts();
            assert!(
                graph.symbols.len() < crate::MAX_SYMBOLS_PER_FILE
                    && graph.relations.len() < crate::MAX_RELATIONS_PER_FILE,
                "fixture exhausted raw graph before lexical admission: symbols {}, relations {}",
                graph.symbols.len(),
                graph.relations.len()
            );
            assert_eq!(facts.calls().len(), 1);
            assert_eq!(
                !facts.bindings_incomplete, complete,
                "{scope_stress}/{amount}"
            );
            assert_eq!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                complete,
                "{scope_stress}/{amount}: {facts:?}"
            );
        }
    }

    #[test]
    fn kotlin_constructor_views_share_the_lexical_retention_budget() {
        use std::fmt::Write as _;
        for (classes, parameters, complete) in [
            (2, super::MAX_LEXICAL_BINDINGS / 6 - 8, true),
            (2, super::MAX_LEXICAL_BINDINGS / 6 + 1, false),
            (1, super::MAX_LEXICAL_BINDINGS + 1, false),
        ] {
            let mut source = String::from("fun run(){}\nfun caller(){run()}\n");
            for class in 0..classes {
                assert!(write!(&mut source, "class Owner{class}(").is_ok());
                for parameter in 0..parameters {
                    if parameter > 0 {
                        source.push(',');
                    }
                    assert!(write!(&mut source, "val p{parameter}:Int").is_ok());
                }
                source.push_str("){val initialized=0}\n");
            }
            let result = extract_symbol_graph_with_semantics_controlled(
                "owners.kt",
                Some("kotlin"),
                &source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else { return };
            let facts = extracted.semantics();
            assert_eq!(facts.calls().len(), 1);
            assert_eq!(
                !facts.bindings_incomplete, complete,
                "{classes} classes/{parameters} parameters"
            );
            assert_eq!(
                matches!(
                    facts.calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                complete
            );
        }
    }

    #[test]
    fn deferred_bodies_resolve_unique_later_enclosing_bindings() {
        for (path, language, content) in [
            (
                "main.py",
                "python",
                "def caller(): return run()\ndef run(): pass\n",
            ),
            (
                "main.py",
                "python",
                "def outer():\n def caller(): return run()\n def run(): pass\n",
            ),
            (
                "main.py",
                "python",
                "class Api:\n def caller(self): return run()\ndef run(): pass\n",
            ),
            (
                "main.js",
                "javascript",
                "const caller = () => run(); const run = () => {};",
            ),
            (
                "main.js",
                "javascript",
                "function outer(){const caller = () => run(); const run = () => {}; }",
            ),
            (
                "main.js",
                "javascript",
                "class Api { caller(){run();} } const run = () => {};",
            ),
            (
                "main.ts",
                "typescript",
                "const caller = (): void => run(); const run = (): void => {};",
            ),
            (
                "main.tsx",
                "tsx",
                "const caller = (): void => run(); const run = (): void => {};",
            ),
            (
                "main.py",
                "python",
                "pending = (run() for _ in [1])\ndef run(): pass\n",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 1, "{language}: {content}");
            assert!(
                matches!(
                    facts.calls[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                "{language}: {content}: {:?}",
                facts.calls[0].binding
            );
            let SourceCallBinding::LocalDeclaration(target) = facts.calls[0].binding else {
                return;
            };
            assert_eq!(graph.symbols[target].name, "run", "{language}: {content}");
        }
    }

    #[test]
    fn instance_field_values_defer_without_deferring_class_evaluation() {
        for language in ["javascript", "typescript", "tsx"] {
            for (source, expected) in [
                (
                    "class Api { @run() value=0; } const run=()=>{};",
                    vec![false],
                ),
                (
                    "const run=()=>{}; class Api { @run() value=0; }",
                    vec![true],
                ),
                (
                    "new (class {value=run();})(); const run=()=>{};",
                    vec![false],
                ),
                (
                    "class Api { value = run(); } const run = () => {};",
                    vec![true],
                ),
                (
                    "class Api { #value = run(); } const run = () => {};",
                    vec![true],
                ),
                (
                    "const Api = class { value = run(); }; const run = () => {};",
                    vec![true],
                ),
                (
                    "class Api { static = run(); } const run = () => {};",
                    vec![true],
                ),
                (
                    "class Api { [run()] = run(); } const run = () => {};",
                    vec![false, true],
                ),
                (
                    "class Api { static value = run(); } const run = () => {};",
                    vec![false],
                ),
                (
                    "class Api { static {run();} } const run = () => {};",
                    vec![false],
                ),
                (
                    "class Api extends run() {} const run = () => {};",
                    vec![false],
                ),
                (
                    "new class { value = run(); }; const run = () => {};",
                    vec![false],
                ),
                (
                    "class Api { value = run(); } const run = () => {}; run = unknown;",
                    vec![false],
                ),
                (
                    "const unknown=()=>{}; var run=()=>{}; class Api { value=(run=unknown); } var run=()=>{}; function caller(){run();}",
                    vec![false],
                ),
                (
                    "class Api { value = (run = unknown, run()); } let run = () => {};",
                    vec![false],
                ),
                (
                    "class Api { value = (run = unknown); } let run = () => {}; function caller(){run();}",
                    vec![false],
                ),
                (
                    "class Mutator {value=(run=unknown);} let run=()=>{}; class Api {value=run();}",
                    vec![false],
                ),
            ] {
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "fields.source",
                    Some(language),
                    source,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {source}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let actual: Vec<_> = extracted
                    .semantics()
                    .calls()
                    .iter()
                    .map(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                    .collect();
                assert_eq!(
                    actual,
                    expected,
                    "{language}: {source}: {:?}",
                    extracted.semantics()
                );
            }
        }
    }

    #[test]
    fn instance_field_evaluation_preserves_admitted_script_hosts_and_callbacks() {
        for (path, open) in [
            ("main.html", "<script>"),
            ("main.htm", "<script type=module>"),
            ("main.vue", "<script lang=ts>"),
            ("main.vue", "<script setup lang=ts>"),
            ("main.svelte", "<script module lang=ts>"),
            ("main.svelte", "<script context=module>"),
            ("main.svelte", "<script>"),
        ] {
            for (source, expected) in [
                (
                    "class Mutator {value=(run=unknown);} let run=()=>{}; class Api {value=run();}",
                    false,
                ),
                ("class Api { value = run(); } const run = () => {};", true),
                (
                    "class Api { static value = run(); } const run = () => {};",
                    false,
                ),
                ("class Api { [run()] = 0; } const run = () => {};", false),
                ("new class { value = run(); }; const run = () => {};", false),
                (
                    "class Api { static value = () => run(); } const run = () => {};",
                    true,
                ),
            ] {
                let source = format!("{open}{source}</script>");
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    None,
                    &source,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{path}: {source}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let calls = extracted.semantics().calls();
                assert_eq!(calls.len(), 1, "{path}: {source}");
                assert_eq!(
                    matches!(calls[0].binding, SourceCallBinding::LocalDeclaration(_)),
                    expected,
                    "{path}: {source}: {calls:?}"
                );
            }
        }
    }

    #[test]
    fn deferred_field_writes_withhold_mutated_export_authority() {
        for language in ["javascript", "typescript", "tsx"] {
            let source =
                "class Api { value=(run=1); } export let run=()=>{}; export {run as launch};";
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "fields.source",
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert!(!extracted.semantics().exports().is_empty(), "{language}");
            assert!(
                extracted
                    .semantics()
                    .exports()
                    .iter()
                    .all(|export| !matches!(
                        export.binding,
                        crate::SourceExportBinding::LocalDeclaration(_)
                    )),
                "{language}: {:?}",
                extracted.semantics()
            );
        }
    }

    #[test]
    fn deferred_lookup_keeps_immediate_evaluation_and_future_competitors_conservative() {
        for (path, language, content) in [
            ("main.py", "python", "run()\ndef run(): pass\n"),
            ("main.py", "python", "(lambda: run())()\ndef run(): pass\n"),
            (
                "main.py",
                "python",
                "pending = [run() for _ in [1]]\ndef run(): pass\n",
            ),
            (
                "main.py",
                "python",
                "pending = (0 for _ in run())\ndef run(): pass\n",
            ),
            (
                "main.py",
                "python",
                "def run(): pass\ndef caller(): run()\nrun = unknown\n",
            ),
            (
                "main.py",
                "python",
                "def caller(value=run()): pass\ndef run(): pass\n",
            ),
            ("main.js", "javascript", "run(); const run = () => {};"),
            (
                "main.js",
                "javascript",
                "(() => run())(); const run = () => {};",
            ),
            (
                "main.ts",
                "typescript",
                "((() => run()) as unknown as () => void)(); const run = () => {};",
            ),
            (
                "main.js",
                "javascript",
                "const run = () => {}; function caller(){run();} run = unknown;",
            ),
            (
                "main.js",
                "javascript",
                "function caller(){run();} const run = () => {}; run = unknown;",
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert!(!facts.calls.is_empty(), "{language}: {content}");
            assert!(
                facts.calls.iter().all(|call| matches!(
                    call.binding,
                    SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                )),
                "{language}: {content}: {:?}",
                facts.calls
            );
        }
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.py",
            Some("python"),
            "def caller(): run()\nfrom worker import run\n",
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 1);
        assert!(matches!(
            facts.calls[0].binding,
            SourceCallBinding::Imported { .. }
        ));
    }

    #[test]
    fn ecmascript_script_names_keep_legal_local_call_bindings() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("html", "main.html", true),
        ] {
            for name in [
                "eval",
                "arguments",
                "await",
                "yield",
                "let",
                "static",
                "implements",
                "interface",
                "package",
                "private",
                "protected",
                "public",
            ] {
                let script = format!("function {name}(){{}} {name}();");
                let content = if host {
                    format!("<script>{script}</script>")
                } else {
                    script
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, _, facts) = extracted.into_parts();
                let failure_ast = (facts.calls.len() != 1).then(|| {
                    crate::tree_sitter_language(if host { "javascript" } else { language })
                        .and_then(|grammar| {
                            crate::parse_tree_sitter_language(&grammar, &content, &mut || {
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .ok()
                            .flatten()
                        })
                        .map(|tree| {
                            let mut cursor = tree.root_node().walk();
                            let mut nodes = Vec::new();
                            loop {
                                let node = cursor.node();
                                nodes.push((
                                    node.kind().to_string(),
                                    content.get(node.byte_range()).map(ToString::to_string),
                                ));
                                if cursor.goto_first_child() {
                                    continue;
                                }
                                while !cursor.goto_next_sibling() {
                                    if !cursor.goto_parent() {
                                        return format!("{nodes:?}");
                                    }
                                }
                            }
                        })
                });
                assert_eq!(
                    facts.calls.len(),
                    1,
                    "{language}: {content}: {failure_ast:?}"
                );
                assert!(
                    matches!(
                        facts.calls[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    "{language}: {content}: {:?}",
                    facts.calls
                );
            }
        }
    }

    #[test]
    fn ecmascript_contexts_keep_reference_roles_and_implicit_arguments_owners() {
        for (content, expected) in [
            (
                "function eval(){} function caller(){'use strict';eval();}",
                vec![true],
            ),
            ("function eval(){'use strict';} eval();", vec![false]),
            ("'use\\x20strict'; function eval(){} eval();", vec![true]),
            ("'use strict'; function eval(){} eval();", vec![false]),
            (
                "function arguments(){} arguments(); function caller(){arguments();} const arrow=()=>arguments();",
                vec![true, false, true],
            ),
            (
                "function arguments(){} function caller(){const arrow=()=>arguments();}",
                vec![false],
            ),
            (
                "function arguments(){} const object={method(){arguments();}};",
                vec![false],
            ),
            (
                "function arguments(){} class C{[arguments()]=arguments();}",
                vec![true, false],
            ),
            (
                "const C=class {method(){function eval(){} eval();}};",
                vec![false],
            ),
            ("async function await(){} await();", vec![true]),
            ("function* yield(){} yield();", vec![true]),
            ("const run=async function await(){};run();", vec![false]),
            ("const run=function* yield(){};run();", vec![false]),
            (
                "function eval(){} function caller(eval){'use strict';eval();}",
                vec![false],
            ),
            (
                "function eval(){} function caller(){'use strict';let eval;eval();}",
                vec![false],
            ),
            (
                "function arguments(){} const o={[arguments()](){arguments();}};",
                vec![true, false],
            ),
            (
                "function await(){} class C{field=await();static{await();}}",
                vec![true, false],
            ),
            (
                "function* caller(){const run=function yield(){};run();}",
                vec![true],
            ),
            (
                "async function caller(){const run=function await(){};run();}",
                vec![true],
            ),
            (
                "function Inner(){} const C=class Inner{method(){Inner();}};",
                vec![false],
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.js",
                Some("javascript"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{content}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            let observed: Vec<_> = extracted
                .semantics()
                .calls()
                .iter()
                .map(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                .collect();
            assert_eq!(observed, expected, "{content}: {:?}", extracted.semantics());
        }
    }

    #[test]
    fn html_module_bindings_do_not_leak_into_classic_scripts() {
        let content = "<script type=\"module\">function local(){} local();</script>\n<script>function caller(){local();}</script>";
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.html",
            Some("html"),
            content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else { return };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 2, "{:?}", facts.calls);
        assert!(
            matches!(
                facts.calls[0].binding,
                SourceCallBinding::LocalDeclaration(_)
            ) && matches!(facts.calls[1].binding, SourceCallBinding::Unresolved),
            "{:?}",
            facts.calls
        );
    }

    #[test]
    fn host_programs_keep_scope_direction_and_independent_grammar_ranges() {
        for (language, content, expected) in [
            (
                "html",
                "<script>function shared(){}</script><script type=module>shared();</script><script>shared();</script>",
                vec![true, true],
            ),
            (
                "html",
                "<script type=module>function hidden(){}</script><script type=module>hidden();</script>",
                vec![false],
            ),
            (
                "html",
                "<script>function eval(){}</script><script>'use strict';eval();</script><script>function arguments(){} arguments();</script>",
                vec![true, true],
            ),
            (
                "html",
                "<script>function run(){} run()</script><script>run()</script>",
                vec![true, true],
            ),
            (
                "html",
                "<script lang=ts>function run(){} run()</script><script lang=ts>run()</script>",
                vec![true, true],
            ),
            (
                "html",
                "<script>function broken(</script><script>function run(){} run();</script>",
                vec![true],
            ),
            (
                "vue",
                "<script>function normal(){} setup();</script><script setup>normal(); function setup(){}</script>",
                vec![false, true],
            ),
            (
                "svelte",
                "<script module>function moduleLocal(){} instanceLocal();</script><script>moduleLocal(); function instanceLocal(){}</script>",
                vec![false, true],
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                &format!("main.{language}"),
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            let observed: Vec<_> = extracted
                .semantics()
                .calls()
                .iter()
                .map(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                .collect();
            assert_eq!(
                observed,
                expected,
                "{language}: {content}: {:?}",
                extracted.semantics()
            );
        }
        for (language, instance) in [("vue", "setup"), ("svelte", "")] {
            let module = if language == "vue" { "" } else { "module" };
            let content = format!(
                "<script {instance}>import {{run}} from './worker'; run();</script><script {module}>run();</script>"
            );
            let extracted = extract_symbol_graph_with_semantics_controlled(
                &format!("main.{language}"),
                Some(language),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert_eq!(extracted.semantics().calls().len(), 2);
            assert!(
                extracted
                    .semantics()
                    .calls()
                    .iter()
                    .all(|call| matches!(call.binding, SourceCallBinding::Imported { .. })),
                "{language}: {:?}",
                extracted.semantics()
            );
        }
    }

    #[test]
    fn component_import_hoisting_reaches_a_preceding_module_callable() {
        for (language, source) in [
            (
                "vue",
                "<script>function caller(){run();}</script><script setup>import {run} from './worker.js';</script>",
            ),
            (
                "svelte",
                "<script module>function caller(){run();}</script><script>import {run} from './worker.js';</script>",
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                &format!("main.{language}"),
                Some(language),
                source,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert_eq!(extracted.semantics().calls().len(), 1);
            assert!(
                matches!(
                    extracted.semantics().calls()[0].binding,
                    SourceCallBinding::Imported { .. }
                ),
                "{language}: {:?}: {:?}",
                extracted.semantics(),
                extracted.graph()
            );
        }
    }

    #[test]
    fn contextual_keyword_calls_keep_arguments_and_real_operators_distinct() {
        for language in ["javascript", "typescript", "tsx"] {
            for (content, expected) in [
                (
                    "function await(){} const caller=()=>await();",
                    vec!["await"],
                ),
                (
                    "function yield(){} const caller=()=>yield();",
                    vec!["yield"],
                ),
                (
                    "function interface(){} const caller=()=>interface();",
                    vec!["interface"],
                ),
                ("function yield(){} yield(yield());", vec!["yield", "yield"]),
                ("function await(){} await(await());", vec!["await", "await"]),
                ("'use strict'; function await(){} await();", vec!["await"]),
                (
                    "async function outer(){function inner(){function await(){} await();}}",
                    vec!["await"],
                ),
                (
                    "function yield(){} function* gen(){yield 1;} yield();",
                    vec!["yield"],
                ),
                ("function* gen(){yield 1;}", vec![]),
                ("async function caller(){await unknown;}", vec![]),
                ("export {}; await unknown;", vec![]),
                ("class C{static{await unknown;}}", vec![]),
                ("function await(){} await(", vec![]),
                ("function await(){} await(value:number);", vec![]),
                ("function await(){} await(value?:number);", vec![]),
                ("function await(){} await(...values:number[]);", vec![]),
                ("function await(){} await(...values);", vec!["await"]),
                (
                    "function yield(){} function target(){} yield* target();",
                    vec!["target"],
                ),
                (
                    "function await(){} function target(){} await* target();",
                    vec!["target"],
                ),
                (
                    "function target(){} function* caller(){yield* target();}",
                    vec!["target"],
                ),
                (
                    "function target(){} const caller=async()=>await(target());",
                    vec!["target"],
                ),
            ] {
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    &format!(
                        "main.{}",
                        if language == "javascript" {
                            "js"
                        } else if language == "typescript" {
                            "ts"
                        } else {
                            "tsx"
                        }
                    ),
                    Some(language),
                    content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (.., graph, facts) = extracted.into_parts();
                let calls: Vec<_> = facts
                    .calls
                    .iter()
                    .map(|call| graph.relations[call.relation].target_name.as_str())
                    .collect();
                let ast = (calls != expected).then(|| {
                    crate::tree_sitter_language(language)
                        .and_then(|grammar| {
                            crate::parse_tree_sitter_language(&grammar, content, &mut || {
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .ok()
                            .flatten()
                        })
                        .map(|tree| {
                            let mut cursor = tree.root_node().walk();
                            let mut nodes = Vec::new();
                            loop {
                                let node = cursor.node();
                                nodes.push((
                                    node.kind().to_string(),
                                    content.get(node.byte_range()).map(ToString::to_string),
                                ));
                                if cursor.goto_first_child() {
                                    continue;
                                }
                                while !cursor.goto_next_sibling() {
                                    if !cursor.goto_parent() {
                                        return format!("{nodes:?}");
                                    }
                                }
                            }
                        })
                });
                assert_eq!(calls, expected, "{language}: {content}: {facts:?}: {ast:?}");
                if !expected.is_empty() {
                    assert!(
                        facts.calls.iter().all(|call| matches!(
                            call.binding,
                            SourceCallBinding::LocalDeclaration(_)
                        )),
                        "{language}: {content}: {facts:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn restricted_arrow_keywords_never_gain_identifier_targets() {
        for language in ["javascript", "typescript", "tsx"] {
            for (content, typed) in [
                ("function await(){} const caller=async()=>await();", false),
                (
                    "function await(){} const caller=async value=>await();",
                    false,
                ),
                (
                    "function await(){} const caller=async(value)=>await();",
                    false,
                ),
                (
                    "function await(){} export {}; const caller=()=>await();",
                    false,
                ),
                ("function await(){} const caller=(value:)=>await();", true),
                (
                    "function await(){} const caller=(...values:)=>await();",
                    true,
                ),
                ("function await(){} const caller=()\n=>await();", true),
                (
                    "function await(){} const caller=():number\n=>await();",
                    true,
                ),
                ("function await(){} const caller=async\n()=>await();", true),
            ] {
                if typed && language == "javascript" {
                    continue;
                }
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "main.ts",
                    Some(language),
                    content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{extracted:?}");
                let Ok(extracted) = extracted else { return };
                assert!(
                    extracted.semantics().calls().iter().all(|call| matches!(
                        call.binding,
                        SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                    )),
                    "{language}: {content}: {:?}",
                    extracted.semantics()
                );
            }
        }
    }

    #[test]
    fn concise_arrow_keyword_calls_retain_actual_body_and_formal_contexts() {
        let mut failures = Vec::new();
        for language in ["javascript", "typescript", "tsx"] {
            for (body, typed, resolved) in [
                ("const caller=()=>await();", false, true),
                ("const caller=(()=>await());", false, true),
                ("const caller=()=>(await());", false, true),
                ("const caller=value=>await();", false, true),
                ("const caller=(value)=>await();", false, true),
                ("const caller=()=>await(await());", false, true),
                ("function outer(){return ()=>await();}", false, true),
                (
                    "async function outer(){const caller=()=>await();}",
                    false,
                    true,
                ),
                ("const caller=(await)=>await();", false, false),
                ("const caller=(value:number)=>await();", true, true),
                ("const caller=():number=>await();", true, true),
                ("const caller=<T,>(value:T)=>await();", true, true),
                ("const caller=()/*header*/=>await();", false, true),
                ("const caller=()=>\nawait();", false, true),
                ("const values=[()=>await()];", false, true),
                ("const object={value:()=>await()};", false, true),
                ("const value=flag?()=>await():()=>0;", false, true),
                ("const value=flag?()=>0:()=>await();", false, true),
            ] {
                if typed && language == "javascript" {
                    continue;
                }
                let content = format!("function await(){{}} {body}");
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "main.ts",
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{extracted:?}");
                let Ok(extracted) = extracted else { return };
                let count = if body.contains("await(await())") {
                    2
                } else {
                    1
                };
                let facts = extracted.semantics();
                if facts.calls().len() != count
                    || facts.calls().iter().any(|call| {
                        matches!(call.binding, SourceCallBinding::LocalDeclaration(_)) != resolved
                    })
                {
                    let ast = crate::tree_sitter_language(language)
                        .and_then(|grammar| {
                            crate::parse_tree_sitter_language(&grammar, &content, &mut || {
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .ok()
                            .flatten()
                        })
                        .map(|tree| {
                            let mut cursor = tree.root_node().walk();
                            let mut errors = Vec::new();
                            loop {
                                let node = cursor.node();
                                if node.is_error() {
                                    let mut children = node.walk();
                                    errors.push(
                                        node.children(&mut children)
                                            .map(|child| {
                                                (
                                                    child.kind().to_string(),
                                                    content
                                                        .get(child.byte_range())
                                                        .unwrap_or_default()
                                                        .to_string(),
                                                )
                                            })
                                            .collect::<Vec<_>>(),
                                    );
                                }
                                if cursor.goto_first_child() {
                                    continue;
                                }
                                while !cursor.goto_next_sibling() {
                                    if !cursor.goto_parent() {
                                        return format!(
                                            "{} errors={errors:?}",
                                            tree.root_node().to_sexp()
                                        );
                                    }
                                }
                            }
                        });
                    failures.push(format!(
                        "{language}: {content}: calls={:?}: {ast:?}",
                        facts.calls()
                    ));
                }
                if body == "const caller=(await)=>await();" && facts.calls().len() == 1 {
                    assert!(matches!(
                        facts.calls()[0].binding,
                        SourceCallBinding::Shadowed
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn repaired_arrow_and_real_await_operator_keep_independent_calls() {
        for language in ["javascript", "typescript", "tsx"] {
            for repaired in [
                "const caller=()=>await();",
                "const caller=flag?()=>await():()=>0;",
                "const caller=flag?()=>0:()=>await();",
            ] {
                let content = format!(
                    "function await(){{}} function target(){{}} {repaired} async function real(){{await(target());}}"
                );
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    "main.ts",
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{extracted:?}");
                let Ok(extracted) = extracted else { return };
                let calls = extracted.semantics().calls();
                assert_eq!(calls.len(), 2, "{language}: {content}: {calls:?}");
                assert!(
                    calls
                        .iter()
                        .all(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_))),
                    "{language}: {content}: {calls:?}"
                );
                let names = calls
                    .iter()
                    .map(|call| {
                        extracted.graph().relations[call.relation]
                            .target_name
                            .as_str()
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                assert_eq!(names, std::collections::BTreeSet::from(["await", "target"]));
            }
        }
    }

    #[test]
    fn invalid_import_leaves_shadow_without_claiming_exported_name_aliases() {
        for (attributes, import, expected) in [
            ("", "import {eval} from './worker';", false),
            ("", "import eval from './worker';", false),
            ("", "import * as eval from './worker';", false),
            ("", "import {eval as legal} from './worker';", true),
            ("lang=ts", "import type {eval} from './worker';", true),
            ("lang=ts", "import {type eval} from './worker';", true),
        ] {
            let content = format!(
                "<script {attributes}>function eval(){{}}</script><script type=module {attributes}>{import} eval();</script>"
            );
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.html",
                Some("html"),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert_eq!(extracted.semantics().calls().len(), 1);
            assert_eq!(
                matches!(
                    extracted.semantics().calls()[0].binding,
                    SourceCallBinding::LocalDeclaration(_)
                ),
                expected,
                "{content}: {:?}",
                extracted.semantics()
            );
        }
    }

    #[test]
    fn script_goal_static_imports_preserve_raw_facts_without_module_authority() {
        for (path, language, content) in [
            (
                "main.html",
                "html",
                "<script>import {run} from './worker'; function caller(){run();}</script>",
            ),
            (
                "main.cjs",
                "javascript",
                "import {run} from './worker'; function caller(){run();}",
            ),
            (
                "main.js",
                "javascript",
                "function run(){} function caller(){import {run} from './worker';run();}",
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert_eq!(extracted.semantics().imports().len(), 1);
            assert!(
                extracted
                    .semantics()
                    .imports()
                    .iter()
                    .all(|fact| fact.module.is_none() && fact.references.is_empty()),
                "{path}: {:?}",
                extracted.semantics()
            );
            assert_eq!(extracted.semantics().calls().len(), 1);
            assert!(
                extracted.semantics().calls().iter().all(|call| matches!(
                    call.binding,
                    SourceCallBinding::Unresolved | SourceCallBinding::Shadowed
                )),
                "{path}: {:?}",
                extracted.semantics()
            );
        }
    }

    #[test]
    fn explicit_typescript_language_preserves_imports_without_js_goal_inference() {
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.cts",
            Some("typescript"),
            "import {run} from './worker'; function caller(){run();}",
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else { return };
        assert_eq!(extracted.semantics().imports().len(), 1);
        assert!(extracted.semantics().imports()[0].module.is_some());
        assert_eq!(extracted.semantics().calls().len(), 1);
        assert!(matches!(
            extracted.semantics().calls()[0].binding,
            SourceCallBinding::Imported { .. }
        ));
    }

    #[test]
    fn incomplete_host_roles_and_region_limits_keep_raw_calls_without_authority() {
        for (language, content) in [
            ("vue", "<script>function run(){}</script><script>function caller(){run();}</script>".to_string()),
            ("vue", "<script setup>function run(){}</script><script setup>function caller(){run();}</script>".to_string()),
            ("svelte", "<script module>function run(){}</script><script module>function caller(){run();}</script>".to_string()),
            ("svelte", "<script>function run(){}</script><script>function caller(){run();}</script>".to_string()),
            ("html", format!("<script>function run(){{}} function caller(){{run();}}</script>{}",
                "<script></script>".repeat(crate::semantic::embedded_source::MAX_EMBEDDED_SCRIPT_REGIONS))),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(&format!("main.{language}"), Some(language), &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None));
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else { return };
            assert!(extracted.graph().symbols.iter().any(|symbol| symbol.name == "run"));
            assert!(extracted.semantics().bindings_incomplete);
            assert_eq!(extracted.semantics().calls().len(), 1);
            assert!(extracted.semantics().calls().iter().all(|call| matches!(call.binding, SourceCallBinding::Unresolved)));
        }
    }

    #[test]
    fn host_export_slots_require_module_role_for_target_authority() {
        for (language, content) in [
            (
                "html",
                "<script>export default function Actual(){}</script>",
            ),
            (
                "vue",
                "<script setup>export default function Actual(){}</script>",
            ),
            (
                "svelte",
                "<script>export default function Actual(){}</script>",
            ),
            (
                "svelte",
                "<script module>export default function Actual(){}</script>",
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                &format!("main.{language}"),
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else { return };
            let (_, _, facts) = extracted.into_parts();
            let default = facts.exports().iter().find(|fact| fact.name == "default");
            assert!(default.is_some(), "{language}: {facts:?}");
            assert!(
                default.is_some_and(|fact| matches!(
                    fact.binding,
                    crate::SourceExportBinding::Unresolved
                        | crate::SourceExportBinding::UnresolvedDeclarations(_)
                )),
                "{language}: {content}: {facts:?}"
            );
        }
    }

    #[test]
    fn ecmascript_generator_declarations_retain_callable_identity() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for script in [
                "function* run(){} function caller(){run();}",
                "async function* run(){} function caller(){run();}",
                "export function* run(){} function caller(){run();}",
                "export default function* run(){} function caller(){run();}",
                "function* run(){const local=()=>{};} function caller(){run();}",
                "async function* run(){const local=function*(){};} function caller(){run();}",
            ] {
                let content = if host {
                    if script.starts_with("export ") {
                        ecmascript_module_fixture(language, script)
                    } else {
                        format!("<script>{script}</script>")
                    }
                } else {
                    script.to_string()
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, graph, facts) = extracted.into_parts();
                assert_eq!(facts.calls.len(), 1, "{language}: {script}: {graph:?}");
                assert!(
                    matches!(
                        facts.calls[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    "{language}: {script}: {:?}: {graph:?}",
                    facts.calls
                );
                let SourceCallBinding::LocalDeclaration(symbol) = facts.calls[0].binding else {
                    return;
                };
                assert_eq!(graph.symbols[symbol].name, "run", "{language}: {script}");
                assert_eq!(
                    graph.symbols[symbol].kind,
                    projectatlas_core::symbols::SymbolKind::Function,
                    "{language}: {script}"
                );
                if script.starts_with("export ") {
                    let export_name = if script.starts_with("export default ") {
                        "default"
                    } else {
                        "run"
                    };
                    assert!(facts.exports.iter().any(|fact|
                        fact.name == export_name
                            && if language == "svelte" && export_name == "default" {
                                matches!(fact.binding, crate::SourceExportBinding::UnresolvedDeclarations(_))
                            } else {
                                matches!(fact.binding, crate::SourceExportBinding::LocalDeclaration(target) if target == symbol)
                            }),
                        "{language}: {script}: {:?}", facts.exports);
                }
                if script.contains("const local") {
                    assert!(
                        graph
                            .symbols
                            .iter()
                            .filter(|symbol| symbol.name == "local")
                            .all(|symbol| symbol.kind
                                == projectatlas_core::symbols::SymbolKind::Value
                                && symbol.parent.as_deref() == Some("run")),
                        "{language}: {script}: {graph:?}"
                    );
                    if !host {
                        assert!(
                            graph.symbols.iter().any(|symbol| symbol.name == "local"),
                            "{language}: {script}: missing native local value"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ecmascript_function_body_writes_own_their_var_bindings() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for (script, resolved) in [
                (
                    "function run(){} function* unused(){var run=null;} run();",
                    true,
                ),
                (
                    "function run(){} async function* unused(){var run=null;} run();",
                    true,
                ),
                ("function run(){} function* unused(run){} run();", true),
                (
                    "function run(){} function* caller(value=run()){var run=null;}",
                    true,
                ),
                (
                    "function run(){} async function* caller(value=run()){var run=null;}",
                    true,
                ),
                (
                    "function run(){} const unused=function*(){var run=null;}; run();",
                    true,
                ),
                (
                    "function run(){} const unused=async function*(){var run=null;}; run();",
                    true,
                ),
                (
                    "function run(){} const unused=function* run(){var run=null;}; run();",
                    true,
                ),
                (
                    "function run(){} class Example{*unused(){var run=null;}} run();",
                    true,
                ),
                (
                    "function run(){} class Example{async *unused(){var run=null;}} run();",
                    true,
                ),
                (
                    "function run(){} const example={*unused(){var run=null;}}; run();",
                    true,
                ),
                (
                    "function run(){} const unused=()=>{var run=null;}; run();",
                    true,
                ),
                (
                    "function* caller(replacement){function run(){} var run=replacement; run();}",
                    false,
                ),
                ("function run(){} function* caller(run){run();}", false),
                (
                    "function caller(replacement){function run(){} var run=replacement; run();}",
                    false,
                ),
                ("function caller(){function run(){} var run; run();}", true),
                (
                    "function caller(replacement){var run=replacement; {function run(){} run();}}",
                    true,
                ),
                (
                    "function caller(replacement){function run(){} run=replacement; run();}",
                    false,
                ),
                (
                    "function caller(values){function run(){} for(var run of values){} run();}",
                    false,
                ),
                (
                    "function run(){} class Example{static{var run=null;}} function caller(){run();}",
                    true,
                ),
                (
                    "function caller(replacement){function run(){} try{}catch(run){var run=replacement;} run();}",
                    true,
                ),
                (
                    "function caller(values){function run(){} try{}catch(run){for(var run of values){}} run();}",
                    true,
                ),
                (
                    "function run(){} function caller(replacement){try{}catch(run){var run=replacement;} run();}",
                    false,
                ),
                (
                    "function caller(replacement){function run(){} try{}catch(run){var [run]=replacement;} run();}",
                    true,
                ),
                (
                    "function caller(replacement){function run(){} try{}catch(run){var {value:run}=replacement;} run();}",
                    true,
                ),
                (
                    "function caller(values){function run(){} try{}catch(run){for(var {value:run} of values){}} run();}",
                    true,
                ),
            ] {
                let content = if host {
                    format!("<script>{script}</script>")
                } else {
                    script.to_string()
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, graph, facts) = extracted.into_parts();
                let calls = facts
                    .calls
                    .iter()
                    .filter(|call| graph.relations[call.relation].target_name == "run")
                    .collect::<Vec<_>>();
                assert!(!calls.is_empty(), "{language}: {script}: missing raw call");
                for call in calls {
                    let target = match call.binding {
                        SourceCallBinding::LocalDeclaration(symbol) => Some(&graph.symbols[symbol]),
                        _ => None,
                    };
                    assert_eq!(
                        target.is_some(),
                        resolved,
                        "{language}: {script}: {:?}",
                        facts.calls
                    );
                    if let Some(target) = target {
                        assert_eq!(target.name, "run", "{language}: {script}");
                    }
                }
            }
        }
    }

    #[test]
    fn typescript_namespace_vars_keep_their_own_environment() {
        for (language, path) in [("typescript", "main.ts"), ("tsx", "main.tsx")] {
            for (content, resolved) in [
                (
                    "function run(){} namespace N{var run=null;} function caller(){run();}",
                    true,
                ),
                (
                    "namespace N{function run(){} var run=null; function caller(){run();}}",
                    false,
                ),
                (
                    "namespace N{function run(){} function caller(value=run()){var run=null;}}",
                    true,
                ),
            ] {
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, graph, facts) = extracted.into_parts();
                assert_eq!(facts.calls.len(), 1, "{language}: {content}");
                match facts.calls[0].binding {
                    SourceCallBinding::LocalDeclaration(symbol) => {
                        assert!(resolved, "{language}: {content}");
                        assert_eq!(graph.symbols[symbol].name, "run");
                    }
                    _ => assert!(!resolved, "{language}: {content}: {:?}", facts.calls),
                }
            }
        }
    }

    #[test]
    fn ecmascript_parameter_initializers_exclude_body_bindings() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for (script, resolved) in [
                (
                    "function run(){} function caller(value=run()){var run=null;}",
                    true,
                ),
                (
                    "function run(){} function caller(value=run()){function run(){}}",
                    true,
                ),
                (
                    "function run(){} class Example{caller(value=run()){var run=null;}}",
                    true,
                ),
                (
                    "function run(){} const caller=(value=run())=>{var run=null;};",
                    true,
                ),
                ("function run(){} function caller(run=run()){}", false),
                (
                    "function run(){} function caller(value=run(),run=null){}",
                    false,
                ),
                (
                    "function run(){} function caller(value=run()){let run=null;}",
                    true,
                ),
                (
                    "function run(){} function* caller(value=run()){var run=null;}",
                    true,
                ),
            ] {
                let content = if host {
                    format!("<script>{script}</script>")
                } else {
                    script.to_string()
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, graph, facts) = extracted.into_parts();
                let calls = facts
                    .calls
                    .iter()
                    .filter(|call| graph.relations[call.relation].target_name == "run")
                    .collect::<Vec<_>>();
                assert!(!calls.is_empty(), "{language}: {script}: missing raw call");
                for call in calls {
                    let target = match call.binding {
                        SourceCallBinding::LocalDeclaration(symbol) => Some(&graph.symbols[symbol]),
                        _ => None,
                    };
                    assert_eq!(
                        target.is_some(),
                        resolved,
                        "{language}: {script}: {:?}",
                        facts.calls
                    );
                    if let Some(target) = target {
                        assert_eq!(target.name, "run", "{language}: {script}");
                        assert!(target.parent.is_none(), "{language}: {script}: {target:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn ecmascript_bare_var_reserves_names_without_replacing_values() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for (script, resolved) in [
                ("function run() {} var run; run();", true),
                ("var run; function run() {} run(); var run;", true),
                ("var run = () => {}; var run; run();", true),
                ("var run = function() {}; {var run;} run();", true),
                ("function run() {} for (var run; false;) {} run();", true),
                ("function run() {} function caller(){run();} var run;", true),
                (
                    "export var run = () => {}; var run; function caller(){run();}",
                    true,
                ),
                (
                    "function run() {} function caller(){run(); var run;}",
                    false,
                ),
                (
                    "function run() {} function caller(run){var run; run();}",
                    false,
                ),
                ("function run() {} {let run; run();}", false),
                ("var run = () => {}; var run = unknown; run();", false),
                (
                    "var run = () => {}; for (var run of values) {} run();",
                    false,
                ),
                (
                    "var run = () => {}; for (var run in object) {} run();",
                    false,
                ),
            ] {
                let content = if host {
                    if script.starts_with("export ") {
                        ecmascript_module_fixture(language, script)
                    } else {
                        format!("<script>{script}</script>")
                    }
                } else {
                    script.to_string()
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else {
                    return;
                };
                let (_, _, facts) = extracted.into_parts();
                assert_eq!(facts.calls.len(), 1, "{language}: {script}");
                assert_eq!(
                    matches!(
                        facts.calls[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    resolved,
                    "{language}: {script}: {:?}",
                    facts.calls
                );
                if script.starts_with("export var") {
                    assert!(
                        facts.exports.iter().any(|slot| slot.name == "run"
                            && matches!(
                                slot.binding,
                                crate::SourceExportBinding::LocalDeclaration(_)
                            )),
                        "{language}: {script}: {:?}",
                        facts.exports
                    );
                }
            }
        }
    }

    #[test]
    fn malformed_var_initializers_cannot_preserve_callable_authority() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for (script, resolved) in [
                (
                    "function run(){} var run = ; function caller(){run();}",
                    false,
                ),
                (
                    "function run(){} var run = , other; function caller(){run();}",
                    false,
                ),
                (
                    "function run(){} var run /* trivia */ = , other; function caller(){run();}",
                    false,
                ),
                (
                    "export var run=()=>{}; var run = ; function caller(){run();}",
                    false,
                ),
                (
                    "function run(){} var run, other = ; function caller(){run();}",
                    true,
                ),
                (
                    "function run(){} var other = , run; function caller(){run();}",
                    true,
                ),
            ] {
                let content = if host {
                    format!("<script>{script}</script>")
                } else {
                    script.to_string()
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else { return };
                let (_, graph, facts) = extracted.into_parts();
                assert!(
                    !facts.calls.is_empty(),
                    "{language}: {script}: lost recovered call"
                );
                for call in &facts.calls {
                    let target = match call.binding {
                        SourceCallBinding::LocalDeclaration(symbol) => {
                            Some(graph.symbols[symbol].name.as_str())
                        }
                        _ => None,
                    };
                    assert_eq!(
                        target,
                        resolved.then_some("run"),
                        "{language}: {script}: {:?}",
                        facts.calls
                    );
                }
                if script.starts_with("export var") {
                    let slot = facts.exports.iter().find(|slot| slot.name == "run");
                    assert!(
                        slot.is_some_and(|slot| !matches!(
                            slot.binding,
                            crate::SourceExportBinding::LocalDeclaration(_)
                        )),
                        "{language}: {script}: {:?}",
                        facts.exports
                    );
                }
            }
        }
    }

    #[test]
    fn ecmascript_writes_invalidate_actual_bindings_and_live_exports_only() {
        for (language, path, host) in [
            ("javascript", "main.js", false),
            ("typescript", "main.ts", false),
            ("tsx", "main.tsx", false),
            ("vue", "main.vue", true),
            ("svelte", "main.svelte", true),
            ("html", "main.html", true),
        ] {
            for (suffix, resolved) in [
                ("run = unknown;", false),
                ("run += unknown;", false),
                ("run++;", false),
                ("({run} = unknown);", false),
                ("[run] = unknown;", false),
                ("for (run of values) {}", false),
                ("for (run in object) {}", false),
                ("for ({run} of values) {}", false),
                ("for (let run of values) {}", true),
                ("function mutate(){run = unknown;}", false),
                ("function mutate(){let run; run = unknown;}", true),
                ("const object = {}; object.run = unknown;", true),
            ] {
                let script = format!(
                    "export let run = () => {{}}; function caller(){{run();}} export {{run as launch}}; {suffix}"
                );
                let content = if host {
                    ecmascript_module_fixture(language, &script)
                } else {
                    script
                };
                let extracted = extract_symbol_graph_with_semantics_controlled(
                    path,
                    Some(language),
                    &content,
                    &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
                );
                assert!(extracted.is_ok(), "{language}: {extracted:?}");
                let Ok(extracted) = extracted else {
                    return;
                };
                let (_, _, facts) = extracted.into_parts();
                assert_eq!(facts.calls.len(), 1, "{language}: {suffix}");
                assert_eq!(
                    matches!(
                        facts.calls[0].binding,
                        SourceCallBinding::LocalDeclaration(_)
                    ),
                    resolved,
                    "{language}: {suffix}: {:?}",
                    facts.calls
                );
                for name in ["run", "launch"] {
                    let slot = facts.exports.iter().find(|slot| slot.name == name);
                    assert!(slot.is_some(), "{language}: {suffix}: {:?}", facts.exports);
                    assert_eq!(
                        slot.is_some_and(|slot| matches!(
                            slot.binding,
                            crate::SourceExportBinding::LocalDeclaration(_)
                        )),
                        resolved,
                        "{language}: {suffix}: {:?}",
                        facts.exports
                    );
                }
            }
        }
        for content in [
            "export function run(){} run = unknown;",
            "export default function run(){} function mutate(){run = unknown;}",
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.js",
                Some("javascript"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert!(
                facts.exports.iter().all(|slot| !matches!(
                    slot.binding,
                    crate::SourceExportBinding::LocalDeclaration(_)
                )),
                "{content}: {:?}",
                facts.exports
            );
        }
    }

    #[test]
    fn loop_writes_follow_iterable_evaluation_and_declaration_ownership() {
        for (content, expected) in [
            (
                "let run = () => []; for (run of run()) {run();}",
                vec![true, false],
            ),
            (
                "let run = () => ({}); for (run in run()) {run();}",
                vec![true, false],
            ),
            (
                "let run = () => []; for ({run} of run()) {run();}",
                vec![true, false],
            ),
            (
                "let run = () => []; for (let run of run()) {run();}",
                vec![false, false],
            ),
            (
                "var run = () => []; for (var run of run()) {run();}",
                vec![true, false],
            ),
            (
                "let run = () => {}; for (let run of values) {run();} run();",
                vec![false, true],
            ),
            (
                "let run = () => {}; function mutate(){for (run of values) {}} function caller(){run();}",
                vec![false],
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                "main.js",
                Some("javascript"),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            let resolved: Vec<_> = facts
                .calls
                .iter()
                .map(|call| matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
                .collect();
            assert_eq!(resolved, expected, "{content}: {:?}", facts.calls);
        }
    }

    #[test]
    fn ordinary_fact_free_syntax_retains_exact_callers_under_lexical_limits() {
        let content = format!(
            "function run() {{}} function caller() {{run(); {} }}",
            "{} ".repeat(1_024)
        );
        let extracted = extract_symbol_graph_with_semantics_controlled(
            "main.js",
            Some("javascript"),
            &content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(extracted.is_ok(), "{extracted:?}");
        let Ok(extracted) = extracted else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert!(!facts.bindings_incomplete);
        assert_eq!(facts.calls.len(), 1);
        assert!(matches!(
            facts.calls[0].binding,
            SourceCallBinding::LocalDeclaration(_)
        ));
    }

    #[test]
    fn native_binding_limits_cover_scope_adapters_values_and_deep_syntax() {
        for (path, language, content) in [
            (
                "main.rs",
                "rust",
                format!(
                    "fn run() {{}} fn caller<{}>() {{run();}}",
                    (0..20_000)
                        .map(|index| format!("T{index}"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            (
                "main.rs",
                "rust",
                format!(
                    "fn run() {{}} fn caller() {{ run(); {} }}",
                    "{} ".repeat(20_000)
                ),
            ),
            (
                "main.zig",
                "zig",
                format!(
                    "fn run() void {{}} fn caller() void {{ run(); {} }}",
                    "{} ".repeat(20_000)
                ),
            ),
            (
                "main.py",
                "python",
                format!(
                    "def run(): pass\ndef caller(): run()\n{}",
                    "[x for x in ()]\n".repeat(20_000)
                ),
            ),
            (
                "main.rs",
                "rust",
                format!(
                    "fn run() {{}} fn caller() {{ run(); {} }}",
                    "let run = unknown;".repeat(20_000)
                ),
            ),
            (
                "main.js",
                "javascript",
                format!(
                    "function run() {{}} function caller() {{run(); {}0{} }}",
                    "{".repeat(512),
                    "}".repeat(512)
                ),
            ),
        ] {
            let extracted = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                &content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(extracted.is_ok(), "{language}: {extracted:?}");
            let Ok(extracted) = extracted else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            assert!(
                facts.bindings_incomplete,
                "{language}: binding state escaped its bound"
            );
            assert!(graph.symbols.iter().any(|symbol| symbol.name == "run"));
            assert!(
                !facts.calls.is_empty(),
                "{language}: raw prefix call was lost"
            );
            assert!(
                facts
                    .calls
                    .iter()
                    .all(|call| call.binding == SourceCallBinding::Unresolved)
            );
        }
    }

    #[test]
    fn parallel_fact_free_syntax_exhausts_binding_capacity_without_losing_raw_evidence() {
        std::thread::scope(|threads| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    threads.spawn(|| {
                        let content = format!(
                            "function run() {{}} function caller() {{run();}} {}",
                            "{} ".repeat(50_000)
                        );
                        let result = extract_symbol_graph_with_semantics_controlled(
                            "main.js",
                            Some("javascript"),
                            &content,
                            &IndexWorkControl::new(
                                projectatlas_core::IndexCancellation::new(),
                                None,
                            ),
                        );
                        assert!(result.is_ok(), "{result:?}");
                        let Ok(extracted) = result else {
                            return;
                        };
                        let (_, graph, facts) = extracted.into_parts();
                        assert!(
                            facts.bindings_incomplete,
                            "fact-free scopes escaped the binding budget"
                        );
                        assert_eq!(facts.calls.len(), 1);
                        assert!(graph.symbols.iter().any(|symbol| symbol.name == "run"));
                        assert_eq!(facts.calls[0].binding, SourceCallBinding::Unresolved);
                    })
                })
                .collect();
            for handle in handles {
                assert!(handle.join().is_ok());
            }
        });
    }

    #[test]
    fn future_shadow_timelines_preserve_earlier_outer_calls_at_dense_scale() {
        let content = format!(
            "fn run() {{}}\nfn caller() {{ {} {} }}",
            "run();".repeat(2_000),
            "let run = unknown;".repeat(15_000)
        );
        let result = extract_symbol_graph_with_semantics_controlled(
            "main.rs",
            Some("rust"),
            &content,
            &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
        );
        assert!(result.is_ok(), "{result:?}");
        let Ok(extracted) = result else {
            return;
        };
        let (_, _, facts) = extracted.into_parts();
        assert_eq!(facts.calls.len(), 2_000);
        assert!(
            facts
                .calls
                .iter()
                .all(|fact| fact.binding == SourceCallBinding::LocalDeclaration(0))
        );
    }

    #[test]
    fn recursive_body_names_still_obey_function_wide_local_shadowing() {
        for (path, language, content) in [
            ("main.py", "python", "def run():\n run()\n run = unknown\n"),
            (
                "main.js",
                "javascript",
                "const run = () => { run(); var run = unknown; };",
            ),
        ] {
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, _, facts) = extracted.into_parts();
            assert_eq!(facts.calls.len(), 1);
            assert_eq!(
                facts.calls[0].binding,
                SourceCallBinding::Shadowed,
                "{language}"
            );
        }
    }

    #[test]
    fn registered_native_aliases_share_their_grammar_owned_local_binding_rules() {
        for &language in projectatlas_core::language::builtin_tree_sitter_language_ids() {
            let Some(grammar) = projectatlas_core::language::tree_sitter_grammar(language) else {
                continue;
            };
            let (path, content) = match grammar {
                projectatlas_core::language::TreeSitterGrammar::Rust => (
                    "build.rs",
                    "fn run() {} fn caller(other: fn()) { let run = other; run(); } fn clean() { run(); }",
                ),
                projectatlas_core::language::TreeSitterGrammar::C => (
                    "main.h",
                    "void run(void) {} void caller(void (*other)(void)) { void (*run)(void) = other; run(); } void clean(void) { run(); }",
                ),
                projectatlas_core::language::TreeSitterGrammar::Cpp => (
                    "main.hpp",
                    "void run() {} void caller(void (*other)()) { auto run = other; run(); } void clean() { run(); }",
                ),
                projectatlas_core::language::TreeSitterGrammar::ObjectiveC => (
                    "main.m",
                    "void run(void) {} void caller(void (*other)(void)) { void (*run)(void) = other; run(); } void clean(void) { run(); }",
                ),
                projectatlas_core::language::TreeSitterGrammar::Zig => (
                    "main.zig",
                    "fn run() void {} fn caller(other: *const fn () void) void { const run = other; run(); } fn clean() void { run(); }",
                ),
                _ => continue,
            };
            let result = extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None),
            );
            assert!(result.is_ok(), "{language}: {result:?}");
            let Ok(extracted) = result else {
                return;
            };
            let (_, graph, facts) = extracted.into_parts();
            let calls: Vec<_> = facts
                .calls
                .iter()
                .map(|fact| (&graph.relations[fact.relation].source_name, &fact.binding))
                .collect();
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "caller"
                        && matches!(binding, SourceCallBinding::Shadowed)),
                "{language}: {calls:?}"
            );
            assert!(
                calls
                    .iter()
                    .any(|(owner, binding)| owner.as_str() == "clean"
                        && matches!(binding, SourceCallBinding::LocalDeclaration(_))),
                "{language}: {calls:?}"
            );
        }
    }
}
