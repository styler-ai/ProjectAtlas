//! Normalize parser-owned symbol facts into one generation-bound repository graph.

use super::{
    CliError, INDEX_FRESHNESS_SAMPLE_LIMIT, IndexReadStatus, IndexRefreshReason,
    IndexRefreshRequired, IndexRefreshScope, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
    IndexWorkStage, MAX_SYMBOL_FILE_BYTES, Node, NodeKind, SourceReadFailure, SymbolBuildStage,
    SymbolProjectionChange, normalize_native_path_display, read_source_bytes_controlled,
    source_changed_during_derivation,
};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector,
    ExtendedRelationKind, ExternalSelector, GraphContractError, GraphEntity, GraphIdentityText,
    GraphLimitKind, GraphLimits, GraphRelationKind, LogicalRelation, LogicalRelationKey,
    MAX_GRAPH_IDENTITY_BYTES, PackageSelector, ProjectInstanceId, QUALIFIED_SYMBOL_SCOPE_PREFIX,
    RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath,
    RepositoryNodePath, ResolutionKeyDomain, SourceSpan, SymbolSelector,
};
use projectatlas_core::language::{SemanticProviderOwner, SymbolParserOwner, language_capability};
use projectatlas_core::symbols::{
    ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_db::{
    AtlasStore, IndexPublicationGuard, RepositoryAffectedSourceFootprint,
    RepositoryResolutionCandidate,
};
use projectatlas_fs::RootScanPolicy;
#[cfg(test)]
use projectatlas_fs::ScanOptions;
use projectatlas_symbols::{
    ConfiguredModuleResolution, MAX_RESOLUTION_KEYS_PER_FACT, MarkdownFactCompleteness,
    MarkdownFactLimit, MarkdownFacts, ResolutionKeyProjection, ResolutionProjectionContext,
    ResolutionProjectionError, derive_resolution_keys_with_context,
    extract_markdown_facts_controlled, parse_import_references,
};
use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, btree_map::Entry};
use std::fs::{self, File, OpenOptions};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempDirBuilder, TempDir};

/// Maximum canonical keys or distinct source paths admitted by one incremental closure.
const MAX_INCREMENTAL_RESOLUTION_ITEMS: u32 = GraphLimits::MAX_ROWS;
/// Maximum simultaneous parser/entity/key bytes before rows spill to typed `SQLite` staging.
const MAX_IN_MEMORY_GRAPH_WORK_BYTES: u64 = 512 * 1_024 * 1_024;
/// Maximum aggregate normalized graph rows admitted by one incremental closure.
const MAX_INCREMENTAL_GRAPH_ROWS: u64 = GraphLimits::MAX_ROWS as u64;
/// Maximum conservative graph bytes admitted before requesting a complete refresh.
const MAX_INCREMENTAL_GRAPH_BYTES: u64 = MAX_IN_MEMORY_GRAPH_WORK_BYTES;
/// Maximum persisted key bindings retained by one complete in-memory graph projection.
const MAX_GRAPH_KEY_BINDINGS: u64 = 8_000_000;
/// Conservative fixed bytes counted for each staged graph or binding row.
const STAGED_GRAPH_ROW_BYTES: u64 = 128;
/// Conservative bytes for one generated document relation and its retained indexes.
///
/// This covers the relation, occurrence, dependency, unresolved-reason map entry,
/// final reason tuple, and duplicate-validation digest; candidate text is added
/// separately from the already-counted Markdown fact bytes.
const DOCUMENT_PROJECTION_ROW_BYTES: u64 = STAGED_GRAPH_ROW_BYTES * 8;
/// Maximum graph-map rows processed between cooperative cancellation checks.
const GRAPH_WORK_CHECK_INTERVAL: usize = 256;
/// Borrowed entity references inserted per disposable staging call.
const GRAPH_STAGE_ENTITY_BATCH_SIZE: usize = 1_024;
/// Maximum normalized graph rows retained between disposable staging writes.
const GRAPH_STAGE_ROW_BATCH_SIZE: usize = 8_192;
/// Direct child prefix owned by disposable graph staging.
const GRAPH_STAGE_DIRECTORY_PREFIX: &str = "graph-stage-";
/// Stable cross-process lease protecting active staging and restart cleanup.
const GRAPH_STAGE_LEASE_FILE_NAME: &str = "repository-graph-stage.lock";
/// Typed disposable database inside every owned staging directory.
const GRAPH_STAGE_DATABASE_FILE_NAME: &str = "projectatlas.db";
/// Internal invariant failure for a staging owner already entering teardown.
const GRAPH_STAGE_OWNER_UNAVAILABLE: &str = "repository graph staging owner is unavailable";
/// Maximum persisted symbol-graph paths reconstructed between work checks.
const PERSISTED_GRAPH_PATHS_PER_CHUNK: usize = 256;
/// Stable fallback for a parser relation that omitted a usable display target.
const UNKNOWN_REFERENCE: &str = "unknown-reference";
/// Stable package manager for currently extracted Cargo manifest ownership.
const CARGO_PACKAGE_MANAGER: &str = "cargo";
/// Stable external namespace for the Rust standard-library distribution.
const RUST_TOOLCHAIN_SYSTEM: &str = "rust-toolchain";
/// Stable external namespace for explicitly qualified Node.js built-ins.
const NODE_SYSTEM: &str = "node";
/// Honest coverage diagnostic for fallback or structural relation extraction.
const PARTIAL_COVERAGE_REASON: &str = "parser does not prove complete relationship coverage";
/// External namespace for content-free configuration identities.
const CONFIGURATION_SYSTEM: &str = "configuration";
/// External namespace for static environment-variable identities.
const ENVIRONMENT_SYSTEM: &str = "environment-variable";
/// External namespace for content-free deployment platform identities.
const DEPLOYMENT_SYSTEM: &str = "deployment-platform";
/// Canonical provider owner for exact repository document targets.
const DOCUMENT_PATH_PROVIDER: &str = "projectatlas-document";
/// Canonical resolver language for repository-relative file and heading identities.
const DOCUMENT_PATH_LANGUAGE: &str = "repository-path";
/// Canonical resolver language for case-fold collision invalidation only.
const DOCUMENT_CASEFOLD_LANGUAGE: &str = "repository-path-casefold";
/// Honest content-free coverage diagnostic for bounded Markdown facts.
const DOCUMENT_PARTIAL_COVERAGE_REASON: &str =
    "markdown fact extraction reached a declared limit or unsupported structure";

/// One graph mutation staged outside the database writer transaction.
pub(super) enum RepositoryGraphMutation {
    /// Replace the complete normalized graph.
    Full,
    /// Replace the exact admitted source closure while retaining unrelated rows.
    AffectedPaths(Vec<String>),
}

/// Complete normalized graph and canonical-key rows waiting for publication.
pub(super) struct StagedRepositoryGraph {
    /// Project identity owning every staged graph fact.
    project: ProjectInstanceId,
    /// Full or affected-path replacement selected by the staging caller.
    mutation: RepositoryGraphMutation,
    /// Stable typed entities ready for normalized persistence.
    entities: Vec<GraphEntity>,
    /// Deduplicated logical relationships ready for persistence.
    relations: Vec<LogicalRelation>,
    /// Exact source occurrences retained separately from logical relations.
    occurrences: Vec<RelationOccurrence>,
    /// Parser and relation coverage facts for staged source paths.
    coverage: Vec<CoverageRecord>,
    /// Canonical resolution keys exported by staged entities.
    entity_exports: Vec<EntityResolutionKey>,
    /// Canonical dependency keys retained by staged relations.
    relation_dependencies: Vec<RelationDependencyKey>,
    /// Closed reasons retained only for unresolved canonical document relations.
    document_unresolved_reasons: Vec<(LogicalRelationKey, DocumentTargetUnresolvedReason)>,
    /// Effective scan policy used to recheck non-indexed document targets before commit.
    scan_policy: RootScanPolicy,
    /// Non-indexed target states observed while resolving document candidates.
    document_target_states: Vec<(String, DocumentTargetUnresolvedReason)>,
    /// Optional disposable database replacing the in-memory row vectors.
    database: Option<StagedGraphDatabase>,
    /// Conservative bytes retained until the parent publication completes.
    retained_bytes: u64,
}

/// File-backed graph rows staged outside the main database writer transaction.
struct StagedGraphDatabase {
    /// Open typed store copied into the main publication.
    store: Option<AtlasStore>,
    /// Owning directory removed after the store closes.
    directory: Option<TempDir>,
    /// Cross-process lease preventing restart cleanup while the stage is active.
    _lease: File,
}

impl StagedGraphDatabase {
    /// Return the live typed staging store.
    fn store(&self) -> Result<&AtlasStore, CliError> {
        self.store
            .as_ref()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }

    /// Return the live typed staging store mutably during preparation.
    fn store_mut(&mut self) -> Result<&mut AtlasStore, CliError> {
        self.store
            .as_mut()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }

    /// Return the live disposable staging directory.
    fn directory(&self) -> Result<&TempDir, CliError> {
        self.directory
            .as_ref()
            .ok_or_else(|| CliError::InvalidInput(GRAPH_STAGE_OWNER_UNAVAILABLE.to_string()))
    }
}

impl Drop for StagedGraphDatabase {
    fn drop(&mut self) {
        let prepared = self.store.take().is_some();
        let Some(directory) = self.directory.take() else {
            return;
        };
        let database_path = directory.path().join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let direct_database = fs::symlink_metadata(&database_path).is_ok_and(|metadata| {
            metadata.file_type().is_file() && !metadata.file_type().is_symlink()
        });
        if prepared
            && direct_database
            && remove_owned_graph_stage_payload(directory.path(), &database_path, None).is_ok()
        {
            drop(directory);
        } else {
            let _retained_path: PathBuf = directory.keep();
        }
    }
}

impl StagedRepositoryGraph {
    /// Return conservative retained bytes counted toward the parent staging budget.
    pub(super) const fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    /// Refuse publication if a consulted non-indexed target changed after resolution.
    pub(super) fn revalidate_document_targets(&self, root: &Path) -> Result<(), CliError> {
        if self.document_target_states.is_empty() {
            return Ok(());
        }
        let current = DocumentResolutionIndex::new(root, &[], &self.scan_policy)?;
        for (path, expected) in &self.document_target_states {
            if current.absent_reason(path)? != *expected {
                return Err(source_changed_during_derivation(root, path));
            }
        }
        Ok(())
    }

    /// Apply the complete staged graph through the parent publication transaction.
    pub(super) fn apply(
        &self,
        publication: &mut IndexPublicationGuard<'_>,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::Publication)?;
        if let Some(database) = &self.database {
            if !database.directory()?.path().is_dir() {
                return Err(CliError::InvalidInput(
                    "repository graph staging directory is unavailable".to_string(),
                ));
            }
            if !matches!(self.mutation, RepositoryGraphMutation::Full) {
                return Err(CliError::InvalidInput(
                    "database-backed graph staging only supports full replacement".to_string(),
                ));
            }
            publication.replace_repository_graph_from_staging(
                self.project,
                database.store()?,
                Some(control),
            )?;
            if !self.document_unresolved_reasons.is_empty() {
                publication.set_document_unresolved_reasons_controlled(
                    &self.document_unresolved_reasons,
                    control,
                )?;
            }
            control.check(IndexWorkStage::Publication)?;
            return Ok(());
        }
        match &self.mutation {
            RepositoryGraphMutation::Full => {
                publication.replace_repository_graph_with_resolution_keys(
                    self.project,
                    &self.entities,
                    &self.relations,
                    &self.occurrences,
                    &self.coverage,
                    &self.entity_exports,
                    &self.relation_dependencies,
                )?;
            }
            RepositoryGraphMutation::AffectedPaths(paths) => {
                publication.replace_repository_graph_for_paths_with_resolution_keys(
                    self.project,
                    paths,
                    &self.entities,
                    &self.relations,
                    &self.occurrences,
                    &self.coverage,
                    &self.entity_exports,
                    &self.relation_dependencies,
                )?;
            }
        }
        if !self.document_unresolved_reasons.is_empty() {
            publication.set_document_unresolved_reasons_controlled(
                &self.document_unresolved_reasons,
                control,
            )?;
        }
        control.check(IndexWorkStage::Publication)?;
        Ok(())
    }
}

/// Stage a complete repository graph from current parser output plus safe reused graphs.
pub(super) fn stage_full_repository_graph(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    nodes: &[Node],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    cleanup_abandoned_repository_graph_staging(store, root, control)?;
    let project = selected_project(store)?;
    let generation = next_generation(base_generation)?;
    let paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let graphs = complete_symbol_graphs(store, &paths, symbols, control)?;
    let document_facts = complete_markdown_facts(root, nodes, &graphs, symbols, control)?;
    control.check(IndexWorkStage::SymbolParsing)?;
    let configured_modules =
        super::module_resolution::load_configured_module_resolution(root, nodes, control)?;
    let packages = PackageIndex::from_graphs(&graphs)?;
    let entity_projection = build_entity_projection_with_config(
        project,
        generation,
        nodes,
        &graphs,
        &packages,
        &configured_modules,
        true,
        control,
    )?;
    let candidates = resolution_registry_from_exports(&entity_projection, control)?;
    enforce_resolution_staging_budget(&entity_projection, &candidates)?;
    let graph_work_bytes = symbols
        .retained_bytes
        .saturating_add(entity_projection.retained_bytes)
        .saturating_add(candidates.retained_bytes)
        .saturating_add(document_fact_map_retained_bytes(&document_facts))
        .saturating_add(document_projection_retained_bytes(&document_facts));
    if graph_work_bytes > MAX_IN_MEMORY_GRAPH_WORK_BYTES {
        finish_projection_in_database_with_documents(
            root,
            nodes,
            project,
            generation,
            &graphs,
            &document_facts,
            entity_projection,
            &candidates,
            scan_policy,
            control,
        )
    } else {
        finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            root,
            nodes,
            &document_facts,
            entity_projection,
            &candidates,
            scan_policy,
            control,
        )
    }
}

/// Stage one bounded dependency-aware graph closure for an incremental publication.
pub(super) fn stage_incremental_repository_graph(
    store: &AtlasStore,
    root: &Path,
    base_generation: IndexGeneration,
    expected_nodes: &[Node],
    direct_paths: &[String],
    scan_policy: &RootScanPolicy,
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let project = selected_project(store)?;
    let generation = next_generation(base_generation)?;
    let configured_modules =
        super::module_resolution::load_configured_module_resolution(root, expected_nodes, control)?;
    let direct_paths = direct_paths.iter().cloned().collect::<BTreeSet<_>>();
    enforce_incremental_count(
        root,
        "direct source paths",
        direct_paths.len(),
        &direct_paths,
    )?;
    let _direct_footprint =
        admitted_persisted_footprint(store, project, root, &direct_paths, control)?;

    let current_file_paths = expected_nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let manifest_paths = expected_nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File && is_cargo_manifest_path(&node.path))
        .map(|node| node.path.clone())
        .collect::<BTreeSet<_>>();
    let package_graph_paths = direct_paths
        .intersection(&current_file_paths)
        .cloned()
        .chain(manifest_paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    let package_graphs = complete_symbol_graphs(store, &package_graph_paths, symbols, control)?;
    let packages = PackageIndex::from_graphs(&package_graphs)?;
    let direct_graphs = package_graphs
        .iter()
        .filter(|graph| direct_paths.contains(&graph.path))
        .cloned()
        .collect::<Vec<_>>();

    let old_exports = store.repository_export_keys_for_paths(
        project,
        &direct_paths.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if old_exports.truncated {
        return Err(dependency_closure_limit(
            root,
            direct_paths.iter().cloned(),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut changed_keys = old_exports.rows.into_iter().collect::<BTreeSet<_>>();
    for path in &direct_paths {
        changed_keys.insert(document_file_resolution_key(project, path)?);
        changed_keys.insert(document_casefold_resolution_key(project, path)?);
    }
    for graph in &direct_graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        let projection = resolution_projection_with_config(
            project,
            packages.package_name(&graph.path),
            graph,
            &configured_modules,
        )?;
        changed_keys.extend(projection.source_keys().iter().cloned());
        for symbol in projection.symbol_keys() {
            changed_keys.extend(symbol.keys().iter().cloned());
        }
        for symbol in &graph.symbols {
            if symbol.kind == SymbolKind::Heading {
                changed_keys.insert(document_heading_resolution_key(
                    project,
                    &graph.path,
                    &symbol.signature,
                )?);
            }
        }
    }
    enforce_incremental_count(
        root,
        "old and new export keys",
        changed_keys.len(),
        &direct_paths,
    )?;

    let inbound = store.repository_affected_source_paths(
        project,
        &changed_keys.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if inbound.truncated {
        return Err(dependency_closure_limit(
            root,
            inbound.rows.iter().map(|path| path.as_str().to_string()),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut affected_paths = direct_paths;
    affected_paths.extend(inbound.rows.into_iter().map(String::from));
    enforce_incremental_count(
        root,
        "affected source paths",
        affected_paths.len(),
        &affected_paths,
    )?;
    control.check(IndexWorkStage::SymbolParsing)?;
    let persisted_footprint =
        admitted_persisted_footprint(store, project, root, &affected_paths, control)?;

    let affected_graph_paths = affected_paths
        .intersection(&current_file_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    let affected_graphs = complete_symbol_graphs(store, &affected_graph_paths, symbols, control)?;
    let document_facts =
        complete_markdown_facts(root, expected_nodes, &affected_graphs, symbols, control)?;
    let affected_nodes = expected_nodes
        .iter()
        .filter(|node| affected_paths.contains(&node.path))
        .cloned()
        .collect::<Vec<_>>();
    let entity_projection = build_entity_projection_with_config(
        project,
        generation,
        &affected_nodes,
        &affected_graphs,
        &packages,
        &configured_modules,
        false,
        control,
    )?;

    let mut dependency_keys = entity_projection
        .keys_by_graph
        .values()
        .flat_map(ResolutionKeyProjection::relation_keys)
        .flat_map(|relation| relation.keys().iter().cloned())
        .collect::<BTreeSet<_>>();
    dependency_keys.extend(document_dependency_keys(project, &document_facts)?);
    enforce_incremental_count(
        root,
        "affected dependency keys",
        dependency_keys.len(),
        &affected_paths,
    )?;
    let persisted = store.repository_resolution_candidates_for_keys(
        project,
        &dependency_keys.iter().cloned().collect::<Vec<_>>(),
        MAX_INCREMENTAL_RESOLUTION_ITEMS,
    )?;
    if persisted.truncated {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            usize::try_from(MAX_INCREMENTAL_RESOLUTION_ITEMS).unwrap_or(usize::MAX) + 1,
        ));
    }
    let mut candidates = resolution_registry_from_persisted(
        project,
        generation,
        persisted.rows,
        &affected_paths,
        control,
    )?;
    merge_resolution_registries(
        &mut candidates,
        resolution_registry_from_exports(&entity_projection, control)?,
        control,
    )?;
    enforce_incremental_projection_budget(
        root,
        &affected_paths,
        0,
        entity_projection
            .retained_bytes
            .saturating_add(candidates.retained_bytes)
            .saturating_add(document_fact_map_retained_bytes(&document_facts))
            .saturating_add(document_projection_retained_bytes(&document_facts)),
    )?;
    let staged = finish_projection_with_documents(
        project,
        generation,
        RepositoryGraphMutation::AffectedPaths(affected_paths.iter().cloned().collect()),
        &affected_graphs,
        root,
        expected_nodes,
        &document_facts,
        entity_projection,
        &candidates,
        scan_policy,
        control,
    )?;
    enforce_incremental_projection_limits(root, &affected_paths, persisted_footprint, &staged)?;
    Ok(staged)
}

/// Entity/key facts prepared before relation resolution.
struct EntityProjection {
    /// Entities keyed by their compact digest for collision-safe lookup.
    entity_by_digest: BTreeMap<String, GraphEntity>,
    /// File and symbol owners keyed by parser graph path.
    owners_by_graph: BTreeMap<String, GraphOwners>,
    /// Canonical resolution-key projections keyed by parser graph path.
    keys_by_graph: BTreeMap<String, ResolutionKeyProjection>,
    /// Canonical resolution keys exported by staged entities.
    entity_exports: Vec<EntityResolutionKey>,
    /// Conservative bytes retained by entities and export keys.
    retained_bytes: u64,
}

/// File and symbol entities associated with one parser graph.
struct GraphOwners {
    /// Digest of the file entity owning the parser graph.
    file_digest: String,
    /// Optional stable entity digest corresponding to each parser symbol row.
    symbol_digests: Vec<Option<String>>,
}

/// Borrowed per-graph symbol lookup used by every relation in that file.
struct GraphSymbolIndex<'graph> {
    /// Parser symbol indices grouped by exact source name.
    indices_by_name: BTreeMap<&'graph str, Vec<usize>>,
}

impl<'graph> GraphSymbolIndex<'graph> {
    /// Build one bounded name index instead of rescanning all symbols per relation.
    fn new(graph: &'graph SymbolGraph, control: &IndexWorkControl) -> Result<Self, CliError> {
        let mut indices_by_name = BTreeMap::new();
        for (index, symbol) in graph.symbols.iter().enumerate() {
            check_graph_work(control, index)?;
            indices_by_name
                .entry(symbol.name.as_str())
                .or_insert_with(Vec::new)
                .push(index);
        }
        Ok(Self { indices_by_name })
    }

    /// Return exact parser symbol rows for one source name.
    fn get(&self, name: &str) -> &[usize] {
        self.indices_by_name.get(name).map_or(&[], Vec::as_slice)
    }
}

/// One conservative additive relation derived from already bounded parser facts.
struct DerivedRelationFact {
    /// Additive graph relation kind.
    kind: ExtendedRelationKind,
    /// Parser-compatible source, target, span, context, and trust facts.
    relation: SymbolRelation,
    /// Resolution strategy for the derived target.
    target: DerivedRelationTarget,
}

/// Closed target classes accepted by additive relation projection.
enum DerivedRelationTarget {
    /// Resolve through the same typed provider keys as one parser relation.
    Parser {
        /// Canonical provider-owned dependency keys.
        keys: Vec<CanonicalResolutionKey>,
    },
    /// Resolve one statically visible repository-relative file path.
    RepositoryPath,
    /// Bind one content-free external namespace and identity.
    External {
        /// Closed external namespace.
        system: &'static str,
    },
}

/// Package names assigned by longest manifest-directory ownership.
struct PackageIndex {
    /// Cargo packages ordered from the most-specific repository root.
    packages: Vec<PackageOwner>,
}

/// One Cargo package and its owning repository prefix.
struct PackageOwner {
    /// Repository prefix owned by this package.
    root: String,
    /// Canonical package name from the manifest graph.
    name: String,
    /// Repository-relative manifest path proving ownership.
    manifest: String,
}

impl PackageIndex {
    /// Build deterministic longest-prefix package ownership from manifest graphs.
    fn from_graphs(graphs: &[impl Borrow<SymbolGraph>]) -> Result<Self, CliError> {
        let mut packages = Vec::new();
        for graph in graphs {
            let graph = graph.borrow();
            for symbol in &graph.symbols {
                if symbol.kind != SymbolKind::Package {
                    continue;
                }
                GraphIdentityText::new(symbol.name.clone()).map_err(invalid_graph_contract)?;
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?;
                let root = graph
                    .path
                    .rsplit_once('/')
                    .map_or(String::new(), |(parent, _manifest)| parent.to_string());
                packages.push(PackageOwner {
                    root,
                    name: symbol.name.clone(),
                    manifest: graph.path.clone(),
                });
            }
        }
        packages.sort_by(|left, right| {
            right
                .root
                .len()
                .cmp(&left.root.len())
                .then_with(|| left.root.cmp(&right.root))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.manifest.cmp(&right.manifest))
        });
        packages.dedup_by(|left, right| {
            left.root == right.root && left.name == right.name && left.manifest == right.manifest
        });
        Ok(Self { packages })
    }

    /// Return the most-specific Cargo package owning one repository path.
    fn package_name(&self, path: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|package| repository_path_belongs_to(path, &package.root))
            .map(|package| package.name.as_str())
    }
}

/// Return whether one path belongs to a normalized repository prefix.
fn repository_path_belongs_to(path: &str, root: &str) -> bool {
    root.is_empty()
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Return whether one normalized repository path is a Cargo package manifest.
fn is_cargo_manifest_path(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Qualify graph-only parent identity from the parser's existing containment rows.
///
/// Sorting and active-scope lookup are `O(n log n)`. Identity work is
/// `O(n * MAX_GRAPH_IDENTITY_BYTES)`: every retained parent is bounded, and an
/// overflowing candidate is hashed without materializing more than one bounded
/// parent plus one admitted component.
fn qualified_symbol_parents(
    graph: &SymbolGraph,
) -> Result<Vec<Option<GraphIdentityText>>, CliError> {
    let mut order = (0..graph.symbols.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (graph.symbols[index].line_start, index));
    let mut active_by_name = BTreeMap::<&str, Vec<usize>>::new();
    let mut qualified_names = vec![None::<GraphIdentityText>; graph.symbols.len()];
    let mut parents = vec![None; graph.symbols.len()];
    for index in order {
        let symbol = &graph.symbols[index];
        let name = source_symbol_identity(symbol.name.clone())?;
        let mut parent = symbol
            .parent
            .clone()
            .map(source_symbol_identity)
            .transpose()?;
        if let Some(immediate_parent) = parent.as_ref()
            && let Some(candidates) = active_by_name.get_mut(immediate_parent.as_str())
        {
            while candidates.last().is_some_and(|&candidate_index| {
                graph.symbols[candidate_index].line_end < symbol.line_end
            }) {
                candidates.pop();
            }
            if let Some(qualified_parent) = candidates
                .last()
                .and_then(|&candidate_index| qualified_names[candidate_index].clone())
            {
                parent = Some(qualified_parent);
            }
        }
        qualified_names[index] = Some(match parent.as_ref() {
            Some(parent) => qualified_symbol_identity(parent, &name)?,
            None => name,
        });
        parents[index] = parent;
        active_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(index);
    }
    Ok(parents)
}

/// Admit one parser-owned symbol identity outside the derived-scope namespace.
fn source_symbol_identity(value: String) -> Result<GraphIdentityText, CliError> {
    let identity = GraphIdentityText::new(value).map_err(invalid_graph_contract)?;
    if identity.as_str().starts_with(QUALIFIED_SYMBOL_SCOPE_PREFIX) {
        return Err(invalid_graph_contract(
            GraphContractError::InvalidIdentityText {
                reason: "source symbol identity uses the reserved derived-scope namespace",
            },
        ));
    }
    Ok(identity)
}

/// Derive one exact or compact qualified scope from validated components.
fn qualified_symbol_identity(
    parent: &GraphIdentityText,
    name: &GraphIdentityText,
) -> Result<GraphIdentityText, CliError> {
    const SEPARATOR: &str = "::";
    const DIGEST_DOMAIN: &str = "projectatlas.graph.qualified-symbol-scope.v1";
    let qualified_len = parent.as_str().len() + SEPARATOR.len() + name.as_str().len();
    if qualified_len <= MAX_GRAPH_IDENTITY_BYTES {
        let mut qualified = String::with_capacity(qualified_len);
        qualified.push_str(parent.as_str());
        qualified.push_str(SEPARATOR);
        qualified.push_str(name.as_str());
        return GraphIdentityText::new(qualified).map_err(invalid_graph_contract);
    }

    let compact_len =
        QUALIFIED_SYMBOL_SCOPE_PREFIX.len() + 64 + SEPARATOR.len() + name.as_str().len();
    if compact_len > MAX_GRAPH_IDENTITY_BYTES {
        return Err(invalid_graph_contract(
            GraphContractError::InvalidIdentityText {
                reason: "derived scope cannot retain its nearest admitted symbol name",
            },
        ));
    }
    let mut hasher = blake3::Hasher::new_derive_key(DIGEST_DOMAIN);
    hasher.update(parent.as_str().as_bytes());
    hasher.update(SEPARATOR.as_bytes());
    hasher.update(name.as_str().as_bytes());
    let digest = hasher.finalize().to_hex();
    let mut compact = String::with_capacity(compact_len);
    compact.push_str(QUALIFIED_SYMBOL_SCOPE_PREFIX);
    compact.push_str(digest.as_str());
    compact.push_str(SEPARATOR);
    compact.push_str(name.as_str());
    GraphIdentityText::new(compact).map_err(invalid_graph_contract)
}

/// Project file, symbol, package, and canonical export facts from parser graphs.
#[cfg(test)]
fn build_entity_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    include_project: bool,
    control: &IndexWorkControl,
) -> Result<EntityProjection, CliError> {
    build_entity_projection_with_config(
        project,
        generation,
        nodes,
        graphs,
        packages,
        &ConfiguredModuleResolution::default(),
        include_project,
        control,
    )
}

/// Project entity/key facts with one shared configured-module snapshot.
#[allow(clippy::too_many_arguments)]
fn build_entity_projection_with_config(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    configured_modules: &ConfiguredModuleResolution,
    include_project: bool,
    control: &IndexWorkControl,
) -> Result<EntityProjection, CliError> {
    let mut entity_by_digest = BTreeMap::new();
    let mut entity_exports = Vec::new();
    if include_project {
        insert_entity(
            &mut entity_by_digest,
            GraphEntity::new(project, EntitySelector::Project, generation)
                .map_err(invalid_graph_contract)?,
        )?;
    }
    for node in nodes {
        control.check(IndexWorkStage::SymbolParsing)?;
        let selector = match node.kind {
            NodeKind::Folder => EntitySelector::Folder {
                path: RepositoryNodePath::new(Path::new(&node.path))
                    .map_err(invalid_graph_contract)?,
            },
            NodeKind::File => EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&node.path))
                    .map_err(invalid_graph_contract)?,
            },
        };
        let entity =
            GraphEntity::new(project, selector, generation).map_err(invalid_graph_contract)?;
        if node.kind == NodeKind::File {
            for key in [
                document_file_resolution_key(project, &node.path)?,
                document_casefold_resolution_key(project, &node.path)?,
            ] {
                entity_exports.push(
                    EntityResolutionKey::new(entity.key().clone(), key)
                        .map_err(invalid_graph_contract)?,
                );
            }
        }
        insert_entity(&mut entity_by_digest, entity)?;
    }

    let mut owners_by_graph = BTreeMap::new();
    let mut keys_by_graph = BTreeMap::new();
    let mut retained_bytes = 0_u64;
    for graph in graphs {
        let graph = graph.borrow();
        control.check(IndexWorkStage::SymbolParsing)?;
        let file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&graph.path))
                    .map_err(invalid_graph_contract)?,
            },
            generation,
        )
        .map_err(invalid_graph_contract)?;
        let file_digest = file.key().digest().to_string();
        insert_entity(&mut entity_by_digest, file)?;
        let mut symbol_digests = Vec::with_capacity(graph.symbols.len());
        let qualified_parents = qualified_symbol_parents(graph)?;
        for (symbol, qualified_parent) in graph.symbols.iter().zip(qualified_parents) {
            control.check(IndexWorkStage::SymbolParsing)?;
            let entity = match symbol.kind {
                SymbolKind::Import | SymbolKind::Dependency | SymbolKind::Workspace => None,
                SymbolKind::Package => Some(
                    GraphEntity::new(
                        project,
                        EntitySelector::Package {
                            package: PackageSelector {
                                manager: GraphIdentityText::new(CARGO_PACKAGE_MANAGER)
                                    .map_err(invalid_graph_contract)?,
                                name: GraphIdentityText::new(symbol.name.clone())
                                    .map_err(invalid_graph_contract)?,
                                manifest: RepositoryFilePath::new(Path::new(&graph.path))
                                    .map_err(invalid_graph_contract)?,
                            },
                        },
                        generation,
                    )
                    .map_err(invalid_graph_contract)?,
                ),
                _ => Some(
                    GraphEntity::new(
                        project,
                        EntitySelector::Symbol {
                            symbol: SymbolSelector {
                                file: RepositoryFilePath::new(Path::new(&graph.path))
                                    .map_err(invalid_graph_contract)?,
                                name: GraphIdentityText::new(symbol.name.clone())
                                    .map_err(invalid_graph_contract)?,
                                kind: symbol.kind,
                                parent: qualified_parent,
                                signature: GraphIdentityText::new(
                                    if symbol.signature.trim().is_empty() {
                                        symbol.name.clone()
                                    } else {
                                        symbol.signature.trim().to_string()
                                    },
                                )
                                .map_err(invalid_graph_contract)?,
                            },
                        },
                        generation,
                    )
                    .map_err(invalid_graph_contract)?,
                ),
            };
            let entity_digest = entity
                .as_ref()
                .map(|entity| entity.key().digest().to_string());
            if let Some(entity) = entity {
                if symbol.kind == SymbolKind::Heading {
                    entity_exports.push(
                        EntityResolutionKey::new(
                            entity.key().clone(),
                            document_heading_resolution_key(
                                project,
                                &graph.path,
                                &symbol.signature,
                            )?,
                        )
                        .map_err(invalid_graph_contract)?,
                    );
                }
                insert_entity(&mut entity_by_digest, entity)?;
            }
            symbol_digests.push(entity_digest);
        }
        let resolution = resolution_projection_with_config(
            project,
            packages.package_name(&graph.path),
            graph,
            configured_modules,
        )?;
        let file = entity_by_digest
            .get(&file_digest)
            .ok_or_else(|| CliError::InvalidInput("graph file owner was not staged".to_string()))?;
        for key in resolution.source_keys() {
            entity_exports.push(
                EntityResolutionKey::new(file.key().clone(), key.clone())
                    .map_err(invalid_graph_contract)?,
            );
        }
        for symbol_keys in resolution.symbol_keys() {
            let Some(entity) = symbol_digests
                .get(symbol_keys.symbol_index())
                .and_then(Option::as_ref)
                .and_then(|digest| entity_by_digest.get(digest))
            else {
                continue;
            };
            for key in symbol_keys.keys() {
                entity_exports.push(
                    EntityResolutionKey::new(entity.key().clone(), key.clone())
                        .map_err(invalid_graph_contract)?,
                );
            }
        }
        retained_bytes = retained_bytes.saturating_add(resolution_retained_bytes(&resolution));
        owners_by_graph.insert(
            graph.path.clone(),
            GraphOwners {
                file_digest,
                symbol_digests,
            },
        );
        keys_by_graph.insert(graph.path.clone(), resolution);
    }
    sort_dedup_exports(&mut entity_exports);
    enforce_key_binding_limit(entity_exports.len())?;
    retained_bytes = entity_by_digest
        .values()
        .fold(retained_bytes, |bytes, entity| {
            bytes.saturating_add(entity_retained_bytes(entity))
        })
        .saturating_add(
            STAGED_GRAPH_ROW_BYTES
                .saturating_mul(u64::try_from(entity_exports.len()).unwrap_or(u64::MAX)),
        );
    enforce_resolution_registry_budget(retained_bytes)?;
    Ok(EntityProjection {
        entity_by_digest,
        owners_by_graph,
        keys_by_graph,
        entity_exports,
        retained_bytes,
    })
}

