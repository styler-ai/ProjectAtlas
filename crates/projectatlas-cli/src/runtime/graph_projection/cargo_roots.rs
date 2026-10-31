//! Shared invalidation cohorts for manifest-listed sibling Cargo roots.

use super::{
    CanonicalResolutionKey, CliError, IndexWorkControl, IndexWorkStage,
    MAX_INCREMENTAL_RESOLUTION_ITEMS, PackageIndex, PackageOwner, ProjectInstanceId,
    RepositoryFilePath, STAGED_GRAPH_ROW_BYTES, SourceBindingCache, resolution_key_limit_failure,
};
use projectatlas_symbols::{
    SourceCargoTargetKind, SourceCargoTargets, cargo_automatic_target_directory,
    cargo_root_cohort_key, cargo_target_inventory_key,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Distinguish automatic candidate discovery from exact configured-root cohorts.
#[derive(Clone, Copy)]
enum CohortKind {
    /// Every flat source can be an automatic target candidate.
    Automatic,
    /// Only manifest-listed roots emit the configured cohort key.
    Explicit,
}

/// One package/manifest-scoped parent inventory, shared by its exact roots.
pub(super) struct CargoRootCohort {
    /// Nonempty repository-relative directory identity; `.` denotes the root.
    pub(super) directory: Arc<str>,
    /// Admitted key whose producer is shared with source projection.
    pub(super) key: CanonicalResolutionKey,
}

/// Only actual explicit roots receive custom cohort source keys.
#[derive(Default)]
pub(super) struct CargoRootCohortIndex {
    /// Exact listed sibling roots outside the conventional automatic layouts.
    explicit: BTreeMap<String, Arc<CargoRootCohort>>,
    /// Existing flat automatic-directory keys, without new placement rules.
    automatic: BTreeMap<String, Arc<CargoRootCohort>>,
}

impl CargoRootCohortIndex {
    /// Borrow a producer only for a manifest-listed custom root.
    pub(super) fn explicit_directory(&self, path: &str) -> Option<&str> {
        self.explicit
            .get(path)
            .map(|cohort| cohort.directory.as_ref())
    }

    /// Select an admitted candidate group; callers still prove actual membership.
    pub(super) fn candidate(&self, path: &str, package_root: &str) -> Option<Arc<CargoRootCohort>> {
        self.explicit
            .get(path)
            .or_else(|| {
                cargo_automatic_target_directory(path, package_root)
                    .and_then(|directory| self.automatic.get(directory))
            })
            .map(Arc::clone)
    }
}

/// Admit manifest-relative paths only inside this repository.
pub(super) fn repository_join(directory: &str, path: &str) -> Option<String> {
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

/// Borrow a canonical repository path's parent without allocating.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

impl SourceBindingCache<'_> {
    /// Reserve a derived cache or temporary construction before inserting it.
    fn reserve_cohort_bytes(&mut self, bytes: u64) -> Result<(), CliError> {
        let next = self.retained_bytes.saturating_add(bytes);
        super::enforce_resolution_registry_budget_with_limit(next, self.retained_limit)?;
        self.retained_bytes = next;
        Ok(())
    }

    /// Admit one shared key; an invalid identity retains exact file causes instead.
    fn cohort(
        &mut self,
        project: ProjectInstanceId,
        package: &PackageOwner,
        directory: &str,
        kind: CohortKind,
    ) -> Result<Option<Arc<CargoRootCohort>>, CliError> {
        // Includes a conservative key/header envelope and both directory witnesses.
        let reservation = STAGED_GRAPH_ROW_BYTES
            .saturating_mul(8)
            .saturating_add(package.name.len() as u64)
            .saturating_add(package.manifest.len() as u64)
            .saturating_add((directory.len() as u64).saturating_mul(2));
        self.reserve_cohort_bytes(reservation)?;
        let key = match kind {
            CohortKind::Automatic => {
                cargo_target_inventory_key(project, &package.name, &package.manifest, directory)
            }
            CohortKind::Explicit => {
                cargo_root_cohort_key(project, &package.name, &package.manifest, directory)
            }
        };
        let Ok(key) = key else {
            self.retained_bytes -= reservation;
            return Ok(None);
        };
        Ok(Some(Arc::new(CargoRootCohort {
            directory: Arc::from(directory),
            key,
        })))
    }

    /// Cache only derived roots, sharing the already captured manifest's parse.
    pub(super) fn cargo_root_cohorts(
        &mut self,
        project: ProjectInstanceId,
        package: &PackageOwner,
        targets: &SourceCargoTargets,
        packages: &PackageIndex,
        control: &IndexWorkControl,
    ) -> Result<Arc<CargoRootCohortIndex>, CliError> {
        if let Some(index) = self.cargo_cohorts.get(&package.manifest) {
            return Ok(Arc::clone(index));
        }
        let before = self.retained_bytes;
        let result = (|| {
            let mut index = CargoRootCohortIndex::default();
            let mut paths = Vec::new();
            let mut temporary_rows = 0_u64;
            let mut temporary_strings = 0_u64;
            for target in &targets.targets {
                control.check(IndexWorkStage::SymbolParsing)?;
                let Some(path) = repository_join(&package.root, &target.path) else {
                    continue;
                };
                if cargo_automatic_target_directory(&path, &package.root).is_some_and(|directory| {
                    targets.automatic.iter().any(|automatic| {
                        automatic
                            .kind
                            .automatic_directory()
                            .and_then(|relative| repository_join(&package.root, relative))
                            .is_some_and(|enabled| enabled == directory)
                    })
                }) || [
                    SourceCargoTargetKind::Library,
                    SourceCargoTargetKind::Binary,
                    SourceCargoTargetKind::BuildScript,
                ]
                .into_iter()
                .any(|kind| {
                    kind.conventional_root()
                        .and_then(|root| repository_join(&package.root, root))
                        .is_some_and(|root| root == path)
                }) || packages
                    .package_owner(&path)
                    .is_none_or(|owner| owner.manifest != package.manifest)
                {
                    continue;
                }
                if paths.len() >= MAX_INCREMENTAL_RESOLUTION_ITEMS as usize {
                    return Err(resolution_key_limit_failure(paths.len().saturating_add(1)));
                }
                let string_bytes = path.capacity() as u64;
                self.reserve_cohort_bytes(STAGED_GRAPH_ROW_BYTES.saturating_add(string_bytes))?;
                temporary_rows = temporary_rows.saturating_add(STAGED_GRAPH_ROW_BYTES);
                temporary_strings = temporary_strings.saturating_add(string_bytes);
                paths.push(path);
            }
            paths.sort_unstable_by(|left, right| {
                parent(left)
                    .cmp(parent(right))
                    .then_with(|| left.cmp(right))
            });
            // Duplicate target declarations cannot create extra retained roots.
            paths.dedup();
            let mut retained_strings = 0_u64;
            let mut paths = paths.into_iter().peekable();
            while let Some(path) = paths.next() {
                control.check(IndexWorkStage::SymbolParsing)?;
                let directory = parent(&path);
                if paths.peek().is_none_or(|next| parent(next) != directory) {
                    continue;
                }
                let identity = if directory.is_empty() { "." } else { directory };
                let Some(cohort) = self.cohort(project, package, identity, CohortKind::Explicit)?
                else {
                    continue;
                };
                let mut member = path;
                loop {
                    self.reserve_cohort_bytes(STAGED_GRAPH_ROW_BYTES)?;
                    retained_strings = retained_strings.saturating_add(member.capacity() as u64);
                    index.explicit.insert(member, Arc::clone(&cohort));
                    if !paths.peek().is_some_and(|next| {
                        let directory = parent(next);
                        (if directory.is_empty() { "." } else { directory })
                            == cohort.directory.as_ref()
                    }) {
                        break;
                    }
                    let Some(next) = paths.next() else { break };
                    member = next;
                }
            }
            self.retained_bytes = self
                .retained_bytes
                .saturating_sub(temporary_rows)
                .saturating_sub(temporary_strings.saturating_sub(retained_strings));
            for automatic in &targets.automatic {
                control.check(IndexWorkStage::SymbolParsing)?;
                let Some(relative) = automatic.kind.automatic_directory() else {
                    continue;
                };
                let Some(directory) = repository_join(&package.root, relative) else {
                    continue;
                };
                if let Some(cohort) =
                    self.cohort(project, package, &directory, CohortKind::Automatic)?
                {
                    self.reserve_cohort_bytes(
                        STAGED_GRAPH_ROW_BYTES.saturating_add(directory.capacity() as u64),
                    )?;
                    index.automatic.insert(directory, cohort);
                }
            }
            self.reserve_cohort_bytes(
                STAGED_GRAPH_ROW_BYTES.saturating_add(package.manifest.len() as u64),
            )?;
            Ok(Arc::new(index))
        })();
        match result {
            Ok(index) => {
                self.cargo_cohorts
                    .insert(package.manifest.clone(), Arc::clone(&index));
                Ok(index)
            }
            Err(error) => {
                self.retained_bytes = before;
                Err(error)
            }
        }
    }
}

impl PackageIndex {
    /// Preserve admitted sidecars and hydrate only manifests needed by Rust projection.
    pub(super) fn from_sources(
        project: ProjectInstanceId,
        graphs: &[&super::SourceGraph<'_>],
        sources: &mut SourceBindingCache<'_>,
        control: &IndexWorkControl,
    ) -> Result<Self, CliError> {
        let mut package_bytes = 0_u64;
        let mut needed_bytes = 0_u64;
        let result = (|| -> Result<Self, CliError> {
            control.check(IndexWorkStage::SymbolParsing)?;
            let mut index =
                Self::from_graph_iter(graphs.iter().map(|graph| graph.graph()), &mut |bytes| {
                    sources.reserve_cohort_bytes(bytes)?;
                    package_bytes = package_bytes.saturating_add(bytes);
                    Ok(())
                })?;
            let mut needed = std::collections::BTreeSet::new();
            for graph in graphs {
                control.check(IndexWorkStage::SymbolParsing)?;
                if graph
                    .graph()
                    .language
                    .as_deref()
                    .and_then(projectatlas_core::language::language_capability)
                    .and_then(|language| language.effective_semantic_provider())
                    == Some(projectatlas_core::language::SemanticProviderOwner::Rust)
                    && let Some(package) = index.package_owner(&graph.graph().path)
                    && !needed.contains(&package.manifest)
                {
                    let bytes =
                        STAGED_GRAPH_ROW_BYTES.saturating_add(package.manifest.len() as u64);
                    sources.reserve_cohort_bytes(bytes)?;
                    needed_bytes = needed_bytes.saturating_add(bytes);
                    needed.insert(package.manifest.clone());
                }
            }
            for position in 0..index.packages.len() {
                let package = &index.packages[position];
                if !needed.contains(&package.manifest) {
                    continue;
                }
                control.check(IndexWorkStage::SymbolParsing)?;
                let fresh = graphs
                    .iter()
                    .find(|graph| graph.graph().path == package.manifest)
                    .and_then(|graph| graph.semantics())
                    .and_then(|facts| facts.cargo());
                let captured = if fresh.is_none() {
                    Some(sources.capture(&package.manifest, control)?)
                } else {
                    None
                };
                let Some(targets) = fresh
                    .or_else(|| {
                        captured
                            .as_ref()
                            .and_then(|source| source.semantics.cargo())
                    })
                    .filter(|targets| targets.complete)
                else {
                    continue;
                };
                let cohorts =
                    sources.cargo_root_cohorts(project, package, targets, &index, control)?;
                index.packages[position].cohorts = Some(cohorts);
            }
            Ok(index)
        })();
        // The temporary selector set always drops. A failed constructor also
        // drops its package rows; keep completed captured/cohort cache owners.
        let kept_package_bytes = result.as_ref().map_or(0, |index| index.retained_bytes);
        sources.retained_bytes = sources
            .retained_bytes
            .saturating_sub(needed_bytes)
            .saturating_sub(package_bytes.saturating_sub(kept_package_bytes));
        result
    }

    /// Source projection publishes a custom cohort only on an exact listed root.
    pub(super) fn root_cohort_directory(&self, path: &str) -> Option<&str> {
        self.package_owner(path)?
            .cohorts
            .as_ref()?
            .explicit_directory(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::graph_projection::SourceGraph;
    use projectatlas_core::IndexCancellation;
    use projectatlas_db::AtlasStore;
    use projectatlas_symbols::extract_symbol_graph_with_semantics_controlled;
    use std::error::Error;
    use std::fmt::Write as _;
    use std::io;

    /// Preserve useful diagnostics without panicking in fallible tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if !condition {
            return Err(io::Error::other(message).into());
        }
        Ok(())
    }

    #[test]
    fn explicit_root_cohorts_publish_only_listed_siblings_and_cache_once()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let store = AtlasStore::open_for_project(&temp.path().join("atlas.db"), temp.path())?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("missing project"))?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let mut manifest =
            "[package]\nname=\"cohorts\"\nversion=\"0.1.0\"\nedition=\"2024\"\nautobins=false\n"
                .to_string();
        let paths = [
            "app/a.rs",
            "app/b.rs",
            "one.rs",
            "two.rs",
            "solo/entry.rs",
            "nested/one/main.rs",
            "nested/two/main.rs",
            "src/lib.rs",
            "src/bin/a.rs",
            "src/bin/b.rs",
        ];
        for (index, path) in paths.iter().enumerate() {
            write!(
                manifest,
                "[[bin]]\nname=\"target{index}\"\npath=\"{path}\"\n"
            )?;
        }
        let (_, manifest_graph, manifest_facts) = extract_symbol_graph_with_semantics_controlled(
            "Cargo.toml",
            Some("cargo-manifest"),
            &manifest,
            &control,
        )?
        .into_parts();
        let (_, graph, facts) = extract_symbol_graph_with_semantics_controlled(
            "app/a.rs",
            Some("rust"),
            "fn run(){}",
            &control,
        )?
        .into_parts();
        let graphs = [
            SourceGraph::Staged {
                graph: &manifest_graph,
                semantics: Some(&manifest_facts),
            },
            SourceGraph::Staged {
                graph: &graph,
                semantics: Some(&facts),
            },
        ];
        let references = graphs.iter().collect::<Vec<_>>();
        let mut sources = SourceBindingCache::new(temp.path(), &[], None);
        let packages = PackageIndex::from_sources(project, &references, &mut sources, &control)?;
        require(
            packages.root_cohort_directory("app/a.rs") == Some("app"),
            "first sibling has no producer",
        )?;
        require(
            packages.root_cohort_directory("app/b.rs") == Some("app"),
            "missing sibling lost its producer",
        )?;
        require(
            packages.root_cohort_directory("one.rs") == Some("."),
            "root directory identity was empty",
        )?;
        for path in [
            "app/helper.rs",
            "solo/entry.rs",
            "nested/one/main.rs",
            "src/lib.rs",
        ] {
            require(
                packages.root_cohort_directory(path).is_none(),
                "non-cohort source gained custom authority",
            )?;
        }
        let package = packages
            .package_owner("app/a.rs")
            .ok_or_else(|| io::Error::other("missing package"))?;
        let cohorts = package
            .cohorts
            .as_ref()
            .ok_or_else(|| io::Error::other("missing cohort index"))?;
        let first = cohorts
            .candidate("app/a.rs", "")
            .ok_or_else(|| io::Error::other("missing first group"))?;
        let second = cohorts
            .candidate("app/b.rs", "")
            .ok_or_else(|| io::Error::other("missing second group"))?;
        require(
            Arc::ptr_eq(&first, &second),
            "siblings copied their cohort owner",
        )?;
        let configured = super::super::ConfiguredModuleResolution::default();
        let context =
            super::super::resolution_projection_context("app/a.rs", &packages, &configured)
                .with_source_semantics(&facts);
        let keys = projectatlas_symbols::derive_resolution_keys_with_context(
            project,
            Some("cohorts"),
            &graph,
            context,
        )?;
        require(
            keys.source_keys().contains(&first.key),
            "source producer disagrees with membership",
        )?;
        require(
            packages.root_cohort_directory("src/bin/a.rs") == Some("src/bin"),
            "disabled-kind explicit root lost its exclusive producer",
        )?;
        let disabled = cohorts
            .candidate("src/bin/a.rs", "")
            .ok_or_else(|| io::Error::other("missing disabled-kind sibling cohort"))?;
        let (_, helper_graph, helper_facts) = extract_symbol_graph_with_semantics_controlled(
            "src/bin/helper.rs",
            Some("rust"),
            "fn helper(){}",
            &control,
        )?
        .into_parts();
        let helper_context = super::super::resolution_projection_context(
            "src/bin/helper.rs",
            &packages,
            &configured,
        )
        .with_source_semantics(&helper_facts);
        let helper_keys = projectatlas_symbols::derive_resolution_keys_with_context(
            project,
            Some("cohorts"),
            &helper_graph,
            helper_context,
        )?;
        require(
            !helper_keys.source_keys().contains(&disabled.key),
            "disabled auto discovery made an unlisted helper a root invalidator",
        )?;
        let before = sources.retained_bytes;
        let repeated = PackageIndex::from_sources(project, &references, &mut sources, &control)?;
        require(
            sources.retained_bytes == before.saturating_add(repeated.retained_bytes)
                && sources.captured.is_empty(),
            "repeated package owner omitted its allocation or duplicated cached metadata",
        )?;
        let repeated_index = repeated
            .package_owner("app/a.rs")
            .and_then(|package| package.cohorts.as_ref())
            .ok_or_else(|| io::Error::other("missing repeated index"))?;
        require(
            Arc::ptr_eq(cohorts, repeated_index),
            "operation duplicated the derived root index",
        )?;
        Ok(())
    }

    #[test]
    fn root_cohort_refusal_and_cancellation_leave_no_partial_cache() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let store = AtlasStore::open_for_project(&temp.path().join("atlas.db"), temp.path())?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("missing project"))?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let manifest = "[package]\nname=\"cohorts\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[bin]]\nname=\"a\"\npath=\"app/a.rs\"\n[[bin]]\nname=\"b\"\npath=\"app/b.rs\"\n";
        let (_, manifest_graph, manifest_facts) = extract_symbol_graph_with_semantics_controlled(
            "Cargo.toml",
            Some("cargo-manifest"),
            manifest,
            &control,
        )?
        .into_parts();
        let (_, graph, facts) = extract_symbol_graph_with_semantics_controlled(
            "app/a.rs",
            Some("rust"),
            "fn run(){}",
            &control,
        )?
        .into_parts();
        let graphs = [
            SourceGraph::Staged {
                graph: &manifest_graph,
                semantics: Some(&manifest_facts),
            },
            SourceGraph::Staged {
                graph: &graph,
                semantics: Some(&facts),
            },
        ];
        let references = graphs.iter().collect::<Vec<_>>();
        let mut sources = SourceBindingCache::new(temp.path(), &[], None);
        let before = super::super::super::MAX_PUBLICATION_STAGING_BYTES - 1;
        sources.retained_bytes = before;
        require(
            PackageIndex::from_sources(project, &references, &mut sources, &control).is_err(),
            "root cache crossed its owner budget",
        )?;
        require(
            sources.retained_bytes == before && sources.cargo_cohorts.is_empty(),
            "refusal retained partial cohort state",
        )?;
        sources.retained_bytes = 0;
        let package_envelope =
            PackageIndex::from_graphs(std::slice::from_ref(&manifest_graph))?.retained_bytes;
        sources.set_retained_limit(
            package_envelope
                .saturating_add(STAGED_GRAPH_ROW_BYTES)
                .saturating_add("Cargo.toml".len() as u64)
                .saturating_add(1),
        )?;
        require(
            PackageIndex::from_sources(project, &references, &mut sources, &control).is_err(),
            "mid-construction resource limit was ignored",
        )?;
        require(
            sources.retained_bytes == 0 && sources.cargo_cohorts.is_empty(),
            "failed package construction leaked a partial reservation",
        )?;
        sources.set_retained_limit(super::super::super::MAX_PUBLICATION_STAGING_BYTES)?;
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let cancelled = IndexWorkControl::new(cancellation, None);
        sources.retained_bytes = 0;
        require(
            PackageIndex::from_sources(project, &references, &mut sources, &cancelled).is_err(),
            "cancelled cohort construction succeeded",
        )?;
        require(
            sources.retained_bytes == 0 && sources.cargo_cohorts.is_empty(),
            "cancellation retained partial cohort state",
        )?;
        Ok(())
    }
}
