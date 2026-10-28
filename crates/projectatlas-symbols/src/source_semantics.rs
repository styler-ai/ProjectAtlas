//! Parser-owned binding evidence kept separate from the compatible public graph.

use crate::ImportReference;
use projectatlas_core::graph::SourceSpan;
use projectatlas_core::symbols::{ParserKind, RelationKind, SymbolGraph};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Invalid ordinal correspondence between captured evidence and an admitted graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SourceSemanticRemapError {
    /// Evidence or mapping dimensions belong to a different source graph.
    #[error("source semantic evidence does not match the graph dimensions")]
    GraphMismatch,
    /// A mapping repeats an output ordinal or addresses a missing output fact.
    #[error("source semantic ordinal mapping is not one-to-one and in bounds")]
    InvalidOrdinalMap,
    /// A surviving occurrence points to a different relation family.
    #[error("source semantic occurrence does not match its relation family")]
    RelationMismatch,
}

/// Exact UTF-8 byte extent in the captured source file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceByteSpan {
    /// Inclusive source byte offset.
    pub start: usize,
    /// Exclusive source byte offset.
    pub end: usize,
}

/// Static binding selected at one call site by its grammar owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceCallBinding {
    /// Exactly one declaration in this parser graph is lexically visible.
    LocalDeclaration(usize),
    /// Several visible declarations remain possible, shared without per-call copies.
    LocalDeclarations(Arc<[usize]>),
    /// An AST-qualified Rust anchor or unshadowed toolchain module owns this path.
    Qualified(ImportReference),
    /// An AST-proven Rust module or explicit crate-relative path owns this call.
    RustPath(crate::SourceRustCall),
    /// A particular visible import binding owns this call.
    Imported {
        /// Ordinal of the import evidence, not the raw graph relation.
        import: usize,
        /// Ordinal of the reference within that import evidence.
        reference: usize,
        /// Explicit member after an imported namespace or type, if any.
        member: Option<String>,
        /// Source-proven toolchain identity; never inferred from the alias spelling.
        rust_toolchain: Option<Box<ImportReference>>,
    },
    /// A parameter, local value or not-yet-initialized binding hides a declaration.
    Shadowed,
    /// Source does not establish a static declaration or visible import target.
    Unresolved,
    /// The existing PHP namespace owner supplies its separately validated local rules.
    PhpNamespace,
}

/// One call occurrence and its parser-owned lexical decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCallFact {
    /// Exact original source lines and Unicode-scalar columns, when captured.
    pub source_span: Option<SourceSpan>,
    /// Ordinal in the accompanying graph's relation vector.
    pub relation: usize,
    /// Exact original source occurrence, independent of display context.
    pub span: SourceByteSpan,
    /// Exact enclosing declaration ordinal, absent for module or unknown owners.
    pub owner: Option<usize>,
    /// Binding proof used before name-based candidate lookup.
    pub binding: SourceCallBinding,
}

/// One import declaration's independently visible references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceImportFact {
    /// Exact original source lines and Unicode-scalar columns, when captured.
    pub source_span: Option<SourceSpan>,
    /// Ordinal in the accompanying graph's relation vector.
    pub relation: usize,
    /// Original import extent, before canonical display normalization.
    pub span: SourceByteSpan,
    /// Independently static module target, even when no local binding is valid.
    pub module: Option<String>,
    /// Provider-owned canonical bindings; no lexical scope is inferred from their names.
    pub references: Vec<ImportReference>,
    /// Rust's complete use-tree leaf before module versus value classification.
    pub rust: Option<Arc<crate::SourceRustUse>>,
    /// Source-proven toolchain import, absent for repository or unproved roots.
    pub rust_toolchain: Option<Box<ImportReference>>,
}

/// Static declaration or forwarding source of one explicit module export slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceExportBinding {
    /// Export syntax exists but does not prove a declaration or forwarding target.
    Unresolved,
    /// An explicit declaration in the exporting source owns this slot.
    LocalDeclaration(usize),
    /// Indistinguishable declaration metadata cannot establish one export target.
    AmbiguousDeclarations(Arc<[usize]>),
    /// Declared export owners exist, but this binding does not prove a callable target.
    ///
    /// Retain their ordinals to prevent raw exported metadata from re-admitting
    /// an unproved variable or destructuring name through fallback.
    UnresolvedDeclarations(Arc<[usize]>),
    /// An explicit static module reference forwards this exported name.
    ReExport(ImportReference),
    /// An explicit Rust pub-use retains its complete path and module owner.
    RustReExport(Arc<crate::SourceRustUse>),
}

/// Language namespace occupied by one explicitly exported name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceExportNamespace {
    /// A runtime value or callable binding.
    Value,
    /// Type-only syntax cannot conflict with or supply runtime callable evidence.
    Type,
}

/// One parser-owned exported name without a synthesized declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceExportFact {
    /// Namespace governing competing slots and callable-target eligibility.
    pub namespace: SourceExportNamespace,
    /// Cooked exported slot, including the explicit default slot.
    pub name: String,
    /// Exact original source extent of the export syntax.
    pub span: SourceByteSpan,
    /// One-based start line for identity-admission diagnostics.
    pub line: usize,
    /// Static declaration or module evidence owning the export.
    pub binding: SourceExportBinding,
}

/// Distinguish safe declaration correspondence from a coalesced candidate row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceSymbolRemap {
    /// One original declaration uniquely owns the output ordinal.
    Retained(usize),
    /// An output row exists, but cannot select among original declarations.
    Ambiguous(usize),
    /// No surviving output declaration can retain this source role.
    Omitted,
}

impl SourceSymbolRemap {
    /// Return only a declaration whose exact correspondence is established.
    pub(crate) const fn retained(self) -> Option<usize> {
        match self {
            Self::Retained(index) => Some(index),
            Self::Ambiguous(_) | Self::Omitted => None,
        }
    }

    /// Retain a candidate for ambiguity markers without authorizing a target.
    const fn candidate(self) -> Option<usize> {
        match self {
            Self::Retained(index) | Self::Ambiguous(index) => Some(index),
            Self::Omitted => None,
        }
    }
}

/// Transient binding proof paired with one exact captured source graph.
///
/// This is not a serialized public graph or a persisted second authority. A
/// consumer must retain the source identity and remap ordinals when filtering or
/// merging the accompanying graph. Missing facts never authorize an exact call.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceSemanticFacts {
    /// Owning repository-relative path.
    pub(crate) path: String,
    /// Hash of the original captured bytes, including any masked purpose header.
    pub(crate) content_hash: String,
    /// Symbol count at the last ordinal-association boundary.
    pub(crate) symbol_count: usize,
    /// Relation count at the last ordinal-association boundary.
    pub(crate) relation_count: usize,
    /// Digest of the ordered fact identities to detect same-size ordinal drift.
    pub(crate) graph_digest: [u8; 32],
    /// Source-order import evidence.
    pub(crate) imports: Vec<SourceImportFact>,
    /// Omitted declaration/import evidence may compete with retained lexical bindings.
    pub(crate) bindings_incomplete: bool,
    /// Call occurrences in raw relation order.
    pub(crate) calls: Vec<SourceCallFact>,
    /// Explicit module export slots in captured source order.
    pub(crate) exports: Vec<SourceExportFact>,
    /// Export cardinality exceeded its bound; no export target may be narrowed.
    pub(crate) exports_incomplete: bool,
    /// Actual Rust module declarations and module-owned value slots.
    pub(crate) rust: Option<crate::SourceRustFacts>,
    /// Target settings retained from the existing Cargo TOML parse.
    pub(crate) cargo: Option<crate::SourceCargoTargets>,
}

impl SourceSemanticFacts {
    /// Borrow the exact source path that owns these ordinals.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Borrow the hash that must match the captured inventory before reuse.
    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    /// Borrow source-order import bindings.
    #[must_use]
    pub fn imports(&self) -> &[SourceImportFact] {
        &self.imports
    }

    /// Borrow call-site decisions and exact source extents.
    #[must_use]
    pub fn calls(&self) -> &[SourceCallFact] {
        &self.calls
    }

    /// Borrow explicit module exports and their source-owned targets.
    #[must_use]
    pub fn exports(&self) -> &[SourceExportFact] {
        &self.exports
    }

    /// Borrow Rust module ownership captured by the native source parser.
    #[must_use]
    pub const fn rust(&self) -> Option<&crate::SourceRustFacts> {
        self.rust.as_ref()
    }

    /// Borrow Cargo target settings from this exact manifest's existing parse.
    #[must_use]
    pub const fn cargo(&self) -> Option<&crate::SourceCargoTargets> {
        self.cargo.as_ref()
    }

    /// Return whether all competing source export slots fit within the evidence bound.
    #[must_use]
    pub const fn exports_complete(&self) -> bool {
        !self.exports_incomplete
    }