/// Acquire the one project-local graph-staging lease without waiting.
fn try_graph_stage_lease(staging_parent: &Path) -> Result<Option<File>, CliError> {
    let path = staging_parent.join(GRAPH_STAGE_LEASE_FILE_NAME);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.file_type().is_file() || metadata.file_type().is_symlink())
    {
        return Err(CliError::InvalidInput(format!(
            "repository graph staging lease is not a direct file: {}",
            normalize_native_path_display(&path)
        )));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(source)) => Err(CliError::Io { path, source }),
    }
}

/// Remove inactive disposable graph stages owned by the selected project.
pub(super) fn cleanup_abandoned_repository_graph_staging(
    store: &AtlasStore,
    root: &Path,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    cleanup_abandoned_graph_staging(root, selected_project(store)?, control)
}

/// Remove only validated, inactive disposable graph stages left by an earlier process.
fn cleanup_abandoned_graph_staging(
    root: &Path,
    project: ProjectInstanceId,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    control.check(IndexWorkStage::Publication)?;
    let staging_parent = root.join(".projectatlas");
    if !staging_parent.is_dir() {
        return Ok(());
    }
    let Some(_lease) = try_graph_stage_lease(&staging_parent)? else {
        return Ok(());
    };
    cleanup_abandoned_graph_staging_while_locked(&staging_parent, root, project, control)
}

