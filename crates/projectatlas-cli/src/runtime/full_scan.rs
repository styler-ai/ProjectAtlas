//! Full-scan staging, rebuild, atomic publication, and exact file cleanup.

use super::{
    PurposeImportReport, ScanConfigurationInput, ScanReport, ScanRuntimePlan, SymbolBuildOptions,
    TextIndexOptions, build_symbols_for_staging, imported_purpose_records,
    refresh_structural_summaries_for_nodes, refresh_text_index_for_nodes_with_rows,
    repository_state_signature, seed_builtin_projectatlas_purposes,
};
use crate::CliError;
use projectatlas_core::{Node, PurposeSource, PurposeStatus};
use projectatlas_db::{AtlasStore, StructuralStaging};
use projectatlas_fs::{ScanOptions, scan_repo};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Responsibility-named prefix for sibling staging database files.
const STAGING_FILE_PREFIX: &str = "projectatlas-full-scan";
/// Process-local uniqueness source for staging database allocation.
static STAGING_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Execute one full scan entirely in a separate database before publication.
pub(crate) fn run_scan_pipeline(
    store: &mut AtlasStore,
    db_path: &Path,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
) -> Result<ScanReport, CliError> {
    let inputs = StructuralInputManifest::capture(plan, symbol_options)?;
    let mut files = StagingDatabaseFiles::allocate(db_path)?;
    let staging = store.create_structural_staging(db_path, files.path(), &plan.root)?;
    let mut staging_store = AtlasStore::open(staging.path())?;
    staging_store.prepare_structural_full_scan()?;
    let mut report = rebuild_staging(&mut staging_store, plan, symbol_options, &inputs.nodes)?;
    staging_store.set_staged_structural_state_signature(&staging, &inputs.state_signature)?;
    staging_store.seal_structural_staging(&staging)?;
    drop(staging_store);

    publish_if_inputs_current(store, &staging, plan, symbol_options, &inputs)?;
    report.overview = store.overview()?;
    files.cleanup()?;
    Ok(report)
}

/// Run the existing scan, purpose, lexical, symbol, and summary work on staging.
fn rebuild_staging(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    nodes: &[Node],
) -> Result<ScanReport, CliError> {
    store.set_project_root(&plan.root)?;
    store.replace_scan(nodes)?;
    seed_builtin_projectatlas_purposes(store, nodes)?;
    let text_refresh =
        refresh_text_index_for_nodes_with_rows(store, &plan.root, nodes, plan.text_options)?;
    let text_index = text_refresh.report.clone();
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    let existing_purpose_paths = store
        .load_nodes()?
        .into_iter()
        .filter(|node| {
            matches!(
                node.purpose.status,
                PurposeStatus::Approved | PurposeStatus::Stale
            )
        })
        .map(|node| node.node.path)
        .collect::<HashSet<_>>();
    let mut purpose_import = PurposeImportReport::default();
    if let Some(config) = plan.config.as_ref() {
        for record in imported_purpose_records(config)? {
            if !indexed_paths.contains(record.path.as_str()) {
                purpose_import.skipped_stale += 1;
                continue;
            }
            if existing_purpose_paths.contains(record.path.as_str()) {
                purpose_import.skipped_existing += 1;
                continue;
            }
            store.set_purpose(&record.path, &record.summary, PurposeSource::Imported)?;
            purpose_import.imported += 1;
        }
    }
    let symbols = build_symbols_for_staging(store, &plan.root, symbol_options)?;
    let structural_summaries =
        refresh_structural_summaries_for_nodes(store, nodes, &text_refresh.rows)?;
    let overview = store.overview()?;
    Ok(ScanReport {
        overview,
        purpose_import,
        text_index,
        structural_summaries,
        symbols,
    })
}