    /// Estimate retained bytes from actual vector/string capacities and shared sets.
    ///
    /// Immutable overload arrays are charged once per shared allocation, including
    /// their reference-count header. This is staging accounting, not measured RSS.
    #[must_use]
    pub fn retained_bytes(&self) -> u64 {
        let mut rust_uses = HashSet::new();
        let mut rust_modules = HashSet::new();
        let mut bytes = (std::mem::size_of::<Self>() as u64)
            .saturating_add(self.path.capacity() as u64)
            .saturating_add(self.content_hash.capacity() as u64)
            .saturating_add(
                (self.imports.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<SourceImportFact>() as u64),
            )
            .saturating_add(
                (self.calls.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<SourceCallFact>() as u64),
            );
        for import in &self.imports {
            if let Some(reference) = &import.rust_toolchain {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<ImportReference>() as u64)
                    .saturating_add(reference.retained_heap_bytes());
            }
            if let Some(binding) = &import.rust {
                bytes = bytes.saturating_add(crate::rust_binding_source::use_retained_bytes(
                    binding,
                    &mut rust_uses,
                    &mut rust_modules,
                ));
            }
            bytes = bytes
                .saturating_add(import.module.as_ref().map_or(0, String::capacity) as u64)
                .saturating_add(
                    (import.references.capacity() as u64)
                        .saturating_mul(std::mem::size_of::<ImportReference>() as u64),
                );
            for reference in &import.references {
                bytes = bytes.saturating_add(reference.retained_heap_bytes());
            }
        }
        let mut shared = HashSet::new();
        bytes = bytes
            .saturating_add(call_binding_heap_bytes_with_shared(
                &self.calls,
                &mut shared,
                &mut rust_modules,
            ))
            .saturating_add(
                (self.exports.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<SourceExportFact>() as u64),
            );
        for export in &self.exports {
            bytes = bytes
                .saturating_add(export.name.capacity() as u64)
                .saturating_add(match &export.binding {
                    SourceExportBinding::ReExport(reference) => reference.retained_heap_bytes(),
                    SourceExportBinding::RustReExport(binding) => {
                        crate::rust_binding_source::use_retained_bytes(
                            binding,
                            &mut rust_uses,
                            &mut rust_modules,
                        )
                    }
                    SourceExportBinding::AmbiguousDeclarations(symbols)
                    | SourceExportBinding::UnresolvedDeclarations(symbols)
                        if shared.insert(Arc::as_ptr(symbols)) =>
                    {
                        ((symbols.len() as u64).saturating_add(2))
                            .saturating_mul(std::mem::size_of::<usize>() as u64)
                    }
                    SourceExportBinding::LocalDeclaration(_)
                    | SourceExportBinding::AmbiguousDeclarations(_)
                    | SourceExportBinding::UnresolvedDeclarations(_)
                    | SourceExportBinding::Unresolved => 0,
                });
        }
        if let Some(rust) = &self.rust {
            bytes = bytes.saturating_add(rust.retained_bytes(&mut rust_uses, &mut rust_modules));
        }
        if let Some(cargo) = &self.cargo {
            bytes = bytes.saturating_add(cargo.retained_bytes());
        }
        bytes
    }

    /// Check the owning path and vector shape before an ordinal-based operation.
    #[must_use]
    pub fn matches_graph(&self, graph: &SymbolGraph) -> bool {
        let mut checked_declarations = HashSet::new();
        let mut checked_rust_imports = HashSet::new();
        self.path == graph.path
            && self.symbol_count == graph.symbols.len()
            && self.relation_count == graph.relations.len()
            && self.graph_digest == graph_ordinal_digest(graph)
            && self.imports.iter().all(|fact| {
                graph
                    .relations
                    .get(fact.relation)
                    .is_some_and(|relation| relation.kind == RelationKind::Imports)
            })
            && self.calls.iter().all(|fact| {
                graph
                    .relations
                    .get(fact.relation)
                    .is_some_and(|relation| relation.kind == RelationKind::Calls)
                    && self.binding_is_in_bounds(&fact.binding, &mut checked_declarations)
                    && fact.owner.is_none_or(|owner| owner < self.symbol_count)
            })
            && self.exports.iter().all(|fact| match &fact.binding {
                SourceExportBinding::LocalDeclaration(symbol) => *symbol < self.symbol_count,
                SourceExportBinding::AmbiguousDeclarations(symbols)
                | SourceExportBinding::UnresolvedDeclarations(symbols) => {
                    !symbols.is_empty()
                        && (!checked_declarations.insert(Arc::as_ptr(symbols))
                            || symbols.iter().all(|symbol| *symbol < self.symbol_count))
                }
                SourceExportBinding::ReExport(_)
                | SourceExportBinding::RustReExport(_)
                | SourceExportBinding::Unresolved => true,
            })
            && self.rust.as_ref().is_none_or(|rust| {
                rust.declarations
                    .iter()
                    .all(|declaration| match declaration.binding {
                        crate::SourceRustBinding::Declaration(symbol)
                        | crate::SourceRustBinding::TypeDeclaration(symbol) => {
                            symbol < self.symbol_count
                        }
                        crate::SourceRustBinding::Use(_)
                        | crate::SourceRustBinding::Unresolved
                        | crate::SourceRustBinding::UnresolvedType
                        | crate::SourceRustBinding::UnresolvedImport => true,
                    })
                    && rust.candidate_calls.iter().all(|call| {
                        graph
                            .relations
                            .get(call.relation)
                            .is_some_and(|relation| relation.kind == RelationKind::Calls)
                            && !call.bindings.is_empty()
                            && (!checked_declarations.insert(Arc::as_ptr(&call.declarations))
                                || call
                                    .declarations
                                    .iter()
                                    .all(|symbol| *symbol < self.symbol_count))
                            && (!checked_rust_imports.insert(Arc::as_ptr(&call.bindings))
                                || call.bindings.iter().all(|(import, reference)| {
                                    self.imports
                                        .get(*import)
                                        .is_some_and(|import| *reference < import.references.len())
                                }))
                    })
                    && rust.associated.iter().all(|method| {
                        method
                            .declaration
                            .is_none_or(|ordinal| ordinal < self.symbol_count)
                    })
            })
    }

    /// Move source evidence through explicit filtering or reordering maps.
    ///
    /// Each map addresses an original ordinal and optionally supplies its output
    /// ordinal. Dropping any required binding proof makes a surviving call
    /// unresolved; filtering cannot turn an ambiguous binding into an exact one.
    /// New graph facts without captured evidence gain no binding proof.
    ///
    /// # Errors
    ///
    /// Returns an error for another source path, mismatched mapping dimensions,
    /// duplicate/out-of-bounds outputs or a changed relation family. The original
    /// evidence remains unchanged on failure.
    pub fn remap(
        &self,
        original: &SymbolGraph,
        graph: &SymbolGraph,
        symbols: &[Option<usize>],
        relations: &[Option<usize>],
    ) -> Result<Self, SourceSemanticRemapError> {
        if !self.matches_graph(original)
            || self.path != graph.path
            || self.symbol_count != symbols.len()
            || self.relation_count != relations.len()
        {
            return Err(SourceSemanticRemapError::GraphMismatch);
        }
        validate_ordinal_map(symbols, graph.symbols.len())?;
        validate_ordinal_map(relations, graph.relations.len())?;
        let (imports, calls) = self.remap_associations(symbols, relations);
        let exports = self.remap_exports(symbols, None);
        let result = Self {
            path: self.path.clone(),
            content_hash: self.content_hash.clone(),
            symbol_count: graph.symbols.len(),
            relation_count: graph.relations.len(),
            graph_digest: graph_ordinal_digest(graph),
            imports,
            bindings_incomplete: self.bindings_incomplete,
            calls,
            exports,
            exports_incomplete: self.exports_incomplete,
            rust: self.remap_rust(symbols, relations),
            cargo: self.cargo.clone(),
        };
        if !result.matches_graph(graph) {
            return Err(SourceSemanticRemapError::RelationMismatch);
        }
        Ok(result)
    }

    /// Retain graph facts and their source proof together without cloning the graph.
    ///
    /// True mask entries retain the corresponding original ordinal. The source
    /// hash is preserved; dropped declaration/import proof cannot authorize a call.
    ///
    /// # Errors
    ///
    /// Returns a source-identity or mask-dimension mismatch before either graph or
    /// evidence changes. Once validated, filtering and remapping are infallible.
    pub fn retain_graph_facts(
        &mut self,
        graph: &mut SymbolGraph,
        symbols: &[bool],
        relations: &[bool],
    ) -> Result<(), SourceSemanticRemapError> {
        if !self.matches_graph(graph)
            || symbols.len() != graph.symbols.len()
            || relations.len() != graph.relations.len()
        {
            return Err(SourceSemanticRemapError::GraphMismatch);
        }
        let symbol_ordinals = retained_ordinals(symbols);
        let relation_ordinals = retained_ordinals(relations);
        let (imports, calls) = self.remap_associations(&symbol_ordinals, &relation_ordinals);
        let exports = self.remap_exports(&symbol_ordinals, None);
        let rust = self.remap_rust(&symbol_ordinals, &relation_ordinals);
        let mut symbol = 0;
        graph.symbols.retain(|_| {
            let retained = symbols[symbol];
            symbol += 1;
            retained
        });
        let mut relation = 0;
        graph.relations.retain(|_| {
            let retained = relations[relation];
            relation += 1;
            retained
        });
        self.symbol_count = graph.symbols.len();
        self.relation_count = graph.relations.len();
        self.graph_digest = graph_ordinal_digest(graph);
        self.imports = imports;
        self.calls = calls;
        self.exports = exports;
        self.rust = rust;
        Ok(())
    }

    /// Append parser-owned evidence through an append-only host graph merge.
    ///
    /// The parser owner seals `source` before moving its graph rows and supplies
    /// one-to-one surviving ordinals. Existing host ordinals remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns correspondence errors before adding any new source facts.
    pub(crate) fn append_mapped(
        &mut self,
        source: Self,
        graph: &SymbolGraph,
        symbols: &[SourceSymbolRemap],
        relations: &[Option<usize>],
    ) -> Result<(), SourceSemanticRemapError> {
        if source.rust.is_some()
            || source.cargo.is_some()
            || source.path != graph.path
            || (!self.path.is_empty() && self.path != graph.path)
            || source.symbol_count != symbols.len()
            || source.relation_count != relations.len()
            || self.symbol_count > graph.symbols.len()
            || self.relation_count > graph.relations.len()
        {
            return Err(SourceSemanticRemapError::GraphMismatch);
        }
        if symbols.iter().any(|symbol| {
            symbol
                .candidate()
                .is_some_and(|index| index >= graph.symbols.len())
        }) {
            return Err(SourceSemanticRemapError::InvalidOrdinalMap);
        }
        let retained_symbols = symbols
            .iter()
            .map(|symbol| symbol.retained())
            .collect::<Vec<_>>();
        validate_ordinal_map(&retained_symbols, graph.symbols.len())?;
        validate_ordinal_map(relations, graph.relations.len())?;
        let (imports, calls) = source.remap_associations(&retained_symbols, relations);
        let exports = source.remap_exports(&retained_symbols, Some(symbols));
        let mut mapped = Self {
            path: source.path,
            content_hash: String::new(),
            symbol_count: graph.symbols.len(),
            relation_count: graph.relations.len(),
            graph_digest: graph_ordinal_digest(graph),
            imports,
            bindings_incomplete: source.bindings_incomplete,
            calls,
            exports,
            exports_incomplete: source.exports_incomplete,
            rust: None,
            cargo: None,
        };
        if !mapped.matches_graph(graph) {
            return Err(SourceSemanticRemapError::RelationMismatch);
        }
        for call in &mut mapped.calls {
            if let SourceCallBinding::Imported { import, .. } = &mut call.binding {
                *import = import
                    .checked_add(self.imports.len())
                    .ok_or(SourceSemanticRemapError::InvalidOrdinalMap)?;
            }
        }
        self.bindings_incomplete |= mapped.bindings_incomplete;
        self.imports.extend(mapped.imports);
        self.calls.extend(mapped.calls);
        self.exports.extend(mapped.exports);
        self.exports_incomplete |= mapped.exports_incomplete;
        self.path = mapped.path;
        self.symbol_count = mapped.symbol_count;
        self.relation_count = mapped.relation_count;
        self.graph_digest = mapped.graph_digest;
        Ok(())
    }

    /// Remap validated ordinals once, preserving shared overload-set allocation.
    fn remap_associations(
        &self,
        symbols: &[Option<usize>],
        relations: &[Option<usize>],
    ) -> (Vec<SourceImportFact>, Vec<SourceCallFact>) {
        let mut imports = Vec::with_capacity(self.imports.len());
        let mut import_ordinals = vec![None; self.imports.len()];
        for (ordinal, fact) in self.imports.iter().enumerate() {
            let Some(relation) = relations.get(fact.relation).copied().flatten() else {
                continue;
            };
            import_ordinals[ordinal] = Some(imports.len());
            imports.push(SourceImportFact {
                relation,
                span: fact.span,
                source_span: fact.source_span,
                module: fact.module.clone(),
                references: fact.references.clone(),
                rust: fact.rust.clone(),
                rust_toolchain: fact.rust_toolchain.clone(),
            });
        }
        let mut calls = Vec::with_capacity(self.calls.len());
        let mut declaration_remaps = HashMap::new();
        for fact in &self.calls {
            let Some(relation) = relations.get(fact.relation).copied().flatten() else {
                continue;
            };
            calls.push(SourceCallFact {
                relation,
                span: fact.span,
                source_span: fact.source_span,
                owner: fact
                    .owner
                    .and_then(|owner| symbols.get(owner).copied().flatten()),
                binding: remap_call_binding(
                    &fact.binding,
                    symbols,
                    &import_ordinals,
                    &mut declaration_remaps,
                ),
            });
        }
        (imports, calls)
    }

    /// Preserve discarded Rust declaration slots as unresolved competitors.
    fn remap_rust(
        &self,
        symbols: &[Option<usize>],
        relations: &[Option<usize>],
    ) -> Option<crate::SourceRustFacts> {
        self.rust.as_ref().map(|facts| {
            let surviving_uses = self
                .imports
                .iter()
                .filter(|import| relations.get(import.relation).copied().flatten().is_some())
                .filter_map(|import| import.rust.as_ref().map(Arc::as_ptr))
                .collect::<HashSet<_>>();
            let mut mapped = facts.clone();
            for declaration in &mut mapped.declarations {
                declaration.binding = match &declaration.binding {
                    crate::SourceRustBinding::Declaration(symbol) => {
                        symbols.get(*symbol).copied().flatten().map_or(
                            crate::SourceRustBinding::Unresolved,
                            crate::SourceRustBinding::Declaration,
                        )
                    }
                    crate::SourceRustBinding::TypeDeclaration(symbol) => {
                        symbols.get(*symbol).copied().flatten().map_or(
                            crate::SourceRustBinding::UnresolvedType,
                            crate::SourceRustBinding::TypeDeclaration,
                        )
                    }
                    crate::SourceRustBinding::Use(binding)
                        if !surviving_uses.contains(&Arc::as_ptr(binding)) =>
                    {
                        crate::SourceRustBinding::UnresolvedImport
                    }
                    binding => binding.clone(),
                };
            }
            for method in &mut mapped.associated {
                method.declaration = method
                    .declaration
                    .and_then(|ordinal| symbols.get(ordinal).copied().flatten());
            }
            let mut next_import = 0;
            let import_ordinals = self
                .imports
                .iter()
                .map(|import| {
                    relations.get(import.relation).copied().flatten().map(|_| {
                        let ordinal = next_import;
                        next_import += 1;
                        ordinal
                    })
                })
                .collect::<Vec<_>>();
            let mut candidates =
                HashMap::<*const [(usize, usize)], Option<Arc<[(usize, usize)]>>>::new();
            let mut declaration_candidates = HashMap::<*const [usize], Option<Arc<[usize]>>>::new();
            mapped.candidate_calls = facts
                .candidate_calls
                .iter()
                .filter_map(|call| {
                    let relation = relations.get(call.relation).copied().flatten()?;
                    let bindings = candidates
                        .entry(Arc::as_ptr(&call.bindings))
                        .or_insert_with(|| {
                            call.bindings
                                .iter()
                                .map(|(import, reference)| {
                                    import_ordinals
                                        .get(*import)
                                        .copied()
                                        .flatten()
                                        .map(|import| (import, *reference))
                                })
                                .collect::<Option<Vec<_>>>()
                                .map(Arc::from)
                        })
                        .as_ref()?;
                    let declarations = declaration_candidates
                        .entry(Arc::as_ptr(&call.declarations))
                        .or_insert_with(|| {
                            call.declarations
                                .iter()
                                .map(|symbol| symbols.get(*symbol).copied().flatten())
                                .collect::<Option<Vec<_>>>()
                                .map(Arc::from)
                        })
                        .as_ref()?;
                    Some(crate::SourceRustCallImports {
                        relation,
                        declarations: Arc::clone(declarations),
                        bindings: Arc::clone(bindings),
                        member: call.member.clone(),
                    })
                })
                .collect();
            mapped
        })
    }

    /// Preserve ambiguous export provenance even when only one candidate survives filtering.
    fn remap_exports(
        &self,
        symbols: &[Option<usize>],
        candidates: Option<&[SourceSymbolRemap]>,
    ) -> Vec<SourceExportFact> {
        let mut ambiguity = HashMap::new();
        self.exports
            .iter()
            .map(|fact| {
                let binding = match &fact.binding {
                    SourceExportBinding::LocalDeclaration(symbol) => {
                        if let Some(symbol) = symbols.get(*symbol).copied().flatten() {
                            SourceExportBinding::LocalDeclaration(symbol)
                        } else if let Some(candidate) = candidates
                            .and_then(|candidates| candidates.get(*symbol))
                            .and_then(|candidate| candidate.candidate())
                        {
                            SourceExportBinding::AmbiguousDeclarations(vec![candidate].into())
                        } else {
                            SourceExportBinding::Unresolved
                        }
                    }
                    SourceExportBinding::AmbiguousDeclarations(original)
                    | SourceExportBinding::UnresolvedDeclarations(original) => {
                        let mapped: &Arc<[usize]> =
                            ambiguity.entry(original.as_ptr()).or_insert_with(|| {
                                let mut mapped = original
                                    .iter()
                                    .filter_map(|symbol| {
                                        symbols
                                            .get(*symbol)
                                            .copied()
                                            .flatten()
                                            .or_else(|| candidates?.get(*symbol)?.candidate())
                                    })
                                    .collect::<Vec<_>>();
                                mapped.sort_unstable();
                                mapped.dedup();
                                mapped.into()
                            });
                        if mapped.is_empty() {
                            SourceExportBinding::Unresolved
                        } else if matches!(
                            fact.binding,
                            SourceExportBinding::UnresolvedDeclarations(_)
                        ) {
                            SourceExportBinding::UnresolvedDeclarations(Arc::clone(mapped))
                        } else {
                            SourceExportBinding::AmbiguousDeclarations(Arc::clone(mapped))
                        }
                    }
                    SourceExportBinding::ReExport(reference) => {
                        SourceExportBinding::ReExport(reference.clone())
                    }
                    SourceExportBinding::RustReExport(binding) => {
                        SourceExportBinding::RustReExport(Arc::clone(binding))
                    }
                    SourceExportBinding::Unresolved => SourceExportBinding::Unresolved,
                };
                SourceExportFact {
                    namespace: fact.namespace,
                    name: fact.name.clone(),
                    span: fact.span,
                    line: fact.line,
                    binding,
                }
            })
            .collect()
    }

    /// Require all binding ordinals to belong to their captured vectors.
    fn binding_is_in_bounds(
        &self,
        binding: &SourceCallBinding,
        checked_declarations: &mut HashSet<*const [usize]>,
    ) -> bool {
        match binding {
            SourceCallBinding::LocalDeclaration(symbol) => *symbol < self.symbol_count,
            SourceCallBinding::LocalDeclarations(symbols) => {
                !symbols.is_empty()
                    && (!checked_declarations.insert(Arc::as_ptr(symbols))
                        || symbols.iter().all(|symbol| *symbol < self.symbol_count))
            }
            SourceCallBinding::Imported {
                import, reference, ..
            } => self
                .imports
                .get(*import)
                .is_some_and(|fact| *reference < fact.references.len()),
            SourceCallBinding::Qualified(_)
            | SourceCallBinding::RustPath(_)
            | SourceCallBinding::Shadowed
            | SourceCallBinding::Unresolved
            | SourceCallBinding::PhpNamespace => true,
        }
    }

    /// Derive exact occurrence coordinates once, after final lexical and host mapping.
    ///
    /// # Errors
    ///
    /// Returns the caller's cancellation or deadline failure without exposing a partial extraction.
    pub(crate) fn bind_source_spans<E>(
        &mut self,
        content: &str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let mut offsets = Vec::with_capacity(
            self.calls
                .len()
                .saturating_add(self.imports.len())
                .saturating_mul(2),
        );
        for span in self
            .calls
            .iter()
            .map(|fact| fact.span)
            .chain(self.imports.iter().map(|fact| fact.span))
        {
            check()?;
            offsets.push(span.start);
            offsets.push(span.end);
        }
        let positions = crate::source_positions::source_positions(content, offsets, check)?;
        for (span, target) in self
            .calls
            .iter_mut()
            .map(|fact| (fact.span, &mut fact.source_span))
            .chain(
                self.imports
                    .iter_mut()
                    .map(|fact| (fact.span, &mut fact.source_span)),
            )
        {
            check()?;
            *target = occurrence_source_span(span, &positions);
        }
        Ok(())
    }

    /// Seal source identity after the final parser-side merge/remap.
    pub(crate) fn bind_source(&mut self, graph: &SymbolGraph, content: &str) {
        self.path.clone_from(&graph.path);
        self.content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        self.symbol_count = graph.symbols.len();
        self.relation_count = graph.relations.len();
        self.graph_digest = graph_ordinal_digest(graph);
    }
}