/// Remove stage payloads while retaining the validated ownership database as a crash marker.
fn remove_owned_graph_stage_payload(
    stage: &Path,
    database_path: &Path,
    control: Option<&IndexWorkControl>,
) -> Result<(), CliError> {
    let entries = fs::read_dir(stage).map_err(|source| CliError::Io {
        path: stage.to_path_buf(),
        source,
    })?;
    for entry in entries {
        if let Some(control) = control {
            control.check(IndexWorkStage::Publication)?;
        }
        let entry = entry.map_err(|source| CliError::Io {
            path: stage.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path == database_path {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })?;
        let result = if metadata.file_type().is_symlink() {
            remove_graph_stage_symlink(&path)
        } else if metadata.file_type().is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

/// Remove only a graph-stage symlink leaf, never its target.
#[cfg(windows)]
fn remove_graph_stage_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path).or_else(|_directory_error| fs::remove_file(path))
}

/// Remove only a graph-stage symlink leaf, never its target.
#[cfg(not(windows))]
fn remove_graph_stage_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

/// Remove direct child stages whose typed database binds the exact project.
fn cleanup_abandoned_graph_staging_while_locked(
    staging_parent: &Path,
    root: &Path,
    project: ProjectInstanceId,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let entries = fs::read_dir(staging_parent).map_err(|source| CliError::Io {
        path: staging_parent.to_path_buf(),
        source,
    })?;
    for entry in entries {
        control.check(IndexWorkStage::Publication)?;
        let entry = entry.map_err(|source| CliError::Io {
            path: staging_parent.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.starts_with(GRAPH_STAGE_DIRECTORY_PREFIX)
            || file_name.len() == GRAPH_STAGE_DIRECTORY_PREFIX.len()
        {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let database_path = path.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let database_metadata = match fs::symlink_metadata(&database_path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let _remove_empty_shell = fs::remove_dir(&path);
                continue;
            }
            Err(_) => continue,
        };
        if !database_metadata.file_type().is_file() || database_metadata.file_type().is_symlink() {
            continue;
        }
        let owned = AtlasStore::repository_graph_staging_belongs_to(&database_path, root, project)
            .unwrap_or(false);
        if !owned {
            continue;
        }
        control.check(IndexWorkStage::Publication)?;
        remove_owned_graph_stage_payload(&path, &database_path, Some(control))?;
        control.check(IndexWorkStage::Publication)?;
        fs::remove_file(&database_path).map_err(|source| CliError::Io {
            path: database_path,
            source,
        })?;
        fs::remove_dir(&path).map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

/// Spill a large full graph to typed disposable `SQLite` rows before main publication.
fn finish_projection_in_database_with_documents(
    root: &Path,
    nodes: &[Node],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    document_facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let document_index = DocumentResolutionIndex::new(root, nodes, scan_policy)?;
    let staging_parent = root.join(".projectatlas");
    fs::create_dir_all(&staging_parent).map_err(|source| CliError::Io {
        path: staging_parent.clone(),
        source,
    })?;
    let lease = try_graph_stage_lease(&staging_parent)?.ok_or_else(|| {
        CliError::InvalidInput(
            "another repository graph staging operation is active for this project".to_string(),
        )
    })?;
    cleanup_abandoned_graph_staging_while_locked(&staging_parent, root, project, control)?;
    let directory = TempDirBuilder::new()
        .prefix(GRAPH_STAGE_DIRECTORY_PREFIX)
        .tempdir_in(&staging_parent)
        .map_err(|source| CliError::Io {
            path: staging_parent,
            source,
        })?;
    let mut database = StagedGraphDatabase {
        store: None,
        directory: Some(directory),
        _lease: lease,
    };
    let database_path = database
        .directory()?
        .path()
        .join(GRAPH_STAGE_DATABASE_FILE_NAME);
    database.store = Some(AtlasStore::create_repository_graph_staging(
        &database_path,
        root,
        project,
    )?);
    database.store_mut()?.replace_scan(nodes)?;
    {
        let mut staging = database
            .store_mut()?
            .begin_repository_graph_staging(project, generation)?;
        let mut entity_batch = Vec::with_capacity(GRAPH_STAGE_ENTITY_BATCH_SIZE);
        for entity in entities.entity_by_digest.values() {
            entity_batch.push(entity);
            if entity_batch.len() == GRAPH_STAGE_ENTITY_BATCH_SIZE {
                control.check(IndexWorkStage::Publication)?;
                staging.append_entity_refs(&entity_batch)?;
                entity_batch.clear();
            }
        }
        if !entity_batch.is_empty() {
            staging.append_entity_refs(&entity_batch)?;
            entity_batch.clear();
        }
        staging.append_batch(&[], &[], &[], &[], &entities.entity_exports, &[])?;
        let mut staged_rows = ProjectedGraphRows::default();
        for graph in graphs {
            let graph = graph.borrow();
            let rows = project_graph_rows(
                project,
                generation,
                graph,
                document_facts.get(&graph.path).map(Cow::as_ref),
                &document_index,
                &mut entities,
                candidates,
                control,
            )?;
            staged_rows.append(rows);
            if staged_rows.row_count() < GRAPH_STAGE_ROW_BATCH_SIZE {
                continue;
            }
            staging.append_batch(
                &staged_rows.external_entities,
                &staged_rows.relations,
                &staged_rows.occurrences,
                &staged_rows.coverage,
                &[],
                &staged_rows.relation_dependencies,
            )?;
            if !staged_rows.document_unresolved_reasons.is_empty() {
                staging.set_document_unresolved_reasons_controlled(
                    &staged_rows.document_unresolved_reasons,
                    control,
                )?;
            }
            staged_rows.clear();
        }
        if !staged_rows.is_empty() {
            staging.append_batch(
                &staged_rows.external_entities,
                &staged_rows.relations,
                &staged_rows.occurrences,
                &staged_rows.coverage,
                &[],
                &staged_rows.relation_dependencies,
            )?;
            if !staged_rows.document_unresolved_reasons.is_empty() {
                staging.set_document_unresolved_reasons_controlled(
                    &staged_rows.document_unresolved_reasons,
                    control,
                )?;
            }
        }
        staging.complete()?;
    }
    database.store()?.checkpoint_repository_graph_staging()?;
    database.store()?.begin_index_read_snapshot()?;
    let _staged_generation = database.store()?.repository_graph_generation()?;
    let document_target_states = document_index.observed_absent_states();
    let retained_bytes = normalize_native_path_display(&database_path).len() as u64
        + document_target_states
            .iter()
            .map(|(path, _reason)| path.len() as u64 + STAGED_GRAPH_ROW_BYTES)
            .sum::<u64>();
    Ok(StagedRepositoryGraph {
        project,
        mutation: RepositoryGraphMutation::Full,
        entities: Vec::new(),
        relations: Vec::new(),
        occurrences: Vec::new(),
        coverage: Vec::new(),
        entity_exports: Vec::new(),
        relation_dependencies: Vec::new(),
        document_unresolved_reasons: Vec::new(),
        scan_policy: scan_policy.clone(),
        document_target_states,
        database: Some(database),
        retained_bytes,
    })
}

/// Test-only compatibility wrapper for graph fixtures without Markdown facts.
#[cfg(test)]
fn finish_projection_in_database(
    root: &Path,
    nodes: &[Node],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    finish_projection_in_database_with_documents(
        root,
        nodes,
        project,
        generation,
        graphs,
        &BTreeMap::new(),
        entities,
        candidates,
        scan_policy,
        control,
    )
}

/// Resolve staged relationships and finish one complete normalized graph batch.
fn finish_projection_with_documents(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    mutation: RepositoryGraphMutation,
    graphs: &[impl Borrow<SymbolGraph>],
    root: &Path,
    nodes: &[Node],
    document_facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    scan_policy: &RootScanPolicy,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let document_index = DocumentResolutionIndex::new(root, nodes, scan_policy)?;
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut coverage = Vec::new();
    let mut document_unresolved_reasons = BTreeMap::new();
    for graph in graphs {
        let graph = graph.borrow();
        let rows = project_graph_rows(
            project,
            generation,
            graph,
            document_facts.get(&graph.path).map(Cow::as_ref),
            &document_index,
            &mut entities,
            candidates,
            control,
        )?;
        for (relation_index, relation) in rows.relations.into_iter().enumerate() {
            insert_relation(
                &mut relations_by_digest,
                relation,
                &graph.path,
                "projected",
                relation_index,
            )?;
        }
        occurrences.extend(rows.occurrences);
        relation_dependencies.extend(rows.relation_dependencies);
        for (key, reason) in rows.document_unresolved_reasons {
            insert_document_unresolved_reason(&mut document_unresolved_reasons, key, reason)?;
        }
        coverage.extend(rows.coverage);
        entities.retained_bytes = rows
            .external_entities
            .iter()
            .fold(entities.retained_bytes, |bytes, entity| {
                bytes.saturating_add(entity_retained_bytes(entity))
            });
        for entity in rows.external_entities {
            insert_entity(&mut entities.entity_by_digest, entity)?;
        }
    }
    let mut relations = relations_by_digest.into_values().collect::<Vec<_>>();
    relations.sort_by(|left, right| left.key().digest().cmp(right.key().digest()));
    occurrences.sort_by(|left, right| {
        left.relation()
            .digest()
            .cmp(right.relation().digest())
            .then_with(|| left.file().as_str().cmp(right.file().as_str()))
            .then_with(|| left.span().start_line().cmp(&right.span().start_line()))
            .then_with(|| left.span().start_column().cmp(&right.span().start_column()))
    });
    occurrences.dedup();
    sort_dedup_dependencies(&mut relation_dependencies);
    enforce_key_binding_limit(relation_dependencies.len())?;
    let added_rows = relations
        .len()
        .saturating_add(occurrences.len())
        .saturating_add(coverage.len())
        .saturating_add(relation_dependencies.len());
    let added_rows = added_rows.saturating_add(document_unresolved_reasons.len());
    let document_target_states = document_index.observed_absent_states();
    entities.retained_bytes = entities
        .retained_bytes
        .saturating_add(
            STAGED_GRAPH_ROW_BYTES.saturating_mul(u64::try_from(added_rows).unwrap_or(u64::MAX)),
        )
        .saturating_add(
            document_target_states
                .iter()
                .fold(0_u64, |bytes, (path, _reason)| {
                    bytes
                        .saturating_add(STAGED_GRAPH_ROW_BYTES)
                        .saturating_add(path.len() as u64)
                }),
        );
    Ok(StagedRepositoryGraph {
        project,
        mutation,
        entities: entities.entity_by_digest.into_values().collect(),
        relations,
        occurrences,
        coverage,
        entity_exports: entities.entity_exports,
        relation_dependencies,
        document_unresolved_reasons: document_unresolved_reasons.into_values().collect(),
        scan_policy: scan_policy.clone(),
        document_target_states,
        database: None,
        retained_bytes: entities.retained_bytes,
    })
}

/// Test-only compatibility wrapper for graph fixtures without Markdown facts.
#[cfg(test)]
fn finish_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    mutation: RepositoryGraphMutation,
    graphs: &[impl Borrow<SymbolGraph>],
    entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let scan_policy = RootScanPolicy::discover(Path::new("."), &ScanOptions::default(), control)
        .map_err(|source| CliError::InvalidInput(source.to_string()))?;
    finish_projection_with_documents(
        project,
        generation,
        mutation,
        graphs,
        Path::new("."),
        &[],
        &BTreeMap::new(),
        entities,
        candidates,
        &scan_policy,
        control,
    )
}

/// One graph's normalized rows, bounded by the parser graph already in memory.
#[derive(Default)]
struct ProjectedGraphRows {
    /// Deduplicated logical relations owned by the graph.
    relations: Vec<LogicalRelation>,
    /// Exact supporting source occurrences.
    occurrences: Vec<RelationOccurrence>,
    /// Coverage rows for the graph.
    coverage: Vec<CoverageRecord>,
    /// Content-free external entities referenced by the graph.
    external_entities: Vec<GraphEntity>,
    /// Canonical dependency keys retained by the graph relations.
    relation_dependencies: Vec<RelationDependencyKey>,
    /// Closed reasons for unresolved canonical document relations.
    document_unresolved_reasons: Vec<(LogicalRelationKey, DocumentTargetUnresolvedReason)>,
}

impl ProjectedGraphRows {
    /// Append one graph while retaining a single bounded staging batch.
    fn append(&mut self, rows: Self) {
        self.relations.extend(rows.relations);
        self.occurrences.extend(rows.occurrences);
        self.coverage.extend(rows.coverage);
        self.external_entities.extend(rows.external_entities);
        self.relation_dependencies
            .extend(rows.relation_dependencies);
        self.document_unresolved_reasons
            .extend(rows.document_unresolved_reasons);
    }

    /// Return aggregate normalized rows retained by this staging batch.
    fn row_count(&self) -> usize {
        self.relations
            .len()
            .saturating_add(self.occurrences.len())
            .saturating_add(self.coverage.len())
            .saturating_add(self.external_entities.len())
            .saturating_add(self.relation_dependencies.len())
            .saturating_add(self.document_unresolved_reasons.len())
    }

    /// Return whether the staging batch is empty.
    fn is_empty(&self) -> bool {
        self.row_count() == 0
    }

    /// Release all successfully staged rows while preserving allocated capacity.
    fn clear(&mut self) {
        self.relations.clear();
        self.occurrences.clear();
        self.coverage.clear();
        self.external_entities.clear();
        self.relation_dependencies.clear();
        self.document_unresolved_reasons.clear();
    }
}

/// Exact staged file inventory plus platform-aware case and root checks.
struct DocumentResolutionIndex<'a> {
    /// Selected source root used only for bounded filesystem checks.
    root: PathBuf,
    /// Canonical selected root used to reject symlink escape.
    canonical_root: PathBuf,
    /// Exact case-preserving staged identities.
    kinds_by_path: HashMap<&'a str, NodeKind>,
    /// Unicode-lowercase identity counts used to refuse guesses and collisions.
    casefold_path_counts: HashMap<String, u32>,
    /// Effective repository admission policy shared with full and incremental scans.
    scan_policy: &'a RootScanPolicy,
    /// Consulted non-indexed states, memoized for repeated high-fan-out references.
    absent_states: RefCell<BTreeMap<String, DocumentTargetUnresolvedReason>>,
}

impl<'a> DocumentResolutionIndex<'a> {
    /// Build one bounded exact-identity view from the staged source inventory.
    fn new(
        root: &Path,
        nodes: &'a [Node],
        scan_policy: &'a RootScanPolicy,
    ) -> Result<Self, CliError> {
        let canonical_root = fs::canonicalize(root).map_err(|source| CliError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let kinds_by_path = nodes
            .iter()
            .map(|node| (node.path.as_str(), node.kind))
            .collect::<HashMap<_, _>>();
        let mut casefold_path_counts = HashMap::new();
        for node in nodes {
            let count = casefold_path_counts
                .entry(node.path.to_lowercase())
                .or_insert(0_u32);
            *count = count.saturating_add(1);
        }
        let retained_bytes = nodes.iter().fold(0_u64, |bytes, node| {
            bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES.saturating_mul(2))
                .saturating_add(node.path.len() as u64)
        });
        enforce_resolution_registry_budget(retained_bytes)?;
        Ok(Self {
            root: root.to_path_buf(),
            canonical_root,
            kinds_by_path,
            casefold_path_counts,
            scan_policy,
            absent_states: RefCell::new(BTreeMap::new()),
        })
    }

    /// Return a closed unresolved reason, or `None` for one exact admitted file.
    fn unresolved_reason(
        &self,
        path: &str,
    ) -> Result<Option<DocumentTargetUnresolvedReason>, CliError> {
        let casefold_count = self
            .casefold_path_counts
            .get(&path.to_lowercase())
            .copied()
            .unwrap_or(0);
        if casefold_count > 1 {
            return Ok(Some(DocumentTargetUnresolvedReason::CaseConflict));
        }
        Ok(match self.kinds_by_path.get(path).copied() {
            Some(NodeKind::File) => None,
            Some(NodeKind::Folder) => Some(DocumentTargetUnresolvedReason::Unsupported),
            None if casefold_count == 1 => Some(DocumentTargetUnresolvedReason::CaseConflict),
            None => Some(self.absent_reason(path)?),
        })
    }

    /// Distinguish absent source from excluded or escaping filesystem state.
    fn absent_reason(&self, path: &str) -> Result<DocumentTargetUnresolvedReason, CliError> {
        if let Some(reason) = self.absent_states.borrow().get(path).copied() {
            return Ok(reason);
        }
        let native = self.root.join(Path::new(path));
        let reason = if self
            .scan_policy
            .excludes_path(&native)
            .map_err(|source| CliError::InvalidInput(source.to_string()))?
        {
            DocumentTargetUnresolvedReason::Ignored
        } else {
            match fs::symlink_metadata(&native) {
                Ok(metadata) if metadata.file_type().is_dir() => {
                    DocumentTargetUnresolvedReason::Unsupported
                }
                Ok(_metadata) => match fs::canonicalize(&native) {
                    Ok(canonical) if !canonical.starts_with(&self.canonical_root) => {
                        DocumentTargetUnresolvedReason::OutsideRoot
                    }
                    Ok(_canonical) => DocumentTargetUnresolvedReason::Unsupported,
                    Err(_source) => DocumentTargetUnresolvedReason::Unsupported,
                },
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    self.missing_or_escaping_ancestor(&native)
                }
                Err(_source) => DocumentTargetUnresolvedReason::Unsupported,
            }
        };
        self.absent_states
            .borrow_mut()
            .insert(path.to_string(), reason);
        Ok(reason)
    }

    /// Return the bounded states actually consulted during document resolution.
    fn observed_absent_states(&self) -> Vec<(String, DocumentTargetUnresolvedReason)> {
        self.absent_states
            .borrow()
            .iter()
            .map(|(path, reason)| (path.clone(), *reason))
            .collect()
    }

    /// Check the nearest existing ancestor so a missing child cannot hide symlink escape.
    fn missing_or_escaping_ancestor(&self, native: &Path) -> DocumentTargetUnresolvedReason {
        let mut ancestor = native.parent();
        while let Some(path) = ancestor {
            if !path.starts_with(&self.root) {
                break;
            }
            match fs::canonicalize(path) {
                Ok(canonical) if !canonical.starts_with(&self.canonical_root) => {
                    return DocumentTargetUnresolvedReason::OutsideRoot;
                }
                Ok(_canonical) => return DocumentTargetUnresolvedReason::Missing,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = path.parent();
                }
                Err(_source) => return DocumentTargetUnresolvedReason::Unsupported,
            }
        }
        DocumentTargetUnresolvedReason::Missing
    }
}

/// One resolved or typed-unresolved document candidate plus invalidation keys.
struct DocumentResolutionOutcome {
    /// Existing normalized graph resolution envelope.
    resolution: RelationResolution,
    /// Closed reason required only for unresolved document relations.
    unresolved_reason: Option<DocumentTargetUnresolvedReason>,
    /// Exact file and optional heading keys that invalidate this relation.
    dependencies: Vec<CanonicalResolutionKey>,
}

/// Project one Markdown fact batch into canonical document relations and coverage.
#[allow(clippy::too_many_arguments)]
fn project_document_rows(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graph: &SymbolGraph,
    facts: &MarkdownFacts,
    owners: &GraphOwners,
    document_index: &DocumentResolutionIndex<'_>,
    candidates: &ProjectResolutionRegistry,
    staged_entities: &BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ProjectedGraphRows, CliError> {
    let file_source = staged_entities.get(&owners.file_digest).ok_or_else(|| {
        CliError::InvalidInput("document graph file owner was not staged".to_string())
    })?;
    let completeness = match facts.coverage.completeness {
        MarkdownFactCompleteness::Complete => Completeness::Complete,
        MarkdownFactCompleteness::Partial => Completeness::Partial,
    };
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut unresolved_reasons = BTreeMap::new();
    for (candidate_index, candidate) in facts.link_candidates.iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        if normalize_document_target(&graph.path, &candidate.selector)
            .is_ok_and(|target| target.path == graph.path && target.fragment.is_none())
        {
            continue;
        }
        let source = file_source;
        let outcome = resolve_document_candidate(
            project,
            &graph.path,
            &candidate.selector,
            document_index,
            candidates,
            staged_entities,
            control,
        )?;
        if matches!(
            &outcome.resolution,
            RelationResolution::Resolved { target, .. } if target == source.key()
        ) {
            continue;
        }
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            outcome.resolution,
            ConfidenceClass::High,
            completeness,
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "document",
            candidate_index,
        )?;
        if let Some(reason) = outcome.unresolved_reason {
            insert_document_unresolved_reason(
                &mut unresolved_reasons,
                relation.key().clone(),
                reason,
            )?;
        }
        let start_line = u32::try_from(candidate.source.line_start).map_err(|error| {
            CliError::InvalidInput(format!("document source line exceeds graph range: {error}"))
        })?;
        let end_line = u32::try_from(candidate.source.line_end).map_err(|error| {
            CliError::InvalidInput(format!("document end line exceeds graph range: {error}"))
        })?;
        let start_column = u32::try_from(candidate.source.column_start).map_err(|error| {
            CliError::InvalidInput(format!(
                "document source column exceeds graph range: {error}"
            ))
        })?;
        let end_column = u32::try_from(candidate.source.column_end).map_err(|error| {
            CliError::InvalidInput(format!("document end column exceeds graph range: {error}"))
        })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(start_line, start_column, end_line, end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        for key in outcome.dependencies {
            relation_dependencies.push(
                RelationDependencyKey::new(relation.key().clone(), key)
                    .map_err(invalid_graph_contract)?,
            );
        }
    }
    Ok(ProjectedGraphRows {
        relations: relations_by_digest.into_values().collect(),
        occurrences,
        coverage: vec![document_coverage(&graph.path, facts, generation)?],
        external_entities: Vec::new(),
        relation_dependencies,
        document_unresolved_reasons: unresolved_reasons.into_values().collect(),
    })
}

/// Resolve one normalized document target through exact staged/persisted identities.
#[allow(clippy::too_many_arguments)]
fn resolve_document_candidate(
    project: ProjectInstanceId,
    document_path: &str,
    selector: &str,
    document_index: &DocumentResolutionIndex<'_>,
    candidates: &ProjectResolutionRegistry,
    staged_entities: &BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<DocumentResolutionOutcome, CliError> {
    let target = match normalize_document_target(document_path, selector) {
        Ok(target) => target,
        Err(reason) => {
            return unresolved_document_outcome(selector, reason, Vec::new());
        }
    };
    let file_key = document_file_resolution_key(project, &target.path)?;
    let mut dependencies = vec![
        file_key.clone(),
        document_casefold_resolution_key(project, &target.path)?,
    ];
    let heading_key = target
        .fragment
        .as_deref()
        .map(|fragment| document_heading_resolution_key(project, &target.path, fragment))
        .transpose()?;
    if let Some(key) = heading_key.as_ref() {
        dependencies.push(key.clone());
    }
    if let Some(reason) = document_index.unresolved_reason(&target.path)? {
        return unresolved_document_outcome(selector, reason, dependencies);
    }
    if let Some(key) = heading_key.as_ref() {
        let matches = registry_resolution_matches(
            std::slice::from_ref(key),
            candidates,
            staged_entities,
            control,
        )?;
        match matches.count {
            0 => {
                return unresolved_document_outcome(
                    selector,
                    DocumentTargetUnresolvedReason::Missing,
                    dependencies,
                );
            }
            1 => {
                let target = matches.first.ok_or_else(|| {
                    CliError::InvalidInput("resolved document heading disappeared".to_string())
                })?;
                return Ok(DocumentResolutionOutcome {
                    resolution: RelationResolution::resolved(target)
                        .map_err(invalid_graph_contract)?,
                    unresolved_reason: None,
                    dependencies,
                });
            }
            _ => {
                return unresolved_document_outcome(
                    selector,
                    DocumentTargetUnresolvedReason::CaseConflict,
                    dependencies,
                );
            }
        }
    }
    let matches = registry_resolution_matches(
        std::slice::from_ref(&file_key),
        candidates,
        staged_entities,
        control,
    )?;
    match matches.count {
        1 => Ok(DocumentResolutionOutcome {
            resolution: RelationResolution::resolved(matches.first.ok_or_else(|| {
                CliError::InvalidInput("resolved document file disappeared".to_string())
            })?)
            .map_err(invalid_graph_contract)?,
            unresolved_reason: None,
            dependencies,
        }),
        0 => Err(CliError::InvalidInput(format!(
            "admitted document target lacked its exact graph identity: {}",
            target.path
        ))),
        _ => unresolved_document_outcome(
            selector,
            DocumentTargetUnresolvedReason::CaseConflict,
            dependencies,
        ),
    }
}

/// Construct one privacy-safe unresolved document outcome.
fn unresolved_document_outcome(
    selector: &str,
    reason: DocumentTargetUnresolvedReason,
    dependencies: Vec<CanonicalResolutionKey>,
) -> Result<DocumentResolutionOutcome, CliError> {
    Ok(DocumentResolutionOutcome {
        resolution: RelationResolution::Unresolved {
            reference: GraphIdentityText::new(selector).map_err(invalid_graph_contract)?,
        },
        unresolved_reason: Some(reason),
        dependencies,
    })
}

/// Project bounded Markdown fact coverage into the existing relation coverage table.
fn document_coverage(
    path: &str,
    facts: &MarkdownFacts,
    generation: IndexGeneration,
) -> Result<CoverageRecord, CliError> {
    let scope = CoverageScope::Path {
        path: RepositoryNodePath::new(Path::new(path)).map_err(invalid_graph_contract)?,
    };
    let covered = u64::try_from(facts.link_candidates.len()).unwrap_or(u64::MAX);
    let reached_limit = facts.coverage.limits.first().map(|limit| match limit {
        MarkdownFactLimit::HeadingCount | MarkdownFactLimit::CandidateCount => GraphLimitKind::Rows,
        MarkdownFactLimit::InputBytes
        | MarkdownFactLimit::LabelBytes
        | MarkdownFactLimit::SelectorBytes
        | MarkdownFactLimit::EvidenceBytes => GraphLimitKind::IntermediateBytes,
    });
    let relation = Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents));
    match facts.coverage.completeness {
        MarkdownFactCompleteness::Complete if covered == 0 => CoverageRecord::new(
            scope,
            relation,
            CoverageState::NoCandidates,
            0,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Complete => CoverageRecord::new(
            scope,
            relation,
            CoverageState::Complete,
            covered,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Partial if covered > 0 => CoverageRecord::new(
            scope,
            relation,
            CoverageState::Partial,
            covered,
            1,
            generation,
            Some(
                GraphIdentityText::new(DOCUMENT_PARTIAL_COVERAGE_REASON)
                    .map_err(invalid_graph_contract)?,
            ),
            reached_limit,
        )
        .map_err(invalid_graph_contract),
        MarkdownFactCompleteness::Partial => CoverageRecord::new(
            scope,
            relation,
            if facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::InputBytes)
            {
                CoverageState::Oversized
            } else {
                CoverageState::Failed
            },
            0,
            1,
            generation,
            Some(
                GraphIdentityText::new(DOCUMENT_PARTIAL_COVERAGE_REASON)
                    .map_err(invalid_graph_contract)?,
            ),
            reached_limit,
        )
        .map_err(invalid_graph_contract),
    }
}

/// Deduplicate unresolved reasons while rejecting one logical-key contradiction.
fn insert_document_unresolved_reason(
    reasons: &mut BTreeMap<String, (LogicalRelationKey, DocumentTargetUnresolvedReason)>,
    key: LogicalRelationKey,
    reason: DocumentTargetUnresolvedReason,
) -> Result<(), CliError> {
    let digest = key.digest().to_string();
    match reasons.entry(digest) {
        Entry::Vacant(entry) => {
            entry.insert((key, reason));
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == &(key, reason) => Ok(()),
        Entry::Occupied(_entry) => Err(CliError::InvalidInput(
            "document relation retained conflicting unresolved reasons".to_string(),
        )),
    }
}

/// Resolve one parser graph and release its temporary owner/key workspace.
fn project_graph_rows(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graph: &SymbolGraph,
    document_facts: Option<&MarkdownFacts>,
    document_index: &DocumentResolutionIndex<'_>,
    entities: &mut EntityProjection,
    candidates: &ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<ProjectedGraphRows, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let owners = entities
        .owners_by_graph
        .remove(&graph.path)
        .ok_or_else(|| CliError::InvalidInput("graph owners were not staged".to_string()))?;
    let resolution_keys = entities
        .keys_by_graph
        .remove(&graph.path)
        .ok_or_else(|| CliError::InvalidInput("graph keys were not staged".to_string()))?;
    let symbol_index = GraphSymbolIndex::new(graph, control)?;
    let keys_by_relation = resolution_keys
        .relation_keys()
        .iter()
        .map(|entry| (entry.relation_index(), entry.keys()))
        .collect::<BTreeMap<_, _>>();
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut coverage = Vec::new();
    let mut external_entities = BTreeMap::new();
    let mut document_unresolved_reasons = BTreeMap::new();
    for (relation_index, source_relation) in graph.relations.iter().enumerate() {
        control.check(IndexWorkStage::SymbolParsing)?;
        let source = relation_source(
            &owners,
            &entities.entity_by_digest,
            &symbol_index,
            source_relation,
        )?;
        let dependency_keys = keys_by_relation
            .get(&relation_index)
            .copied()
            .unwrap_or(&[]);
        let resolution = relation_resolution(
            project,
            generation,
            source_relation,
            &owners,
            graph,
            &symbol_index,
            dependency_keys,
            candidates,
            &entities.entity_by_digest,
            &mut external_entities,
            control,
        )?;
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::from_legacy(source_relation.kind),
            resolution,
            relation_confidence(source_relation.parser),
            relation_completeness(source_relation.parser),
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "logical",
            relation_index,
        )?;
        let line = u32::try_from(source_relation.line).map_err(|error| {
            CliError::InvalidInput(format!("relation source line exceeds graph range: {error}"))
        })?;
        let end_column =
            u32::try_from(source_relation.context.chars().count()).map_err(|error| {
                CliError::InvalidInput(format!(
                    "relation source context exceeds graph range: {error}"
                ))
            })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(line.max(1), 0, line.max(1), end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        for key in dependency_keys {
            relation_dependencies.push(
                RelationDependencyKey::new(relation.key().clone(), key.clone())
                    .map_err(invalid_graph_contract)?,
            );
        }
    }
    for (fact_index, fact) in derived_relation_facts(graph, &keys_by_relation)
        .into_iter()
        .enumerate()
    {
        control.check(IndexWorkStage::SymbolParsing)?;
        let source = relation_source(
            &owners,
            &entities.entity_by_digest,
            &symbol_index,
            &fact.relation,
        )?;
        let resolution = derived_relation_resolution(
            project,
            generation,
            &fact,
            &owners,
            graph,
            &symbol_index,
            candidates,
            &entities.entity_by_digest,
            &mut external_entities,
            control,
        )?;
        let relation = LogicalRelation::new(
            source,
            GraphRelationKind::Extended(fact.kind),
            resolution,
            relation_confidence(fact.relation.parser),
            relation_completeness(fact.relation.parser),
            generation,
        )
        .map_err(invalid_graph_contract)?;
        insert_relation(
            &mut relations_by_digest,
            relation.clone(),
            &graph.path,
            "derived",
            fact_index,
        )?;
        let line = u32::try_from(fact.relation.line).map_err(|error| {
            CliError::InvalidInput(format!(
                "derived relation source line exceeds graph range: {error}"
            ))
        })?;
        let end_column = u32::try_from(fact.relation.context.chars().count()).map_err(|error| {
            CliError::InvalidInput(format!(
                "derived relation source context exceeds graph range: {error}"
            ))
        })?;
        occurrences.push(
            RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
                SourceSpan::new(line.max(1), 0, line.max(1), end_column)
                    .map_err(invalid_graph_contract)?,
                generation,
            )
            .map_err(invalid_graph_contract)?,
        );
        if let DerivedRelationTarget::Parser { keys } = fact.target {
            for key in keys {
                relation_dependencies.push(
                    RelationDependencyKey::new(relation.key().clone(), key)
                        .map_err(invalid_graph_contract)?,
                );
            }
        }
    }
    if let Some(facts) = document_facts {
        let rows = project_document_rows(
            project,
            generation,
            graph,
            facts,
            &owners,
            document_index,
            candidates,
            &entities.entity_by_digest,
            control,
        )?;
        for (relation_index, relation) in rows.relations.into_iter().enumerate() {
            insert_relation(
                &mut relations_by_digest,
                relation,
                &graph.path,
                "document",
                relation_index,
            )?;
        }
        occurrences.extend(rows.occurrences);
        coverage.extend(rows.coverage);
        relation_dependencies.extend(rows.relation_dependencies);
        for (key, reason) in rows.document_unresolved_reasons {
            insert_document_unresolved_reason(&mut document_unresolved_reasons, key, reason)?;
        }
    }
    let mut relations = relations_by_digest.into_values().collect::<Vec<_>>();
    relations.sort_by(|left, right| left.key().digest().cmp(right.key().digest()));
    occurrences.sort_by(|left, right| {
        left.relation()
            .digest()
            .cmp(right.relation().digest())
            .then_with(|| left.file().as_str().cmp(right.file().as_str()))
            .then_with(|| left.span().start_line().cmp(&right.span().start_line()))
            .then_with(|| left.span().start_column().cmp(&right.span().start_column()))
    });
    occurrences.dedup();
    sort_dedup_dependencies(&mut relation_dependencies);
    enforce_key_binding_limit(relation_dependencies.len())?;
    entities.retained_bytes = entities
        .retained_bytes
        .saturating_sub(resolution_retained_bytes(&resolution_keys));
    let mut graph_coverage = vec![coverage_for_graph(graph, generation)?];
    graph_coverage.extend(coverage);
    Ok(ProjectedGraphRows {
        relations,
        occurrences,
        coverage: graph_coverage,
        external_entities: external_entities.into_values().collect(),
        relation_dependencies,
        document_unresolved_reasons: document_unresolved_reasons.into_values().collect(),
    })
}

/// Deduplicate relation occurrences while conservatively retaining ambiguity.
fn insert_relation(
    relations: &mut BTreeMap<String, LogicalRelation>,
    relation: LogicalRelation,
    graph_path: &str,
    relation_kind: &str,
    relation_index: usize,
) -> Result<(), CliError> {
    let digest = relation.key().digest().to_string();
    match relations.entry(digest) {
        Entry::Vacant(entry) => {
            entry.insert(relation);
            Ok(())
        }
        Entry::Occupied(mut entry) => {
            let existing = entry.get();
            if existing == &relation {
                return Ok(());
            }
            let mergeable = existing.key() == relation.key()
                && existing.source() == relation.source()
                && existing.kind() == relation.kind()
                && existing.confidence() == relation.confidence()
                && existing.completeness() == relation.completeness()
                && existing.generation() == relation.generation();
            if mergeable
                && let (
                    RelationResolution::Ambiguous {
                        reference: existing_reference,
                        candidates: existing_candidates,
                    },
                    RelationResolution::Ambiguous {
                        reference: incoming_reference,
                        candidates: incoming_candidates,
                    },
                ) = (existing.resolution(), relation.resolution())
                && existing_reference == incoming_reference
            {
                if incoming_candidates > existing_candidates {
                    entry.insert(relation);
                }
                return Ok(());
            }
            Err(CliError::InvalidInput(format!(
                "{relation_kind} relation digest retained conflicting facts for {graph_path} \
                 relation {relation_index}: existing={existing:?}, incoming={relation:?}"
            )))
        }
    }
}

/// Project-wide stable entities and their canonical resolution-key bindings.
#[derive(Default)]
struct ProjectResolutionRegistry {
    /// Persisted candidates not already owned by the staged entity projection.
    supplemental_entities_by_digest: BTreeMap<String, GraphEntity>,
    /// Canonical keys mapped only to sorted, deduplicated entity digests.
    candidate_digests_by_key: BTreeMap<CanonicalResolutionKey, BTreeSet<String>>,
    /// Conservative peak bytes retained by this temporary resolution registry.
    retained_bytes: u64,
}

impl ProjectResolutionRegistry {
    /// Insert one key-to-entity binding without cloning the entity per exported key.
    fn insert_candidate(
        &mut self,
        key: &CanonicalResolutionKey,
        entity: &GraphEntity,
    ) -> Result<(), CliError> {
        let digest = entity.key().digest().to_string();
        match self.supplemental_entities_by_digest.entry(digest.clone()) {
            Entry::Occupied(entry) if entry.get() != entity => {
                return Err(CliError::InvalidInput(
                    "graph entity digest retained conflicting selectors".to_string(),
                ));
            }
            Entry::Occupied(_entry) => {}
            Entry::Vacant(entry) => {
                self.retained_bytes = self
                    .retained_bytes
                    .saturating_add(entity_retained_bytes(entity))
                    .saturating_add(digest.len() as u64);
                entry.insert(entity.clone());
            }
        }
        self.insert_candidate_binding(key, digest)
    }

    /// Bind an entity already owned by the staged projection without cloning it.
    fn insert_staged_candidate(
        &mut self,
        key: &CanonicalResolutionKey,
        entity: &GraphEntity,
    ) -> Result<(), CliError> {
        self.insert_candidate_binding(key, entity.key().digest().to_string())
    }

    /// Insert one canonical-key binding for an entity owned by either registry.
    fn insert_candidate_binding(
        &mut self,
        key: &CanonicalResolutionKey,
        digest: String,
    ) -> Result<(), CliError> {
        if let Some(existing) = self.candidate_digests_by_key.get_mut(key) {
            if existing.insert(digest.clone()) {
                self.retained_bytes = self
                    .retained_bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(digest.len() as u64);
            }
        } else {
            self.retained_bytes = self
                .retained_bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(key.canonical_identity().len() as u64)
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(digest.len() as u64);
            self.candidate_digests_by_key
                .insert(key.clone(), BTreeSet::from([digest]));
        }
        enforce_resolution_registry_budget(self.retained_bytes)?;
        Ok(())
    }
}

/// Reject a temporary resolution registry before it can exceed publication memory.
fn enforce_resolution_registry_budget(retained_bytes: u64) -> Result<(), CliError> {
    if retained_bytes > super::MAX_PUBLICATION_STAGING_BYTES {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::OutputBytes,
            super::MAX_PUBLICATION_STAGING_BYTES,
            retained_bytes,
        )
        .into());
    }
    Ok(())
}

/// Reject the simultaneous entity projection and lookup registry peak.
fn enforce_resolution_staging_budget(
    projection: &EntityProjection,
    registry: &ProjectResolutionRegistry,
) -> Result<(), CliError> {
    enforce_resolution_registry_budget(
        projection
            .retained_bytes
            .saturating_add(registry.retained_bytes),
    )
}

/// Count conservative fixed and variable bytes retained by one graph entity.
fn entity_retained_bytes(entity: &GraphEntity) -> u64 {
    let selector_bytes = match entity.selector() {
        EntitySelector::Project => 0,
        EntitySelector::Folder { path } => path.as_str().len() as u64,
        EntitySelector::File { path } => path.as_str().len() as u64,
        EntitySelector::Package { package } => {
            (package.manager.as_str().len()
                + package.name.as_str().len()
                + package.manifest.as_str().len()) as u64
        }
        EntitySelector::Symbol { symbol } => {
            (symbol.file.as_str().len()
                + symbol.name.as_str().len()
                + symbol
                    .parent
                    .as_ref()
                    .map_or(0, |parent| parent.as_str().len())
                + symbol.signature.as_str().len()) as u64
        }
        EntitySelector::External { external } => {
            (external.system.as_str().len() + external.identity.as_str().len()) as u64
        }
    };
    STAGED_GRAPH_ROW_BYTES
        .saturating_add(entity.key().digest().len() as u64)
        .saturating_add(entity.key().canonical_identity().len() as u64)
        .saturating_add(selector_bytes)
}

/// Build current resolution candidates from newly staged entity exports.
fn resolution_registry_from_exports(
    projection: &EntityProjection,
    control: &IndexWorkControl,
) -> Result<ProjectResolutionRegistry, CliError> {
    let mut candidates = ProjectResolutionRegistry::default();
    for (index, binding) in projection.entity_exports.iter().enumerate() {
        check_graph_work(control, index)?;
        let digest = binding.entity().digest().to_string();
        let entity = projection.entity_by_digest.get(&digest).ok_or_else(|| {
            CliError::InvalidInput("resolution export owner was not staged".to_string())
        })?;
        candidates.insert_staged_candidate(binding.key(), entity)?;
    }
    Ok(candidates)
}

/// Rebind unaffected persisted candidates to the pending graph generation.
fn resolution_registry_from_persisted(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    candidates: Vec<RepositoryResolutionCandidate>,
    replaced_paths: &BTreeSet<String>,
    control: &IndexWorkControl,
) -> Result<ProjectResolutionRegistry, CliError> {
    let mut by_key = ProjectResolutionRegistry::default();
    for (index, candidate) in candidates.into_iter().enumerate() {
        check_graph_work(control, index)?;
        if entity_owner_path(candidate.entity()).is_some_and(|path| replaced_paths.contains(path)) {
            continue;
        }
        let entity = GraphEntity::new(project, candidate.entity().selector().clone(), generation)
            .map_err(invalid_graph_contract)?;
        by_key.insert_candidate(candidate.key(), &entity)?;
    }
    Ok(by_key)
}

/// Merge normalized candidate registries while retaining one entity per digest.
fn merge_resolution_registries(
    target: &mut ProjectResolutionRegistry,
    source: ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let ProjectResolutionRegistry {
        supplemental_entities_by_digest,
        candidate_digests_by_key,
        retained_bytes: _retained_bytes,
    } = source;
    for entity in supplemental_entities_by_digest.into_values() {
        check_graph_work(control, target.supplemental_entities_by_digest.len())?;
        let digest = entity.key().digest().to_string();
        match target.supplemental_entities_by_digest.entry(digest.clone()) {
            Entry::Occupied(entry) if entry.get() != &entity => {
                return Err(CliError::InvalidInput(
                    "resolution candidate entity retained conflicting selectors".to_string(),
                ));
            }
            Entry::Occupied(_entry) => {}
            Entry::Vacant(entry) => {
                target.retained_bytes = target
                    .retained_bytes
                    .saturating_add(entity_retained_bytes(&entity))
                    .saturating_add(digest.len() as u64);
                entry.insert(entity);
            }
        }
    }
    let mut bindings = 0_usize;
    for (key, candidates) in candidate_digests_by_key {
        for digest in candidates {
            check_graph_work(control, bindings)?;
            target.insert_candidate_binding(&key, digest)?;
            bindings = bindings.saturating_add(1);
        }
    }
    Ok(())
}

/// Observe cancellation and deadline state at bounded graph-map intervals.
fn check_graph_work(control: &IndexWorkControl, index: usize) -> Result<(), CliError> {
    if index.is_multiple_of(GRAPH_WORK_CHECK_INTERVAL) {
        control.check(IndexWorkStage::SymbolParsing)?;
    }
    Ok(())
}

/// Return the source-owner path for entities eligible to export graph keys.
fn entity_owner_path(entity: &GraphEntity) -> Option<&str> {
    match entity.selector() {
        EntitySelector::File { path } => Some(path.as_str()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str()),
        EntitySelector::Package { package } => Some(package.manifest.as_str()),
        EntitySelector::Project
        | EntitySelector::Folder { .. }
        | EntitySelector::External { .. } => None,
    }
}

/// Derive additive families only from exact bounded parser facts and path policy.
fn derived_relation_facts(
    graph: &SymbolGraph,
    keys_by_relation: &BTreeMap<usize, &[CanonicalResolutionKey]>,
) -> Vec<DerivedRelationFact> {
    let mut facts = Vec::new();
    let test_path = is_test_path(&graph.path);
    for (index, relation) in graph.relations.iter().enumerate() {
        if test_path && matches!(relation.kind, RelationKind::Imports | RelationKind::Calls) {
            let keys = keys_by_relation
                .get(&index)
                .copied()
                .unwrap_or_default()
                .to_vec();
            push_derived_relation(
                &mut facts,
                ExtendedRelationKind::Tests,
                relation.clone(),
                DerivedRelationTarget::Parser { keys },
            );
        }
        if relation.kind == RelationKind::Calls {
            if let Some(handler) = static_route_handler(relation) {
                push_derived_relation(
                    &mut facts,
                    ExtendedRelationKind::RoutesTo,
                    derived_parser_relation(relation, handler),
                    DerivedRelationTarget::Parser { keys: Vec::new() },
                );
            }
            if let Some(key) = static_environment_key(relation) {
                push_derived_relation(
                    &mut facts,
                    ExtendedRelationKind::Configures,
                    derived_parser_relation(relation, key),
                    DerivedRelationTarget::External {
                        system: ENVIRONMENT_SYSTEM,
                    },
                );
            }
            if let Some(kind) = static_data_access_kind(&relation.target_name)
                && let Some(path) = static_string_argument(&relation.context)
                    .and_then(normalize_static_repository_path)
            {
                push_derived_relation(
                    &mut facts,
                    kind,
                    derived_parser_relation(relation, path),
                    DerivedRelationTarget::RepositoryPath,
                );
            }
        }
    }
    if let Some(identity) = configuration_file_identity(&graph.path) {
        push_derived_relation(
            &mut facts,
            ExtendedRelationKind::Configures,
            file_owned_relation(graph, identity),
            DerivedRelationTarget::External {
                system: CONFIGURATION_SYSTEM,
            },
        );
    }
    if let Some(identity) = deployment_platform_identity(&graph.path) {
        push_derived_relation(
            &mut facts,
            ExtendedRelationKind::Deploys,
            file_owned_relation(graph, identity),
            DerivedRelationTarget::External {
                system: DEPLOYMENT_SYSTEM,
            },
        );
    }
    facts
}

/// Retain one additive fact from the already bounded parser graph.
fn push_derived_relation(
    facts: &mut Vec<DerivedRelationFact>,
    kind: ExtendedRelationKind,
    relation: SymbolRelation,
    target: DerivedRelationTarget,
) {
    facts.push(DerivedRelationFact {
        kind,
        relation,
        target,
    });
}

/// Copy source context while replacing only the statically selected target.
fn derived_parser_relation(relation: &SymbolRelation, target_name: String) -> SymbolRelation {
    SymbolRelation {
        path: relation.path.clone(),
        source_name: relation.source_name.clone(),
        target_name,
        kind: RelationKind::Calls,
        line: relation.line,
        context: relation.context.clone(),
        parser: relation.parser,
    }
}