/// Complete external inputs whose identity can change while staging is built.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StructuralInputManifest {
    /// Canonical project root used by discovery.
    root: PathBuf,
    /// Exact configuration source and content used to derive scan policy.
    configuration: ScanConfigurationInput,
    /// Effective `ProjectAtlas` ignore policy used by discovery.
    scan_options: ScanOptions,
    /// Effective lexical byte budget used by staging.
    text_options: TextIndexOptions,
    /// Ordered source, ignore-file, root-folder, and content identities.
    nodes: Vec<Node>,
    /// Versioned content, Git, policy, and structural-budget identity.
    state_signature: String,
}

impl StructuralInputManifest {
    /// Capture current source and scan-policy inputs through the production scanner.
    fn capture(
        plan: &ScanRuntimePlan,
        symbol_options: &SymbolBuildOptions,
    ) -> Result<Self, CliError> {
        let current = ScanRuntimePlan::for_path(
            plan.requested_config_path.as_deref(),
            &plan.root,
            plan.text_index_max_bytes,
        )?;
        if current.root != plan.root
            || current.configuration_input != plan.configuration_input
            || current.scan_options != plan.scan_options
            || current.text_options != plan.text_options
        {
            return Err(CliError::ScanInputsChanged {
                detail: "configuration, ignore policy, root, or structural budget changed"
                    .to_owned(),
            });
        }
        let nodes = scan_repo(&current.root, &current.scan_options)?;
        let state_signature = repository_state_signature(
            &current.root,
            &nodes,
            &current.scan_options,
            current.text_options,
            symbol_options,
        );
        Ok(Self {
            root: current.root,
            configuration: current.configuration_input,
            scan_options: current.scan_options,
            text_options: current.text_options,
            nodes,
            state_signature,
        })
    }

    /// Recompute every mutable input and reject a stale staged generation.
    fn ensure_current(
        &self,
        plan: &ScanRuntimePlan,
        symbol_options: &SymbolBuildOptions,
    ) -> Result<(), CliError> {
        let current = Self::capture(plan, symbol_options)?;
        if *self == current {
            return Ok(());
        }
        Err(CliError::ScanInputsChanged {
            detail: self.first_difference(&current),
        })
    }

    /// Describe the first material input difference without exposing file contents.
    fn first_difference(&self, current: &Self) -> String {
        if self.root != current.root {
            return "canonical project root changed".to_owned();
        }
        if self.configuration != current.configuration {
            return "configuration source or content changed".to_owned();
        }
        if self.scan_options != current.scan_options {
            return "effective ProjectAtlas ignore policy changed".to_owned();
        }
        if self.text_options != current.text_options {
            return "structural text-index budget changed".to_owned();
        }
        let differing_path = self
            .nodes
            .iter()
            .zip(&current.nodes)
            .find_map(|(before, after)| (before != after).then_some(after.path.as_str()))
            .or_else(|| {
                self.nodes
                    .get(current.nodes.len())
                    .map(|node| node.path.as_str())
            })
            .or_else(|| {
                current
                    .nodes
                    .get(self.nodes.len())
                    .map(|node| node.path.as_str())
            })
            .unwrap_or("repository source set");
        format!("repository input changed at '{differing_path}'")
    }
}

/// Publish only while the staged generation still matches live inputs.
fn publish_if_inputs_current(
    store: &mut AtlasStore,
    staging: &StructuralStaging,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    inputs: &StructuralInputManifest,
) -> Result<(), CliError> {
    inputs.ensure_current(plan, symbol_options)?;
    store.publish_structural_staging(staging)?;
    Ok(())
}

/// Exact staging database and `SQLite` sidecars owned by one scan invocation.
struct StagingDatabaseFiles {
    /// Allocated sibling staging database path.
    path: PathBuf,
    /// Whether explicit cleanup already completed successfully.
    cleaned: bool,
}