/// Preserve absent or invalid coordinate evidence rather than inventing a location.
fn occurrence_source_span(
    span: SourceByteSpan,
    positions: &[(usize, usize, usize)],
) -> Option<SourceSpan> {
    let start = positions.get(
        positions
            .binary_search_by_key(&span.start, |point| point.0)
            .ok()?,
    )?;
    let end = positions.get(
        positions
            .binary_search_by_key(&span.end, |point| point.0)
            .ok()?,
    )?;
    SourceSpan::new(
        u32::try_from(start.1).ok()?,
        u32::try_from(start.2).ok()?,
        u32::try_from(end.1).ok()?,
        u32::try_from(end.2).ok()?,
    )
    .ok()
}

/// Bind ordered declaration/relation identities without serializing a second graph.
fn graph_ordinal_digest(graph: &SymbolGraph) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"source-binding-ordinals");
    hash_text(&mut hash, &graph.path);
    hash_optional_text(&mut hash, graph.language.as_deref());
    hash.update(&[graph.parser as u8]);
    hash.update(&(graph.symbols.len() as u64).to_le_bytes());
    for symbol in &graph.symbols {
        hash_text(&mut hash, &symbol.path);
        hash_text(&mut hash, &symbol.name);
        hash_text(&mut hash, &symbol.signature);
        hash_optional_text(&mut hash, symbol.parent.as_deref());
        hash.update(&[
            symbol.kind as u8,
            symbol.parser as u8,
            u8::from(symbol.exported),
        ]);
        hash.update(&(symbol.line_start as u64).to_le_bytes());
        hash.update(&(symbol.line_end as u64).to_le_bytes());
        if let Some(selector) = symbol.source_selector {
            hash.update(&[1]);
            for ordinal in [
                selector.byte_start,
                selector.byte_end,
                selector.column_start,
                selector.column_end,
            ] {
                hash.update(&(ordinal as u64).to_le_bytes());
            }
        } else {
            hash.update(&[0]);
        }
    }
    hash.update(&(graph.relations.len() as u64).to_le_bytes());
    for relation in &graph.relations {
        hash_text(&mut hash, &relation.path);
        hash_text(&mut hash, &relation.source_name);
        hash_text(&mut hash, &relation.target_name);
        hash_text(&mut hash, &relation.context);
        hash.update(&[relation.kind as u8, relation.parser as u8]);
        hash.update(&(relation.line as u64).to_le_bytes());
    }
    *hash.finalize().as_bytes()
}

