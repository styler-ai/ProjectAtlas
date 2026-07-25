//! Normalize parser-owned symbol facts into one generation-bound repository graph.

use super::{
    CliError, INDEX_FRESHNESS_SAMPLE_LIMIT, IndexReadStatus, IndexRefreshReason,
    IndexRefreshRequired, IndexRefreshScope, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
    IndexWorkStage, Node, NodeKind, SymbolBuildStage, SymbolProjectionChange,
    normalize_native_path_display,
};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, EntityResolutionKey, EntitySelector, ExtendedRelationKind, ExternalSelector,
    GraphContractError, GraphEntity, GraphIdentityText, GraphLimits, GraphRelationKind,
    LogicalRelation, PackageSelector, ProjectInstanceId, RelationDependencyKey, RelationOccurrence,
    RelationResolution, RepositoryFilePath, RepositoryNodePath, SourceSpan, SymbolSelector,
};
use projectatlas_core::language::{SemanticProviderOwner, language_capability};
use projectatlas_core::symbols::{
    ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_db::{
    AtlasStore, IndexPublicationGuard, RepositoryAffectedSourceFootprint,
    RepositoryResolutionCandidate,
};
use projectatlas_symbols::{
    MAX_RESOLUTION_KEYS_PER_FACT, ResolutionKeyProjection, ResolutionProjectionError,
    derive_resolution_keys, parse_import_references,
};
use std::borrow::{Borrow, Cow};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, btree_map::Entry};
use std::fs::{self, File, OpenOptions};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempDirBuilder, TempDir};