impl StagingDatabaseFiles {
    /// Allocate one nonexistent sibling staging database path.
    fn allocate(live_path: &Path) -> Result<Self, CliError> {
        let parent = live_path.parent().unwrap_or_else(|| Path::new("."));
        let process_id = std::process::id();
        for _ in 0..100 {
            let sequence = STAGING_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".{STAGING_FILE_PREFIX}-{process_id}-{sequence}.db"));
            if sqlite_paths(&path)
                .iter()
                .all(|candidate| !candidate.exists())
            {
                return Ok(Self {
                    path,
                    cleaned: false,
                });
            }
        }
        Err(CliError::InvalidInput(format!(
            "could not allocate a unique full-scan staging database beside '{}'",
            live_path.display()
        )))
    }

    /// Return the owned staging database path.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Remove the exact database, WAL, and shared-memory files.
    fn cleanup(&mut self) -> Result<(), CliError> {
        remove_sqlite_paths(&self.path)?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for StagingDatabaseFiles {
    fn drop(&mut self) {
        if !self.cleaned {
            drop(remove_sqlite_paths(&self.path));
        }
    }
}

/// Remove one database path and only its exact `SQLite` sidecars.
fn remove_sqlite_paths(path: &Path) -> Result<(), CliError> {
    for candidate in sqlite_paths(path) {
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CliError::Io {
                    path: candidate,
                    source,
                });
            }
        }
    }
    Ok(())
}

/// Return the exact database, WAL, and shared-memory paths for cleanup.
fn sqlite_paths(path: &Path) -> [PathBuf; 3] {
    [
        path.to_path_buf(),
        append_suffix(path, "-wal"),
        append_suffix(path, "-shm"),
    ]
}