/// Keep adjacent UTF-8 identity components unambiguous in the digest.
fn hash_text(hash: &mut blake3::Hasher, text: &str) {
    hash.update(&(text.len() as u64).to_le_bytes());
    hash.update(text.as_bytes());
}

/// Distinguish absent identity text from an explicitly empty component.
fn hash_optional_text(hash: &mut blake3::Hasher, text: Option<&str>) {
    if let Some(text) = text {
        hash.update(&[1]);
        hash_text(hash, text);
    } else {
        hash.update(&[0]);
    }
}

/// Derive dense retained ordinals from one source-order mask.
fn retained_ordinals(retained: &[bool]) -> Vec<Option<usize>> {
    let mut next = 0;
    retained
        .iter()
        .map(|retained| {
            if *retained {
                let ordinal = next;
                next += 1;
                Some(ordinal)
            } else {
                None
            }
        })
        .collect()
}

/// Validate a bounded injective map without sorting or materializing graph facts.
fn validate_ordinal_map(
    mapping: &[Option<usize>],
    output_count: usize,
) -> Result<(), SourceSemanticRemapError> {
    let mut seen = vec![false; output_count];
    for ordinal in mapping.iter().flatten() {
        let Some(slot) = seen.get_mut(*ordinal) else {
            return Err(SourceSemanticRemapError::InvalidOrdinalMap);
        };
        if *slot {
            return Err(SourceSemanticRemapError::InvalidOrdinalMap);
        }
        *slot = true;
    }
    Ok(())
}

/// Count owned call payloads, charging each immutable overload allocation once.
pub(crate) fn call_binding_heap_bytes(calls: &[SourceCallFact]) -> u64 {
    let mut shared = HashSet::new();
    call_binding_heap_bytes_with_shared(calls, &mut shared, &mut HashSet::new())
}

/// Share allocation accounting across call policies and export ambiguity markers.
fn call_binding_heap_bytes_with_shared(
    calls: &[SourceCallFact],
    shared: &mut HashSet<*const [usize]>,
    rust_modules: &mut HashSet<*const [String]>,
) -> u64 {
    calls.iter().fold(0_u64, |bytes, call| {
        bytes.saturating_add(match &call.binding {
            SourceCallBinding::Qualified(reference) => reference.retained_heap_bytes(),
            SourceCallBinding::RustPath(call) => call.path.retained_bytes().saturating_add(
                crate::rust_binding_source::module_retained_bytes(&call.module, rust_modules),
            ),
            SourceCallBinding::Imported {
                member,
                rust_toolchain,
                ..
            } => (member.as_ref().map_or(0, String::capacity) as u64).saturating_add(
                rust_toolchain.as_ref().map_or(0, |reference| {
                    (std::mem::size_of::<ImportReference>() as u64)
                        .saturating_add(reference.retained_heap_bytes())
                }),
            ),
            SourceCallBinding::LocalDeclarations(declarations)
                if shared.insert(Arc::as_ptr(declarations)) =>
            {
                ((declarations.len() as u64).saturating_add(2))
                    .saturating_mul(std::mem::size_of::<usize>() as u64)
            }
            SourceCallBinding::LocalDeclaration(_)
            | SourceCallBinding::LocalDeclarations(_)
            | SourceCallBinding::Shadowed
            | SourceCallBinding::Unresolved
            | SourceCallBinding::PhpNamespace => 0,
        })
    })
}