/// Create one file-owned content-free configuration or deployment fact.
fn file_owned_relation(graph: &SymbolGraph, target_name: String) -> SymbolRelation {
    SymbolRelation {
        path: graph.path.clone(),
        source_name: "<module>".to_string(),
        target_name,
        kind: RelationKind::Calls,
        line: 1,
        context: graph.path.clone(),
        parser: graph.parser,
    }
}

/// Resolve one additive relation through its closed target class.
fn derived_relation_resolution<'a>(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    fact: &DerivedRelationFact,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    candidates: &'a ProjectResolutionRegistry,
    entities: &'a BTreeMap<String, GraphEntity>,
    external_entities: &mut BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<RelationResolution, CliError> {
    match &fact.target {
        DerivedRelationTarget::Parser { keys } => relation_resolution(
            project,
            generation,
            &fact.relation,
            owners,
            graph,
            symbol_index,
            keys,
            candidates,
            entities,
            external_entities,
            control,
        ),
        DerivedRelationTarget::RepositoryPath => {
            let candidate = GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(&fact.relation.target_name))
                        .map_err(invalid_graph_contract)?,
                },
                generation,
            )
            .map_err(invalid_graph_contract)?;
            match entities.get(candidate.key().digest()) {
                Some(entity) if entity == &candidate => {
                    RelationResolution::resolved(entity).map_err(invalid_graph_contract)
                }
                Some(_conflict) => Err(CliError::InvalidInput(
                    "static repository-path target collided with another graph entity".to_string(),
                )),
                None => Ok(RelationResolution::Unresolved {
                    reference: GraphIdentityText::new(fact.relation.target_name.clone())
                        .map_err(invalid_graph_contract)?,
                }),
            }
        }
        DerivedRelationTarget::External { system } => {
            let entity = GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new(*system).map_err(invalid_graph_contract)?,
                        identity: GraphIdentityText::new(fact.relation.target_name.clone())
                            .map_err(invalid_graph_contract)?,
                    },
                },
                generation,
            )
            .map_err(invalid_graph_contract)?;
            let resolution =
                RelationResolution::external(&entity).map_err(invalid_graph_contract)?;
            insert_entity(external_entities, entity)?;
            Ok(resolution)
        }
    }
}

/// Return whether a repository path is an accepted test-source location.
fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    normalized
        .split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
        || file.contains(".test.")
        || file.contains(".spec.")
        || file
            .split_once('.')
            .is_some_and(|(stem, _extension)| stem.ends_with("_test") || stem.starts_with("test_"))
}

/// Extract one exact handler identifier from a static route registration.
fn static_route_handler(relation: &SymbolRelation) -> Option<String> {
    let leaf = call_leaf(&relation.target_name);
    if !matches!(
        leaf,
        "route" | "add_route" | "map_get" | "map_post" | "map_put" | "map_patch" | "map_delete"
    ) {
        return None;
    }
    let route = static_string_argument(&relation.context)?;
    if !route.starts_with('/')
        || route.chars().any(char::is_control)
        || relation.context.matches(',').count() != 1
    {
        return None;
    }
    let handler = relation
        .context
        .rsplit_once(',')?
        .1
        .trim()
        .trim_end_matches([')', ';'])
        .trim();
    let valid_handler = !handler.is_empty()
        && handler != relation.source_name
        && handler
            .chars()
            .next()
            .is_some_and(|character| !character.is_ascii_digit())
        && handler.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | ':' | '.' | '$')
        });
    valid_handler.then(|| handler.trim_matches('$').to_string())
}

/// Extract one static environment key without retaining its value.
fn static_environment_key(relation: &SymbolRelation) -> Option<String> {
    let normalized = relation
        .target_name
        .trim()
        .trim_end_matches('!')
        .to_ascii_lowercase();
    if !(normalized.ends_with("env::var")
        || normalized.ends_with("env::var_os")
        || normalized.ends_with("os.getenv")
        || normalized.ends_with("getenvironmentvariable")
        || matches!(normalized.as_str(), "getenv" | "getenv_os"))
    {
        return None;
    }
    let key = static_string_argument(&relation.context)?;
    (!key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
    .then(|| key.to_string())
}

/// Classify a call as one accepted bounded static read or write API.
fn static_data_access_kind(target: &str) -> Option<ExtendedRelationKind> {
    let normalized = target.trim().trim_end_matches('!').to_ascii_lowercase();
    let leaf = call_leaf(&normalized);
    if matches!(
        leaf,
        "read" | "read_to_string" | "readfile" | "readfilesync"
    ) || normalized.ends_with("file::open")
    {
        Some(ExtendedRelationKind::Reads)
    } else if matches!(leaf, "write" | "writefile" | "writefilesync" | "create")
        || normalized.ends_with("file::create")
    {
        Some(ExtendedRelationKind::Writes)
    } else {
        None
    }
}

/// Return the normalized leaf of one qualified call target.
fn call_leaf(target: &str) -> &str {
    target
        .trim()
        .trim_end_matches('!')
        .rsplit([':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or_default()
}

/// Return the first argument only when it is one complete static string literal.
fn static_string_argument(context: &str) -> Option<&str> {
    let open = context.find('(')?;
    let argument = context[open + 1..].trim_start();
    let quote = argument.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let value = &argument[quote.len_utf8()..];
    let end = value.find(quote)?;
    let value = &value[..end];
    (!value.contains('\\')
        && !value.contains("${")
        && !value.contains("#{")
        && !value.chars().any(char::is_control))
    .then_some(value)
}

/// Normalize one static relative source literal into a repository path.
fn normalize_static_repository_path(value: &str) -> Option<String> {
    let value = value.replace('\\', "/");
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('~')
        || value.contains("://")
        || value.contains('$')
        || value.contains('{')
        || value.contains('}')
        || value
            .split('/')
            .next()
            .is_some_and(|part| part.contains(':'))
    {
        return None;
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part == "."
                || part == ".."
                || part.chars().any(char::is_control)
                || part.trim() != part =>
            {
                return None;
            }
            part => parts.push(part.to_string()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Classify one exact repository configuration filename without reading values.
fn configuration_file_identity(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let identity = if file == ".env" || file.starts_with(".env.") {
        "dotenv"
    } else if matches!(
        file,
        "config.json"
            | "config.yaml"
            | "config.yml"
            | "config.toml"
            | "settings.json"
            | "settings.yaml"
            | "settings.yml"
            | "settings.toml"
            | "appsettings.json"
    ) {
        "application-config"
    } else {
        return None;
    };
    Some(identity.to_string())
}

/// Classify one accepted infrastructure path into a content-free platform.
fn deployment_platform_identity(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let identity = if file_has_extension(file, "tf")
        || file
            .strip_suffix(".json")
            .is_some_and(|stem| file_has_extension(stem, "tf"))
    {
        "terraform"
    } else if file == "dockerfile"
        || file.starts_with("dockerfile.")
        || matches!(
            file,
            "compose.yaml" | "compose.yml" | "docker-compose.yaml" | "docker-compose.yml"
        )
    {
        "containers"
    } else if matches!(
        file,
        "chart.yaml" | "kustomization.yaml" | "kustomization.yml"
    ) || (normalized
        .split('/')
        .any(|segment| matches!(segment, "k8s" | "kubernetes" | "helm" | "kustomize"))
        && matches!(
            Path::new(file).extension().and_then(|value| value.to_str()),
            Some("yaml" | "yml" | "json")
        ))
    {
        "kubernetes"
    } else if file_has_extension(file, "bicep") {
        "azure-bicep"
    } else if file == "sam-template.yaml"
        || (file.contains("cloudformation")
            && matches!(
                Path::new(file).extension().and_then(|value| value.to_str()),
                Some("yaml" | "yml" | "json")
            ))
    {
        "cloudformation"
    } else if file == "playbook.yaml" || file == "playbook.yml" {
        "ansible"
    } else {
        return None;
    };
    Some(identity.to_string())
}

/// Match one exact extension without platform case assumptions.
fn file_has_extension(file: &str, expected: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

/// Resolve one parser relation's unique local source entity when possible.
fn relation_source<'a>(
    owners: &GraphOwners,
    entities: &'a BTreeMap<String, GraphEntity>,
    symbol_index: &GraphSymbolIndex<'_>,
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Result<&'a GraphEntity, CliError> {
    let file = entities.get(&owners.file_digest).ok_or_else(|| {
        CliError::InvalidInput("graph file owner entity was not staged".to_string())
    })?;
    let mut matched = None;
    for &index in symbol_index.get(&relation.source_name) {
        let digest = owners.symbol_digests.get(index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner index was not staged".to_string())
        })?;
        let Some(digest) = digest else {
            continue;
        };
        let entity = entities.get(digest).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner entity was not staged".to_string())
        })?;
        if matched.is_some() {
            return Ok(file);
        }
        matched = Some(entity);
    }
    Ok(matched.unwrap_or(file))
}

/// Resolve one relation from canonical dependency keys and bounded candidates.
#[derive(Clone, Copy)]
struct ResolutionMatches<'a> {
    /// First target in stable digest order when at least one target exists.
    first: Option<&'a GraphEntity>,
    /// Exact number of distinct target entities.
    count: u32,
}

/// Resolve one relation from canonical dependency keys and bounded candidates.
fn relation_resolution<'a>(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    relation: &projectatlas_core::symbols::SymbolRelation,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    dependency_keys: &[CanonicalResolutionKey],
    candidates: &'a ProjectResolutionRegistry,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    external_entities: &mut BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<RelationResolution, CliError> {
    let matches = match relation.kind {
        RelationKind::Contains => local_relation_matches(
            relation,
            owners,
            graph,
            symbol_index,
            staged_entities,
            control,
        )?,
        RelationKind::Calls => {
            let local = local_relation_matches(
                relation,
                owners,
                graph,
                symbol_index,
                staged_entities,
                control,
            )?;
            if local.count == 0 {
                registry_resolution_matches(dependency_keys, candidates, staged_entities, control)?
            } else {
                local
            }
        }
        RelationKind::Imports | RelationKind::DependsOn => {
            registry_resolution_matches(dependency_keys, candidates, staged_entities, control)?
        }
    };
    match matches.count {
        0 => {
            if let Some(external) = explicit_external_selector(graph, relation)? {
                let entity =
                    GraphEntity::new(project, EntitySelector::External { external }, generation)
                        .map_err(invalid_graph_contract)?;
                let resolution =
                    RelationResolution::external(&entity).map_err(invalid_graph_contract)?;
                insert_entity(external_entities, entity)?;
                return Ok(resolution);
            }
            Ok(RelationResolution::Unresolved {
                reference: GraphIdentityText::new(relation_reference(relation))
                    .map_err(invalid_graph_contract)?,
            })
        }
        1 => RelationResolution::resolved(
            matches
                .first
                .ok_or_else(|| CliError::InvalidInput("resolved target disappeared".to_string()))?,
        )
        .map_err(invalid_graph_contract),
        count => Ok(RelationResolution::Ambiguous {
            reference: GraphIdentityText::new(relation_reference(relation))
                .map_err(invalid_graph_contract)?,
            candidates: NonZeroU32::new(count).ok_or_else(|| {
                CliError::InvalidInput("ambiguous target count was zero".to_string())
            })?,
        }),
    }
}

/// Resolve exact declarations owned by the relation's source file.
fn local_relation_matches<'a>(
    relation: &projectatlas_core::symbols::SymbolRelation,
    owners: &GraphOwners,
    graph: &SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ResolutionMatches<'a>, CliError> {
    let mut targets = BTreeMap::<&str, &GraphEntity>::new();
    let target_name = relation.target_name.trim();
    let source_parent = if relation.kind == RelationKind::Calls {
        unique_source_parent(graph, symbol_index, &relation.source_name, control)?
    } else {
        None
    };
    for (candidate_index, &row_index) in symbol_index.get(target_name).iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        let symbol = graph.symbols.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol lookup index was invalid".to_string())
        })?;
        let digest = owners.symbol_digests.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol owner index was not staged".to_string())
        })?;
        let exact_match = match relation.kind {
            RelationKind::Contains => {
                symbol.name == target_name
                    && symbol.parent.as_deref() == Some(relation.source_name.as_str())
            }
            RelationKind::Calls => {
                symbol.name == target_name
                    && (symbol.parent.is_none()
                        || symbol.parent.as_deref() == Some(relation.source_name.as_str())
                        || source_parent.is_some_and(|source_parent| {
                            symbol.parent.as_deref() == Some(source_parent)
                        }))
            }
            RelationKind::Imports | RelationKind::DependsOn => false,
        };
        if exact_match && let Some(digest) = digest {
            let entity = staged_entities.get(digest).ok_or_else(|| {
                CliError::InvalidInput("graph symbol owner entity was not staged".to_string())
            })?;
            if !targets.contains_key(entity.key().digest()) {
                enforce_resolution_match_budget(targets.len().saturating_add(1))?;
            }
            targets.insert(entity.key().digest(), entity);
        }
    }
    let count = distinct_resolution_count(targets.len())?;
    Ok(ResolutionMatches {
        first: targets.into_values().next(),
        count,
    })
}

/// Return one unambiguous containing scope for the parser relation source.
fn unique_source_parent<'a>(
    graph: &'a SymbolGraph,
    symbol_index: &GraphSymbolIndex<'_>,
    source_name: &str,
    control: &IndexWorkControl,
) -> Result<Option<&'a str>, CliError> {
    let mut parent = None;
    let mut source_found = false;
    for (candidate_index, &row_index) in symbol_index.get(source_name).iter().enumerate() {
        check_graph_work(control, candidate_index)?;
        let symbol = graph.symbols.get(row_index).ok_or_else(|| {
            CliError::InvalidInput("graph symbol lookup index was invalid".to_string())
        })?;
        let candidate = symbol.parent.as_deref();
        if !source_found {
            parent = candidate;
            source_found = true;
        } else if parent != candidate {
            return Ok(None);
        }
    }
    Ok(parent)
}

/// Merge sorted candidate streams without materializing their full union.
fn registry_resolution_matches<'a>(
    dependency_keys: &[CanonicalResolutionKey],
    candidates: &'a ProjectResolutionRegistry,
    staged_entities: &'a BTreeMap<String, GraphEntity>,
    control: &IndexWorkControl,
) -> Result<ResolutionMatches<'a>, CliError> {
    enforce_resolution_match_budget(dependency_keys.len().saturating_mul(2))?;
    let mut streams = dependency_keys
        .iter()
        .filter_map(|key| candidates.candidate_digests_by_key.get(key))
        .map(BTreeSet::iter)
        .collect::<Vec<_>>();
    let mut frontier = BinaryHeap::new();
    for (stream_index, stream) in streams.iter_mut().enumerate() {
        if let Some(digest) = stream.next() {
            frontier.push(Reverse((digest.as_str(), stream_index)));
        }
    }

    let mut first = None;
    let mut last_digest = None;
    let mut count = 0_u32;
    let mut visited = 0_usize;
    while let Some(Reverse((digest, stream_index))) = frontier.pop() {
        check_graph_work(control, visited)?;
        visited = visited.saturating_add(1);
        if last_digest != Some(digest) {
            let entity = staged_entities
                .get(digest)
                .or_else(|| candidates.supplemental_entities_by_digest.get(digest))
                .ok_or_else(|| {
                    CliError::InvalidInput(
                        "resolution candidate entity was not registered".to_string(),
                    )
                })?;
            first.get_or_insert(entity);
            count = count.checked_add(1).ok_or_else(|| {
                IndexWorkFailure::resource_limit(
                    IndexWorkStage::SymbolParsing,
                    IndexWorkResource::RelationRows,
                    u64::from(u32::MAX),
                    u64::from(u32::MAX) + 1,
                )
            })?;
            last_digest = Some(digest);
        }
        if let Some(next) = streams[stream_index].next() {
            frontier.push(Reverse((next.as_str(), stream_index)));
        }
    }
    Ok(ResolutionMatches { first, count })
}

/// Convert one exact distinct target count without truncation.
fn distinct_resolution_count(count: usize) -> Result<u32, CliError> {
    u32::try_from(count).map_err(|_conversion_error| {
        IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::RelationRows,
            u64::from(u32::MAX),
            u64::try_from(count).unwrap_or(u64::MAX),
        )
        .into()
    })
}

/// Bound temporary maps and heap storage used while selecting relation targets.
fn enforce_resolution_match_budget(rows: usize) -> Result<(), CliError> {
    enforce_resolution_registry_budget(
        STAGED_GRAPH_ROW_BYTES.saturating_mul(u64::try_from(rows).unwrap_or(u64::MAX)),
    )
}

/// Return one explicit external identity without guessing ordinary missing packages.
fn explicit_external_selector(
    graph: &SymbolGraph,
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Result<Option<ExternalSelector>, CliError> {
    let semantic_provider = graph
        .language
        .as_deref()
        .and_then(language_capability)
        .and_then(|capability| capability.effective_semantic_provider());
    let classified = match (semantic_provider, relation.kind) {
        (Some(SemanticProviderOwner::Cargo), RelationKind::DependsOn) => {
            external_reference_identity(&relation.target_name)
                .map(|identity| (CARGO_PACKAGE_MANAGER, identity))
        }
        (Some(SemanticProviderOwner::Rust), RelationKind::Imports | RelationKind::Calls) => {
            rust_toolchain_identity(relation).map(|identity| (RUST_TOOLCHAIN_SYSTEM, identity))
        }
        (Some(SemanticProviderOwner::EcmaScript), RelationKind::Imports) => {
            node_builtin_identity(relation).map(|identity| (NODE_SYSTEM, identity))
        }
        (
            Some(
                SemanticProviderOwner::Python
                | SemanticProviderOwner::Unavailable
                | SemanticProviderOwner::Cargo
                | SemanticProviderOwner::EcmaScript,
            )
            | None,
            _,
        )
        | (Some(SemanticProviderOwner::Rust), RelationKind::Contains | RelationKind::DependsOn) => {
            None
        }
    };
    classified
        .map(|(system, identity)| {
            Ok(ExternalSelector {
                system: GraphIdentityText::new(system).map_err(invalid_graph_contract)?,
                identity: GraphIdentityText::new(identity).map_err(invalid_graph_contract)?,
            })
        })
        .transpose()
}

/// Normalize one explicit non-empty external reference.
fn external_reference_identity(reference: &str) -> Option<String> {
    let reference = reference.trim();
    (!reference.is_empty()).then(|| reference.to_string())
}

/// Recognize only explicitly qualified Rust toolchain roots.
fn rust_toolchain_identity(
    relation: &projectatlas_core::symbols::SymbolRelation,
) -> Option<String> {
    if relation.kind == RelationKind::Imports {
        let mut references = parse_import_references(&relation.target_name).into_iter();
        let first = rust_import_identity(&references.next()?)?;
        return references.try_fold(first, |common, reference| {
            let identity = rust_import_identity(&reference)?;
            common_rust_path(&common, &identity)
        });
    }
    let target = relation_reference(relation);
    rust_toolchain_root(&target)?;
    Some(target)
}

/// Return one explicitly toolchain-owned Rust import identity.
fn rust_import_identity(reference: &projectatlas_symbols::ImportReference) -> Option<String> {
    let module = reference.module();
    rust_toolchain_root(module)?;
    Some(reference.imported().map_or_else(
        || module.to_string(),
        |imported| format!("{module}::{imported}"),
    ))
}

/// Return the non-empty common Rust module path shared by two imports.
fn common_rust_path(left: &str, right: &str) -> Option<String> {
    let components = left
        .split("::")
        .zip(right.split("::"))
        .take_while(|(left, right)| left == right)
        .map(|(component, _right)| component)
        .collect::<Vec<_>>();
    (!components.is_empty()).then(|| components.join("::"))
}

/// Return the accepted toolchain root of one Rust module path.
fn rust_toolchain_root(path: &str) -> Option<&str> {
    let root = path.trim().split("::").next()?;
    matches!(root, "std" | "core" | "alloc").then_some(root)
}

/// Recognize only the explicit `node:` built-in module scheme.
fn node_builtin_identity(relation: &projectatlas_core::symbols::SymbolRelation) -> Option<String> {
    parse_import_references(&relation.target_name)
        .into_iter()
        .find_map(|reference| {
            reference
                .module()
                .strip_prefix("node:")
                .filter(|identity| !identity.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            quoted_ecmascript_module(&relation.target_name)
                .and_then(|module| module.strip_prefix("node:"))
                .filter(|identity| !identity.is_empty())
                .map(ToString::to_string)
        })
}

/// Read the explicit quoted module from one parser-validated ECMAScript import.
fn quoted_ecmascript_module(import: &str) -> Option<&str> {
    let import = import.trim();
    if !import.starts_with("import ") {
        return None;
    }
    let (quote_index, quote) = import
        .char_indices()
        .find(|(_index, character)| matches!(character, '\'' | '"'))?;
    let remainder = &import[quote_index + quote.len_utf8()..];
    let end = remainder.find(quote)?;
    Some(&remainder[..end])
}

/// Normalize an empty parser reference into one stable diagnostic identity.
fn nonempty_reference(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        UNKNOWN_REFERENCE.to_string()
    } else {
        value.to_string()
    }
}

/// Keep call arguments and literal values out of persisted relation diagnostics.
fn relation_reference(relation: &SymbolRelation) -> String {
    let mut value = if relation.kind == RelationKind::Calls {
        relation
            .target_name
            .split_once('(')
            .map_or(relation.target_name.as_str(), |(target, _arguments)| target)
    } else {
        &relation.target_name
    };
    if relation.kind == RelationKind::Calls && value.contains(['\'', '"']) {
        value = call_leaf(value);
    }
    nonempty_reference(value)
}

/// Project parser trust into one path-scoped coverage record.
fn coverage_for_graph(
    graph: &SymbolGraph,
    generation: IndexGeneration,
) -> Result<CoverageRecord, CliError> {
    let scope = CoverageScope::Path {
        path: RepositoryNodePath::new(Path::new(&graph.path)).map_err(invalid_graph_contract)?,
    };
    let covered = u64::try_from(graph.relations.len()).unwrap_or(u64::MAX);
    match graph.parser {
        ParserKind::TreeSitter | ParserKind::Manifest => CoverageRecord::new(
            scope,
            None,
            CoverageState::Complete,
            covered,
            0,
            generation,
            None,
            None,
        )
        .map_err(invalid_graph_contract),
        ParserKind::Structural | ParserKind::Fallback if covered > 0 => CoverageRecord::new(
            scope,
            None,
            CoverageState::Partial,
            covered,
            1,
            generation,
            Some(GraphIdentityText::new(PARTIAL_COVERAGE_REASON).map_err(invalid_graph_contract)?),
            None,
        )
        .map_err(invalid_graph_contract),
        ParserKind::Structural | ParserKind::Fallback => CoverageRecord::new(
            scope,
            None,
            CoverageState::Failed,
            0,
            1,
            generation,
            Some(GraphIdentityText::new(PARTIAL_COVERAGE_REASON).map_err(invalid_graph_contract)?),
            None,
        )
        .map_err(invalid_graph_contract),
    }
}

/// Map parser strength to relation confidence.
fn relation_confidence(parser: ParserKind) -> ConfidenceClass {
    match parser {
        ParserKind::TreeSitter | ParserKind::Manifest => ConfidenceClass::Exact,
        ParserKind::Structural => ConfidenceClass::Medium,
        ParserKind::Fallback => ConfidenceClass::Low,
    }
}

/// Map parser strength to relation completeness.
fn relation_completeness(parser: ParserKind) -> Completeness {
    match parser {
        ParserKind::TreeSitter | ParserKind::Manifest => Completeness::Complete,
        ParserKind::Structural | ParserKind::Fallback => Completeness::Partial,
    }
}

/// Overlay staged symbol changes on persisted graphs for exact selected paths.
fn complete_symbol_graphs<'a>(
    store: &AtlasStore,
    paths: &BTreeSet<String>,
    symbols: &'a SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<Vec<Cow<'a, SymbolGraph>>, CliError> {
    let paths = paths.iter().cloned().collect::<Vec<_>>();
    let mut graphs = BTreeMap::new();
    for chunk in paths.chunks(PERSISTED_GRAPH_PATHS_PER_CHUNK) {
        control.check(IndexWorkStage::SymbolParsing)?;
        for graph in store.load_symbol_graphs_for_paths(chunk)? {
            graphs.insert(graph.path.clone(), Cow::Owned(graph));
        }
    }
    for (index, change) in symbols.changes.iter().enumerate() {
        check_graph_work(control, index)?;
        match change {
            SymbolProjectionChange::Parsed(parsed) if paths.binary_search(&parsed.path).is_ok() => {
                graphs.insert(parsed.path.clone(), Cow::Borrowed(&parsed.graph));
            }
            SymbolProjectionChange::Clear { path, .. } if paths.binary_search(path).is_ok() => {
                graphs.remove(path);
            }
            SymbolProjectionChange::Parsed(_) | SymbolProjectionChange::Clear { .. } => {}
        }
    }
    graphs.retain(|path, _graph| paths.binary_search(path).is_ok());
    Ok(graphs.into_values().collect())
}

/// Overlay staged Markdown facts and parse only persisted graphs not parsed in this operation.
fn complete_markdown_facts<'a>(
    root: &Path,
    nodes: &[Node],
    graphs: &[Cow<'a, SymbolGraph>],
    symbols: &'a SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<BTreeMap<String, Cow<'a, MarkdownFacts>>, CliError> {
    let graph_paths = graphs
        .iter()
        .map(|graph| graph.path.as_str())
        .collect::<BTreeSet<_>>();
    let nodes_by_path = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| (node.path.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut facts = BTreeMap::new();
    for change in &symbols.changes {
        let SymbolProjectionChange::Parsed(parsed) = change else {
            continue;
        };
        if graph_paths.contains(parsed.path.as_str())
            && let Some(markdown) = parsed.markdown_facts.as_ref()
        {
            facts.insert(parsed.path.clone(), Cow::Borrowed(markdown.as_ref()));
        }
    }
    for graph in graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        if facts.contains_key(&graph.path)
            || !graph
                .language
                .as_deref()
                .and_then(language_capability)
                .is_some_and(|capability| capability.symbol_parser == SymbolParserOwner::Markdown)
        {
            continue;
        }
        let node = nodes_by_path.get(graph.path.as_str()).ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Markdown graph path was absent from the staged file inventory: {}",
                graph.path
            ))
        })?;
        let native_path = root.join(Path::new(&graph.path));
        let bytes = match read_source_bytes_controlled(
            &native_path,
            MAX_SYMBOL_FILE_BYTES,
            IndexWorkStage::SymbolParsing,
            control,
        ) {
            Ok(bytes) => bytes,
            Err(SourceReadFailure::IndexWork(failure)) => return Err(failure.into()),
            Err(SourceReadFailure::LimitExceeded { .. }) => {
                return Err(source_changed_during_derivation(root, &graph.path));
            }
            Err(SourceReadFailure::Io(source)) => {
                return Err(CliError::Io {
                    path: native_path,
                    source,
                });
            }
        };
        if node
            .content_hash
            .as_deref()
            .is_none_or(|expected| blake3::hash(&bytes).to_hex().as_str() != expected)
        {
            return Err(source_changed_during_derivation(root, &graph.path));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_source| source_changed_during_derivation(root, &graph.path))?;
        facts.insert(
            graph.path.clone(),
            Cow::Owned(extract_markdown_facts_controlled(&content, control)?),
        );
    }
    enforce_resolution_registry_budget(document_fact_map_retained_bytes(&facts))?;
    Ok(facts)
}