/// Maximum canonical keys or distinct source paths admitted by one incremental closure.
const MAX_INCREMENTAL_RESOLUTION_ITEMS: u32 = GraphLimits::MAX_ROWS;
/// Maximum aggregate normalized graph rows admitted by one incremental closure.
const MAX_INCREMENTAL_GRAPH_ROWS: u64 = GraphLimits::MAX_ROWS as u64;
/// Maximum conservative graph bytes admitted before requesting a complete refresh.
const MAX_INCREMENTAL_GRAPH_BYTES: u64 = super::MAX_PUBLICATION_STAGING_BYTES;
/// Maximum persisted key bindings retained by one complete in-memory graph projection.
const MAX_GRAPH_KEY_BINDINGS: u64 = 8_000_000;
/// Conservative fixed bytes counted for each staged graph or binding row.
const STAGED_GRAPH_ROW_BYTES: u64 = 128;
/// Maximum graph-map rows processed between cooperative cancellation checks.
const GRAPH_WORK_CHECK_INTERVAL: usize = 256;
/// Maximum simultaneous parser/entity/key bytes before rows spill to typed `SQLite` staging.
const MAX_IN_MEMORY_GRAPH_WORK_BYTES: u64 = 512 * 1_024 * 1_024;
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
    control.check(IndexWorkStage::SymbolParsing)?;
    let packages = PackageIndex::from_graphs(&graphs)?;
    let entity_projection = build_entity_projection(
        project, generation, nodes, &graphs, &packages, true, control,
    )?;
    let candidates = resolution_registry_from_exports(&entity_projection, control)?;
    enforce_resolution_staging_budget(&entity_projection, &candidates)?;
    let graph_work_bytes = symbols
        .retained_bytes
        .saturating_add(entity_projection.retained_bytes)
        .saturating_add(candidates.retained_bytes);
    if graph_work_bytes > MAX_IN_MEMORY_GRAPH_WORK_BYTES {
        finish_projection_in_database(
            root,
            nodes,
            project,
            generation,
            &graphs,
            entity_projection,
            &candidates,
            control,
        )
    } else {
        finish_projection(
            project,
            generation,
            RepositoryGraphMutation::Full,
            &graphs,
            entity_projection,
            &candidates,
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
    symbols: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let project = selected_project(store)?;
    let generation = next_generation(base_generation)?;
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
    for graph in &direct_graphs {
        control.check(IndexWorkStage::SymbolParsing)?;
        let projection = resolution_projection(project, packages.package_name(&graph.path), graph)?;
        changed_keys.extend(projection.source_keys().iter().cloned());
        for symbol in projection.symbol_keys() {
            changed_keys.extend(symbol.keys().iter().cloned());
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
    let affected_nodes = expected_nodes
        .iter()
        .filter(|node| affected_paths.contains(&node.path))
        .cloned()
        .collect::<Vec<_>>();
    let entity_projection = build_entity_projection(
        project,
        generation,
        &affected_nodes,
        &affected_graphs,
        &packages,
        false,
        control,
    )?;

    let dependency_keys = entity_projection
        .keys_by_graph
        .values()
        .flat_map(ResolutionKeyProjection::relation_keys)
        .flat_map(|relation| relation.keys().iter().cloned())
        .collect::<BTreeSet<_>>();
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
    enforce_resolution_staging_budget(&entity_projection, &candidates)?;
    let staged = finish_projection(
        project,
        generation,
        RepositoryGraphMutation::AffectedPaths(affected_paths.iter().cloned().collect()),
        &affected_graphs,
        entity_projection,
        &candidates,
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
/// Sorting is `O(n log n)`; each same-name candidate is pushed and popped at most once.
fn qualified_symbol_parents(graph: &SymbolGraph) -> Vec<Option<String>> {
    let mut order = (0..graph.symbols.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (graph.symbols[index].line_start, index));
    let mut active_by_name = BTreeMap::<&str, Vec<usize>>::new();
    let mut qualified_names = vec![None::<String>; graph.symbols.len()];
    let mut parents = vec![None; graph.symbols.len()];
    for index in order {
        let symbol = &graph.symbols[index];
        let parent = symbol.parent.as_deref().map(|parent| {
            let Some(candidates) = active_by_name.get_mut(parent) else {
                return parent.to_string();
            };
            while candidates.last().is_some_and(|&candidate_index| {
                graph.symbols[candidate_index].line_end < symbol.line_end
            }) {
                candidates.pop();
            }
            candidates
                .last()
                .and_then(|&candidate_index| qualified_names[candidate_index].clone())
                .unwrap_or_else(|| parent.to_string())
        });
        qualified_names[index] = Some(parent.as_ref().map_or_else(
            || symbol.name.clone(),
            |parent| format!("{parent}::{}", symbol.name),
        ));
        parents[index] = parent;
        active_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(index);
    }
    parents
}

/// Project file, symbol, package, and canonical export facts from parser graphs.
fn build_entity_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    nodes: &[Node],
    graphs: &[impl Borrow<SymbolGraph>],
    packages: &PackageIndex,
    include_project: bool,
    control: &IndexWorkControl,
) -> Result<EntityProjection, CliError> {
    let mut entity_by_digest = BTreeMap::new();
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
        insert_entity(
            &mut entity_by_digest,
            GraphEntity::new(project, selector, generation).map_err(invalid_graph_contract)?,
        )?;
    }

    let mut owners_by_graph = BTreeMap::new();
    let mut keys_by_graph = BTreeMap::new();
    let mut entity_exports = Vec::new();
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
        let qualified_parents = qualified_symbol_parents(graph);
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
                                parent: qualified_parent
                                    .map(GraphIdentityText::new)
                                    .transpose()
                                    .map_err(invalid_graph_contract)?,
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
                insert_entity(&mut entity_by_digest, entity)?;
            }
            symbol_digests.push(entity_digest);
        }
        let resolution = resolution_projection(project, packages.package_name(&graph.path), graph)?;
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
fn finish_projection_in_database(
    root: &Path,
    nodes: &[Node],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graphs: &[impl Borrow<SymbolGraph>],
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
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
        }
        staging.complete()?;
    }
    database.store()?.checkpoint_repository_graph_staging()?;
    database.store()?.begin_index_read_snapshot()?;
    let _staged_generation = database.store()?.repository_graph_generation()?;
    let retained_bytes = normalize_native_path_display(&database_path).len() as u64;
    Ok(StagedRepositoryGraph {
        project,
        mutation: RepositoryGraphMutation::Full,
        entities: Vec::new(),
        relations: Vec::new(),
        occurrences: Vec::new(),
        coverage: Vec::new(),
        entity_exports: Vec::new(),
        relation_dependencies: Vec::new(),
        database: Some(database),
        retained_bytes,
    })
}