/// Preserve a binding only when every required original owner survives.
fn remap_call_binding(
    binding: &SourceCallBinding,
    symbols: &[Option<usize>],
    imports: &[Option<usize>],
    declaration_remaps: &mut HashMap<*const usize, SourceCallBinding>,
) -> SourceCallBinding {
    match binding {
        SourceCallBinding::LocalDeclaration(symbol) => {
            symbols.get(*symbol).copied().flatten().map_or(
                SourceCallBinding::Unresolved,
                SourceCallBinding::LocalDeclaration,
            )
        }
        SourceCallBinding::LocalDeclarations(original) => declaration_remaps
            .entry(original.as_ptr())
            .or_insert_with(|| {
                original
                    .iter()
                    .map(|symbol| symbols.get(*symbol).copied().flatten())
                    .collect::<Option<Vec<_>>>()
                    .filter(|mapped| !mapped.is_empty())
                    .map_or(SourceCallBinding::Unresolved, |mapped| {
                        SourceCallBinding::LocalDeclarations(mapped.into())
                    })
            })
            .clone(),
        SourceCallBinding::Imported {
            import,
            reference,
            member,
            rust_toolchain,
        } => imports.get(*import).copied().flatten().map_or(
            SourceCallBinding::Unresolved,
            |import| SourceCallBinding::Imported {
                import,
                reference: *reference,
                member: member.clone(),
                rust_toolchain: rust_toolchain.clone(),
            },
        ),
        SourceCallBinding::Shadowed
        | SourceCallBinding::RustPath(_)
        | SourceCallBinding::Qualified(_)
        | SourceCallBinding::Unresolved
        | SourceCallBinding::PhpNamespace => binding.clone(),
    }
}

/// One extraction with independent source provenance and transient binding proof.
#[derive(Debug)]
pub struct SourceExtraction {
    /// Parser that produced the captured source facts.
    pub(crate) source_parser: ParserKind,
    /// Compatible graph facts used by existing storage and public renderers.
    pub(crate) graph: SymbolGraph,
    /// Typed source evidence; never reconstructed from a cached display string.
    pub(crate) semantics: SourceSemanticFacts,
}

impl SourceExtraction {
    /// Borrow the compatible graph without losing its associated evidence.
    #[must_use]
    pub const fn graph(&self) -> &SymbolGraph {
        &self.graph
    }

    /// Borrow the binding decisions paired with this graph.
    #[must_use]
    pub const fn semantics(&self) -> &SourceSemanticFacts {
        &self.semantics
    }