/// Count map ownership plus evidence owned only for persisted graphs parsed on demand.
fn document_fact_map_retained_bytes(facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>) -> u64 {
    facts.iter().fold(0_u64, |bytes, (path, facts)| {
        let bytes = bytes
            .saturating_add(STAGED_GRAPH_ROW_BYTES)
            .saturating_add(path.len() as u64);
        if !matches!(facts, Cow::Owned(_)) {
            return bytes;
        }
        let heading_bytes = facts.headings.iter().fold(0_u64, |bytes, heading| {
            bytes
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add(heading.text.len() as u64)
                .saturating_add(heading.slug.len() as u64)
        });
        facts.link_candidates.iter().fold(
            bytes.saturating_add(heading_bytes),
            |bytes, candidate| {
                bytes
                    .saturating_add(STAGED_GRAPH_ROW_BYTES)
                    .saturating_add(candidate.selector.len() as u64)
                    .saturating_add(candidate.label.as_ref().map_or(0, String::len) as u64)
                    .saturating_add(
                        candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64
                    )
            },
        )
    })
}

/// Count generated document rows, reason maps, and duplicate-validation keys.
fn document_projection_retained_bytes(facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>) -> u64 {
    facts.values().fold(0_u64, |bytes, facts| {
        facts
            .link_candidates
            .iter()
            .fold(bytes, |bytes, candidate| {
                bytes
                    .saturating_add(DOCUMENT_PROJECTION_ROW_BYTES)
                    .saturating_add(candidate.selector.len() as u64)
                    .saturating_add(candidate.label.as_ref().map_or(0, String::len) as u64)
                    .saturating_add(
                        candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64
                    )
            })
    })
}

/// One normalized repository-local document target and optional heading fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DocumentTargetIdentity {
    /// Exact slash-separated path under the selected root.
    path: String,
    /// Static fragment retained only when it can address a heading export.
    fragment: Option<String>,
}

/// Normalize one parser-admitted selector relative to its owning document.
fn normalize_document_target(
    document_path: &str,
    selector: &str,
) -> Result<DocumentTargetIdentity, DocumentTargetUnresolvedReason> {
    let (path_and_query, fragment) = selector
        .split_once('#')
        .map_or((selector, None), |(path, fragment)| (path, Some(fragment)));
    let path = path_and_query
        .split_once('?')
        .map_or(path_and_query, |(path, _query)| path);
    let path = strip_document_line_selector(path);
    if path.is_empty() {
        return Err(DocumentTargetUnresolvedReason::NoStaticTarget);
    }
    let mut components = document_path
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _file)| {
            parent.split('/').map(str::to_owned).collect::<Vec<_>>()
        });
    for component in path.split('/') {
        match component {
            "" => return Err(DocumentTargetUnresolvedReason::Unsupported),
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(DocumentTargetUnresolvedReason::OutsideRoot);
                }
            }
            value if value.contains(':') => {
                return Err(DocumentTargetUnresolvedReason::Unsupported);
            }
            value => components.push(value.to_owned()),
        }
    }
    if components.is_empty() {
        return Err(DocumentTargetUnresolvedReason::NoStaticTarget);
    }
    let fragment = match fragment.map(str::trim) {
        None => None,
        Some(value)
            if !value.is_empty()
                && !value.contains(['/', '\\', '?', '#', '{', '}', '<', '>', '|', '*', '$'])
                && !value.chars().any(char::is_whitespace) =>
        {
            Some(value.to_lowercase())
        }
        Some(_value) => return Err(DocumentTargetUnresolvedReason::NoStaticTarget),
    };
    Ok(DocumentTargetIdentity {
        path: components.join("/"),
        fragment,
    })
}

/// Remove one supported `:12`, `:L12-L20`, or equivalent line selector.
fn strip_document_line_selector(path: &str) -> &str {
    let Some((identity, selector)) = path.rsplit_once(':') else {
        return path;
    };
    let selector = selector.strip_prefix('L').unwrap_or(selector);
    let valid = selector.split_once('-').map_or_else(
        || !selector.is_empty() && selector.chars().all(|character| character.is_ascii_digit()),
        |(start, end)| {
            !start.is_empty()
                && !end.is_empty()
                && start.chars().all(|character| character.is_ascii_digit())
                && end
                    .strip_prefix('L')
                    .unwrap_or(end)
                    .chars()
                    .all(|character| character.is_ascii_digit())
        },
    );
    if valid { identity } else { path }
}

/// Construct one exact file identity through the existing module-key domain.
fn document_file_resolution_key(
    project: ProjectInstanceId,
    path: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key(project, ResolutionKeyDomain::Module, path)
}

/// Construct one case-fold collision key through the existing module-key domain.
fn document_casefold_resolution_key(
    project: ProjectInstanceId,
    path: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key_with_language(
        project,
        ResolutionKeyDomain::Module,
        DOCUMENT_CASEFOLD_LANGUAGE,
        &path.to_lowercase(),
    )
}

/// Construct one exact Markdown heading identity through the declaration-key domain.
fn document_heading_resolution_key(
    project: ProjectInstanceId,
    path: &str,
    fragment: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key(
        project,
        ResolutionKeyDomain::Declaration,
        &format!("{path}#{fragment}"),
    )
}

/// Construct one project-qualified canonical document identity without a new key family.
fn document_resolution_key(
    project: ProjectInstanceId,
    domain: ResolutionKeyDomain,
    identity: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    document_resolution_key_with_language(project, domain, DOCUMENT_PATH_LANGUAGE, identity)
}

/// Construct one project-qualified document key under a selected resolver language.
fn document_resolution_key_with_language(
    project: ProjectInstanceId,
    domain: ResolutionKeyDomain,
    resolver_language: &str,
    identity: &str,
) -> Result<CanonicalResolutionKey, CliError> {
    let provider =
        GraphIdentityText::new(DOCUMENT_PATH_PROVIDER).map_err(invalid_graph_contract)?;
    let language = GraphIdentityText::new(resolver_language).map_err(invalid_graph_contract)?;
    let identity = GraphIdentityText::new(identity).map_err(invalid_graph_contract)?;
    Ok(CanonicalResolutionKey::new(
        project,
        domain,
        &provider,
        &language,
        None,
        None,
        Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents)),
        &identity,
    ))
}

/// Return all exact path and heading keys that can affect staged document relations.
fn document_dependency_keys(
    project: ProjectInstanceId,
    facts: &BTreeMap<String, Cow<'_, MarkdownFacts>>,
) -> Result<BTreeSet<CanonicalResolutionKey>, CliError> {
    let mut keys = BTreeSet::new();
    for (document_path, facts) in facts {
        for candidate in &facts.link_candidates {
            let Ok(target) = normalize_document_target(document_path, &candidate.selector) else {
                continue;
            };
            keys.insert(document_file_resolution_key(project, &target.path)?);
            keys.insert(document_casefold_resolution_key(project, &target.path)?);
            if let Some(fragment) = target.fragment {
                keys.insert(document_heading_resolution_key(
                    project,
                    &target.path,
                    &fragment,
                )?);
            }
        }
    }
    Ok(keys)
}

/// Derive canonical resolution keys with repository compiler configuration.
fn resolution_projection_with_config(
    project: ProjectInstanceId,
    package: Option<&str>,
    graph: &SymbolGraph,
    configured_modules: &ConfiguredModuleResolution,
) -> Result<ResolutionKeyProjection, CliError> {
    let context = ResolutionProjectionContext::with_configured_modules(configured_modules);
    match derive_resolution_keys_with_context(project, package, graph, context) {
        Ok(projection) => Ok(projection),
        Err(ResolutionProjectionError::KeyLimit { requested, .. }) => {
            Err(IndexWorkFailure::resource_limit(
                IndexWorkStage::SymbolParsing,
                IndexWorkResource::RelationRows,
                u64::try_from(MAX_RESOLUTION_KEYS_PER_FACT).unwrap_or(u64::MAX),
                u64::try_from(requested).unwrap_or(u64::MAX),
            )
            .into())
        }
        Err(ResolutionProjectionError::Contract(error)) => Err(invalid_graph_contract(error)),
    }
}

/// Load the project identity required by normalized graph projection.
fn selected_project(store: &AtlasStore) -> Result<ProjectInstanceId, CliError> {
    store.project_instance_id()?.ok_or_else(|| {
        CliError::InvalidInput("repository graph requires a bound project identity".to_string())
    })
}

/// Return the pending graph generation after one successful publication.
fn next_generation(base: IndexGeneration) -> Result<IndexGeneration, CliError> {
    base.checked_next().ok_or_else(|| {
        CliError::InvalidInput("repository graph generation is exhausted".to_string())
    })
}

/// Insert one stable entity while rejecting digest collisions.
fn insert_entity(
    entities: &mut BTreeMap<String, GraphEntity>,
    entity: GraphEntity,
) -> Result<(), CliError> {
    let digest = entity.key().digest().to_string();
    if let Some(existing) = entities.get(&digest) {
        if !existing
            .key()
            .reconcile(entity.key())
            .map_err(invalid_graph_contract)?
        {
            return Err(CliError::InvalidInput(
                "graph entity digest retained conflicting ownership".to_string(),
            ));
        }
        return Ok(());
    }
    entities.insert(digest, entity);
    Ok(())
}

/// Sort and deduplicate entity export bindings by canonical identity and owner.
fn sort_dedup_exports(exports: &mut Vec<EntityResolutionKey>) {
    exports.sort_by(|left, right| {
        left.key()
            .cmp(right.key())
            .then_with(|| left.entity().digest().cmp(right.entity().digest()))
    });
    exports.dedup();
}

/// Sort and deduplicate relation dependency bindings by canonical key and owner.
fn sort_dedup_dependencies(dependencies: &mut Vec<RelationDependencyKey>) {
    dependencies.sort_by(|left, right| {
        left.key()
            .cmp(right.key())
            .then_with(|| left.relation().digest().cmp(right.relation().digest()))
    });
    dependencies.dedup();
}

/// Enforce the complete-projection ceiling for canonical key bindings.
fn enforce_key_binding_limit(count: usize) -> Result<(), CliError> {
    let observed = u64::try_from(count).unwrap_or(u64::MAX);
    if observed > MAX_GRAPH_KEY_BINDINGS {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            IndexWorkResource::RelationRows,
            MAX_GRAPH_KEY_BINDINGS,
            observed,
        )
        .into());
    }
    Ok(())
}

/// Enforce one path/key count before incremental graph work expands further.
fn enforce_incremental_count<T>(
    root: &Path,
    _context: &'static str,
    count: usize,
    sample: &BTreeSet<T>,
) -> Result<(), CliError>
where
    T: ToString + Ord,
{
    if count > MAX_INCREMENTAL_RESOLUTION_ITEMS as usize {
        return Err(dependency_closure_limit(
            root,
            sample.iter().map(ToString::to_string),
            count,
        ));
    }
    Ok(())
}

/// Load and admit the persisted rows that an incremental replacement will remove.
fn admitted_persisted_footprint(
    store: &AtlasStore,
    project: ProjectInstanceId,
    root: &Path,
    affected_paths: &BTreeSet<String>,
    control: &IndexWorkControl,
) -> Result<RepositoryAffectedSourceFootprint, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let footprint = store.repository_affected_source_footprint(
        project,
        &affected_paths.iter().cloned().collect::<Vec<_>>(),
        u32::try_from(MAX_INCREMENTAL_GRAPH_ROWS).unwrap_or(u32::MAX),
    )?;
    control.check(IndexWorkStage::SymbolParsing)?;
    if footprint.truncated {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            usize::try_from(footprint.rows).unwrap_or(usize::MAX),
        ));
    }
    enforce_incremental_projection_budget(
        root,
        affected_paths,
        footprint.rows,
        footprint.retained_bytes,
    )?;
    Ok(footprint)
}

/// Enforce aggregate old-removal and new-insertion work for one closure.
fn enforce_incremental_projection_limits(
    root: &Path,
    affected_paths: &BTreeSet<String>,
    persisted: RepositoryAffectedSourceFootprint,
    staged: &StagedRepositoryGraph,
) -> Result<(), CliError> {
    let staged_rows = [
        affected_paths.len(),
        staged.entities.len(),
        staged.relations.len(),
        staged.occurrences.len(),
        staged.coverage.len(),
        staged.entity_exports.len(),
        staged.relation_dependencies.len(),
        staged.document_unresolved_reasons.len(),
    ]
    .into_iter()
    .fold(0_u64, |total, count| {
        total.saturating_add(u64::try_from(count).unwrap_or(u64::MAX))
    });
    enforce_incremental_projection_budget(
        root,
        affected_paths,
        persisted.rows.saturating_add(staged_rows),
        persisted
            .retained_bytes
            .saturating_add(staged.retained_bytes),
    )
}

/// Return typed full-refresh guidance when an incremental projection exceeds a bound.
fn enforce_incremental_projection_budget(
    root: &Path,
    affected_paths: &BTreeSet<String>,
    rows: u64,
    retained_bytes: u64,
) -> Result<(), CliError> {
    if rows > MAX_INCREMENTAL_GRAPH_ROWS || retained_bytes > MAX_INCREMENTAL_GRAPH_BYTES {
        return Err(dependency_closure_limit(
            root,
            affected_paths.iter().cloned(),
            affected_paths.len(),
        ));
    }
    Ok(())
}

/// Construct deterministic full-refresh guidance for an oversized dependency closure.
fn dependency_closure_limit(
    root: &Path,
    sample: impl IntoIterator<Item = String>,
    observed: usize,
) -> CliError {
    CliError::RefreshRequired(Box::new(IndexRefreshRequired {
        project_root: normalize_native_path_display(root),
        worktree: None,
        status: IndexReadStatus::RefreshRequired,
        reason: IndexRefreshReason::DependencyClosureLimit,
        scope: IndexRefreshScope::Full,
        changed: observed,
        added: 0,
        removed: 0,
        modified: observed,
        sample_paths: sample
            .into_iter()
            .take(INDEX_FRESHNESS_SAMPLE_LIMIT)
            .collect(),
    }))
}

/// Translate a graph-domain contract violation for the CLI boundary.
fn invalid_graph_contract(error: GraphContractError) -> CliError {
    CliError::from(error)
}

impl From<GraphContractError> for CliError {
    fn from(error: GraphContractError) -> Self {
        Self::InvalidInput(format!("repository graph projection failed: {error}"))
    }
}