/// Append an `SQLite` sidecar suffix without replacing the database extension.
fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_arri_ut_arri_3_6() -> Result<(), CliError> {
        let _: fn(
            &mut AtlasStore,
            &Path,
            &ScanRuntimePlan,
            &SymbolBuildOptions,
        ) -> Result<ScanReport, CliError> = run_scan_pipeline;

        for (owner, source, expected) in [
            (
                "runtime re-export",
                include_str!("../runtime.rs"),
                "pub(crate) use full_scan::run_scan_pipeline;",
            ),
            (
                "CLI scan dispatch",
                include_str!("../main.rs"),
                "run_scan_pipeline(&mut store, &cli.db, &plan, &symbol_options)?;",
            ),
            (
                "MCP scan dispatch",
                include_str!("../mcp.rs"),
                "run_scan_pipeline(&mut store, &state.db_path, &plan, &symbol_options)?;",
            ),
            (
                "init bootstrap",
                include_str!("../runtime.rs"),
                "run_scan_pipeline(&mut store, db_path, &plan, &symbol_options)",
            ),
        ] {
            if !source.contains(expected) {
                return Err(CliError::InvalidInput(format!(
                    "full-scan runtime split lost its {owner} caller"
                )));
            }
        }
        Ok(())
    }

    #[test]
    fn staging_cleanup_removes_only_owned_database_and_sidecars() -> Result<(), CliError> {
        let temp = tempfile::tempdir().map_err(|source| CliError::Io {
            path: PathBuf::from("temporary-directory"),
            source,
        })?;
        let live_path = temp.path().join("projectatlas.db");
        fs::write(&live_path, b"live").map_err(|source| CliError::Io {
            path: live_path.clone(),
            source,
        })?;
        let mut files = StagingDatabaseFiles::allocate(&live_path)?;
        for path in sqlite_paths(files.path()) {
            fs::write(&path, b"stage").map_err(|source| CliError::Io {
                path: path.clone(),
                source,
            })?;
        }
        files.cleanup()?;
        if sqlite_paths(files.path()).iter().any(|path| path.exists()) {
            return Err(CliError::InvalidInput(
                "staging cleanup retained an owned SQLite file".to_owned(),
            ));
        }
        let live = fs::read(&live_path).map_err(|source| CliError::Io {
            path: live_path,
            source,
        })?;
        if live != b"live" {
            return Err(CliError::InvalidInput(
                "staging cleanup mutated the live database path".to_owned(),
            ));
        }
        Ok(())
    }

    #[test]
    fn source_change_before_publication_preserves_active_generation() -> Result<(), CliError> {
        let temp = tempfile::tempdir().map_err(|source| CliError::Io {
            path: PathBuf::from("temporary-directory"),
            source,
        })?;
        let root = temp.path().join("repository");
        let source_path = root.join("src").join("lib.rs");
        fs::create_dir_all(source_path.parent().unwrap_or(&root)).map_err(|source| {
            CliError::Io {
                path: source_path.clone(),
                source,
            }
        })?;
        fs::write(&source_path, b"pub fn value() -> u8 { 1 }\n").map_err(|source| {
            CliError::Io {
                path: source_path.clone(),
                source,
            }
        })?;
        let db_path = root.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(db_path.parent().unwrap_or(&root)).map_err(|source| CliError::Io {
            path: db_path.clone(),
            source,
        })?;

        let mut live = AtlasStore::open(&db_path)?;
        live.set_project_root(&root)?;
        let plan = ScanRuntimePlan::for_path(None, &root, None)?;
        let symbol_options = SymbolBuildOptions::new(1024 * 1024, Some(1), Some(30));
        let inputs = StructuralInputManifest::capture(&plan, &symbol_options)?;
        let mut files = StagingDatabaseFiles::allocate(&db_path)?;
        let staging = live.create_structural_staging(&db_path, files.path(), &root)?;
        let mut staging_store = AtlasStore::open(staging.path())?;
        staging_store.prepare_structural_full_scan()?;
        let _report = rebuild_staging(&mut staging_store, &plan, &symbol_options, &inputs.nodes)?;
        staging_store.set_staged_structural_state_signature(&staging, &inputs.state_signature)?;
        staging_store.seal_structural_staging(&staging)?;
        drop(staging_store);
        let before = live.publication_state()?;

        fs::write(&source_path, b"pub fn value() -> u8 { 2 }\n").map_err(|source| {
            CliError::Io {
                path: source_path,
                source,
            }
        })?;
        match publish_if_inputs_current(&mut live, &staging, &plan, &symbol_options, &inputs) {
            Err(CliError::ScanInputsChanged { .. }) => {}
            Err(other) => return Err(other),
            Ok(()) => {
                return Err(CliError::InvalidInput(
                    "stale structural staging unexpectedly published".to_owned(),
                ));
            }
        }
        if live.publication_state()? != before {
            return Err(CliError::InvalidInput(
                "stale input validation advanced the active publication".to_owned(),
            ));
        }
        files.cleanup()?;
        Ok(())
    }

    #[test]
    fn external_configuration_change_invalidates_scan_inputs() -> Result<(), CliError> {
        let temp = tempfile::tempdir().map_err(|source| CliError::Io {
            path: PathBuf::from("temporary-directory"),
            source,
        })?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root).map_err(|source| CliError::Io {
            path: root.clone(),
            source,
        })?;
        let config_path = temp.path().join("scan-config.toml");
        fs::write(&config_path, b"# initial scan configuration\n").map_err(|source| {
            CliError::Io {
                path: config_path.clone(),
                source,
            }
        })?;
        let plan = ScanRuntimePlan::for_path(Some(&config_path), &root, None)?;
        let symbol_options = SymbolBuildOptions::new(1024 * 1024, Some(1), Some(30));
        let inputs = StructuralInputManifest::capture(&plan, &symbol_options)?;

        fs::write(&config_path, b"# changed scan configuration\n").map_err(|source| {
            CliError::Io {
                path: config_path,
                source,
            }
        })?;
        match inputs.ensure_current(&plan, &symbol_options) {
            Err(CliError::ScanInputsChanged { .. }) => Ok(()),
            Err(other) => Err(other),
            Ok(()) => Err(CliError::InvalidInput(
                "external configuration drift was not detected".to_owned(),
            )),
        }
    }
}