    /// Move the independently owned provenance, graph and semantic evidence together.
    #[must_use]
    pub fn into_parts(self) -> (ParserKind, SymbolGraph, SourceSemanticFacts) {
        (self.source_parser, self.graph, self.semantics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImportSyntax, extract_symbol_graph};
    use projectatlas_core::symbols::SymbolKind;
    use std::fmt::Write as _;

    /// Unpack positive fixtures while keeping assertions in unit-returning tests.
    macro_rules! fixture_ok {
        ($value:expr) => {{
            let outcome = $value;
            if let Err(error) = &outcome {
                assert!(outcome.is_ok(), "fixture operation failed: {error:?}");
            }
            let Ok(value) = outcome else {
                return;
            };
            value
        }};
    }

    #[test]
    fn native_and_host_occurrences_keep_original_scalar_coordinates_through_filtering() {
        for (path, language, source) in [
            (
                "main.rs",
                "rust",
                "/* Purpose: 💡π */ fn target(){} fn caller(){let _=\"💡π\";target();target();}",
            ),
            (
                "build.rs",
                "rust-build-script",
                "fn target(){} fn caller(){let _=\"💡π\";target();target();}",
            ),
            (
                "main.py",
                "python",
                "def target(): pass\ndef caller(): \"💡π\";target();target()\n",
            ),
            (
                "main.js",
                "javascript",
                "function target(){} function caller(){const x=\"💡π\";target();target();}",
            ),
            (
                "main.ts",
                "typescript",
                "function target(){} function caller(){const x=\"💡π\";target();target();}",
            ),
            (
                "main.tsx",
                "tsx",
                "function target(){} function caller(){const x=\"💡π\";target();target();}",
            ),
            (
                "main.go",
                "go",
                "package p; func Target(){}; func Caller(){_ = \"💡π\";Target();Target()}",
            ),
            (
                "main.c",
                "c",
                "void target(){} void caller(){const char*x=\"💡π\";(void)x;target();target();}",
            ),
            (
                "main.cpp",
                "cpp",
                "void target(){} void caller(){const char*x=\"💡π\";(void)x;target();target();}",
            ),
            (
                "main.m",
                "objective-c",
                "void target(){} void caller(){const char*x=\"💡π\";(void)x;target();target();}",
            ),
            (
                "Main.java",
                "java",
                "class Main {void target(){} void caller(){String x=\"💡π\";target();target();}}",
            ),
            (
                "Main.cs",
                "csharp",
                "class Main {void target(){} void caller(){string x=\"💡π\";target();target();}}",
            ),
            (
                "main.kt",
                "kotlin",
                "fun target(){}\r\nfun caller(){val x=\"💡π\";target();target()}\r\n",
            ),
            (
                "main.zig",
                "zig",
                "fn target() void {} fn caller() void {const x=\"💡π\";_=x;target();target();}",
            ),
            (
                "main.php",
                "php",
                "<?php function target(){} function caller(){$x=\"💡π\";target();target();}",
            ),
            (
                "view.html",
                "html",
                "<div>💡π</div><script>function target(){} function caller(){target();target();}</script>",
            ),
            (
                "view.vue",
                "vue",
                "<template>💡π</template><script setup>function target(){} function caller(){target();target();}</script>",
            ),
            (
                "view.svelte",
                "svelte",
                "<div>💡π</div><script>function target(){} function caller(){target();target();}</script>",
            ),
        ] {
            let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                source,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            assert_eq!(
                result.semantics().calls().len(),
                2,
                "{language}: {:?}",
                result.graph()
            );
            let original = result.semantics().calls();
            assert_ne!(
                original[0].source_span, original[1].source_span,
                "{language}"
            );
            for call in original {
                let span = fixture_ok!(call.source_span.ok_or("missing captured coordinates"));
                let prefix = &source[..call.span.start];
                let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let column = prefix
                    .rsplit('\n')
                    .next()
                    .map_or(0, |line| line.chars().count());
                assert_eq!(
                    (span.start_line() as usize, span.start_column() as usize),
                    (line, column),
                    "{language}"
                );
                let end = &source[..call.span.end];
                let end_line = end.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let end_column = end
                    .rsplit('\n')
                    .next()
                    .map_or(0, |line| line.chars().count());
                assert_eq!(
                    (span.end_line() as usize, span.end_column() as usize),
                    (end_line, end_column),
                    "{language}"
                );
            }
            let mut graph = result.graph().clone();
            let mut facts = result.semantics().clone();
            let expected = original[1].source_span;
            let mut relations = vec![true; graph.relations.len()];
            relations[original[0].relation] = false;
            fixture_ok!(facts.retain_graph_facts(
                &mut graph,
                &vec![true; result.graph().symbols.len()],
                &relations
            ));
            assert_eq!(facts.calls().len(), 1, "{language}");
            assert_eq!(facts.calls()[0].source_span, expected, "{language}");
            assert!(facts.matches_graph(&graph));
        }
    }

    #[test]
    fn multiline_imports_and_refined_callbacks_keep_their_exact_coordinates() {
        for (path, language, source) in [
            (
                "main.rs",
                "rust",
                "/*π*/ use std::{\r\n mem as aa,\r\n}; use std::mem as ab;",
            ),
            (
                "main.py",
                "python",
                "from worker import (\r\n target as aa,\r\n)\r\nfrom worker import target as ab\r\n",
            ),
            (
                "main.js",
                "javascript",
                "/*π*/ import {\r\n target as aa\r\n} from './πworker.js'; import {target as ab} from './πworker.js';",
            ),
        ] {
            let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                source,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            let imports = result.semantics().imports();
            assert_eq!(imports.len(), 2, "{language}");
            let first = fixture_ok!(imports[0].source_span.ok_or("missing import coordinates"));
            let second = fixture_ok!(imports[1].source_span.ok_or("missing import coordinates"));
            assert_ne!(first, second);
            assert!(first.end_line() > first.start_line());
        }
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "main.rs",
            Some("rust"),
            "mod worker {pub fn target(){}} fn caller(){let _=(1 == 1).then(worker::target);let _=(1 == 1).then(worker::target);}",
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        let callbacks = result
            .semantics()
            .calls()
            .iter()
            .filter(|call| result.graph().relations[call.relation].target_name == "worker::target")
            .collect::<Vec<_>>();
        assert_eq!(callbacks.len(), 2);
        assert_ne!(callbacks[0].source_span, callbacks[1].source_span);
        for callback in callbacks {
            let span = fixture_ok!(callback.source_span.ok_or("missing callback coordinates"));
            assert_eq!(
                span.end_column() - span.start_column(),
                "worker::target".len() as u32
            );
        }
    }

    #[test]
    fn native_member_declarations_do_not_replace_ordinary_function_values() {
        let mut failures = Vec::new();
        for (path, language, source, name) in [
            (
                "source.rs",
                "rust",
                "fn run(){} struct A; impl A {fn run(){} fn caller(){run();}}",
                "run",
            ),
            (
                "trait.rs",
                "rust",
                "fn run(){} trait A {fn run(){} fn caller(){run();}}",
                "run",
            ),
            (
                "source.go",
                "go",
                "package p; func Run(){}; type A struct{}; func(A) Run(){}; func(A) Caller(){Run()}",
                "Run",
            ),
            (
                "source.js",
                "javascript",
                "function run(){} class A {run(){} caller(){run();}}",
                "run",
            ),
            (
                "object.js",
                "javascript",
                "function run(){} export const obj={run(){},caller(){run();}};",
                "run",
            ),
            (
                "source.ts",
                "typescript",
                "function run(){} class A {run(){} caller(){run();}}",
                "run",
            ),
            (
                "source.tsx",
                "tsx",
                "function run(){} class A {run(){} caller(){run();}}",
                "run",
            ),
            (
                "source.m",
                "objective-c",
                "void run(void){} @interface A -(void)run; -(void)caller; @end @implementation A -(void)run{} -(void)caller{run();} @end",
                "run",
            ),
            (
                "source.py",
                "python",
                "def run(): pass\nclass A:\n def run(self): pass\n def caller(self): run()\n",
                "run",
            ),
        ] {
            let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                source,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            let correct = !result.semantics().calls().is_empty()
                && result
                    .semantics()
                    .calls()
                    .iter()
                    .all(|call| match call.binding {
                        SourceCallBinding::LocalDeclaration(ordinal) => {
                            result.graph().symbols.get(ordinal).is_some_and(|symbol| {
                                symbol.name == name
                                    && symbol.kind == SymbolKind::Function
                                    && symbol.parent.is_none()
                            })
                        }
                        _ => false,
                    });
            if !correct {
                failures.push((path, result.semantics().calls().to_vec()));
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn distinct_declarations_keep_implicit_member_lookup_and_real_ambiguity() {
        for (path, language, source) in [
            (
                "source.java",
                "java",
                "class One {void run(){} void caller(){run();}} class Two {void run(){} void caller(){run();}}",
            ),
            (
                "source.cs",
                "csharp",
                "class One {void run(){} void caller(){run();}} class Two {void run(){} void caller(){run();}}",
            ),
            (
                "source.cpp",
                "cpp",
                "class One {public:void run(){} void caller(){run();}}; class Two {public:void run(){} void caller(){run();}};",
            ),
            (
                "source.kt",
                "kotlin",
                "class One {\n fun run() {}\n fun caller() { run() }\n}\nclass Two {\n fun run() {}\n fun caller() { run() }\n}\n",
            ),
        ] {
            let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                source,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            assert_eq!(
                result.semantics().calls().len(),
                2,
                "{language}: {:?}",
                result.graph()
            );
            for call in result.semantics().calls() {
                let owner = fixture_ok!(
                    call.owner
                        .and_then(|ordinal| result.graph().symbols.get(ordinal))
                        .ok_or("missing exact caller")
                );
                assert!(
                    matches!(call.binding, SourceCallBinding::LocalDeclaration(_)),
                    "{language}: {:?}",
                    call.binding
                );
                let SourceCallBinding::LocalDeclaration(ordinal) = call.binding else {
                    return;
                };
                let target =
                    fixture_ok!(result.graph().symbols.get(ordinal).ok_or("missing target"));
                assert_eq!(target.name, "run");
                assert_eq!(target.parent, owner.parent, "{language}");
            }
        }
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "same.rs",
            Some("rust"),
            "fn run(){} fn run(){} fn caller(){run();}",
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        assert!(
            result
                .semantics()
                .calls()
                .iter()
                .all(|call| !matches!(call.binding, SourceCallBinding::LocalDeclaration(_)))
        );
    }

    #[test]
    fn rust_scoped_import_candidates_preserve_sharing_and_cannot_narrow_after_filtering() {
        let source = "mod m {pub struct Foo{}} fn foo(){} fn caller(){use crate::m::Foo;use crate::foo as Foo; Foo();\nFoo();} fn local(){use crate::m::Foo;fn Foo(){} Foo();}";
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        let graph = result.graph();
        let original = result.semantics();
        let rust = fixture_ok!(original.rust().ok_or("Rust facts missing"));
        assert_eq!(rust.candidate_calls.len(), 3);
        assert!(Arc::ptr_eq(
            &rust.candidate_calls[0].bindings,
            &rust.candidate_calls[1].bindings
        ));
        assert!(
            rust.candidate_calls
                .iter()
                .any(|call| !call.declarations.is_empty())
        );
        let mut filtered = graph.clone();
        let mut facts = original.clone();
        let symbols = graph
            .symbols
            .iter()
            .map(|symbol| symbol.name != "Foo" || symbol.kind != SymbolKind::Function)
            .collect::<Vec<_>>();
        let discarded = original.imports()[1].relation;
        let relations = graph
            .relations
            .iter()
            .enumerate()
            .map(|(ordinal, _)| ordinal != discarded)
            .collect::<Vec<_>>();
        fixture_ok!(facts.retain_graph_facts(&mut filtered, &symbols, &relations));
        assert!(facts.matches_graph(&filtered));
        assert!(
            facts
                .rust()
                .is_some_and(|rust| rust.candidate_calls.is_empty())
        );
        assert!(
            facts
                .calls()
                .iter()
                .all(|call| matches!(call.binding, SourceCallBinding::Unresolved))
        );
        assert_eq!(facts.imports().len(), original.imports().len() - 1);
    }

    #[test]
    fn go_receiver_parameters_shadow_file_functions_without_hiding_siblings() {
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "receiver.go",
            Some("go"),
            "package p; type T func(); func Run(){}; func(Run T) Caller(){Run()}; func(T) Clean(){Run()}",
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        assert_eq!(result.semantics().calls().len(), 2);
        for call in result.semantics().calls() {
            let name = result.graph().relations[call.relation].source_name.as_str();
            if name == "Caller" {
                assert_eq!(call.binding, SourceCallBinding::Shadowed);
            } else {
                assert_eq!(name, "Clean");
                assert!(matches!(
                    call.binding,
                    SourceCallBinding::LocalDeclaration(_)
                ));
            }
        }
    }

    #[test]
    fn omitted_export_dependencies_cannot_retain_forwarding_authority() {
        let mut source = "function target(){} function caller(){".to_string();
        for _ in 0..crate::MAX_RELATIONS_PER_FILE {
            source.push_str("target();");
        }
        source.push_str("} export {run as launch} from './target.js';");
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "forward.js",
            Some("javascript"),
            &source,
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        assert!(result.semantics().bindings_incomplete);
        assert!(result.semantics().exports_incomplete);
        assert!(result.semantics().exports().is_empty());
        assert!(!result.graph().relations.is_empty());
    }

    #[test]
    fn omitted_native_imports_cannot_leave_exact_call_authority() {
        for (path, language, prefix, import) in [
            (
                "source.py",
                "python",
                "def target(): pass\ndef caller(): target()\n",
                "import module",
            ),
            (
                "source.js",
                "javascript",
                "function target(){} function caller(){target();}\n",
                "import * as module",
            ),
        ] {
            let mut source = prefix.to_string();
            for ordinal in 0..=crate::MAX_RELATIONS_PER_FILE {
                if language == "python" {
                    fixture_ok!(writeln!(source, "{import}{ordinal}"));
                } else {
                    fixture_ok!(writeln!(
                        source,
                        "{import}{ordinal} from './module{ordinal}';"
                    ));
                }
            }
            let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                &source,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            let facts = result.semantics();
            assert!(facts.bindings_incomplete, "{language}");
            assert!(
                !facts.imports().is_empty() && !facts.calls().is_empty(),
                "{language}"
            );
            assert!(
                facts
                    .calls()
                    .iter()
                    .all(|call| matches!(call.binding, SourceCallBinding::Unresolved)),
                "{language}"
            );
            let mut graph = result.graph().clone();
            let mut remapped = facts.clone();
            fixture_ok!(remapped.retain_graph_facts(
                &mut graph,
                &vec![true; result.graph().symbols.len()],
                &vec![true; result.graph().relations.len()]
            ));
            assert!(remapped.bindings_incomplete && remapped.matches_graph(&graph));
        }
    }

    #[test]
    fn rust_remapping_retains_blockers_and_drops_lost_import_authority() {
        let source = "pub fn run(){} pub struct Service; impl Service {pub fn work(){}} pub use crate::run as launch; use std::mem::drop as dispose; fn caller(){launch();dispose(1);}";
        let result = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        let (graph, original) = (result.graph(), result.semantics());
        let mut filtered = graph.clone();
        let mut facts = original.clone();
        let symbols = graph
            .symbols
            .iter()
            .map(|symbol| symbol.name != "run" && symbol.name != "Service" && symbol.name != "work")
            .collect::<Vec<_>>();
        let relations = graph
            .relations
            .iter()
            .map(|relation| relation.kind != RelationKind::Imports)
            .collect::<Vec<_>>();
        fixture_ok!(facts.retain_graph_facts(&mut filtered, &symbols, &relations));
        assert!(facts.matches_graph(&filtered));
        let rust = fixture_ok!(facts.rust().ok_or("Rust facts missing"));
        assert!(
            rust.declarations
                .iter()
                .any(|declaration| declaration.name == "run"
                    && declaration.binding == crate::SourceRustBinding::Unresolved)
        );
        assert!(
            rust.declarations
                .iter()
                .any(|declaration| declaration.name == "Service"
                    && declaration.binding == crate::SourceRustBinding::UnresolvedType)
        );
        assert!(
            rust.declarations
                .iter()
                .any(|declaration| declaration.name == "launch"
                    && declaration.binding == crate::SourceRustBinding::UnresolvedImport)
        );
        assert!(
            rust.associated
                .iter()
                .all(|method| method.declaration.is_none())
        );
        assert!(
            facts
                .calls()
                .iter()
                .all(|call| !matches!(call.binding, SourceCallBinding::Imported { .. }))
        );
        assert!(
            original
                .imports()
                .iter()
                .any(|import| import.rust_toolchain.is_some())
        );
        assert!(facts.imports().is_empty());
    }

    #[test]
    fn embedded_hosts_keep_binding_ownership_and_original_byte_occurrences() {
        let content = "<template>🙂</template>\n<script lang=\"ts\">\nimport launch, { run as named } from './worker';\nfunction caller(launch: () => void) { launch(); }\nfunction clean() { launch(); named(); named(); }\n</script>\n<style>fake { content: 'launch()'; }</style>";
        for (path, language) in [
            ("view.vue", "vue"),
            ("view.svelte", "svelte"),
            ("view.html", "html"),
        ] {
            let extraction = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                path,
                Some(language),
                content,
                &projectatlas_core::IndexWorkControl::new(
                    projectatlas_core::IndexCancellation::new(),
                    None
                )
            ));
            let (_, graph, facts) = extraction.into_parts();
            assert!(facts.matches_graph(&graph), "{language}");
            assert_eq!(
                facts.content_hash(),
                blake3::hash(content.as_bytes()).to_hex().as_str()
            );
            assert_eq!(facts.imports().len(), 1, "{language}");
            assert_eq!(facts.imports()[0].references.len(), 2, "{language}");
            let calls = facts.calls();
            assert_eq!(calls.len(), 4, "{language}: {graph:?}");
            assert_eq!(calls[0].binding, SourceCallBinding::Shadowed, "{language}");
            for call in &calls[1..] {
                assert!(
                    matches!(call.binding, SourceCallBinding::Imported { .. }),
                    "{language}: {call:?}"
                );
                assert_eq!(
                    call.owner.map(|owner| graph.symbols[owner].name.as_str()),
                    Some("clean")
                );
                assert!(matches!(
                    &content[call.span.start..call.span.end],
                    "launch()" | "named()"
                ));
            }
            assert_ne!(
                calls[2].relation, calls[3].relation,
                "distinct same-line occurrences"
            );
            assert_eq!(
                graph.parser,
                if language == "svelte" {
                    ParserKind::Fallback
                } else {
                    ParserKind::Structural
                },
                "{language} host provenance"
            );
        }
    }

    #[test]
    fn embedded_script_families_keep_independent_import_ordinals() {
        let content = "<script>import { run as first } from './first'; function js_caller() { first(); }</script>\n<script lang='ts'>import { run as second } from './second'; function ts_caller() { second(); }</script>";
        let extraction = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "view.html",
            Some("html"),
            content,
            &projectatlas_core::IndexWorkControl::new(
                projectatlas_core::IndexCancellation::new(),
                None
            )
        ));
        let (_, graph, facts) = extraction.into_parts();
        assert!(facts.matches_graph(&graph));
        assert_eq!(facts.imports().len(), 2);
        assert_eq!(facts.calls().len(), 2);
        for fact in facts.calls() {
            assert!(
                matches!(fact.binding, SourceCallBinding::Imported { .. }),
                "missing independently mapped import: {fact:?}"
            );
            let SourceCallBinding::Imported {
                import, reference, ..
            } = fact.binding
            else {
                return;
            };
            let module = facts.imports()[import].references[reference].module();
            assert_eq!(
                module,
                if graph.relations[fact.relation].target_name == "first" {
                    "./first"
                } else {
                    "./second"
                }
            );
        }
    }