/// Count conservative retained canonical resolution-key bytes.
fn resolution_retained_bytes(projection: &ResolutionKeyProjection) -> u64 {
    projection
        .source_keys()
        .iter()
        .chain(
            projection
                .symbol_keys()
                .iter()
                .flat_map(projectatlas_symbols::SymbolResolutionKeys::keys),
        )
        .chain(
            projection
                .relation_keys()
                .iter()
                .flat_map(projectatlas_symbols::RelationResolutionKeys::keys),
        )
        .fold(0_u64, |bytes, key| {
            bytes
                .saturating_add(32)
                .saturating_add(key.canonical_identity().len() as u64)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        CliError, DOCUMENT_PROJECTION_ROW_BYTES, DocumentResolutionIndex, DocumentTargetIdentity,
        GRAPH_STAGE_DATABASE_FILE_NAME, GRAPH_STAGE_DIRECTORY_PREFIX, GraphOwners,
        GraphSymbolIndex, MAX_IN_MEMORY_GRAPH_WORK_BYTES, MAX_INCREMENTAL_GRAPH_BYTES,
        MAX_INCREMENTAL_GRAPH_ROWS, PackageIndex, ProjectResolutionRegistry,
        QUALIFIED_SYMBOL_SCOPE_PREFIX, RepositoryGraphMutation, StagedRepositoryGraph,
        build_entity_projection, build_entity_projection_with_config,
        cleanup_abandoned_graph_staging, document_casefold_resolution_key, document_coverage,
        document_fact_map_retained_bytes, document_projection_retained_bytes,
        enforce_incremental_projection_budget, enforce_incremental_projection_limits,
        explicit_external_selector, finish_projection, finish_projection_in_database,
        finish_projection_in_database_with_documents, finish_projection_with_documents,
        insert_relation, is_cargo_manifest_path, normalize_document_target, project_document_rows,
        qualified_symbol_identity, qualified_symbol_parents, registry_resolution_matches,
        relation_resolution, remove_owned_graph_stage_payload, repository_path_belongs_to,
        resolution_registry_from_exports, rust_toolchain_identity, source_symbol_identity,
        stage_full_repository_graph, stage_incremental_repository_graph, try_graph_stage_lease,
    };
    use crate::runtime::{
        IndexRefreshReason, IndexRefreshScope, SymbolBuildReport, SymbolBuildStage,
        SymbolParseSuccess, SymbolProjectionChange,
    };
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageScope, CoverageState,
        DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector, ExtendedRelationKind,
        GraphEntity, GraphIdentityText, GraphLimitKind, GraphLimits, GraphRelationKind,
        LogicalRelation, MAX_GRAPH_IDENTITY_BYTES, PackageSelector, ProjectInstanceId,
        RelationDependencyKey, RelationResolution, RepositoryFilePath, RepositoryNodePath,
        ResolutionKeyDomain, ReusableTargetSelector, SymbolSelector,
    };
    use projectatlas_core::relation_capabilities::{
        RELATION_FAMILY_CAPABILITIES, RelationFamilyState,
    };
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SourceParseMetadata, SymbolGraph, SymbolKind,
        SymbolRelation,
    };
    use projectatlas_core::{
        IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkStage,
        Node, NodeKind,
    };
    use projectatlas_db::{
        AtlasStore, RepositoryAffectedSourceFootprint, RepositoryGraphRelationQuery,
    };
    use projectatlas_fs::{RootScanPolicy, ScanOptions};
    use projectatlas_symbols::extract_symbol_graph;
    use projectatlas_symbols::{
        ConfiguredModuleResolution, EcmaScriptConfigKind, EcmaScriptModuleConfig,
        EcmaScriptPathMapping, MAX_DOCUMENT_LINK_CANDIDATES, MAX_DOCUMENT_SELECTOR_BYTES,
        MAX_MARKDOWN_EVIDENCE_BYTES, MAX_MARKDOWN_LABEL_BYTES, MarkdownFactLimit,
    };
    use std::borrow::Cow;
    use std::collections::{BTreeMap, BTreeSet};
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use std::num::NonZeroU32;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(source) if source.raw_os_error() == Some(1314) => {
                let status = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(link)
                    .arg(target)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(source)
                }
            }
            Err(source) => Err(source),
        }
    }

    #[cfg(unix)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[test]
    fn cargo_package_ownership_uses_the_longest_repository_prefix() -> Result<(), Box<dyn Error>> {
        let graphs = vec![
            package_graph("Cargo.toml", "workspace"),
            package_graph("crates/member/Cargo.toml", "member"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        require_eq(
            &packages.package_name("src/lib.rs"),
            &Some("workspace"),
            "root package ownership",
        )?;
        require_eq(
            &packages.package_name("crates/member/src/lib.rs"),
            &Some("member"),
            "nested package ownership",
        )?;
        require(
            repository_path_belongs_to("crates/member/src/lib.rs", "crates/member"),
            "member path was not owned by its package",
        )?;
        require(
            !repository_path_belongs_to("crates/membership/src/lib.rs", "crates/member"),
            "package prefix matched a partial segment",
        )?;
        require(is_cargo_manifest_path("Cargo.toml"), "root manifest")?;
        require(
            is_cargo_manifest_path("crates/member/Cargo.toml"),
            "nested manifest",
        )?;
        require(
            !is_cargo_manifest_path("docs/Cargo.toml.example"),
            "manifest suffix lookalike",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_projection_budget_requests_one_complete_refresh() -> Result<(), Box<dyn Error>> {
        let root = Path::new("repository");
        let affected_paths = BTreeSet::from(["src/lib.rs".to_string()]);
        for (rows, retained_bytes) in [
            (MAX_INCREMENTAL_GRAPH_ROWS + 1, 0),
            (0, MAX_INCREMENTAL_GRAPH_BYTES + 1),
        ] {
            let error =
                enforce_incremental_projection_budget(root, &affected_paths, rows, retained_bytes)
                    .err()
                    .ok_or_else(|| io::Error::other("oversized closure reached publication"))?;
            let CliError::RefreshRequired(report) = error else {
                return Err(io::Error::other(format!(
                    "expected typed full-refresh guidance, found {error:?}"
                ))
                .into());
            };
            require_eq(
                &report.reason,
                &IndexRefreshReason::DependencyClosureLimit,
                "incremental budget reason",
            )?;
            require_eq(
                &report.scope,
                &IndexRefreshScope::Full,
                "incremental budget scope",
            )?;
            require_eq(&report.changed, &1, "incremental changed paths")?;
            require_eq(
                &report.sample_paths,
                &vec!["src/lib.rs".to_string()],
                "incremental sample paths",
            )?;
        }
        Ok(())
    }

    #[test]
    fn incremental_projection_budget_combines_old_and_new_work() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let affected_paths = BTreeSet::from(["src/lib.rs".to_string()]);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let persisted = RepositoryAffectedSourceFootprint {
            rows: MAX_INCREMENTAL_GRAPH_ROWS,
            retained_bytes: 0,
            truncated: false,
        };
        let staged = StagedRepositoryGraph {
            project: ProjectInstanceId::from_bytes([1; 16])?,
            mutation: RepositoryGraphMutation::AffectedPaths(vec!["src/lib.rs".to_string()]),
            entities: Vec::new(),
            relations: Vec::new(),
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: Vec::new(),
            relation_dependencies: Vec::new(),
            document_unresolved_reasons: Vec::new(),
            scan_policy: RootScanPolicy::discover(root, &ScanOptions::default(), &control)?,
            document_target_states: Vec::new(),
            database: None,
            retained_bytes: 0,
        };
        let error =
            enforce_incremental_projection_limits(root, &affected_paths, persisted, &staged)
                .err()
                .ok_or_else(|| io::Error::other("combined old and new work was admitted"))?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "combined budget reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "combined budget scope",
        )?;
        Ok(())
    }

    #[test]
    fn document_projection_memory_budget_includes_reason_validation_state()
    -> Result<(), Box<dyn Error>> {
        let facts: BTreeMap<String, Cow<'static, projectatlas_symbols::MarkdownFacts>> = (0..12)
            .map(|document| {
                let source = (0..1_024)
                    .map(|link| format!("[missing]({document}-{link}.md)"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    format!("docs/source-{document}.md"),
                    Cow::Owned(projectatlas_symbols::extract_markdown_facts(&source)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let candidate_count = facts
            .values()
            .map(|document| document.link_candidates.len())
            .sum::<usize>();
        require_eq(&candidate_count, &12_288, "document candidate fixture size")?;
        let fact_bytes = document_fact_map_retained_bytes(&facts);
        let projection_bytes = document_projection_retained_bytes(&facts);
        require(
            projection_bytes >= u64::try_from(candidate_count)? * DOCUMENT_PROJECTION_ROW_BYTES,
            "generated document projection state was under-accounted",
        )?;
        require(
            fact_bytes.saturating_add(projection_bytes) < 512 * 1_024 * 1_024,
            "bounded document projection fixture exceeded the in-memory envelope",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_document_projection_overflow_requests_full_refresh_staging()
    -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 513;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("incremental document budget project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let template = projectatlas_symbols::extract_markdown_facts("[missing](shared.md)");
        let candidate = template
            .link_candidates
            .first()
            .cloned()
            .ok_or("document budget candidate fixture is missing")?;
        let mut template = template;
        while template.link_candidates.len() < MAX_DOCUMENT_LINK_CANDIDATES {
            template.link_candidates.push(candidate.clone());
        }

        let mut nodes = Vec::with_capacity(DOCUMENT_COUNT);
        let mut graphs = Vec::with_capacity(DOCUMENT_COUNT);
        let mut changes = Vec::with_capacity(DOCUMENT_COUNT);
        let mut paths = Vec::with_capacity(DOCUMENT_COUNT);
        for document in 0..DOCUMENT_COUNT {
            let path = format!("content/source-{document:03}.md");
            let graph = template.symbol_graph(&path, Some("markdown"));
            nodes.push(test_file_node(&path, "markdown"));
            paths.push(path.clone());
            graphs.push(graph.clone());
            changes.push(SymbolProjectionChange::Parsed(SymbolParseSuccess {
                path,
                graph,
                markdown_facts: Some(Box::new(template.clone())),
                source_parser: ParserKind::Structural,
                summary: String::new(),
                summary_is_structural: true,
                purpose_suggestion: None,
            }));
        }
        store.replace_scan(&nodes)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entities = build_entity_projection(
            project,
            IndexGeneration::new(1),
            &nodes,
            &graphs,
            &packages,
            false,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let projected_facts = paths
            .iter()
            .map(|path| (path.clone(), Cow::Borrowed(&template)))
            .collect::<BTreeMap<_, _>>();
        let pre_document_bytes = entities
            .retained_bytes
            .saturating_add(candidates.retained_bytes)
            .saturating_add(document_fact_map_retained_bytes(&projected_facts));
        let document_projection_bytes = document_projection_retained_bytes(&projected_facts);
        require(
            pre_document_bytes < MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            "pre-document incremental state already exceeded the staging budget",
        )?;
        require(
            pre_document_bytes.saturating_add(document_projection_bytes)
                > MAX_IN_MEMORY_GRAPH_WORK_BYTES,
            "document projection did not cross the aggregate staging budget",
        )?;

        let mut symbols = empty_symbol_build_stage();
        symbols.changes = changes;
        let direct_paths = paths.clone();
        let error = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &direct_paths,
            &scan_policy,
            &symbols,
            &control,
        )
        .err()
        .ok_or("document projection overflow was admitted incrementally")?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "document projection overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "document projection overflow scope",
        )?;

        let staged = stage_full_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require(
            staged.database.is_some(),
            "full-refresh guidance did not select disposable SQLite staging",
        )?;
        Ok(())
    }

    #[test]
    fn resolution_registry_reuses_staged_export_entities() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([2; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"atlas\"\n",
            ),
            extract_symbol_graph("src/lib.rs", Some("rust"), "pub fn run() {}\n"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let registry = resolution_registry_from_exports(&projection, &control)?;
        let registered_bindings = registry
            .candidate_digests_by_key
            .values()
            .map(BTreeSet::len)
            .sum::<usize>();

        require_eq(
            &registry.supplemental_entities_by_digest.len(),
            &0,
            "duplicate registry-owned entity count",
        )?;
        require_eq(
            &registered_bindings,
            &projection.entity_exports.len(),
            "registry key binding count",
        )?;
        require(
            registry
                .candidate_digests_by_key
                .values()
                .flatten()
                .all(|digest| projection.entity_by_digest.contains_key(digest)),
            "resolution key referenced an unstaged entity digest",
        )?;
        let mut bindings_per_entity = BTreeMap::<&str, usize>::new();
        for digest in registry.candidate_digests_by_key.values().flatten() {
            *bindings_per_entity.entry(digest).or_default() += 1;
        }
        require(
            bindings_per_entity.values().any(|count| *count > 1),
            "fixture did not exercise one entity exported under multiple canonical keys",
        )?;
        Ok(())
    }

    #[test]
    fn restart_cleanup_removes_only_inactive_owned_graph_stages() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("restart-cleanup");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let database = atlas_dir.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;

        let owned = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}owned"));
        fs::create_dir(&owned)?;
        let owned_database = owned.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        drop(AtlasStore::create_repository_graph_staging(
            &owned_database,
            &root,
            project,
        )?);
        let owned_payload = owned.join("payload");
        fs::create_dir(&owned_payload)?;
        fs::write(owned_payload.join("row"), "discard")?;
        let owned_link_target = temp.path().join("owned-link-target");
        fs::create_dir(&owned_link_target)?;
        fs::write(owned_link_target.join("sentinel"), "preserve")?;
        let owned_payload_link = owned.join("linked-payload");
        create_directory_link(&owned_link_target, &owned_payload_link)?;
        let interrupted_shell =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}interrupted-shell"));
        fs::create_dir(&interrupted_shell)?;
        let unvalidated_nonempty = atlas_dir.join(format!(
            "{GRAPH_STAGE_DIRECTORY_PREFIX}unvalidated-nonempty"
        ));
        fs::create_dir(&unvalidated_nonempty)?;
        fs::write(unvalidated_nonempty.join("sentinel"), "preserve")?;
        let lookalike = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}lookalike"));
        fs::create_dir(&lookalike)?;
        drop(AtlasStore::open_for_project(
            &lookalike.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
        )?);
        let foreign_root = temp.path().join("foreign-project");
        fs::create_dir(&foreign_root)?;
        let foreign_store =
            AtlasStore::open_for_project(&foreign_root.join("projectatlas.db"), &foreign_root)?;
        let foreign_project = foreign_store
            .project_instance_id()?
            .ok_or("foreign project identity is missing")?;
        let foreign_project_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}foreign-project"));
        fs::create_dir(&foreign_project_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &foreign_project_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
            foreign_project,
        )?);
        let foreign_root_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}foreign-root"));
        fs::create_dir(&foreign_root_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &foreign_root_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &foreign_root,
            project,
        )?);
        let linked_stage_target = temp.path().join("linked-stage-target");
        fs::create_dir(&linked_stage_target)?;
        fs::write(linked_stage_target.join("sentinel"), "preserve")?;
        drop(AtlasStore::create_repository_graph_staging(
            &linked_stage_target.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            &root,
            project,
        )?);
        let linked_stage = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}linked"));
        create_directory_link(&linked_stage_target, &linked_stage)?;

        let linked_database_target = temp.path().join("linked-stage-database.db");
        drop(AtlasStore::create_repository_graph_staging(
            &linked_database_target,
            &root,
            project,
        )?);
        let linked_database_stage =
            atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}linked-database"));
        fs::create_dir(&linked_database_stage)?;
        let linked_database = linked_database_stage.join(GRAPH_STAGE_DATABASE_FILE_NAME);
        let linked_database_created =
            match create_file_link(&linked_database_target, &linked_database) {
                Ok(()) => true,
                #[cfg(windows)]
                Err(source) if source.raw_os_error() == Some(1314) => false,
                Err(source) => return Err(source.into()),
            };
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let lease = try_graph_stage_lease(&atlas_dir)?
            .ok_or("test could not acquire graph staging lease")?;
        remove_owned_graph_stage_payload(&owned, &owned_database, Some(&control))?;
        require(
            owned_database.is_file() && !owned_payload.exists(),
            "payload cleanup did not retain the ownership database until last",
        )?;
        require(
            fs::symlink_metadata(&owned_payload_link).is_err()
                && owned_link_target.join("sentinel").is_file(),
            "payload cleanup followed or retained a linked child",
        )?;
        cleanup_abandoned_graph_staging(&root, project, &control)?;
        require(
            owned.exists(),
            "restart cleanup removed an actively leased stage",
        )?;
        drop(lease);

        let canceled_control = IndexWorkControl::new(IndexCancellation::new(), None);
        canceled_control.cancel();
        let canceled = cleanup_abandoned_graph_staging(&root, project, &canceled_control)
            .err()
            .ok_or("canceled restart cleanup unexpectedly succeeded")?;
        require(
            matches!(
                canceled,
                CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::Publication
                })
            ),
            "restart cleanup did not preserve typed cancellation",
        )?;
        require(
            owned.exists(),
            "canceled restart cleanup removed an owned stage",
        )?;

        cleanup_abandoned_graph_staging(&root, project, &control)?;
        require(
            !owned.exists(),
            "restart cleanup retained an inactive owned stage",
        )?;
        require(
            !interrupted_shell.exists(),
            "restart cleanup retained an empty interrupted stage shell",
        )?;
        require(
            unvalidated_nonempty.join("sentinel").is_file(),
            "restart cleanup removed a non-empty unvalidated stage",
        )?;
        require(
            lookalike.exists(),
            "restart cleanup removed an unvalidated lookalike stage",
        )?;
        require(
            foreign_project_stage.exists(),
            "restart cleanup removed a valid stage owned by another project",
        )?;
        require(
            foreign_root_stage.exists(),
            "restart cleanup removed a valid stage bound to another root",
        )?;
        require(
            fs::symlink_metadata(&linked_stage).is_ok()
                && linked_stage_target.join("sentinel").is_file(),
            "restart cleanup followed a linked stage directory",
        )?;
        if linked_database_created {
            require(
                fs::symlink_metadata(&linked_database).is_ok() && linked_database_target.is_file(),
                "restart cleanup followed a linked staging database",
            )?;
        }
        Ok(())
    }

    #[test]
    fn restart_cleanup_observes_cancellation_between_owned_stages() -> Result<(), Box<dyn Error>> {
        const STAGE_COUNT: usize = 64;
        const FILES_PER_STAGE: usize = 64;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("restart-cleanup-cancellation");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let store =
            AtlasStore::open_for_project(&atlas_dir.join(GRAPH_STAGE_DATABASE_FILE_NAME), &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let mut stages = Vec::with_capacity(STAGE_COUNT);
        for stage_index in 0..STAGE_COUNT {
            let stage = atlas_dir.join(format!("{GRAPH_STAGE_DIRECTORY_PREFIX}{stage_index:03}"));
            fs::create_dir(&stage)?;
            drop(AtlasStore::create_repository_graph_staging(
                &stage.join(GRAPH_STAGE_DATABASE_FILE_NAME),
                &root,
                project,
            )?);
            let payload = stage.join("payload");
            fs::create_dir(&payload)?;
            for file_index in 0..FILES_PER_STAGE {
                fs::write(payload.join(format!("{file_index:03}")), b"x")?;
            }
            stages.push(stage);
        }

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let worker_root = root;
        let worker =
            thread::spawn(move || cleanup_abandoned_graph_staging(&worker_root, project, &control));
        let observation_deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = stages.iter().filter(|stage| stage.exists()).count();
            if remaining < STAGE_COUNT {
                cancellation.cancel();
                break;
            }
            if worker.is_finished() {
                return Err(io::Error::other(
                    "restart cleanup completed before in-flight cancellation was observed",
                )
                .into());
            }
            if Instant::now() >= observation_deadline {
                cancellation.cancel();
                return Err(io::Error::other(
                    "restart cleanup removed no stage within the test deadline",
                )
                .into());
            }
            thread::yield_now();
        }
        let result = worker
            .join()
            .map_err(|_panic| io::Error::other("restart cleanup worker panicked"))?;
        require(
            matches!(
                result,
                Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::Publication
                }))
            ),
            "restart cleanup did not return typed in-flight cancellation",
        )?;
        let remaining = stages.iter().filter(|stage| stage.exists()).count();
        require(
            remaining > 0 && remaining < STAGE_COUNT,
            "in-flight cancellation did not preserve a partial cleanup boundary",
        )
    }

    #[test]
    fn staged_database_owner_retains_incomplete_creation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir(&atlas_dir)?;
        let directory = tempfile::Builder::new()
            .prefix(GRAPH_STAGE_DIRECTORY_PREFIX)
            .tempdir_in(&atlas_dir)?;
        let staging_path = directory.path().to_path_buf();
        fs::write(
            staging_path.join(GRAPH_STAGE_DATABASE_FILE_NAME),
            "incomplete",
        )?;
        fs::write(staging_path.join("payload"), "preserve")?;
        let lease = try_graph_stage_lease(&atlas_dir)?
            .ok_or("test could not acquire graph staging lease")?;
        let owner = super::StagedGraphDatabase {
            store: None,
            directory: Some(directory),
            _lease: lease,
        };

        drop(owner);

        require(
            staging_path.join(GRAPH_STAGE_DATABASE_FILE_NAME).is_file()
                && staging_path.join("payload").is_file(),
            "incomplete staging creation was recursively deleted",
        )
    }

    #[test]
    fn database_staging_publishes_and_removes_its_disposable_store() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("database-staging");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let database = atlas_dir.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        )];
        let nodes = vec![test_file_node("src/lib.rs", "rust")];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = finish_projection_in_database(
            &root,
            &nodes,
            project,
            generation,
            &graphs,
            projection,
            &candidates,
            &scan_policy,
            &control,
        )?;
        let staging_path = staged
            .database
            .as_ref()
            .ok_or("database staging was not selected")?
            .directory()?
            .path()
            .to_path_buf();
        let drop_payload = staging_path.join("drop-payload");
        fs::create_dir(&drop_payload)?;
        fs::write(drop_payload.join("row"), "discard")?;
        let drop_link_target = temp.path().join("drop-link-target");
        fs::create_dir(&drop_link_target)?;
        fs::write(drop_link_target.join("sentinel"), "preserve")?;
        create_directory_link(&drop_link_target, &staging_path.join("drop-linked-payload"))?;
        require(
            staging_path.exists(),
            "database staging directory is missing",
        )?;
        {
            let mut publication = store.begin_index_publication("database-staging")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(staged);
        require(
            !staging_path.exists(),
            "database staging directory survived publication",
        )?;
        require(
            drop_link_target.join("sentinel").is_file(),
            "database staging drop followed a linked payload",
        )?;
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let coverage = reader.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/lib.rs"))?,
            },
            8,
        )?;
        require(
            !coverage.truncated,
            "database-staged coverage was truncated",
        )?;
        require(
            !coverage.rows.is_empty(),
            "database-staged graph rows were not published",
        )?;
        Ok(())
    }

    #[test]
    fn accepted_relation_families_publish_and_reopen() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("accepted-relation-families");
        for directory in ["src", "tests", "config", "infra", "data"] {
            fs::create_dir_all(root.join(directory))?;
        }
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("relation inventory identity is missing"))?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                concat!(
                    "[package]\nname = \"relation-fixture\"\nversion = \"0.1.0\"\n",
                    "\n[dependencies]\nserde = \"1\"\n",
                ),
            ),
            extract_symbol_graph(
                "src/lib.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "pub struct Router { pub enabled: bool }\n",
                    "impl Router { pub fn install(&self) {} }\n",
                    "pub fn handler() {}\n",
                    "pub fn register() {\n",
                    "    route(\"/health\", handler);\n",
                    "    let _ = std::env::var(\"ATLAS_MODE\").unwrap_or_else(|_| \"super-secret\".into());\n",
                    "    let _ = fs::read_to_string(\"data/input.txt\");\n",
                    "    let _ = fs::write(\"data/output.txt\", \"ok\");\n",
                    "}\n",
                    "fn route(_path: &str, _handler: fn()) {}\n",
                ),
            ),
            extract_symbol_graph(
                "tests/feature_test.rs",
                Some("rust"),
                "fn subject() {}\nfn verifies_subject() { subject(); }\n",
            ),
            extract_symbol_graph(
                "config/appsettings.json",
                Some("json"),
                "{\"token\":\"super-secret\"}\n",
            ),
            extract_symbol_graph(
                "infra/main.tf",
                Some("terraform"),
                "resource \"null_resource\" \"fixture\" {}\n",
            ),
            extract_symbol_graph("data/input.txt", None, "input\n"),
            extract_symbol_graph("data/output.txt", None, ""),
        ];
        let nodes = graphs
            .iter()
            .map(|graph| {
                test_file_node(&graph.path, graph.language.as_deref().unwrap_or("unknown"))
            })
            .collect::<Vec<_>>();
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        {
            let mut publication = store.begin_index_publication("accepted-relation-families")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        for capability in RELATION_FAMILY_CAPABILITIES
            .iter()
            .filter(|capability| capability.state == RelationFamilyState::Active)
        {
            for &family in capability.graph_relations {
                let page = reader.repository_graph_relations(
                    RepositoryGraphRelationQuery::Family { relation: family },
                    128,
                )?;
                require(!page.truncated, &format!("{family:?} page was truncated"))?;
                require(
                    !page.rows.is_empty(),
                    &format!("{family:?} had no reopened persisted relation"),
                )?;
                for relation in page.rows {
                    let occurrences = reader.repository_graph_occurrences(&relation, 32)?;
                    require(
                        !occurrences.rows.is_empty() && !occurrences.truncated,
                        &format!("{family:?} lost exact source occurrences"),
                    )?;
                    require(
                        !format!("{relation:?}").contains("super-secret"),
                        &format!("secret value escaped into persisted {family:?} relation"),
                    )?;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn accepted_relation_families_abstain_without_static_evidence() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([13; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/dynamic.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "struct Client;\n",
                    "impl Client { fn get(&self, _path: &str, _handler: fn()) {} }\n",
                    "fn handler() {}\n",
                    "fn dynamic(client: &Client, route_path: &str, key: &str, file: &str) {\n",
                    "    client.get(\"/health\", handler);\n",
                    "    route(route_path, handler);\n",
                    "    let _ = std::env::var(key);\n",
                    "    let _ = fs::read_to_string(file);\n",
                    "    let _ = fs::write(file, \"super-secret\");\n",
                    "}\n",
                    "fn route(_path: &str, _handler: fn()) {}\n",
                ),
            ),
            extract_symbol_graph(
                "src/escaping.rs",
                Some("rust"),
                concat!(
                    "use std::fs;\n",
                    "fn unsafe_paths() {\n",
                    "    let _ = fs::read_to_string(\"../secret.txt\");\n",
                    "    let _ = fs::write(\"C:/secret.txt\", \"super-secret\");\n",
                    "}\n",
                ),
            ),
            extract_symbol_graph(
                "src/dynamic.js",
                Some("javascript"),
                concat!(
                    "function handler() {}\n",
                    "function middleware() {}\n",
                    "function route(...args) {}\n",
                    "route(\"/health\", handler, middleware);\n",
                ),
            ),
            extract_symbol_graph("docs/main.tf.example", None, "not infrastructure\n"),
            extract_symbol_graph("docs/k8s/README.md", None, "not infrastructure\n"),
            extract_symbol_graph("docs/cloudformation-notes.md", None, "not infrastructure\n"),
            extract_symbol_graph("config/settings.json.bak", None, "super-secret\n"),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        for family in [
            ExtendedRelationKind::Tests,
            ExtendedRelationKind::RoutesTo,
            ExtendedRelationKind::Configures,
            ExtendedRelationKind::Deploys,
            ExtendedRelationKind::Reads,
            ExtendedRelationKind::Writes,
        ] {
            require(
                staged
                    .relations
                    .iter()
                    .all(|relation| relation.kind() != GraphRelationKind::Extended(family)),
                &format!("dynamic or lookalike input fabricated {family:?}"),
            )?;
        }
        require(
            staged
                .entities
                .iter()
                .all(|entity| !entity.key().canonical_identity().contains("super-secret")),
            "negative fixture leaked a secret into graph identity",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_identity_preserves_boundaries_and_compacts_stably()
    -> Result<(), Box<dyn Error>> {
        let name = GraphIdentityText::new("leaf")?;
        require_eq(
            &qualified_symbol_identity(&GraphIdentityText::new("outer")?, &name)?,
            &GraphIdentityText::new("outer::leaf")?,
            "shallow qualified identity",
        )?;

        let exact_parent = GraphIdentityText::new(
            "x".repeat(MAX_GRAPH_IDENTITY_BYTES - "::".len() - name.as_str().len()),
        )?;
        let exact = qualified_symbol_identity(&exact_parent, &name)?;
        require_eq(
            &exact.as_str().len(),
            &MAX_GRAPH_IDENTITY_BYTES,
            "exact-boundary qualified identity bytes",
        )?;
        require(
            exact.as_str().ends_with("::leaf"),
            "exact-boundary qualified identity changed its readable suffix",
        )?;

        let first_overbound_parent = GraphIdentityText::new(
            "x".repeat(MAX_GRAPH_IDENTITY_BYTES - "::".len() - name.as_str().len() + 1),
        )?;
        let compact = qualified_symbol_identity(&first_overbound_parent, &name)?;
        require(
            compact.as_str().len() <= MAX_GRAPH_IDENTITY_BYTES,
            "first overbound qualified identity remained oversized",
        )?;
        require(
            compact.as_str().ends_with("::leaf"),
            "compacted qualified identity lost its nearest symbol name",
        )?;
        require_eq(
            &qualified_symbol_identity(&first_overbound_parent, &name)?,
            &compact,
            "repeated compact qualified identity",
        )?;

        let multibyte_parent = GraphIdentityText::new(format!(
            "{}x",
            "é".repeat((MAX_GRAPH_IDENTITY_BYTES - "::".len() - "界".len() - 1) / "é".len())
        ))?;
        let multibyte =
            qualified_symbol_identity(&multibyte_parent, &GraphIdentityText::new("界")?)?;
        require_eq(
            &multibyte.as_str().len(),
            &MAX_GRAPH_IDENTITY_BYTES,
            "multibyte exact-boundary qualified identity bytes",
        )?;
        require(
            multibyte.as_str().ends_with("::界"),
            "multibyte qualified identity changed its readable suffix",
        )?;

        let parent_a = GraphIdentityText::new("a".repeat(MAX_GRAPH_IDENTITY_BYTES))?;
        let parent_b = GraphIdentityText::new("b".repeat(MAX_GRAPH_IDENTITY_BYTES))?;
        let scoped_a = qualified_symbol_identity(&parent_a, &name)?;
        let scoped_b = qualified_symbol_identity(&parent_b, &name)?;
        require(
            scoped_a != scoped_b,
            "distinct deep ancestors with an equal suffix shared one identity",
        )?;
        require(
            scoped_a != qualified_symbol_identity(&parent_a, &GraphIdentityText::new("other")?)?,
            "distinct overbound candidates shared one compact identity",
        )?;
        let (literal_parent, _) = scoped_a
            .as_str()
            .rsplit_once("::")
            .ok_or_else(|| io::Error::other("compact scope omitted its readable suffix"))?;
        require(
            source_symbol_identity(literal_parent.to_string()).is_err(),
            "compact scope namespace remained admissible as an exact source parent",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_parents_reject_invalid_raw_components() -> Result<(), Box<dyn Error>> {
        for (name, parent) in [
            ("bad\nname".to_string(), None),
            ("valid".to_string(), Some("bad\nparent".to_string())),
            (
                format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}literal"),
                Some("parent".to_string()),
            ),
            (
                "valid".to_string(),
                Some(format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}literal")),
            ),
            (
                "x".repeat(MAX_GRAPH_IDENTITY_BYTES + 1),
                Some("parent".to_string()),
            ),
        ] {
            let graph = SymbolGraph {
                path: "src/invalid.rs".to_string(),
                language: Some("rust".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![CodeSymbol {
                    path: "src/invalid.rs".to_string(),
                    language: Some("rust".to_string()),
                    name,
                    kind: SymbolKind::Function,
                    signature: "fn valid()".to_string(),
                    exported: false,
                    documentation: None,
                    line_start: 1,
                    line_end: 1,
                    source_selector: None,
                    parent,
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_string()),
                }],
                relations: Vec::new(),
            };
            require(
                qualified_symbol_parents(&graph).is_err(),
                "invalid raw symbol identity reached qualified derivation",
            )?;
        }
        Ok(())
    }

    #[test]
    fn qualified_symbol_parents_bound_four_thousand_deep_scopes() -> Result<(), Box<dyn Error>> {
        const DEPTH: usize = 4_000;
        let names = (0..DEPTH)
            .map(|index| format!("scope_{index:04}_{}", "x".repeat(229)))
            .collect::<Vec<_>>();
        let graph = SymbolGraph {
            path: "src/deep.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: names
                .iter()
                .enumerate()
                .map(|(index, name)| CodeSymbol {
                    path: "src/deep.rs".to_string(),
                    language: Some("rust".to_string()),
                    name: name.clone(),
                    kind: SymbolKind::Module,
                    signature: name.clone(),
                    exported: false,
                    documentation: None,
                    line_start: index + 1,
                    line_end: DEPTH * 2 - index,
                    source_selector: None,
                    parent: index.checked_sub(1).map(|parent| names[parent].clone()),
                    parser: ParserKind::TreeSitter,
                    detail: Some("mod_item".to_string()),
                })
                .collect(),
            relations: Vec::new(),
        };
        let first = qualified_symbol_parents(&graph)?;
        require_eq(&first.len(), &DEPTH, "deep qualified parent count")?;
        require(
            first.first().is_some_and(Option::is_none),
            "deep root unexpectedly gained a parent",
        )?;
        require(
            first
                .iter()
                .flatten()
                .all(|parent| parent.as_str().len() <= MAX_GRAPH_IDENTITY_BYTES),
            "deep qualification retained an oversized parent",
        )?;
        require(
            first
                .iter()
                .flatten()
                .any(|parent| parent.as_str().starts_with("@projectatlas.scope.v1:")),
            "deep qualification never exercised compact scope identity",
        )?;
        require_eq(
            &qualified_symbol_parents(&graph)?,
            &first,
            "repeated deep qualified parents",
        )?;
        Ok(())
    }

    #[test]
    fn qualified_symbol_scopes_produce_distinct_graph_entity_keys() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([3; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        for (path, language, source, expected_parents) in [
            (
                "src/lib.rs",
                "rust",
                concat!(
                    "mod first { struct Runner; impl Runner { fn run(&self) {} } }\n",
                    "mod second { struct Runner; impl Runner { fn run(&self) {} } }\n",
                ),
                ["first::Runner", "second::Runner"],
            ),
            (
                "src/Runner.java",
                "java",
                concat!(
                    "class First { class Runner { void run() {} } }\n",
                    "class Second { class Runner { void run() {} } }\n",
                ),
                ["First::Runner", "Second::Runner"],
            ),
        ] {
            let graph = extract_symbol_graph(path, Some(language), source);
            let method_indices = graph
                .symbols
                .iter()
                .enumerate()
                .filter_map(|(index, symbol)| {
                    (symbol.kind == SymbolKind::Method && symbol.name == "run").then_some(index)
                })
                .collect::<Vec<_>>();
            require_eq(&method_indices.len(), &2, "scoped method count")?;
            let first_symbol = &graph.symbols[method_indices[0]];
            let second_symbol = &graph.symbols[method_indices[1]];
            require_eq(
                &first_symbol.parent.as_deref(),
                &Some("Runner"),
                "legacy leaf parent",
            )?;
            require_eq(
                &first_symbol.parent,
                &second_symbol.parent,
                "same legacy leaf parent",
            )?;
            require_eq(
                &first_symbol.signature,
                &second_symbol.signature,
                "same declaration signature",
            )?;

            let graphs = vec![graph];
            let packages = PackageIndex::from_graphs(&graphs)?;
            let projection = build_entity_projection(
                project,
                generation,
                &[],
                &graphs,
                &packages,
                true,
                &control,
            )?;
            let owners = projection
                .owners_by_graph
                .get(path)
                .ok_or("scoped graph owners are missing")?;
            let first = owners.symbol_digests[method_indices[0]]
                .as_ref()
                .and_then(|digest| projection.entity_by_digest.get(digest))
                .ok_or("first scoped method entity is missing")?;
            let second = owners.symbol_digests[method_indices[1]]
                .as_ref()
                .and_then(|digest| projection.entity_by_digest.get(digest))
                .ok_or("second scoped method entity is missing")?;
            let EntitySelector::Symbol {
                symbol: first_selector,
            } = first.selector()
            else {
                return Err("first scoped entity is not a symbol".into());
            };
            let EntitySelector::Symbol {
                symbol: second_selector,
            } = second.selector()
            else {
                return Err("second scoped entity is not a symbol".into());
            };
            require_eq(
                &first_selector
                    .parent
                    .as_ref()
                    .map(GraphIdentityText::as_str),
                &Some(expected_parents[0]),
                "first graph identity parent",
            )?;
            require_eq(
                &second_selector
                    .parent
                    .as_ref()
                    .map(GraphIdentityText::as_str),
                &Some(expected_parents[1]),
                "second graph identity parent",
            )?;
            require(
                first.key() != second.key(),
                "independent semantic scopes shared one graph entity key",
            )?;
        }
        Ok(())
    }

    #[test]
    fn external_classification_follows_effective_semantic_provider() -> Result<(), Box<dyn Error>> {
        for (language, target, system, identity) in [
            ("html", "import fs from \"node:fs\";", "node", "fs"),
            ("svelte", "import url from \"node:url\";", "node", "url"),
            ("vue", "import path from \"node:path\";", "node", "path"),
        ] {
            let case = resolution_case(language, RelationKind::Imports, target, &[]);
            let external = explicit_external_selector(&case.graph, &case.relation)?
                .ok_or("embedded ECMAScript external classification is missing")?;
            require_eq(&external.system.as_str(), &system, "external system")?;
            require_eq(&external.identity.as_str(), &identity, "external identity")?;
        }

        let lock = resolution_case("cargo-lock", RelationKind::DependsOn, "serde-lock", &[]);
        require(
            explicit_external_selector(&lock.graph, &lock.relation)?.is_none(),
            "Cargo.lock was misclassified as an explicit external dependency",
        )?;
        Ok(())
    }

    #[test]
    fn grouped_rust_imports_use_only_their_common_external_module() -> Result<(), Box<dyn Error>> {
        let grouped = resolution_case("rust", RelationKind::Imports, "use std::{fs, io};", &[]);
        require_eq(
            &rust_toolchain_identity(&grouped.relation),
            &Some("std".to_string()),
            "grouped Rust external root",
        )?;
        let nested = resolution_case(
            "rust",
            RelationKind::Imports,
            "use std::fs::{read, write};",
            &[],
        );
        require_eq(
            &rust_toolchain_identity(&nested.relation),
            &Some("std::fs".to_string()),
            "nested grouped Rust external module",
        )?;
        let ordinary = resolution_case("rust", RelationKind::Imports, "use std::fs;", &[]);
        require_eq(
            &rust_toolchain_identity(&ordinary.relation),
            &Some("std::fs".to_string()),
            "ordinary Rust external module",
        )?;
        Ok(())
    }

    #[test]
    fn registry_candidate_merge_deduplicates_exactly_and_observes_cancellation()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([6; 16])?;
        let generation = IndexGeneration::new(1);
        let first_key = test_resolution_key(project, "first-key")?;
        let second_key = test_resolution_key(project, "second-key")?;
        let first = test_symbol_entity(project, generation, "src/first.rs", "first")?;
        let second = test_symbol_entity(project, generation, "src/second.rs", "second")?;
        let mut registry = ProjectResolutionRegistry::default();
        registry.insert_candidate(&first_key, &first)?;
        registry.insert_candidate(&second_key, &first)?;
        registry.insert_candidate(&second_key, &second)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let staged_entities = BTreeMap::new();
        let matches = registry_resolution_matches(
            &[first_key.clone(), second_key.clone()],
            &registry,
            &staged_entities,
            &control,
        )?;
        require_eq(&matches.count, &2, "distinct merged candidate count")?;
        let expected_first = [&first, &second]
            .into_iter()
            .min_by_key(|entity| entity.key().digest())
            .ok_or("candidate fixture is empty")?;
        require_eq(
            &matches.first.map(|entity| entity.key().digest()),
            &Some(expected_first.key().digest()),
            "stable first candidate",
        )?;

        let cancellation = IndexCancellation::new();
        let canceled_control = IndexWorkControl::new(cancellation.clone(), None);
        cancellation.cancel();
        require(
            registry_resolution_matches(
                &[first_key, second_key],
                &registry,
                &staged_entities,
                &canceled_control,
            )
            .is_err(),
            "candidate merge ignored cancellation",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_ambiguous_relations_keep_the_largest_candidate_count() -> Result<(), Box<dyn Error>>
    {
        let project = ProjectInstanceId::from_bytes([15; 16])?;
        let generation = IndexGeneration::new(1);
        let source = test_file_entity(project, generation, "src/duplicate.ts")?;
        let relation = |candidates| -> Result<LogicalRelation, Box<dyn Error>> {
            Ok(LogicalRelation::new(
                &source,
                GraphRelationKind::from_legacy(RelationKind::Contains),
                RelationResolution::Ambiguous {
                    reference: GraphIdentityText::new("declarations")?,
                    candidates: NonZeroU32::new(candidates).ok_or("candidate count was zero")?,
                },
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?)
        };
        let mut relations = BTreeMap::new();
        insert_relation(
            &mut relations,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            0,
        )?;
        insert_relation(
            &mut relations,
            relation(3)?,
            "src/duplicate.ts",
            "logical",
            1,
        )?;
        insert_relation(
            &mut relations,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            2,
        )?;
        let retained = relations
            .into_values()
            .next()
            .ok_or("deduplicated relation was not retained")?;
        require(
            matches!(
                retained.resolution(),
                RelationResolution::Ambiguous { candidates, .. } if candidates.get() == 3
            ),
            "deduplicated ambiguity did not retain the largest candidate count",
        )?;

        let mut conflicting = BTreeMap::new();
        insert_relation(
            &mut conflicting,
            relation(2)?,
            "src/duplicate.ts",
            "logical",
            0,
        )?;
        let different_confidence = LogicalRelation::new(
            &source,
            GraphRelationKind::from_legacy(RelationKind::Contains),
            RelationResolution::Ambiguous {
                reference: GraphIdentityText::new("declarations")?,
                candidates: NonZeroU32::new(2).ok_or("candidate count was zero")?,
            },
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        require(
            insert_relation(
                &mut conflicting,
                different_confidence,
                "src/duplicate.ts",
                "logical",
                1,
            )
            .is_err(),
            "a non-ambiguity conflict was merged",
        )?;
        Ok(())
    }

    #[test]
    fn same_file_private_calls_resolve_and_duplicate_declarations_stay_ambiguous()
    -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([5; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let private_graph = extract_symbol_graph(
            "src/private.rs",
            Some("rust"),
            "pub fn caller() { helper(); }\nfn helper() {}\n",
        );
        require(
            private_graph
                .symbols
                .iter()
                .any(|symbol| symbol.name == "helper" && !symbol.exported),
            "fixture helper was not private",
        )?;
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&private_graph))?;
        let projection = build_entity_projection(
            project,
            generation,
            &[],
            std::slice::from_ref(&private_graph),
            &packages,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&private_graph),
            projection,
            &candidates,
            &control,
        )?;
        let helper_call = staged
            .relations
            .iter()
            .find(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Resolved {
                        selector: ReusableTargetSelector::Symbol { symbol },
                        ..
                    } if symbol.name.as_str() == "helper"
                        && symbol.file.as_str() == "src/private.rs"
                )
            })
            .ok_or("private same-file helper call did not resolve")?;
        require_eq(
            &helper_call.kind(),
            &GraphRelationKind::from_legacy(RelationKind::Calls),
            "private same-file relation kind",
        )?;

        let duplicate_graph = SymbolGraph {
            path: "src/duplicate.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_code_symbol("src/duplicate.rs", "caller", Some("Owner"), "fn caller()"),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Owner"),
                    "fn helper(first: u8)",
                ),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Owner"),
                    "fn helper(second: u16)",
                ),
                test_code_symbol(
                    "src/duplicate.rs",
                    "helper",
                    Some("Unrelated"),
                    "fn helper()",
                ),
            ],
            relations: vec![SymbolRelation {
                path: "src/duplicate.rs".to_string(),
                source_name: "caller".to_string(),
                target_name: "helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };
        let packages = PackageIndex::from_graphs(std::slice::from_ref(&duplicate_graph))?;
        let projection = build_entity_projection(
            project,
            generation,
            &[],
            std::slice::from_ref(&duplicate_graph),
            &packages,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            std::slice::from_ref(&duplicate_graph),
            projection,
            &candidates,
            &control,
        )?;
        require(
            staged.relations.iter().any(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Ambiguous {
                        reference,
                        candidates,
                    } if reference.as_str() == "helper" && candidates.get() == 2
                )
            }),
            "duplicate same-file declarations did not remain ambiguous",
        )?;
        Ok(())
    }

    #[test]
    fn configured_module_targets_reuse_graph_ambiguity_ownership() -> Result<(), Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([19; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/first/controller.ts",
                Some("typescript"),
                "export function useController() { return 'first'; }\n",
            ),
            extract_symbol_graph(
                "src/second/controller.ts",
                Some("typescript"),
                "export function useController() { return 'second'; }\n",
            ),
            extract_symbol_graph(
                "src/page.ts",
                Some("typescript"),
                "import { useController } from '@/controller';\nexport const value = useController();\n",
            ),
        ];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let configured = ConfiguredModuleResolution::new(vec![EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            None,
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/first/*".to_string(), "src/second/*".to_string()],
            )?],
        )?])?;
        let projection = build_entity_projection_with_config(
            project,
            generation,
            &[],
            &graphs,
            &packages,
            &configured,
            true,
            &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        for kind in [RelationKind::Imports, RelationKind::Calls] {
            require(
                staged.relations.iter().any(|relation| {
                    relation.kind() == GraphRelationKind::from_legacy(kind)
                        && matches!(
                            relation.resolution(),
                            RelationResolution::Ambiguous { candidates, .. }
                                if candidates.get() == 2
                        )
                }),
                "configured mapping did not retain all ambiguity candidates",
            )?;
        }
        Ok(())
    }

    #[test]
    fn extracted_provider_matrix_survives_sqlite_publication_and_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("extracted-provider-matrix");
        fs::create_dir_all(&root)?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![
            extract_symbol_graph(
                "src/lib.rs",
                Some("rust"),
                "use std::{fs, io};\npub fn caller() { private_helper(); }\nfn private_helper() {}\n",
            ),
            extract_symbol_graph(
                "src/config.rs",
                Some("rust"),
                "pub fn load_timeout_millis() -> u64 { 250 }\n",
            ),
            extract_symbol_graph(
                "src/handler.rs",
                Some("rust"),
                "use crate::config;\npub fn health_response() { config::load_timeout_millis(); }\n",
            ),
            extract_symbol_graph(
                "src/router.rs",
                Some("rust"),
                "use crate::handler;\npub fn dispatch(path: &str) -> Option<()> { (path == \"/health\").then(handler::health_response) }\n",
            ),
            extract_symbol_graph(
                "src/app.js",
                Some("javascript"),
                "import path from \"node:path\";\nexport function run() { return path.join('a', 'b'); }\n",
            ),
            extract_symbol_graph(
                "src/app.py",
                Some("python"),
                "import requests\n\ndef run():\n    return requests.get('https://example.test')\n",
            ),
            extract_symbol_graph(
                "Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"matrix-app\"\nversion = \"0.1.0\"\n\n[dependencies]\nduplicate = \"1\"\n",
            ),
            extract_symbol_graph(
                "vendor/first/Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"duplicate\"\nversion = \"1.0.0\"\n",
            ),
            extract_symbol_graph(
                "vendor/second/Cargo.toml",
                Some("cargo-manifest"),
                "[package]\nname = \"duplicate\"\nversion = \"2.0.0\"\n",
            ),
            extract_symbol_graph(
                "public/index.html",
                Some("html"),
                "<script type=\"module\">\nimport fs from \"node:fs\";\nexport function boot() { return fs.readFile; }\n</script>\n",
            ),
            extract_symbol_graph(
                "src/Page.svelte",
                Some("svelte"),
                "<script lang=\"ts\">\nimport url from \"node:url\";\nexport function page() { return url.parse('https://example.test'); }\n</script>\n",
            ),
        ];
        for (language, expected_relation) in [
            ("rust", "std"),
            ("javascript", "node:path"),
            ("python", "requests"),
            ("cargo-manifest", "duplicate"),
            ("html", "node:fs"),
            ("svelte", "node:url"),
        ] {
            require(
                graphs.iter().any(|graph| {
                    graph.language.as_deref() == Some(language)
                        && graph
                            .relations
                            .iter()
                            .any(|relation| relation.target_name.contains(expected_relation))
                }),
                &format!("extracted {language} provider relation is missing"),
            )?;
        }
        let nodes = graphs
            .iter()
            .map(|graph| {
                test_file_node(&graph.path, graph.language.as_deref().unwrap_or("unknown"))
            })
            .collect::<Vec<_>>();
        let packages = PackageIndex::from_graphs(&graphs)?;
        let projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            projection,
            &candidates,
            &control,
        )?;
        let source_keys = staged
            .entities
            .iter()
            .map(|entity| entity.key().clone())
            .collect::<Vec<_>>();
        {
            let mut publication = store.begin_index_publication("extracted-provider-matrix")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let mut reopened = Vec::new();
        for source in source_keys {
            let page = reader.repository_graph_relations(
                RepositoryGraphRelationQuery::Outbound { source },
                128,
            )?;
            require(!page.truncated, "extracted provider matrix was truncated")?;
            reopened.extend(page.rows);
        }
        let mut external = BTreeSet::new();
        let mut unresolved = BTreeSet::new();
        let mut ambiguous = BTreeMap::new();
        let mut resolved_symbols = BTreeSet::new();
        for relation in reopened {
            require_eq(
                &relation.generation(),
                &generation,
                "extracted relation generation",
            )?;
            match relation.resolution() {
                RelationResolution::External {
                    external: target, ..
                } => {
                    external.insert((
                        target.system.as_str().to_string(),
                        target.identity.as_str().to_string(),
                    ));
                }
                RelationResolution::Unresolved { reference } => {
                    unresolved.insert(reference.as_str().to_string());
                }
                RelationResolution::Ambiguous {
                    reference,
                    candidates,
                } => {
                    ambiguous.insert(reference.as_str().to_string(), candidates.get());
                }
                RelationResolution::Resolved { selector, .. } => {
                    if let ReusableTargetSelector::Symbol { symbol } = selector {
                        resolved_symbols.insert((
                            symbol.file.as_str().to_string(),
                            symbol.name.as_str().to_string(),
                        ));
                    }
                }
            }
        }
        for expected in [
            ("rust-toolchain".to_string(), "std".to_string()),
            ("node".to_string(), "path".to_string()),
            ("node".to_string(), "fs".to_string()),
            ("node".to_string(), "url".to_string()),
        ] {
            require(
                external.contains(&expected),
                &format!("reopened external target is missing: {expected:?}"),
            )?;
        }
        require(
            unresolved
                .iter()
                .any(|reference| reference.contains("requests")),
            "reopened Python unresolved import is missing",
        )?;
        require_eq(
            &ambiguous.get("duplicate"),
            &Some(&2),
            "reopened Cargo duplicate ambiguity",
        )?;
        require(
            resolved_symbols.contains(&("src/lib.rs".to_string(), "private_helper".to_string())),
            "reopened private Rust helper resolution is missing",
        )?;
        require(
            resolved_symbols.contains(&(
                "src/config.rs".to_string(),
                "load_timeout_millis".to_string(),
            )),
            "reopened Rust module-qualified call resolution is missing",
        )?;
        require(
            resolved_symbols
                .contains(&("src/handler.rs".to_string(), "health_response".to_string())),
            "reopened Rust callback resolution is missing",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn closed_resolution_states_survive_sqlite_publication_and_reopen() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("closed-resolution-states");
        fs::create_dir_all(&root)?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let source = test_file_entity(project, generation, "src/main.rs")?;
        let unique_target = test_symbol_entity(project, generation, "src/unique.rs", "unique")?;
        let first_shared = test_symbol_entity(project, generation, "src/first.rs", "shared")?;
        let second_shared = test_symbol_entity(project, generation, "src/second.rs", "shared")?;
        let local_package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("local-package")?,
                    manifest: RepositoryFilePath::new(Path::new("vendor/local/Cargo.toml"))?,
                },
            },
            generation,
        )?;
        let unique_key = test_resolution_key(project, "unique")?;
        let shared_key = test_resolution_key(project, "shared")?;
        let local_package_key = test_resolution_key(project, "local-package")?;
        let mut registry = ProjectResolutionRegistry::default();
        registry.insert_candidate(&unique_key, &unique_target)?;
        registry.insert_candidate(&shared_key, &first_shared)?;
        registry.insert_candidate(&shared_key, &second_shared)?;
        registry.insert_candidate(&local_package_key, &local_package)?;
        let source_digest = source.key().digest().to_string();
        let owners = GraphOwners {
            file_digest: source_digest.clone(),
            symbol_digests: Vec::new(),
        };
        let staged_entities = BTreeMap::from([(source_digest, source.clone())]);
        let mut external_entities = BTreeMap::new();
        let cases = [
            resolution_case("rust", RelationKind::Calls, "unique", &[&unique_key]),
            resolution_case("rust", RelationKind::Calls, "shared", &[&shared_key]),
            resolution_case("rust", RelationKind::Calls, "missing", &[]),
            resolution_case("rust", RelationKind::Imports, "use std::{fs, io};", &[]),
            resolution_case("cargo-manifest", RelationKind::DependsOn, "serde", &[]),
            resolution_case("cargo-lock", RelationKind::DependsOn, "serde-lock", &[]),
            resolution_case(
                "javascript",
                RelationKind::Imports,
                "import path from \"node:path\";",
                &[],
            ),
            resolution_case(
                "html",
                RelationKind::Imports,
                "import fs from \"node:fs\";",
                &[],
            ),
            resolution_case(
                "svelte",
                RelationKind::Imports,
                "import url from \"node:url\";",
                &[],
            ),
            resolution_case(
                "javascript",
                RelationKind::Imports,
                "import value from \"left-pad\";",
                &[],
            ),
            resolution_case("python", RelationKind::Imports, "import requests", &[]),
            resolution_case(
                "cargo-manifest",
                RelationKind::DependsOn,
                "local-package",
                &[&local_package_key],
            ),
        ];
        let mut relations = Vec::new();
        let mut dependencies = Vec::new();
        for case in &cases {
            let symbol_index = GraphSymbolIndex::new(&case.graph, &control)?;
            let resolution = relation_resolution(
                project,
                generation,
                &case.relation,
                &owners,
                &case.graph,
                &symbol_index,
                &case.keys,
                &registry,
                &staged_entities,
                &mut external_entities,
                &control,
            )?;
            let relation = LogicalRelation::new(
                &source,
                GraphRelationKind::from_legacy(case.relation.kind),
                resolution,
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?;
            for key in &case.keys {
                dependencies.push(RelationDependencyKey::new(
                    relation.key().clone(),
                    key.clone(),
                )?);
            }
            relations.push(relation);
        }
        let external_keys = external_entities
            .values()
            .map(|entity| entity.key().clone())
            .collect::<Vec<_>>();
        let entities = [
            source.clone(),
            unique_target,
            first_shared,
            second_shared,
            local_package,
        ]
        .into_iter()
        .chain(external_entities.into_values())
        .collect::<Vec<_>>();
        let exports = [
            EntityResolutionKey::new(entities[1].key().clone(), unique_key.clone())?,
            EntityResolutionKey::new(entities[2].key().clone(), shared_key.clone())?,
            EntityResolutionKey::new(entities[3].key().clone(), shared_key.clone())?,
            EntityResolutionKey::new(entities[4].key().clone(), local_package_key.clone())?,
        ];
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let staged = StagedRepositoryGraph {
            project,
            mutation: RepositoryGraphMutation::Full,
            entities,
            relations,
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: exports.into(),
            relation_dependencies: dependencies,
            document_unresolved_reasons: Vec::new(),
            scan_policy,
            document_target_states: Vec::new(),
            database: None,
            retained_bytes: 0,
        };
        {
            let mut publication = store.begin_index_publication("closed-resolution-states")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&[
                test_file_node("src/main.rs", "rust"),
                test_file_node("src/unique.rs", "rust"),
                test_file_node("src/first.rs", "rust"),
                test_file_node("src/second.rs", "rust"),
                test_file_node("vendor/local/Cargo.toml", "cargo-manifest"),
            ])?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        drop(store);

        let reader = AtlasStore::open_read_only_for_project(&database, &root)?;
        let page = reader.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            32,
        )?;
        require(!page.truncated, "closed resolution proof was truncated")?;
        require_eq(&page.rows.len(), &cases.len(), "reopened relation count")?;
        let mut resolved = BTreeSet::new();
        let mut ambiguous = BTreeMap::new();
        let mut unresolved = BTreeSet::new();
        let mut external = BTreeSet::new();
        for relation in &page.rows {
            require_eq(
                &relation.generation(),
                &generation,
                "reopened relation generation",
            )?;
            match relation.resolution() {
                RelationResolution::Resolved {
                    selector,
                    generation: target_generation,
                    ..
                } => {
                    require_eq(target_generation, &generation, "resolved target generation")?;
                    match selector {
                        ReusableTargetSelector::Symbol { symbol } => {
                            resolved.insert(symbol.name.as_str().to_string());
                        }
                        ReusableTargetSelector::Package { package } => {
                            resolved.insert(package.name.as_str().to_string());
                        }
                        ReusableTargetSelector::Folder { .. }
                        | ReusableTargetSelector::File { .. } => {}
                    }
                }
                RelationResolution::Ambiguous {
                    reference,
                    candidates,
                } => {
                    ambiguous.insert(reference.as_str().to_string(), candidates.get());
                }
                RelationResolution::Unresolved { reference } => {
                    unresolved.insert(reference.as_str().to_string());
                }
                RelationResolution::External {
                    external: selector,
                    generation: target_generation,
                    ..
                } => {
                    require_eq(target_generation, &generation, "external target generation")?;
                    external.insert((
                        selector.system.as_str().to_string(),
                        selector.identity.as_str().to_string(),
                    ));
                }
            }
        }
        require_eq(
            &resolved,
            &BTreeSet::from(["local-package".to_string(), "unique".to_string()]),
            "resolved targets",
        )?;
        require_eq(
            &ambiguous,
            &BTreeMap::from([("shared".to_string(), 2)]),
            "ambiguous targets",
        )?;
        require_eq(
            &unresolved,
            &BTreeSet::from([
                "import requests".to_string(),
                "import value from \"left-pad\";".to_string(),
                "missing".to_string(),
                "serde-lock".to_string(),
            ]),
            "unresolved targets",
        )?;
        require_eq(
            &external,
            &BTreeSet::from([
                ("cargo".to_string(), "serde".to_string()),
                ("node".to_string(), "fs".to_string()),
                ("node".to_string(), "path".to_string()),
                ("node".to_string(), "url".to_string()),
                ("rust-toolchain".to_string(), "std".to_string()),
            ]),
            "external targets",
        )?;
        for key in &external_keys {
            let entity = reader
                .repository_graph_entity(key)?
                .ok_or("reopened external entity is missing")?;
            require(
                matches!(entity.selector(), EntitySelector::External { .. }),
                "external relation target reopened as a local entity",
            )?;
            require_eq(
                &entity.generation(),
                &generation,
                "external entity generation",
            )?;
        }
        let affected = reader.repository_affected_source_paths(
            project,
            &[unique_key, shared_key, local_package_key],
            32,
        )?;
        require(!affected.truncated, "dependency source proof was truncated")?;
        require_eq(
            &affected.rows,
            &vec![RepositoryFilePath::new(Path::new("src/main.rs"))?],
            "reopened dependency source paths",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    struct ResolutionCase {
        graph: SymbolGraph,
        relation: SymbolRelation,
        keys: Vec<CanonicalResolutionKey>,
    }

    fn resolution_case(
        language: &str,
        kind: RelationKind,
        target: &str,
        keys: &[&CanonicalResolutionKey],
    ) -> ResolutionCase {
        let parser = if language.starts_with("cargo-") {
            ParserKind::Manifest
        } else {
            ParserKind::TreeSitter
        };
        ResolutionCase {
            graph: SymbolGraph {
                path: "src/main.rs".to_string(),
                language: Some(language.to_string()),
                parser,
                symbols: Vec::new(),
                relations: Vec::new(),
            },
            relation: SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "src/main.rs".to_string(),
                target_name: target.to_string(),
                kind,
                line: 1,
                context: target.to_string(),
                parser,
            },
            keys: keys.iter().map(|key| (*key).clone()).collect(),
        }
    }

    fn test_resolution_key(
        project: ProjectInstanceId,
        identity: &str,
    ) -> Result<CanonicalResolutionKey, Box<dyn Error>> {
        let provider = GraphIdentityText::new("test-provider")?;
        let language = GraphIdentityText::new("test-language")?;
        let identity = GraphIdentityText::new(identity)?;
        Ok(CanonicalResolutionKey::new(
            project,
            ResolutionKeyDomain::Declaration,
            &provider,
            &language,
            None,
            None,
            None,
            &identity,
        ))
    }

    fn test_file_entity(
        project: ProjectInstanceId,
        generation: IndexGeneration,
        path: &str,
    ) -> Result<GraphEntity, Box<dyn Error>> {
        Ok(GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(path))?,
            },
            generation,
        )?)
    }

    fn test_symbol_entity(
        project: ProjectInstanceId,
        generation: IndexGeneration,
        path: &str,
        name: &str,
    ) -> Result<GraphEntity, Box<dyn Error>> {
        Ok(GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new(path))?,
                    name: GraphIdentityText::new(name)?,
                    kind: SymbolKind::Function,
                    parent: None,
                    signature: GraphIdentityText::new(format!("fn {name}()"))?,
                },
            },
            generation,
        )?)
    }

    fn test_code_symbol(
        path: &str,
        name: &str,
        parent: Option<&str>,
        signature: &str,
    ) -> CodeSymbol {
        CodeSymbol {
            path: path.to_string(),
            language: Some("rust".to_string()),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: signature.to_string(),
            exported: false,
            documentation: None,
            line_start: 1,
            line_end: 1,
            source_selector: None,
            parent: parent.map(ToString::to_string),
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }
    }

    #[test]
    fn document_target_normalization_is_relative_bounded_and_platform_neutral()
    -> Result<(), Box<dyn Error>> {
        require_eq(
            &normalize_document_target("docs/guide.md", "../src/lib.rs:L12-L20?view=raw#entry")
                .map_err(|reason| io::Error::other(reason.to_string()))?,
            &DocumentTargetIdentity {
                path: "src/lib.rs".to_string(),
                fragment: Some("entry".to_string()),
            },
            "relative document target",
        )?;
        require_eq(
            &normalize_document_target("guide.md", "README")
                .map_err(|reason| io::Error::other(reason.to_string()))?,
            &DocumentTargetIdentity {
                path: "README".to_string(),
                fragment: None,
            },
            "root extensionless target",
        )?;
        require_eq(
            &normalize_document_target("docs/guide.md", "../../../private.txt"),
            &Err(DocumentTargetUnresolvedReason::OutsideRoot),
            "outside-root selector",
        )?;
        require_eq(
            &normalize_document_target("docs/guide.md", "target.md#runtime value"),
            &Err(DocumentTargetUnresolvedReason::NoStaticTarget),
            "non-static fragment refusal",
        )?;
        Ok(())
    }

    #[test]
    fn document_rows_resolve_exact_files_and_headings_with_typed_absence()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join(".gitignore"), "docs/ignored.md\n")?;
        fs::write(root.join("docs/ignored.md"), "ignored")?;
        let source = "[lib](../src/lib.rs)\n[heading](target.md#api)\n[missing heading](target.md#absent)\n[missing](missing.md)\n[ignored](ignored.md)\n[case](../SRC/lib.rs)\n[folder](../src)\n[outside](../../../private.txt)\n[self](guide.md)\n[self heading](guide.md#api)\n";
        let source_facts = projectatlas_symbols::extract_markdown_facts(source);
        let target_facts = projectatlas_symbols::extract_markdown_facts("# API\n");
        let source_graph = source_facts.symbol_graph("docs/guide.md", Some("markdown"));
        let target_graph = target_facts.symbol_graph("docs/target.md", Some("markdown"));
        let source_code_graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let graphs = vec![source_graph, target_graph, source_code_graph];
        let mut nodes = vec![
            test_file_node("docs/guide.md", "markdown"),
            test_file_node("docs/target.md", "markdown"),
            test_file_node("src/lib.rs", "rust"),
        ];
        let mut folder = test_file_node("src", "unknown");
        folder.kind = NodeKind::Folder;
        folder.language = None;
        folder.extension = None;
        nodes.push(folder);
        let project = ProjectInstanceId::from_bytes([31; 16])?;
        let generation = IndexGeneration::new(4);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let owners = projection
            .owners_by_graph
            .remove("docs/guide.md")
            .ok_or_else(|| io::Error::other("document owners were not projected"))?;
        let scan_policy = RootScanPolicy::discover(root, &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(root, &nodes, &scan_policy)?;
        let rows = project_document_rows(
            project,
            generation,
            &graphs[0],
            &source_facts,
            &owners,
            &index,
            &candidates,
            &projection.entity_by_digest,
            &control,
        )?;
        require_eq(&rows.relations.len(), &9, "document relation count")?;
        require_eq(&rows.occurrences.len(), &9, "document occurrence count")?;
        require_eq(
            &rows
                .relations
                .iter()
                .filter(|relation| {
                    matches!(relation.resolution(), RelationResolution::Resolved { .. })
                })
                .count(),
            &2,
            "resolved document targets",
        )?;
        let reasons = rows
            .document_unresolved_reasons
            .iter()
            .map(|(_key, reason)| *reason)
            .collect::<BTreeSet<_>>();
        require_eq(
            &reasons,
            &BTreeSet::from([
                DocumentTargetUnresolvedReason::Missing,
                DocumentTargetUnresolvedReason::Ignored,
                DocumentTargetUnresolvedReason::OutsideRoot,
                DocumentTargetUnresolvedReason::CaseConflict,
                DocumentTargetUnresolvedReason::Unsupported,
            ]),
            "typed unresolved reasons",
        )?;
        require(
            rows.relations.iter().any(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Resolved {
                        selector: projectatlas_core::graph::ReusableTargetSelector::Symbol {
                            symbol
                        },
                        ..
                    } if symbol.kind == SymbolKind::Heading && symbol.signature.as_str() == "api"
                )
            }),
            "heading fragment did not resolve to its heading entity",
        )?;
        Ok(())
    }

    #[test]
    fn document_rows_use_file_identity_and_deduplicate_across_headings()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("src"))?;
        fs::write(temp.path().join("src/lib.rs"), "pub fn entry() {}\n")?;
        let source = "# First\n[target](../src/lib.rs)\n[second](guide.md#second)\n[self](guide.md#first)\n\n# Second\n[target](../src/lib.rs)\n[target](../src/lib.rs)\n[first](guide.md#first)\n[self](guide.md#second)\n";
        let facts = projectatlas_symbols::extract_markdown_facts(source);
        let enclosing_heading_bytes = facts
            .link_candidates
            .iter()
            .map(|candidate| candidate.enclosing_heading.as_ref().map_or(0, String::len) as u64)
            .sum::<u64>();
        let mut facts_without_heading_owners = facts.clone();
        for candidate in &mut facts_without_heading_owners.link_candidates {
            candidate.enclosing_heading = None;
        }
        let retained_with_heading_owners = document_fact_map_retained_bytes(&BTreeMap::from([(
            "docs/guide.md".to_string(),
            Cow::Owned(facts.clone()),
        )]));
        let retained_without_heading_owners =
            document_fact_map_retained_bytes(&BTreeMap::from([(
                "docs/guide.md".to_string(),
                Cow::Owned(facts_without_heading_owners),
            )]));
        require_eq(
            &retained_with_heading_owners.saturating_sub(retained_without_heading_owners),
            &enclosing_heading_bytes,
            "document registry enclosing-heading bytes",
        )?;
        let source_graph = facts.symbol_graph("docs/guide.md", Some("markdown"));
        let target_graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let graphs = vec![source_graph, target_graph];
        let nodes = vec![
            test_file_node("docs/guide.md", "markdown"),
            test_file_node("src/lib.rs", "rust"),
        ];
        let project = ProjectInstanceId::from_bytes([33; 16])?;
        let generation = IndexGeneration::new(5);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let owners = projection
            .owners_by_graph
            .remove("docs/guide.md")
            .ok_or_else(|| io::Error::other("document owners were not projected"))?;
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        let rows = project_document_rows(
            project,
            generation,
            &graphs[0],
            &facts,
            &owners,
            &index,
            &candidates,
            &projection.entity_by_digest,
            &control,
        )?;
        require_eq(&rows.relations.len(), &3, "file-owned relation count")?;
        require_eq(&rows.occurrences.len(), &7, "distinct link occurrences")?;
        require(
            rows.relations.iter().all(|relation| {
                projection
                    .entity_by_digest
                    .get(relation.source().digest())
                    .is_some_and(|entity| {
                        matches!(
                            entity.selector(),
                            EntitySelector::File { path } if path.as_str() == "docs/guide.md"
                        )
                    })
            }),
            "document relations were not owned by the document file",
        )?;
        let heading_targets = rows
            .relations
            .iter()
            .filter_map(|relation| {
                let RelationResolution::Resolved {
                    selector: projectatlas_core::graph::ReusableTargetSelector::Symbol { symbol },
                    ..
                } = relation.resolution()
                else {
                    return None;
                };
                (symbol.kind == SymbolKind::Heading).then(|| symbol.signature.as_str().to_string())
            })
            .collect::<BTreeSet<_>>();
        require_eq(
            &heading_targets,
            &BTreeSet::from(["first".to_string(), "second".to_string()]),
            "file-owned heading targets",
        )?;
        let source_target_occurrences = rows
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.file().as_str() == "docs/guide.md")
            .count();
        require_eq(
            &source_target_occurrences,
            &7,
            "file-owned relation occurrences",
        )?;
        Ok(())
    }

    #[test]
    fn complete_document_without_static_candidates_reports_no_candidates()
    -> Result<(), Box<dyn Error>> {
        let facts = projectatlas_symbols::extract_markdown_facts(
            "# Overview\n\nLong prose without a repository reference.\n",
        );
        let coverage = document_coverage("docs/overview.md", &facts, IndexGeneration::new(6))?;
        require_eq(
            &coverage.state(),
            &CoverageState::NoCandidates,
            "empty document relation coverage",
        )?;
        require_eq(&coverage.total(), &0, "empty document relation total")?;
        Ok(())
    }

    #[test]
    fn partial_markdown_evidence_limit_maps_to_intermediate_bytes_coverage()
    -> Result<(), Box<dyn Error>> {
        let label = "l".repeat(MAX_MARKDOWN_LABEL_BYTES);
        let selector = format!(
            "src/{}.rs",
            "s".repeat(MAX_DOCUMENT_SELECTOR_BYTES - "src/".len() - ".rs".len())
        );
        let evidence_bytes = label.len() + selector.len();
        let source = format!("[{label}]({selector})\n")
            .repeat(MAX_MARKDOWN_EVIDENCE_BYTES / evidence_bytes + 1);
        let facts = projectatlas_symbols::extract_markdown_facts(&source);
        require(
            facts
                .coverage
                .limits
                .contains(&MarkdownFactLimit::EvidenceBytes),
            "real Markdown extraction did not reach its evidence-byte limit",
        )?;
        require(
            !facts.link_candidates.is_empty(),
            "evidence-limited Markdown extraction lost every valid candidate",
        )?;

        let coverage = document_coverage("docs/limited.md", &facts, IndexGeneration::new(7))?;
        require_eq(
            &coverage.state(),
            &CoverageState::Partial,
            "evidence-limited document coverage state",
        )?;
        require_eq(
            &coverage.reached_limit(),
            &Some(GraphLimitKind::IntermediateBytes),
            "evidence-limited document graph limit",
        )?;
        Ok(())
    }

    #[test]
    fn document_cycles_emit_only_canonical_bounded_edges() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("docs"))?;
        fs::write(temp.path().join("docs/a.md"), "# A\n\n[b](b.md#b)\n")?;
        fs::write(temp.path().join("docs/b.md"), "# B\n\n[a](a.md#a)\n")?;
        let facts = [
            projectatlas_symbols::extract_markdown_facts("# A\n\n[b](b.md#b)\n"),
            projectatlas_symbols::extract_markdown_facts("# B\n\n[a](a.md#a)\n"),
        ];
        let graphs = vec![
            facts[0].symbol_graph("docs/a.md", Some("markdown")),
            facts[1].symbol_graph("docs/b.md", Some("markdown")),
        ];
        let nodes = vec![
            test_file_node("docs/a.md", "markdown"),
            test_file_node("docs/b.md", "markdown"),
        ];
        let project = ProjectInstanceId::from_bytes([35; 16])?;
        let generation = IndexGeneration::new(1);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let control = super::super::standalone_index_work_control();
        let mut projection = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&projection, &control)?;
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        let mut relations = Vec::new();
        for (graph, facts) in graphs.iter().zip(&facts) {
            let owners = projection
                .owners_by_graph
                .remove(&graph.path)
                .ok_or_else(|| io::Error::other("document owners were not projected"))?;
            relations.extend(
                project_document_rows(
                    project,
                    generation,
                    graph,
                    facts,
                    &owners,
                    &index,
                    &candidates,
                    &projection.entity_by_digest,
                    &control,
                )?
                .relations,
            );
        }
        require_eq(&relations.len(), &2, "document cycle relation count")?;
        require(
            relations.iter().all(|relation| {
                relation.kind() == GraphRelationKind::Extended(ExtendedRelationKind::Documents)
                    && matches!(
                        relation.resolution(),
                        RelationResolution::Resolved {
                            selector: ReusableTargetSelector::Symbol { symbol },
                            ..
                        } if symbol.kind == SymbolKind::Heading
                    )
            }),
            "document cycle emitted a non-canonical or unresolved edge",
        )?;
        Ok(())
    }

    #[test]
    fn document_casefold_collisions_refuse_exact_winners_and_share_invalidation_keys()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let nodes = vec![
            test_file_node("src/lib.rs", "rust"),
            test_file_node("SRC/lib.rs", "rust"),
        ];
        let control = super::super::standalone_index_work_control();
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &nodes, &scan_policy)?;
        require_eq(
            &index.unresolved_reason("src/lib.rs")?,
            &Some(DocumentTargetUnresolvedReason::CaseConflict),
            "exact case-collision target",
        )?;
        let project = ProjectInstanceId::from_bytes([32; 16])?;
        require_eq(
            &document_casefold_resolution_key(project, "src/lib.rs")?,
            &document_casefold_resolution_key(project, "SRC/lib.rs")?,
            "casefold invalidation key",
        )?;
        Ok(())
    }

    #[test]
    fn document_target_state_change_refuses_stale_publication() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = super::super::standalone_index_work_control();
        let scan_policy = RootScanPolicy::discover(temp.path(), &ScanOptions::default(), &control)?;
        let index = DocumentResolutionIndex::new(temp.path(), &[], &scan_policy)?;
        let actual = index
            .unresolved_reason("docs/target.md")?
            .ok_or_else(|| std::io::Error::other("absent target unexpectedly resolved"))?;
        let stale = if actual == DocumentTargetUnresolvedReason::Missing {
            DocumentTargetUnresolvedReason::Ignored
        } else {
            DocumentTargetUnresolvedReason::Missing
        };
        let document_target_states = vec![("docs/target.md".to_string(), stale)];
        drop(index);
        let staged = StagedRepositoryGraph {
            project: ProjectInstanceId::from_bytes([34; 16])?,
            mutation: RepositoryGraphMutation::Full,
            entities: Vec::new(),
            relations: Vec::new(),
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: Vec::new(),
            relation_dependencies: Vec::new(),
            document_unresolved_reasons: Vec::new(),
            scan_policy,
            document_target_states,
            database: None,
            retained_bytes: 0,
        };
        require(
            matches!(
                staged.revalidate_document_targets(temp.path()),
                Err(CliError::RefreshRequired(_))
            ),
            "changed non-indexed target state reached publication",
        )?;
        Ok(())
    }

    fn test_file_node(path: &str, language: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: path
                .rsplit_once('/')
                .map(|(parent, _name)| parent.to_string()),
            extension: Path::new(path)
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy())),
            language: Some(language.to_string()),
            size_bytes: Some(1),
            mtime_ns: Some(1),
            content_hash: Some(format!("hash:{path}")),
        }
    }

    #[test]
    fn large_document_projection_publishes_in_memory_staging_and_incremental()
    -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 10;
        const LINKS_PER_DOCUMENT: usize = 1_024;
        const INCREMENTAL_LINKS: usize = 1_024;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join("content/real-target"))?;
        fs::write(root.join("content/resolved.md"), "# Resolved\n")?;
        let symlink_available = match create_directory_link(
            &root.join("content/real-target"),
            &root.join("content/alias"),
        ) {
            Ok(()) => true,
            Err(source) if cfg!(windows) && source.raw_os_error() == Some(1314) => {
                fs::create_dir(root.join("content/alias"))?;
                false
            }
            Err(source) => return Err(source.into()),
        };
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("large document fixture project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let mut nodes = Vec::with_capacity(DOCUMENT_COUNT + 2);
        let mut docs_node = test_file_node("content", "unknown");
        docs_node.kind = NodeKind::Folder;
        docs_node.language = None;
        docs_node.extension = None;
        nodes.push(docs_node);
        let mut graphs = Vec::with_capacity(DOCUMENT_COUNT + 1);
        let mut document_facts: BTreeMap<
            String,
            Cow<'static, projectatlas_symbols::MarkdownFacts>,
        > = BTreeMap::new();
        for document in 0..DOCUMENT_COUNT {
            let path = format!("content/source-{document:02}.md");
            let mut links = Vec::with_capacity(LINKS_PER_DOCUMENT);
            if document == 0 {
                links.push("[resolved](resolved.md)".to_string());
                links.push("[symlink](alias)".to_string());
            }
            while links.len() < LINKS_PER_DOCUMENT {
                let link = links.len();
                links.push(format!(
                    "[missing-{document:02}-{link:04}](missing-{document:02}-{link:04}.md)"
                ));
            }
            let facts = projectatlas_symbols::extract_markdown_facts(&links.join("\n"));
            require_eq(
                &facts.link_candidates.len(),
                &LINKS_PER_DOCUMENT,
                "large document candidate count",
            )?;
            graphs.push(facts.symbol_graph(&path, Some("markdown")));
            document_facts.insert(path.clone(), Cow::Owned(facts));
            nodes.push(test_file_node(&path, "markdown"));
        }
        let resolved_facts = projectatlas_symbols::extract_markdown_facts("# Resolved\n");
        graphs.push(resolved_facts.symbol_graph("content/resolved.md", Some("markdown")));
        nodes.push(test_file_node("content/resolved.md", "markdown"));
        let candidate_count = document_facts
            .values()
            .map(|facts| facts.link_candidates.len())
            .sum::<usize>();
        require_eq(
            &candidate_count,
            &(DOCUMENT_COUNT * LINKS_PER_DOCUMENT),
            "large document projection candidate total",
        )?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let packages = PackageIndex::from_graphs(&graphs)?;

        let generation = IndexGeneration::new(1);
        let entities = build_entity_projection(
            project, generation, &nodes, &graphs, &packages, true, &control,
        )?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let in_memory = finish_projection_with_documents(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            &root,
            &nodes,
            &document_facts,
            entities,
            &candidates,
            &scan_policy,
            &control,
        )?;
        require(
            in_memory.database.is_none(),
            "normal projection unexpectedly selected disposable staging",
        )?;
        require(
            in_memory.document_unresolved_reasons.len() > GraphLimits::MAX_ROWS as usize,
            "normal projection did not retain a multi-ceiling reason vector",
        )?;
        require_eq(
            &in_memory.relations.len(),
            &candidate_count,
            "normal projection relation count",
        )?;
        require(
            in_memory.relations.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::Resolved { .. })
            }),
            "normal projection lost the resolved document result",
        )?;
        let symlink_relation = in_memory
            .relations
            .iter()
            .find(|relation| {
                matches!(
                    relation.resolution(),
                    RelationResolution::Unresolved { reference }
                        if reference.as_str() == "alias"
                )
            })
            .ok_or("normal projection lost the symlink document result")?;
        let symlink_reason = in_memory
            .document_unresolved_reasons
            .iter()
            .find(|(key, _reason)| key == symlink_relation.key())
            .map(|(_key, reason)| *reason);
        require_eq(
            &symlink_reason,
            &Some(DocumentTargetUnresolvedReason::Unsupported),
            "symlink document result did not retain its closed reason",
        )?;
        require(
            !symlink_available || root.join("content/alias").exists(),
            "symlink document fixture disappeared before publication",
        )?;
        {
            let mut publication = store.begin_index_publication("large-document-projection")?;
            publication.upsert_scan_node_batch(&nodes)?;
            in_memory.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        let published_page = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            },
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require(
            published_page.truncated && published_page.rows.len() == GraphLimits::MAX_ROWS as usize,
            "normal projection publication did not expose a bounded multi-page result",
        )?;

        let incremental_path = "content/source-00.md".to_string();
        let incremental_source = (0..INCREMENTAL_LINKS)
            .map(|link| format!("[incremental-{link:04}](incremental-{link:04}.md)"))
            .collect::<Vec<_>>()
            .join("\n");
        let incremental_facts = projectatlas_symbols::extract_markdown_facts(&incremental_source);
        let incremental_graph = incremental_facts.symbol_graph(&incremental_path, Some("markdown"));
        let incremental_nodes = vec![test_file_node(&incremental_path, "markdown")];
        let incremental_packages =
            PackageIndex::from_graphs(std::slice::from_ref(&incremental_graph))?;
        let incremental_entities = build_entity_projection(
            project,
            IndexGeneration::new(2),
            &incremental_nodes,
            std::slice::from_ref(&incremental_graph),
            &incremental_packages,
            false,
            &control,
        )?;
        let incremental_candidates =
            resolution_registry_from_exports(&incremental_entities, &control)?;
        let incremental_staged = finish_projection_with_documents(
            project,
            IndexGeneration::new(2),
            RepositoryGraphMutation::AffectedPaths(vec![incremental_path.clone()]),
            std::slice::from_ref(&incremental_graph),
            &root,
            &nodes,
            &BTreeMap::from([(incremental_path.clone(), Cow::Owned(incremental_facts))]),
            incremental_entities,
            &incremental_candidates,
            &scan_policy,
            &control,
        )?;
        require(
            incremental_staged.document_unresolved_reasons.len() <= GraphLimits::MAX_ROWS as usize,
            "incremental projection exceeded its aggregate row budget",
        )?;
        enforce_incremental_projection_limits(
            &root,
            &BTreeSet::from([incremental_path]),
            RepositoryAffectedSourceFootprint {
                rows: 0,
                retained_bytes: 0,
                truncated: false,
            },
            &incremental_staged,
        )?;
        {
            let mut publication =
                store.begin_index_projection_refresh("large-document-projection")?;
            incremental_staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        require_eq(
            &store.repository_graph_generation()?,
            &Some(IndexGeneration::new(2)),
            "incremental publication generation",
        )?;

        let staging_generation = IndexGeneration::new(3);
        let staging_entities = build_entity_projection(
            project,
            staging_generation,
            &nodes,
            &graphs,
            &packages,
            true,
            &control,
        )?;
        let staging_candidates = resolution_registry_from_exports(&staging_entities, &control)?;
        let staged = finish_projection_in_database_with_documents(
            &root,
            &nodes,
            project,
            staging_generation,
            &graphs,
            &document_facts,
            staging_entities,
            &staging_candidates,
            &scan_policy,
            &control,
        )?;
        let database_stage = staged
            .database
            .as_ref()
            .ok_or("large projection did not select disposable staging")?;
        let staged_database_bytes = fs::metadata(
            database_stage
                .directory()?
                .path()
                .join(GRAPH_STAGE_DATABASE_FILE_NAME),
        )?
        .len();
        require(
            staged_database_bytes > 0,
            "disposable staging database retained no durable bytes",
        )?;
        {
            let mut publication =
                store.begin_index_projection_refresh("large-document-projection")?;
            publication.upsert_scan_node_batch(&nodes)?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }
        let staged_page = store.repository_graph_relation_rows(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            },
            GraphLimits::MAX_ROWS,
            None,
        )?;
        require(
            staged_page.truncated && staged_page.rows.len() == GraphLimits::MAX_ROWS as usize,
            "disposable staging publication did not expose a bounded multi-page result",
        )?;
        Ok(())
    }

    #[test]
    fn incremental_document_admission_uses_emitted_rows() -> Result<(), Box<dyn Error>> {
        const DOCUMENT_COUNT: usize = 10;
        const CANDIDATES_PER_DOCUMENT: usize = 1_024;
        let temp = tempfile::tempdir()?;
        let root = fs::canonicalize(temp.path())?;
        fs::create_dir_all(root.join("content"))?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let paths = (0..DOCUMENT_COUNT)
            .map(|index| format!("content/links-{index:02}.md"))
            .collect::<Vec<_>>();
        let mut nodes = paths
            .iter()
            .map(|path| test_file_node(path, "markdown"))
            .collect::<Vec<_>>();
        store.replace_scan(&nodes)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let symbols = empty_symbol_build_stage();

        for path in &paths {
            let file_name = path
                .rsplit_once('/')
                .map(|(_parent, file_name)| file_name)
                .ok_or("same-file document fixture path has no parent")?;
            let self_source = (0..CANDIDATES_PER_DOCUMENT)
                .map(|index| format!("[self-{index:05}]({file_name})"))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join(path), &self_source)?;
            let self_facts = projectatlas_symbols::extract_markdown_facts(&self_source);
            require_eq(
                &self_facts.link_candidates.len(),
                &CANDIDATES_PER_DOCUMENT,
                "same-file raw candidates per document",
            )?;
            store.replace_symbol_graph(&self_facts.symbol_graph(path, Some("markdown")))?;
        }
        for (node, path) in nodes.iter_mut().zip(&paths) {
            let bytes = fs::read(root.join(path))?;
            node.content_hash = Some(blake3::hash(&bytes).to_hex().to_string());
        }
        store.replace_scan(&nodes)?;
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;
        let incremental = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &paths,
            &scan_policy,
            &symbols,
            &control,
        )?;
        require(
            matches!(
                &incremental.mutation,
                RepositoryGraphMutation::AffectedPaths(affected) if affected == &paths
            ),
            "same-file candidates were not retained as an incremental projection",
        )?;
        require(
            incremental.relations.is_empty() && incremental.document_unresolved_reasons.is_empty(),
            "same-file candidates emitted document rows despite the no-fragment filter",
        )?;
        require(
            store.repository_graph_generation()?.is_none(),
            "incremental admission test unexpectedly published a generation",
        )?;

        for path in &paths {
            let unsupported_source = (0..CANDIDATES_PER_DOCUMENT)
                .map(|index| format!("[external-{index:05}](target-{index:05}.md#invalid?)"))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join(path), &unsupported_source)?;
            let unsupported_facts =
                projectatlas_symbols::extract_markdown_facts(&unsupported_source);
            require_eq(
                &unsupported_facts.link_candidates.len(),
                &CANDIDATES_PER_DOCUMENT,
                "unsupported raw candidates per document",
            )?;
            store.replace_symbol_graph(&unsupported_facts.symbol_graph(path, Some("markdown")))?;
        }
        for (node, path) in nodes.iter_mut().zip(&paths) {
            let bytes = fs::read(root.join(path))?;
            node.content_hash = Some(blake3::hash(&bytes).to_hex().to_string());
        }
        store.replace_scan(&nodes)?;
        let error = stage_incremental_repository_graph(
            &store,
            &root,
            IndexGeneration::new(0),
            &nodes,
            &paths,
            &scan_policy,
            &symbols,
            &control,
        )
        .err()
        .ok_or("emitted document rows over the ceiling were admitted incrementally")?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "emitted-row overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "emitted-row overflow scope",
        )?;
        require(
            store.repository_graph_generation()?.is_none(),
            "emitted-row overflow changed the current generation",
        )?;
        Ok(())
    }

    #[test]
    fn reopened_source_parse_success_does_not_promote_fallback_graph_facts()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open(&database)?;
        let graph = SymbolGraph {
            path: "src/optional.lang".to_string(),
            language: Some("optional-language".to_string()),
            parser: ParserKind::Fallback,
            symbols: vec![CodeSymbol {
                path: "src/optional.lang".to_string(),
                language: Some("optional-language".to_string()),
                name: "entry".to_string(),
                kind: SymbolKind::Function,
                signature: "entry()".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Fallback,
                detail: None,
            }],
            relations: vec![SymbolRelation {
                path: "src/optional.lang".to_string(),
                source_name: "entry".to_string(),
                target_name: "helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper()".to_string(),
                parser: ParserKind::Fallback,
            }],
        };
        store.replace_symbol_graph_with_metadata(
            &graph,
            &SourceParseMetadata {
                path: graph.path.clone(),
                language: graph.language.clone(),
                parser: ParserKind::TreeSitter,
                symbol_count: graph.symbols.len(),
                relation_count: graph.relations.len(),
            },
        )?;
        drop(store);

        let reader = AtlasStore::open_read_only(&database)?;
        let graphs = reader.load_symbol_graphs_for_paths(std::slice::from_ref(&graph.path))?;
        require_eq(&graphs, &vec![graph], "reopened fact graph")?;
        let project = ProjectInstanceId::from_bytes([7; 16])?;
        let generation = IndexGeneration::new(1);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entities =
            build_entity_projection(project, generation, &[], &graphs, &packages, true, &control)?;
        let candidates = resolution_registry_from_exports(&entities, &control)?;
        let staged = finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            entities,
            &candidates,
            &control,
        )?;
        require_eq(&staged.relations.len(), &1, "normalized relation count")?;
        require_eq(
            &staged.relations[0].confidence(),
            &ConfidenceClass::Low,
            "fallback relation confidence after reopen",
        )?;
        require_eq(
            &staged.relations[0].completeness(),
            &Completeness::Partial,
            "fallback relation completeness after reopen",
        )?;
        require(
            staged
                .relations
                .iter()
                .any(|relation| relation.kind() == GraphRelationKind::Legacy(RelationKind::Calls)),
            "reopened graph lost its legacy call relation",
        )?;
        require_eq(&staged.coverage.len(), &1, "normalized coverage count")?;
        require_eq(
            &staged.coverage[0].state(),
            &CoverageState::Partial,
            "fallback coverage after reopen",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn persisted_closure_overflow_preserves_the_complete_generation() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("persisted-closure-overflow");
        fs::create_dir_all(&root)?;
        let mut store = AtlasStore::open_for_project(&root.join("projectatlas.db"), &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("bound project identity is missing")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let graphs = vec![function_graph("src/lib.rs", 1)];
        let packages = PackageIndex::from_graphs(&graphs)?;
        let entity_projection = build_entity_projection(
            project,
            IndexGeneration::new(1),
            &[],
            &graphs,
            &packages,
            true,
            &control,
        )?;
        let dependency_keys = entity_projection
            .keys_by_graph
            .values()
            .flat_map(projectatlas_symbols::ResolutionKeyProjection::relation_keys)
            .flat_map(|relation| relation.keys().iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let candidates = resolution_registry_from_exports(&entity_projection, &control)?;
        let staged = finish_projection(
            project,
            IndexGeneration::new(1),
            RepositoryGraphMutation::Full,
            &graphs,
            entity_projection,
            &candidates,
            &control,
        )?;
        let file_key = staged
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or("staged file entity is missing")?
            .key()
            .clone();
        {
            let mut publication = store.begin_index_publication("closure-overflow")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&[Node {
                path: "src/lib.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(1),
                mtime_ns: Some(1),
                content_hash: Some("initial".to_string()),
            }])?;
            publication.finish_scan_replacement()?;
            staged.apply(&mut publication, &control)?;
            publication.complete()?;
        }

        store.replace_symbol_graph(&function_graph(
            "src/lib.rs",
            usize::try_from(MAX_INCREMENTAL_GRAPH_ROWS).unwrap_or(usize::MAX),
        ))?;
        let publication_before = store
            .index_publication()?
            .ok_or("complete publication is missing")?;
        let entity_before = store
            .repository_graph_entity(&file_key)?
            .ok_or("published file entity is missing")?;
        let exports_before =
            store.repository_export_keys_for_paths(project, &["src/lib.rs".to_string()], 100)?;
        let dependencies_before =
            store.repository_affected_source_paths(project, &dependency_keys, 100)?;
        let empty_symbols = empty_symbol_build_stage();
        let scan_policy = RootScanPolicy::discover(&root, &ScanOptions::default(), &control)?;

        let error = stage_incremental_repository_graph(
            &store,
            &root,
            publication_before.generation,
            &[],
            &["src/lib.rs".to_string()],
            &scan_policy,
            &empty_symbols,
            &control,
        )
        .err()
        .ok_or_else(|| io::Error::other("oversized closure acquired the publication writer"))?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "expected typed full-refresh guidance, found {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.reason,
            &IndexRefreshReason::DependencyClosureLimit,
            "persisted overflow reason",
        )?;
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "persisted overflow scope",
        )?;
        require_eq(
            &store.index_publication()?,
            &Some(publication_before),
            "publication after overflow",
        )?;
        require_eq(
            &store.repository_graph_entity(&file_key)?,
            &Some(entity_before),
            "file entity after overflow",
        )?;
        require_eq(
            &store.repository_export_keys_for_paths(project, &["src/lib.rs".to_string()], 100)?,
            &exports_before,
            "export keys after overflow",
        )?;
        require_eq(
            &store.repository_affected_source_paths(project, &dependency_keys, 100)?,
            &dependencies_before,
            "dependency owners after overflow",
        )?;
        Ok(())
    }

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: actual={actual:?}, expected={expected:?}"
            ))
            .into())
        }
    }

    fn empty_symbol_build_stage() -> SymbolBuildStage {
        SymbolBuildStage {
            report: SymbolBuildReport {
                candidates: 0,
                parsed: 0,
                unchanged: 0,
                too_large: 0,
                binary_or_non_utf8: 0,
                timed_out: 0,
                max_workers: 1,
                timeout_seconds: None,
                symbols: 0,
                relations: 0,
                summaries: 0,
                purpose_suggestions: 0,
            },
            changes: Vec::new(),
            retained_bytes: 0,
        }
    }

    fn function_graph(path: &str, symbol_count: usize) -> SymbolGraph {
        SymbolGraph {
            path: path.to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: (0..symbol_count)
                .map(|index| CodeSymbol {
                    path: path.to_string(),
                    language: Some("rust".to_string()),
                    name: format!("symbol_{index}"),
                    kind: SymbolKind::Function,
                    signature: format!("fn symbol_{index}()"),
                    exported: index == 0,
                    documentation: None,
                    line_start: index + 1,
                    line_end: index + 1,
                    source_selector: None,
                    parent: None,
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_string()),
                })
                .collect(),
            relations: vec![SymbolRelation {
                path: path.to_string(),
                source_name: "symbol_0".to_string(),
                target_name: "dependency".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "dependency()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        }
    }

    fn package_graph(path: &str, name: &str) -> SymbolGraph {
        SymbolGraph {
            path: path.to_string(),
            language: Some("cargo-manifest".to_string()),
            parser: ParserKind::Manifest,
            symbols: vec![CodeSymbol {
                path: path.to_string(),
                language: Some("cargo-manifest".to_string()),
                name: name.to_string(),
                kind: SymbolKind::Package,
                signature: format!("name = \"{name}\""),
                exported: true,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Manifest,
                detail: Some("cargo-package".to_string()),
            }],
            relations: Vec::new(),
        }
    }
}