/// Resolve staged relationships and finish one complete normalized graph batch.
fn finish_projection(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    mutation: RepositoryGraphMutation,
    graphs: &[impl Borrow<SymbolGraph>],
    mut entities: EntityProjection,
    candidates: &ProjectResolutionRegistry,
    control: &IndexWorkControl,
) -> Result<StagedRepositoryGraph, CliError> {
    let mut relations_by_digest = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relation_dependencies = Vec::new();
    let mut coverage = Vec::new();
    for graph in graphs {
        let graph = graph.borrow();
        let rows = project_graph_rows(
            project,
            generation,
            graph,
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
    entities.retained_bytes = entities.retained_bytes.saturating_add(
        STAGED_GRAPH_ROW_BYTES.saturating_mul(u64::try_from(added_rows).unwrap_or(u64::MAX)),
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
        database: None,
        retained_bytes: entities.retained_bytes,
    })
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
    }

    /// Return aggregate normalized rows retained by this staging batch.
    fn row_count(&self) -> usize {
        self.relations
            .len()
            .saturating_add(self.occurrences.len())
            .saturating_add(self.coverage.len())
            .saturating_add(self.external_entities.len())
            .saturating_add(self.relation_dependencies.len())
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
    }
}

/// Resolve one parser graph and release its temporary owner/key workspace.
fn project_graph_rows(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    graph: &SymbolGraph,
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
    let mut external_entities = BTreeMap::new();
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
    Ok(ProjectedGraphRows {
        relations,
        occurrences,
        coverage: vec![coverage_for_graph(graph, generation)?],
        external_entities: external_entities.into_values().collect(),
        relation_dependencies,
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

/// Derive canonical resolution keys with typed resource-limit translation.
fn resolution_projection(
    project: ProjectInstanceId,
    package: Option<&str>,
    graph: &SymbolGraph,
) -> Result<ResolutionKeyProjection, CliError> {
    match derive_resolution_keys(project, package, graph) {
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
        CliError, GRAPH_STAGE_DATABASE_FILE_NAME, GRAPH_STAGE_DIRECTORY_PREFIX, GraphOwners,
        GraphSymbolIndex, MAX_INCREMENTAL_GRAPH_BYTES, MAX_INCREMENTAL_GRAPH_ROWS, PackageIndex,
        ProjectResolutionRegistry, RepositoryGraphMutation, StagedRepositoryGraph,
        build_entity_projection, cleanup_abandoned_graph_staging,
        enforce_incremental_projection_budget, enforce_incremental_projection_limits,
        explicit_external_selector, finish_projection, finish_projection_in_database,
        insert_relation, is_cargo_manifest_path, registry_resolution_matches, relation_resolution,
        remove_owned_graph_stage_payload, repository_path_belongs_to,
        resolution_registry_from_exports, rust_toolchain_identity,
        stage_incremental_repository_graph, try_graph_stage_lease,
    };
    use crate::runtime::{
        IndexRefreshReason, IndexRefreshScope, SymbolBuildReport, SymbolBuildStage,
    };
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageScope, CoverageState,
        EntityResolutionKey, EntitySelector, ExtendedRelationKind, GraphEntity, GraphIdentityText,
        GraphRelationKind, LogicalRelation, PackageSelector, ProjectInstanceId,
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
    use projectatlas_symbols::extract_symbol_graph;
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
        let root = Path::new("repository");
        let affected_paths = BTreeSet::from(["src/lib.rs".to_string()]);
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
        let staged = finish_projection_in_database(
            &root,
            &nodes,
            project,
            generation,
            &graphs,
            projection,
            &candidates,
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
        let staged = StagedRepositoryGraph {
            project,
            mutation: RepositoryGraphMutation::Full,
            entities,
            relations,
            occurrences: Vec::new(),
            coverage: Vec::new(),
            entity_exports: exports.into(),
            relation_dependencies: dependencies,
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
            parent: parent.map(ToString::to_string),
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }
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

        let error = stage_incremental_repository_graph(
            &store,
            &root,
            publication_before.generation,
            &[],
            &["src/lib.rs".to_string()],
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
                parent: None,
                parser: ParserKind::Manifest,
                detail: Some("cargo-package".to_string()),
            }],
            relations: Vec::new(),
        }
    }
}