    #[test]
    fn coalesced_embedded_declarations_cannot_narrow_ambiguous_binding_proof() {
        let script = "function run() {} function run() {} function clean() { run(); }";
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let native = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "script.js",
            Some("javascript"),
            script,
            &control
        ));
        assert_eq!(
            native
                .graph()
                .symbols
                .iter()
                .filter(|symbol| symbol.name == "run")
                .count(),
            2
        );
        let mut merged = native.graph().clone();
        merged.symbols.clear();
        merged.relations.clear();
        let mut mapping = crate::GraphMergeOrdinals {
            symbols: vec![crate::SourceSymbolRemap::Omitted; native.graph().symbols.len()],
            relations: vec![None; native.graph().relations.len()],
        };
        fixture_ok!(crate::merge_missing_graph_entries_checked(
            &mut merged,
            native.graph().clone(),
            &std::collections::BTreeMap::new(),
            projectatlas_core::language::EmbeddedHostKind::HtmlLike,
            &mut || control.check(projectatlas_core::IndexWorkStage::SymbolParsing),
            Some(&mut mapping)
        ));
        for (ordinal, symbol) in native.graph().symbols.iter().enumerate() {
            assert_eq!(
                mapping.symbols[ordinal].retained().is_none(),
                symbol.name == "run",
                "all coalesced owners must lose correspondence"
            );
        }
        let content = format!("<script>{script}</script>");
        let host = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "view.html",
            Some("html"),
            &content,
            &control
        ));
        assert_eq!(
            host.graph()
                .symbols
                .iter()
                .filter(|symbol| symbol.name == "run")
                .count(),
            1
        );
        assert_eq!(host.semantics().calls().len(), 1);
        assert_eq!(
            host.semantics().calls()[0].binding,
            SourceCallBinding::Unresolved
        );
        assert!(host.semantics().matches_graph(host.graph()));
    }

    fn captured_graph() -> Result<(SymbolGraph, SourceSemanticFacts), Box<dyn std::error::Error>> {
        let content = "use dep::run as launch; fn run() {} fn caller() { launch(); }";
        let graph = extract_symbol_graph("src/main.rs", Some("rust"), content);
        let import = graph
            .relations
            .iter()
            .position(|fact| fact.kind == RelationKind::Imports);
        let call = graph
            .relations
            .iter()
            .position(|fact| fact.kind == RelationKind::Calls);
        let import = import.ok_or_else(|| std::io::Error::other("fixture import missing"))?;
        let call = call.ok_or_else(|| std::io::Error::other("fixture call missing"))?;
        let call_start = content
            .rfind("launch()")
            .ok_or_else(|| std::io::Error::other("fixture call span missing"))?;
        let mut facts = SourceSemanticFacts {
            imports: vec![SourceImportFact {
                source_span: None,
                relation: import,
                span: SourceByteSpan { start: 0, end: 23 },
                module: Some("dep".into()),
                references: vec![ImportReference::new(
                    ImportSyntax::Rust,
                    "dep",
                    Some("run"),
                    "launch",
                )],
                rust: None,
                rust_toolchain: None,
            }],
            calls: vec![SourceCallFact {
                source_span: None,
                relation: call,
                span: SourceByteSpan {
                    start: call_start,
                    end: call_start + "launch()".len(),
                },
                binding: SourceCallBinding::Imported {
                    import: 0,
                    reference: 0,
                    member: None,
                    rust_toolchain: None,
                },
                owner: None,
            }],
            ..SourceSemanticFacts::default()
        };
        facts.bind_source(&graph, content);
        Ok((graph, facts))
    }

    #[test]
    fn same_size_graph_reordering_cannot_reuse_binding_ordinals() {
        let (mut graph, facts) = fixture_ok!(captured_graph());
        assert!(facts.matches_graph(&graph));
        graph.relations.swap(0, 1);
        assert!(!facts.matches_graph(&graph));
        let symbols = (0..graph.symbols.len()).map(Some).collect::<Vec<_>>();
        let relations = (0..graph.relations.len()).map(Some).collect::<Vec<_>>();
        assert_eq!(
            facts.remap(&graph, &graph, &symbols, &relations),
            Err(SourceSemanticRemapError::GraphMismatch)
        );
    }

    #[test]
    fn retaining_graph_facts_rebases_targets_and_call_owners_without_source_drift() {
        let content = "fn padding() {} fn run() {} fn caller() { run(); }";
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let extracted = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "main.rs",
            Some("rust"),
            content,
            &control
        ));
        let (_, mut graph, mut facts) = extracted.into_parts();
        let hash = facts.content_hash.clone();
        assert_eq!(graph.symbols.len(), 3);
        assert_eq!(facts.calls[0].owner, Some(2));
        let relations = vec![true; graph.relations.len()];
        fixture_ok!(facts.retain_graph_facts(&mut graph, &[false, true, true], &relations));
        assert!(facts.matches_graph(&graph));
        assert_eq!(facts.content_hash, hash);
        assert_eq!(facts.calls[0].owner, Some(1));
        assert_eq!(
            facts.calls[0].binding,
            SourceCallBinding::LocalDeclaration(0)
        );
        let original_graph = graph.clone();
        let original_facts = facts.clone();
        assert_eq!(
            facts.retain_graph_facts(&mut graph, &[true], &relations),
            Err(SourceSemanticRemapError::GraphMismatch)
        );
        assert_eq!(graph, original_graph);
        assert_eq!(facts, original_facts);
    }

    #[test]
    fn shared_overload_sets_stay_shared_through_ordinal_remapping() {
        let content =
            "void run(int value) {}\nvoid run(double value) {}\nvoid caller() { run(0); run(1); }";
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let extracted = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "main.cpp",
            Some("cpp"),
            content,
            &control
        ));
        let (_, mut graph, mut facts) = extracted.into_parts();
        let symbols = vec![true; graph.symbols.len()];
        let relations = vec![true; graph.relations.len()];
        fixture_ok!(facts.retain_graph_facts(&mut graph, &symbols, &relations));
        assert!(facts.matches_graph(&graph));
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
        assert!(Arc::ptr_eq(first, second));
    }

    #[test]
    fn filtering_an_import_does_not_rebind_its_surviving_call() {
        let (graph, facts) = fixture_ok!(captured_graph());
        let removed = facts.imports[0].relation;
        let mut admitted = graph.clone();
        admitted.relations.remove(removed);
        let symbols = (0..graph.symbols.len()).map(Some).collect::<Vec<_>>();
        let relations = (0..graph.relations.len())
            .map(|index| {
                if index == removed {
                    None
                } else {
                    Some(index - usize::from(index > removed))
                }
            })
            .collect::<Vec<_>>();
        let remapped = fixture_ok!(facts.remap(&graph, &admitted, &symbols, &relations));
        assert!(remapped.matches_graph(&admitted));
        assert!(remapped.imports.is_empty());
        assert_eq!(remapped.calls[0].binding, SourceCallBinding::Unresolved);
        assert_eq!(remapped.content_hash, facts.content_hash);
    }

    #[test]
    fn filtering_ambiguous_declarations_does_not_create_an_exact_target() {
        let binding = SourceCallBinding::LocalDeclarations(vec![0, 1].into());
        assert_eq!(
            remap_call_binding(&binding, &[Some(0), None], &[], &mut HashMap::new()),
            SourceCallBinding::Unresolved
        );
        assert_eq!(
            remap_call_binding(&binding, &[Some(1), Some(0)], &[], &mut HashMap::new()),
            SourceCallBinding::LocalDeclarations(vec![1, 0].into())
        );
    }

    #[test]
    fn duplicate_or_outside_output_ordinals_fail_without_changing_evidence() {
        let (graph, facts) = fixture_ok!(captured_graph());
        let symbols = (0..graph.symbols.len()).map(Some).collect::<Vec<_>>();
        let mut relations = (0..graph.relations.len()).map(Some).collect::<Vec<_>>();
        relations[1] = relations[0];
        assert_eq!(
            facts.remap(&graph, &graph, &symbols, &relations),
            Err(SourceSemanticRemapError::InvalidOrdinalMap)
        );
        relations[1] = Some(graph.relations.len());
        assert_eq!(
            facts.remap(&graph, &graph, &symbols, &relations),
            Err(SourceSemanticRemapError::InvalidOrdinalMap)
        );
        assert!(facts.matches_graph(&graph));
    }

    #[test]
    fn malformed_ecmascript_imports_have_no_admitted_binding_evidence() {
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for language in ["javascript", "typescript", "tsx"] {
            for declaration in [
                "import foo.bar, { run } from './dep';",
                "import class, { run } from './dep';",
                "import { run as class } from './dep';",
                "import * as class from './dep';",
            ] {
                let extraction =
                    fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                        "src/main.tsx",
                        Some(language),
                        declaration,
                        &control,
                    ));
                assert!(extraction.semantics().matches_graph(extraction.graph()));
                assert!(
                    extraction
                        .semantics()
                        .imports()
                        .iter()
                        .flat_map(|fact| &fact.references)
                        .all(|reference| !matches!(reference.local(), "class" | "foo.bar" | "foo")),
                    "{language}: {declaration}"
                );
            }
        }
    }

    #[test]
    fn valid_mixed_import_evidence_retains_independent_references_and_source() {
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        let content =
            "import primary, { run as launch } from './dep'; function caller() { launch(); }";
        let extraction = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
            "src/main.js",
            Some("javascript"),
            content,
            &control,
        ));
        let facts = extraction.semantics();
        assert!(facts.matches_graph(extraction.graph()));
        assert_eq!(facts.imports().len(), 1);
        assert_eq!(facts.imports()[0].references.len(), 2);
        assert_eq!(facts.imports()[0].references[0].local(), "primary");
        assert_eq!(facts.imports()[0].references[1].local(), "launch");
        assert_eq!(facts.calls().len(), 1);
        let span = facts.calls()[0].span;
        assert_eq!(&content[span.start..span.end], "launch()");
        assert_eq!(
            facts.content_hash(),
            blake3::hash(content.as_bytes()).to_hex().as_str()
        );
    }

    #[test]
    fn invalid_binding_leaves_preserve_static_modules_and_valid_siblings() {
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for language in ["javascript", "typescript", "tsx"] {
            for statement in [
                "import class, { run as keep } from './dep';",
                "import foo.bar, { run as keep } from './dep';",
                "import primary, { run as class, stop as keep } from './dep';",
                r"import cl\u0061ss, { run as keep } from './dep';",
                r"import { run as ev\u0061l, stop as keep } from './dep';",
                "import { run as 🙂, stop as keep } from './dep';",
            ] {
                let extraction =
                    fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                        "src/main.tsx",
                        Some(language),
                        statement,
                        &control,
                    ));
                let imports = extraction.semantics().imports();
                assert!(
                    imports
                        .iter()
                        .any(|fact| fact.module.as_deref() == Some("./dep")),
                    "lost module: {language} {statement}"
                );
                let locals = imports
                    .iter()
                    .flat_map(|fact| &fact.references)
                    .map(ImportReference::local)
                    .collect::<Vec<_>>();
                let diagnostic = if locals.contains(&"keep") {
                    String::new()
                } else {
                    let language = fixture_ok!(
                        crate::tree_sitter_language(language)
                            .ok_or_else(|| std::io::Error::other("fixture language missing"))
                    );
                    let mut parser = tree_sitter::Parser::new();
                    fixture_ok!(parser.set_language(&language));
                    fixture_ok!(
                        parser
                            .parse(statement, None)
                            .ok_or_else(|| std::io::Error::other("fixture parse missing"))
                    )
                    .root_node()
                    .to_sexp()
                };
                assert!(
                    locals.contains(&"keep"),
                    "lost valid sibling: {language} {statement}: {locals:?} {diagnostic}"
                );
                assert!(
                    locals
                        .iter()
                        .all(|local| matches!(*local, "primary" | "keep")),
                    "invalid leaf admitted: {language} {statement}: {locals:?}"
                );
            }
            let statement = r"import { class as C, default as D, run as l\u0061unch, stop as undefined } from './dep';";
            let extraction = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                "src/main.tsx",
                Some(language),
                statement,
                &control,
            ));
            let locals = extraction
                .semantics()
                .imports()
                .iter()
                .flat_map(|fact| &fact.references)
                .map(ImportReference::local)
                .collect::<Vec<_>>();
            assert_eq!(locals, ["C", "D", "launch", "undefined"], "{language}");
        }
    }

    #[test]
    fn recovery_uses_the_ast_separator_and_preserves_quoted_export_data() {
        let control = projectatlas_core::IndexWorkControl::new(
            projectatlas_core::IndexCancellation::new(),
            None,
        );
        for language in ["javascript", "typescript", "tsx"] {
            for (statement, module, imported) in [
                (
                    "import { from as keep, run as class } from './dep';",
                    "./dep",
                    "from",
                ),
                (
                    "import { run as class, stop as keep } from './dep from space.js';",
                    "./dep from space.js",
                    "stop",
                ),
                (
                    r#"import { "name from space" as keep, run as class } from './dep from space.js';"#,
                    "./dep from space.js",
                    "name from space",
                ),
                (
                    r#"import { "a, ghost as forged, b" as keep, run as class } from './dep';"#,
                    "./dep",
                    "a, ghost as forged, b",
                ),
                (
                    r#"import { "x as y" as keep, run as class } from './dep';"#,
                    "./dep",
                    "x as y",
                ),
                (
                    "import /*lead*/ primary, {run /*bad*/ as class, from /*good*/ as keep} /*tail*/ from './dep';",
                    "./dep",
                    "from",
                ),
            ] {
                let extraction =
                    fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                        "src/main.tsx",
                        Some(language),
                        statement,
                        &control,
                    ));
                let imports = extraction.semantics().imports();
                assert!(
                    imports
                        .iter()
                        .any(|fact| fact.module.as_deref() == Some(module))
                );
                let references = imports
                    .iter()
                    .flat_map(|fact| &fact.references)
                    .collect::<Vec<_>>();
                assert!(
                    references
                        .iter()
                        .any(|reference| reference.local() == "keep"
                            && reference.imported() == Some(imported)),
                    "{language}: {statement}: {references:?}"
                );
                assert!(
                    references
                        .iter()
                        .all(|reference| matches!(reference.local(), "keep" | "primary")),
                    "fabricated binding: {language}: {statement}: {references:?}"
                );
            }
            let extraction = fixture_ok!(crate::extract_symbol_graph_with_semantics_controlled(
                "src/main.tsx",
                Some(language),
                "import { run as keep,, stop as other } from './dep';",
                &control,
            ));
            assert!(
                extraction
                    .semantics()
                    .imports()
                    .iter()
                    .all(|fact| fact.references.is_empty())
            );
        }
    }
}
