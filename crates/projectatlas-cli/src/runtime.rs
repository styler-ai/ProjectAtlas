//! Purpose: Coordinate shared `ProjectAtlas` CLI and MCP runtime workflows.
//! Shared runtime orchestration for the `ProjectAtlas` CLI and MCP adapters.

mod graph_projection;
mod module_resolution;
#[cfg(feature = "optional-parser-supervisor")]
mod optional_parser_runtime;
mod source_observation;

pub(crate) use source_observation::{
    SourceObservationRegistry, VerifiedReadOutcome, VerifiedReadStamp,
};

use crate::atlas_map::{
    self, init_project_with_config, load_atlas_config, load_atlas_config_for_root,
    load_atlas_config_from_text,
};
use crate::structural::{
    is_scanner_fallback_summary, is_structural_summary_candidate, structural_summary_for_path,
};
use crate::{
    CliError, OutputFormat, WATCH_MODE_NOTIFY, WATCH_MODE_ONCE, WATCH_MODE_POLLING, truthy_env,
};
use blake3::Hasher;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
#[cfg(feature = "optional-parser-supervisor")]
use projectatlas_cli::optional_parser_lifecycle::{
    OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH, OptionalParserPackLifecycle,
    OptionalParserPackLifecycleReport, OptionalParserPackProjectSelection,
};
use projectatlas_core::health::{
    CATEGORY_DUPLICATE_PURPOSE, CATEGORY_MISSING_PURPOSE, CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
    CATEGORY_REPEATED_TEMPORARY_FOLDER, CATEGORY_STALE_PURPOSE, CATEGORY_SUGGESTED_PURPOSE_REVIEW,
    Severity,
};
use projectatlas_core::language::{
    ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION, LANGUAGE_CAPABILITY_REGISTRY_VERSION,
    LanguageRegistryReport, SymbolParserOwner, accepted_language_capability_digest,
    language_capability, language_registry_digest, language_registry_report,
};
#[cfg(all(test, feature = "optional-parser-supervisor"))]
use projectatlas_core::optional_parser_pack::OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION;
use projectatlas_core::outline::estimate_tokens;
use projectatlas_core::relation_capabilities::{
    RelationFamilyInventoryReport, relation_family_inventory_report,
};
use projectatlas_core::symbols::{
    ParserKind, RelationKind, SourceParseMetadata, SymbolGraph, SymbolKind,
};
use projectatlas_core::telemetry::{
    TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_SELECTED_CANDIDATES,
    TOKEN_BUCKET_NAVIGATION_AVOIDANCE, TOKEN_CONFIDENCE_INFERRED, TOKEN_CONFIDENCE_POLICY_ESTIMATE,
    UsageInstanceId, UsageInstanceOwner, usage_from_estimates_with_context, usage_from_text,
};
use projectatlas_core::toon::{encode_agent_payload, render_ranked_node_rows};
use projectatlas_core::{
    IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkResource,
    IndexWorkStage, Node, NodeKind, Overview, PurposeSource, PurposeStatus,
    normalize_native_path_display, normalize_native_path_display_str, normalize_repo_path,
    purpose_review_signal, repo_path_to_native, validated_repo_file_key, validated_repo_node_key,
};
use projectatlas_db::{
    AtlasStore, DatabasePublicationContractState, DatabasePublicationReport,
    DatabaseSchemaCompatibility, DatabaseSettingsReport, HealthFindingsPage, HealthQuery,
    HealthScope, IndexPublication, IndexPublicationGuard, IndexPublicationState, IndexedFileText,
    MAX_PURPOSE_CURATION_BATCH_ROWS, PurposeConditionalApplyRequest, PurposeConditionalApplyState,
    TelemetryRetentionState, database_settings_report, read_project_root_read_only,
    validate_database_location,
};
use projectatlas_fs::{
    FsError, RootScanPolicy, ScanLimits, ScanOptions, gitignore_excludes_path,
    scan_path_with_policy_controlled, scan_repo, scan_repo_controlled,
    scan_repo_controlled_with_work,
};
use projectatlas_service::{
    CoverageDiscoveryReport, FederatedInputWork, FederatedStore, FilePathMatcher,
    MAX_FEDERATED_DATABASE_BYTES, MAX_FEDERATED_INPUT_BYTES, NextStepReport, build_next_report,
    load_ranked_file_nodes_with_reasons, load_ranked_folder_nodes_with_reasons,
    validate_federated_root_count,
};
use projectatlas_symbols::{extract_symbol_graph_controlled, semantic_resolution_contract_digest};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum file size parsed for symbols by default.
pub(crate) const MAX_SYMBOL_FILE_BYTES: u64 = 2_000_000;
/// Default health rows returned when the caller does not request a page size.
pub(crate) const DEFAULT_HEALTH_LIMIT: usize = 50;
/// Maximum health rows returned in one payload.
pub(crate) const MAX_HEALTH_LIMIT: usize = 200;
/// Maximum JSON bytes read for one CLI purpose-review batch.
pub(crate) const MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES: u64 = 2 * 1_024 * 1_024;
/// Maximum output retained from one effective Git config query.
const MAX_GIT_CONFIG_QUERY_OUTPUT_BYTES: usize = 64 * 1_024;
/// Maximum time allowed for one effective Git config query.
const GIT_CONFIG_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Default whole-operation deadline when no narrower parser limit is supplied.
const DEFAULT_INDEX_WORK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Maximum UTF-8 source bytes retained while one publication is staged.
const MAX_STAGED_TEXT_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum aggregate retained string bytes across one in-memory publication batch.
const MAX_PUBLICATION_STAGING_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum scan-node mutations applied between publication cancellation checks.
const PUBLICATION_NODE_BATCH_SIZE: usize = 1_024;
/// Maximum deleted repository paths applied between publication cancellation checks.
const PUBLICATION_PATH_BATCH_SIZE: usize = 128;
/// Maximum persisted source texts applied between publication cancellation checks.
const PUBLICATION_TEXT_BATCH_SIZE: usize = 32;
/// Maximum symbol parse results retained before sequential persistence.
#[cfg(not(feature = "optional-parser-supervisor"))]
const SYMBOL_PARSE_BATCH_SIZE: usize = 64;
/// Maximum symbol candidates accepted by one publication.
const MAX_SYMBOL_PARSE_JOBS: usize = 100_000;
/// Maximum event paths accepted by one incremental watcher publication.
const MAX_INCREMENTAL_CHANGED_PATHS: usize = 100_000;
/// Maximum source bytes accepted by one incremental watcher publication.
const MAX_INCREMENTAL_SOURCE_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum native watcher events buffered before continuity becomes uncertain.
const WATCH_EVENT_QUEUE_CAPACITY: usize = 1_024;
/// Maximum indexing workers regardless of a larger caller request.
pub(crate) const INDEX_WORKER_SAFE_CEILING: usize = 32;
/// Chunk size used by cancellation-aware bounded source reads.
const CONTROLLED_SOURCE_READ_BUFFER_BYTES: usize = 8_192;
/// Maximum aggregate authored-purpose bytes inspected by one publication.
const MAX_PURPOSE_IMPORT_BYTES: u64 = 512 * 1_024 * 1_024;
/// Maximum complete config, map, or non-source purpose input size.
const MAX_PURPOSE_INPUT_FILE_BYTES: u64 = 16 * 1_024 * 1_024;
/// Maximum bytes in one repository path supplied to purpose review.
const MAX_PURPOSE_REVIEW_PATH_BYTES: usize = 4 * 1_024;
/// Maximum bytes in one non-path purpose-review string field.
const MAX_PURPOSE_REVIEW_FIELD_BYTES: usize = 64 * 1_024;
/// Purpose-review report field name for an item error.
const PURPOSE_REVIEW_REPORT_ERROR_FIELD: &str = "error";
/// Maximum aggregate string bytes admitted to one purpose-review batch.
const MAX_PURPOSE_REVIEW_INPUT_BYTES: usize = 512 * 1_024;
/// Maximum retained item/output bytes for one purpose-review report.
const MAX_PURPOSE_REVIEW_REPORT_BYTES: usize = 4 * 1_024 * 1_024;
/// Maximum source prefix inspected for a legacy purpose header.
const MAX_PURPOSE_HEADER_BYTES: u64 = 256 * 1_024;
/// Maximum normalized legacy purpose rows admitted by one publication.
const MAX_PURPOSE_IMPORT_RECORDS: u64 = 1_000_000;

/// Built-in purposes for reserved project-local `ProjectAtlas` metadata inputs.
const BUILTIN_PROJECTATLAS_PURPOSES: &[(&str, &str)] = &[
    (
        ".projectatlas",
        "Store project-local ProjectAtlas metadata, configuration, and runtime state.",
    ),
    (
        ".projectatlas/config.toml",
        "Configure project-local ProjectAtlas scan, lint, purpose, and output policy.",
    ),
    (
        ".projectatlas/projectatlas-nonsource-files.toon",
        "Declare project-local non-source file purposes for ProjectAtlas map compatibility.",
    ),
    (
        ".projectatlas/projectatlas-purpose-review.json",
        "Replay agent-reviewed ProjectAtlas purpose records into the local SQLite index.",
    ),
];
/// Core project-local files whose edits can change source-selection policy.
const CORE_INDEX_POLICY_PATHS: &[&str] = &[".projectatlas/config.toml", "projectatlas.toml"];

/// Resolved scan runtime policy shared by CLI and MCP adapters.
pub(crate) struct ScanRuntimePlan {
    /// Canonical project root.
    pub(crate) root: PathBuf,
    /// Optional `ProjectAtlas` config discovered for the root.
    pub(crate) config: Option<atlas_map::AtlasMapConfig>,
    /// Exact config file selected for this plan, if one exists.
    selected_config_path: Option<PathBuf>,
    /// Explicit config selector supplied by the caller, if any.
    config_path_override: Option<PathBuf>,
    /// Filesystem scanner options derived from config.
    pub(crate) scan_options: ScanOptions,
    /// `SQLite` text-index options derived from config and command override.
    pub(crate) text_options: TextIndexOptions,
    /// Explicit text-index limit supplied by the caller, if any.
    text_index_max_bytes_override: Option<u64>,
    /// Content-free optional parser selection bound into derived publication identity.
    #[cfg(feature = "optional-parser-supervisor")]
    optional_parser_selection: OptionalParserPackProjectSelection,
}

/// Deterministic purpose-import rows and the inputs that produced them.
struct PurposeImportSnapshot {
    /// Normalized purpose records staged by a full scan.
    records: Vec<atlas_map::ImportedPurposeRecord>,
    /// Digest of selected configuration, external inputs, and normalized rows.
    fingerprint: String,
}

/// Hard authored-purpose input limits for one publication attempt.
#[derive(Clone, Copy)]
struct PurposeImportLimits {
    /// Aggregate bytes read across all purpose inputs.
    total_bytes: u64,
    /// Maximum bytes in a complete config, map, or non-source input.
    complete_file_bytes: u64,
    /// Maximum prefix bytes inspected from one source file.
    header_bytes: u64,
    /// Maximum normalized records admitted after parsing.
    records: u64,
}

impl Default for PurposeImportLimits {
    fn default() -> Self {
        Self {
            total_bytes: MAX_PURPOSE_IMPORT_BYTES,
            complete_file_bytes: MAX_PURPOSE_INPUT_FILE_BYTES,
            header_bytes: MAX_PURPOSE_HEADER_BYTES,
            records: MAX_PURPOSE_IMPORT_RECORDS,
        }
    }
}

/// Operation-owned reader for authored purpose and publication inputs.
struct PurposeInputReader<'a> {
    /// Shared cancellation and deadline boundary for the publication.
    control: &'a IndexWorkControl,
    /// Byte and record limits for authored purpose inputs.
    limits: PurposeImportLimits,
    /// Inputs that must be consumed completely rather than as header prefixes.
    complete_paths: BTreeSet<PathBuf>,
    /// Configured legacy folder-purpose filename.
    purpose_filename: String,
    /// Exact digests retained only for complete publication-contract inputs.
    complete_digests: BTreeMap<PathBuf, String>,
}

/// Maximum changed paths included in a freshness failure payload.
const INDEX_FRESHNESS_SAMPLE_LIMIT: usize = 8;
/// Maximum affected paths a normal read may reconcile before answering.
const NORMAL_READ_REFRESH_MAX_PATHS: usize = 64;
/// Maximum current source bytes a normal read may reconcile before answering.
const NORMAL_READ_REFRESH_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum current source bytes one navigation read may allocate and inspect.
const MAX_INDEXED_NAVIGATION_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
/// Explicit version of the built-in derived-index projection contract.
const INDEX_DERIVATION_CONTRACT_VERSION: &str = "2";

/// Closed state returned when an index-backed read cannot proceed safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IndexReadStatus {
    /// The selected project has not been initialized.
    InitRequired,
    /// A bare/common Git directory was selected instead of a source worktree.
    WorktreeRequired,
    /// Current saved local source differs from the durable index.
    RefreshRequired,
    /// Current saved local source could not be inspected completely.
    VerificationIncomplete,
    /// The opened index belongs to a different project root.
    ProjectMismatch,
}

/// Closed reason for refusing an index-backed read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IndexRefreshReason {
    /// Existing indexed source bytes or structural metadata changed.
    SourceChanged,
    /// Paths were added, removed, renamed, ignored, or unignored.
    PathsChanged,
    /// Parser, source-selection, or indexing policy drifted.
    PolicyDrift,
    /// Dependency-aware incremental refresh exceeded its aggregate safe closure.
    DependencyClosureLimit,
}

/// Scope required to recover a stale index safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IndexRefreshScope {
    /// A bounded affected-path publication can restore current results.
    Incremental,
    /// Current publication safety requires a complete one-shot refresh.
    Full,
}

/// Current local-source delta detected before a normal indexed read.
struct IndexFreshnessDelta {
    /// Typed public report when the delta cannot be reconciled automatically.
    report: IndexRefreshRequired,
    /// Complete native path set used for a safe affected-path publication.
    paths: HashSet<PathBuf>,
}

/// Measured work performed by one exact source-freshness verification.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceVerificationWork {
    /// Repository entries inspected by the exact filesystem scan.
    pub(crate) filesystem_entries: u64,
    /// Current source bytes hashed by the exact filesystem scan.
    pub(crate) filesystem_bytes: u64,
    /// `SQLite` read statements owned directly by freshness verification.
    pub(crate) sqlite_read_statements: u64,
    /// Indexed nodes decoded for exact current-versus-durable comparison.
    pub(crate) decoded_nodes: u64,
}

/// Exact freshness assessment plus its measured source/database work.
struct IndexFreshnessAssessment {
    /// Complete source delta, when current source differs from the index.
    delta: Option<IndexFreshnessDelta>,
    /// Work consumed to establish the assessment.
    work: SourceVerificationWork,
}

/// Current read snapshot established through exact source verification.
pub(crate) struct ExactFreshIndexRead {
    /// Root-bound complete `SQLite` read snapshot.
    pub(crate) store: AtlasStore,
    /// Work consumed before this snapshot could be called current.
    pub(crate) work: SourceVerificationWork,
}

/// Closed reason why current local source could not be verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IndexVerificationReason {
    /// Current scan or ignore policy could not be loaded safely.
    PolicyUnavailable,
    /// A root or source path could not be inspected completely.
    SourceInspectionFailed,
    /// The selected source exceeds the bounded navigation-read ceiling.
    SourceTooLarge,
    /// The opened index does not contain a usable project identity.
    ProjectIdentityUnavailable,
    /// A prior multi-projection publication did not complete.
    PublicationIncomplete,
    /// The completed index used a different parser or scan-policy contract.
    PublicationContractMismatch,
}

/// Typed first-use handoff for one selected project root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IndexInitRequired {
    /// Canonical selected project root that needs initialization.
    pub(crate) project_root: String,
    /// Project-local durable index path that initialization will create.
    pub(crate) database: String,
    /// Stable first-use state for adapters.
    pub(crate) status: IndexReadStatus,
}

/// Typed refusal when a bare/common Git directory is selected as source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ProjectWorktreeRequired {
    /// Canonical bare/common Git directory that was selected.
    pub(crate) project_root: String,
    /// Stable source-selection state for adapters.
    pub(crate) status: IndexReadStatus,
}

/// Bounded typed report returned before a stale indexed read can execute.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IndexRefreshRequired {
    /// Canonical selected project root for a reusable recovery call.
    pub(crate) project_root: String,
    /// Stable freshness state for adapters.
    pub(crate) status: IndexReadStatus,
    /// Why current saved source differs from the index.
    pub(crate) reason: IndexRefreshReason,
    /// Safe recovery scope.
    pub(crate) scope: IndexRefreshScope,
    /// Total added, removed, or modified paths.
    pub(crate) changed: usize,
    /// Newly visible paths.
    pub(crate) added: usize,
    /// Paths no longer visible under current source and ignore policy.
    pub(crate) removed: usize,
    /// Existing paths whose source or structural identity changed.
    pub(crate) modified: usize,
    /// Deterministic bounded path sample for agent recovery.
    pub(crate) sample_paths: Vec<String>,
}

/// Bounded typed report returned when source verification is incomplete.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IndexVerificationIncomplete {
    /// Selected project root whose current source could not be verified.
    pub(crate) project_root: String,
    /// Stable verification state for adapters.
    pub(crate) status: IndexReadStatus,
    /// Why the verification could not complete.
    pub(crate) reason: IndexVerificationReason,
    /// Safe recovery scope once the underlying problem is resolved.
    pub(crate) scope: IndexRefreshScope,
    /// Bounded diagnostic from the failed policy or source inspection.
    pub(crate) message: String,
}

/// Typed refusal when a selected project root and durable index disagree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IndexProjectMismatch {
    /// Stable project binding state for adapters.
    pub(crate) status: IndexReadStatus,
    /// Canonical project root selected for this read.
    pub(crate) selected_project_root: String,
    /// Canonical project root recorded by the opened index.
    pub(crate) indexed_project_root: String,
}

impl fmt::Display for IndexRefreshRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.reason == IndexRefreshReason::PolicyDrift {
            return formatter.write_str(
                "refresh_required: derived index policy differs from the current project configuration; run `projectatlas watch --once` or `atlas_watch_once` before retrying",
            );
        }
        if self.reason == IndexRefreshReason::DependencyClosureLimit {
            return formatter.write_str(
                "refresh_required: the dependency-aware incremental closure exceeded its safe limit; run a complete `projectatlas scan` or `atlas_scan` before retrying",
            );
        }
        write!(
            formatter,
            "refresh_required: {} indexed path(s) differ from current local source; run `projectatlas watch --once` or `atlas_watch_once` before retrying",
            self.changed
        )
    }
}

impl fmt::Display for IndexInitRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "init_required: ProjectAtlas index '{}' is missing for selected project root '{}'; run `projectatlas init` from that exact root or call `atlas_init` with that exact `project_path`",
            self.database, self.project_root
        )
    }
}

impl fmt::Display for ProjectWorktreeRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "worktree_required: '{}' is a bare/common Git directory without checked-out source; select a checked-out worktree and initialize that exact root",
            self.project_root
        )
    }
}

impl fmt::Display for IndexVerificationIncomplete {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "verification_incomplete: current local source could not be verified safely: {}",
            self.message
        )
    }
}

impl fmt::Display for IndexProjectMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "project_mismatch: selected project root '{}' does not match index root '{}'",
            self.selected_project_root, self.indexed_project_root
        )
    }
}

/// Verify current saved local source against the durable index in focused tests.
#[cfg(test)]
fn verify_index_freshness(
    store: &AtlasStore,
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(), CliError> {
    let plan = ScanRuntimePlan::for_path(config_path, root, None).map_err(|source| {
        verification_incomplete(root, IndexVerificationReason::PolicyUnavailable, &source)
    })?;
    match detect_index_freshness(store, &plan)? {
        Some(delta) => Err(CliError::RefreshRequired(Box::new(delta.report))),
        None => Ok(()),
    }
}

/// Open a current read snapshot, reconciling one safe bounded delta when possible.
pub(crate) fn open_fresh_atlas_store_for_project(
    db_path: &Path,
    root: &Path,
    config_path: Option<&Path>,
) -> Result<AtlasStore, CliError> {
    let control = standalone_index_work_control();
    open_fresh_atlas_store_for_project_controlled(db_path, root, config_path, &control)
}

/// Open a current read snapshot under one cooperative freshness boundary.
pub(crate) fn open_fresh_atlas_store_for_project_controlled(
    db_path: &Path,
    root: &Path,
    config_path: Option<&Path>,
    control: &IndexWorkControl,
) -> Result<AtlasStore, CliError> {
    Ok(
        open_exact_fresh_atlas_store_for_project_controlled(db_path, root, config_path, control)?
            .store,
    )
}

/// Open a current snapshot and retain exact freshness work for epoch accounting.
pub(crate) fn open_exact_fresh_atlas_store_for_project_controlled(
    db_path: &Path,
    root: &Path,
    config_path: Option<&Path>,
    control: &IndexWorkControl,
) -> Result<ExactFreshIndexRead, CliError> {
    open_exact_fresh_atlas_store_for_project_with_repair(
        db_path,
        root,
        config_path,
        control,
        true,
        ScanLimits::default(),
    )
}

/// Open a current read snapshot without repairing stale source or durable state.
fn open_exact_fresh_atlas_store_for_project_with_repair(
    db_path: &Path,
    root: &Path,
    config_path: Option<&Path>,
    control: &IndexWorkControl,
    repair_safe_delta: bool,
    scan_limits: ScanLimits,
) -> Result<ExactFreshIndexRead, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    let store = open_atlas_store_read_only_for_project(db_path, root)?;
    let plan = ScanRuntimePlan::for_path_controlled(config_path, root, None, control)
        .map_err(|source| publication_input_error(root, source))?;
    let assessment = match detect_index_freshness_controlled(&store, &plan, scan_limits, control) {
        Ok(assessment) => assessment,
        Err(CliError::VerificationIncomplete(report))
            if report.reason == IndexVerificationReason::PublicationContractMismatch =>
        {
            return Err(CliError::RefreshRequired(Box::new(
                index_policy_refresh_required(&plan.root),
            )));
        }
        Err(error) => return Err(error),
    };
    let mut work = assessment.work;
    let Some(delta) = assessment.delta else {
        return Ok(ExactFreshIndexRead { store, work });
    };
    if !repair_safe_delta {
        return Err(CliError::RefreshRequired(Box::new(delta.report)));
    }
    if delta.report.scope != IndexRefreshScope::Incremental {
        return Err(CliError::RefreshRequired(Box::new(delta.report)));
    }

    let refresh_required = delta.report.clone();
    drop(store);
    let repair = (|| {
        let mut writer = open_atlas_store_for_project(db_path, &plan.root)?;
        let changes = WatchChangeSet {
            requires_full_scan: false,
            paths: delta.paths,
        };
        refresh_index_for_changes_controlled(
            &mut writer,
            &plan,
            &changes,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            control,
        )
    })();
    if let Err(error) = repair {
        if automatic_refresh_write_is_unavailable(&error) {
            return Err(CliError::RefreshRequired(Box::new(refresh_required)));
        }
        return Err(error);
    }

    let store = open_atlas_store_read_only_for_project(db_path, &plan.root)?;
    verify_index_project_root(&store, &plan.root)?;
    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
    verify_index_publication(&store, &plan)?;
    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
    Ok(ExactFreshIndexRead { store, work })
}

/// Open an explicit ordered set of current project indexes without mutating any root.
pub(crate) fn open_federated_atlas_stores_for_project(
    selected_db: &Path,
    selected_root: &Path,
    selected_config: Option<&Path>,
    roots: &[PathBuf],
    control: &IndexWorkControl,
) -> Result<Vec<FederatedStore>, CliError> {
    validate_federated_root_count(roots.len()).map_err(CliError::Service)?;
    let selected_root = fs::canonicalize(selected_root).map_err(|source| CliError::Io {
        path: selected_root.to_path_buf(),
        source,
    })?;
    let mut canonical_roots = Vec::with_capacity(roots.len());
    for root in roots {
        let root = fs::canonicalize(root).map_err(|source| CliError::Io {
            path: root.clone(),
            source,
        })?;
        if canonical_roots.contains(&root) {
            return Err(CliError::Service(
                projectatlas_service::ServiceError::InvalidInput(
                    "federated roots must be unique".to_string(),
                ),
            ));
        }
        canonical_roots.push(root);
    }
    if canonical_roots.first() != Some(&selected_root) {
        return Err(CliError::Service(
            projectatlas_service::ServiceError::InvalidInput(
                "the first federated root must be the selected project root".to_string(),
            ),
        ));
    }

    let databases = canonical_roots
        .iter()
        .enumerate()
        .map(|(order, root)| {
            if order == 0 {
                selected_db.to_path_buf()
            } else {
                root.join(".projectatlas").join("projectatlas.db")
            }
        })
        .collect::<Vec<_>>();
    let mut database_bytes = 0_u64;
    for database in &databases {
        let metadata = fs::metadata(database).map_err(|source| CliError::Io {
            path: database.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(CliError::Service(
                projectatlas_service::ServiceError::InvalidInput(
                    "federated database path is not a regular file".to_string(),
                ),
            ));
        }
        database_bytes = database_bytes.checked_add(metadata.len()).ok_or_else(|| {
            CliError::Service(projectatlas_service::ServiceError::InvalidInput(
                "participating database byte count overflowed".to_string(),
            ))
        })?;
        if database_bytes > MAX_FEDERATED_DATABASE_BYTES {
            return Err(CliError::Service(
                projectatlas_service::ServiceError::InvalidInput(format!(
                    "participating databases exceed {MAX_FEDERATED_DATABASE_BYTES} bytes"
                )),
            ));
        }
    }

    let default_scan_limits = ScanLimits::default();
    let mut remaining_input_bytes = MAX_FEDERATED_INPUT_BYTES;
    let mut stores: Vec<FederatedStore> = Vec::with_capacity(canonical_roots.len());
    for (order, (root, database)) in canonical_roots.into_iter().zip(databases).enumerate() {
        let started = Instant::now();
        let config = (order == 0).then_some(selected_config).flatten();
        let exact = match open_exact_fresh_atlas_store_for_project_with_repair(
            &database,
            &root,
            config,
            control,
            false,
            ScanLimits::new(
                default_scan_limits.max_entries(),
                remaining_input_bytes,
                default_scan_limits.max_workers(),
            ),
        ) {
            Ok(exact) => exact,
            Err(error) => {
                for store in stores {
                    drop(store.finish());
                }
                return Err(error);
            }
        };
        remaining_input_bytes = remaining_input_bytes.saturating_sub(exact.work.filesystem_bytes);
        let input_work = FederatedInputWork {
            filesystem_entries: exact.work.filesystem_entries,
            filesystem_bytes: exact.work.filesystem_bytes,
            sqlite_read_statements: exact.work.sqlite_read_statements,
            decoded_nodes: exact.work.decoded_nodes,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        };
        match FederatedStore::new(exact.store, database, root, input_work) {
            Ok(store) => stores.push(store),
            Err(error) => {
                for store in stores {
                    drop(store.finish());
                }
                return Err(CliError::Service(error));
            }
        }
    }
    Ok(stores)
}

/// Distinguish optional repair contention from database-integrity failures.
fn automatic_refresh_write_is_unavailable(error: &CliError) -> bool {
    matches!(error, CliError::Db(source) if source.is_write_unavailable())
}

/// Return the typed full-refresh state for a changed derivation contract.
fn index_policy_refresh_required(root: &Path) -> IndexRefreshRequired {
    IndexRefreshRequired {
        project_root: normalize_native_path_display(root),
        status: IndexReadStatus::RefreshRequired,
        reason: IndexRefreshReason::PolicyDrift,
        scope: IndexRefreshScope::Full,
        changed: 0,
        added: 0,
        removed: 0,
        modified: 0,
        sample_paths: Vec::new(),
    }
}

/// Detect the complete current local-source delta for one selected index.
#[cfg(test)]
fn detect_index_freshness(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
) -> Result<Option<IndexFreshnessDelta>, CliError> {
    let control = standalone_index_work_control();
    Ok(detect_index_freshness_controlled(store, plan, ScanLimits::default(), &control)?.delta)
}

/// Detect the complete local-source delta under one cooperative work boundary.
fn detect_index_freshness_controlled(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
    scan_limits: ScanLimits,
    control: &IndexWorkControl,
) -> Result<IndexFreshnessAssessment, CliError> {
    let mut work = SourceVerificationWork::default();
    verify_index_project_root(store, &plan.root)?;
    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
    verify_index_publication(store, plan)?;
    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
    let scan = scan_repo_controlled_with_work(&plan.root, &plan.scan_options, scan_limits, control)
        .map_err(|source| source_inspection_error(&plan.root, source))?;
    work.filesystem_entries = scan.work.entries;
    work.filesystem_bytes = scan.work.source_bytes;
    let current_nodes = scan.nodes;
    let indexed_nodes = store
        .load_nodes()?
        .into_iter()
        .map(|indexed| indexed.node)
        .collect::<Vec<_>>();
    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
    work.decoded_nodes = u64::try_from(indexed_nodes.len()).unwrap_or(u64::MAX);
    Ok(IndexFreshnessAssessment {
        delta: source_node_delta(&plan.root, &current_nodes, &indexed_nodes),
        work,
    })
}

/// Compare a current exact scan with the source-derived nodes being validated.
fn verify_source_nodes_match(
    root: &Path,
    current_nodes: &[Node],
    indexed_nodes: &[Node],
) -> Result<(), CliError> {
    match source_node_delta(root, current_nodes, indexed_nodes) {
        Some(delta) => Err(CliError::RefreshRequired(Box::new(delta.report))),
        None => Ok(()),
    }
}

/// Build one deterministic affected-path plan from current and indexed nodes.
fn source_node_delta(
    root: &Path,
    current_nodes: &[Node],
    indexed_nodes: &[Node],
) -> Option<IndexFreshnessDelta> {
    let current_by_path = current_nodes
        .iter()
        .map(|node| (node.path.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let indexed_by_path = indexed_nodes
        .iter()
        .map(|node| (node.path.as_str(), node))
        .collect::<BTreeMap<_, _>>();

    let added_paths = current_by_path
        .keys()
        .filter(|path| !indexed_by_path.contains_key(**path))
        .copied()
        .collect::<Vec<_>>();
    let removed_paths = indexed_by_path
        .keys()
        .filter(|path| !current_by_path.contains_key(**path))
        .copied()
        .collect::<Vec<_>>();
    let mut modified_paths = Vec::new();
    for (path, current) in &current_by_path {
        let Some(indexed) = indexed_by_path.get(path) else {
            continue;
        };
        if !same_indexed_source(current, indexed) {
            modified_paths.push(*path);
        }
    }
    let changed = added_paths
        .len()
        .saturating_add(removed_paths.len())
        .saturating_add(modified_paths.len());
    if changed == 0 {
        return None;
    }

    let changed_paths = added_paths
        .iter()
        .chain(&removed_paths)
        .chain(&modified_paths)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let changed_bytes = added_paths
        .iter()
        .chain(&modified_paths)
        .filter_map(|path| current_by_path.get(path).and_then(|node| node.size_bytes))
        .fold(0_u64, u64::saturating_add);
    let requires_full_scan = changed > NORMAL_READ_REFRESH_MAX_PATHS
        || changed_bytes > NORMAL_READ_REFRESH_MAX_BYTES
        || changed_paths
            .iter()
            .any(|path| watch_path_requires_full_scan(root, &root.join(repo_path_to_native(path))));
    let sample_paths = changed_paths
        .iter()
        .take(INDEX_FRESHNESS_SAMPLE_LIMIT)
        .map(|path| (*path).to_string())
        .collect();
    Some(IndexFreshnessDelta {
        report: IndexRefreshRequired {
            project_root: normalize_native_path_display(root),
            status: IndexReadStatus::RefreshRequired,
            reason: if added_paths.is_empty() && removed_paths.is_empty() {
                IndexRefreshReason::SourceChanged
            } else {
                IndexRefreshReason::PathsChanged
            },
            scope: if requires_full_scan {
                IndexRefreshScope::Full
            } else {
                IndexRefreshScope::Incremental
            },
            changed,
            added: added_paths.len(),
            removed: removed_paths.len(),
            modified: modified_paths.len(),
            sample_paths,
        },
        paths: changed_paths
            .into_iter()
            .map(|path| root.join(repo_path_to_native(path)))
            .collect(),
    })
}

/// Verify that the opened database belongs to the selected canonical root.
fn verify_index_project_root(store: &AtlasStore, selected_root: &Path) -> Result<(), CliError> {
    let Some(indexed_root) = store.project_root()? else {
        return Err(verification_incomplete(
            selected_root,
            IndexVerificationReason::ProjectIdentityUnavailable,
            &CliError::InvalidInput("index project root metadata is missing".to_string()),
        ));
    };
    let indexed_root_path = PathBuf::from(&indexed_root);
    let indexed_root = canonical_project_root(&indexed_root_path).map_err(|source| {
        verification_incomplete(
            selected_root,
            IndexVerificationReason::ProjectIdentityUnavailable,
            &source,
        )
    })?;
    let selected_root = canonical_project_root(selected_root).map_err(|source| {
        verification_incomplete(
            selected_root,
            IndexVerificationReason::SourceInspectionFailed,
            &source,
        )
    })?;
    if indexed_root != selected_root {
        return Err(CliError::ProjectMismatch(Box::new(IndexProjectMismatch {
            status: IndexReadStatus::ProjectMismatch,
            selected_project_root: normalize_native_path_display(selected_root),
            indexed_project_root: normalize_native_path_display(indexed_root),
        })));
    }
    Ok(())
}

/// Reject mixed or runtime-incompatible derived projections before source reads.
fn verify_index_publication(store: &AtlasStore, plan: &ScanRuntimePlan) -> Result<(), CliError> {
    let expected_fingerprint = plan.publication_contract_fingerprint();
    let Some(publication) = store.index_publication()? else {
        return Err(verification_incomplete(
            &plan.root,
            IndexVerificationReason::PublicationIncomplete,
            &CliError::InvalidInput(
                "derived index publication state is missing; run one refresh".to_string(),
            ),
        ));
    };
    if publication.state == IndexPublicationState::Updating {
        return Err(verification_incomplete(
            &plan.root,
            IndexVerificationReason::PublicationIncomplete,
            &CliError::InvalidInput(
                "a prior derived index publication did not complete; run one refresh".to_string(),
            ),
        ));
    }
    if publication.generation == projectatlas_core::IndexGeneration::ZERO {
        return Err(verification_incomplete(
            &plan.root,
            IndexVerificationReason::PublicationIncomplete,
            &CliError::InvalidInput(
                "derived index has no complete publication generation; run one refresh".to_string(),
            ),
        ));
    }
    if publication.contract_fingerprint.as_deref() != Some(expected_fingerprint.as_str()) {
        return Err(verification_incomplete(
            &plan.root,
            IndexVerificationReason::PublicationContractMismatch,
            &CliError::InvalidInput(
                "derived index parser or scan-policy contract changed; run one refresh".to_string(),
            ),
        ));
    }
    Ok(())
}

/// Return whether symbol reuse and incremental publication share the current derivation contract.
fn publication_contract_matches(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
) -> Result<bool, CliError> {
    let Some(publication) = store.index_publication()? else {
        return Ok(false);
    };
    Ok(publication.state == IndexPublicationState::Complete
        && publication.generation != IndexGeneration::ZERO
        && publication.contract_fingerprint.as_deref()
            == Some(plan.publication_contract_fingerprint().as_str()))
}

/// Hash the parser registry and source-selection policy that own derived rows.
fn index_derivation_fingerprint(
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
    #[cfg(feature = "optional-parser-supervisor")]
    optional_parser_selection: &OptionalParserPackProjectSelection,
) -> String {
    index_derivation_fingerprint_with_semantic_digest(
        scan_options,
        text_options,
        #[cfg(feature = "optional-parser-supervisor")]
        optional_parser_selection,
        &semantic_resolution_contract_digest(),
    )
}

/// Hash one exact parser, semantic, and source-selection contract.
fn index_derivation_fingerprint_with_semantic_digest(
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
    #[cfg(feature = "optional-parser-supervisor")]
    optional_parser_selection: &OptionalParserPackProjectSelection,
    semantic_resolution_digest: &str,
) -> String {
    let mut hasher = Hasher::new();
    hash_index_contract_value(
        &mut hasher,
        "contract_version",
        INDEX_DERIVATION_CONTRACT_VERSION,
    );
    hash_index_contract_value(
        &mut hasher,
        "language_registry_version",
        &LANGUAGE_CAPABILITY_REGISTRY_VERSION.to_string(),
    );
    hash_index_contract_value(
        &mut hasher,
        "accepted_language_set_version",
        &ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION.to_string(),
    );
    hash_index_contract_value(
        &mut hasher,
        "language_registry_digest",
        &language_registry_digest(),
    );
    hash_index_contract_value(
        &mut hasher,
        "accepted_language_set_digest",
        &accepted_language_capability_digest(),
    );
    hash_index_contract_value(
        &mut hasher,
        "semantic_resolution_contract_digest",
        semantic_resolution_digest,
    );
    for value in &scan_options.exclude_dir_names {
        hash_index_contract_value(&mut hasher, "exclude_dir_name", value);
    }
    for value in &scan_options.exclude_dir_suffixes {
        hash_index_contract_value(&mut hasher, "exclude_dir_suffix", value);
    }
    for value in &scan_options.exclude_path_prefixes {
        hash_index_contract_value(&mut hasher, "exclude_path_prefix", value);
    }
    for (selector, language) in &scan_options.language_overrides {
        hash_index_contract_value(&mut hasher, "language_override_selector", selector);
        hash_index_contract_value(&mut hasher, "language_override_target", language);
    }
    hash_index_contract_value(
        &mut hasher,
        "text_index_max_bytes",
        &text_options.max_bytes.to_string(),
    );
    #[cfg(feature = "optional-parser-supervisor")]
    hash_index_contract_value(
        &mut hasher,
        "optional_parser_selection",
        optional_parser_selection
            .selection_key()
            .map_or("inactive", |selection| selection.as_str()),
    );
    hasher.finalize().to_hex().to_string()
}

/// Recheck current policy and source before making staged rows visible.
#[cfg(test)]
fn revalidate_index_publication_inputs_controlled(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
    expected_purpose_import_fingerprint: Option<&str>,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    revalidate_index_publication_inputs_controlled_with_limits(
        store,
        plan,
        expected_purpose_import_fingerprint,
        control,
        PurposeImportLimits::default(),
    )
}

/// Recheck publication inputs under explicit purpose limits used by focused tests.
#[cfg(test)]
fn revalidate_index_publication_inputs_controlled_with_limits(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
    expected_purpose_import_fingerprint: Option<&str>,
    control: &IndexWorkControl,
    purpose_limits: PurposeImportLimits,
) -> Result<(), CliError> {
    let staged_nodes = store
        .load_nodes()?
        .into_iter()
        .map(|indexed| indexed.node)
        .collect::<Vec<_>>();
    revalidate_staged_publication_inputs_controlled_with_limits(
        plan,
        &staged_nodes,
        expected_purpose_import_fingerprint,
        None,
        control,
        purpose_limits,
    )
}

/// Recheck policy and exact source against one off-writer publication batch.
fn revalidate_staged_publication_inputs_controlled(
    plan: &ScanRuntimePlan,
    staged_nodes: &[Node],
    expected_purpose_import_fingerprint: Option<&str>,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    revalidate_staged_publication_inputs_controlled_with_limits(
        plan,
        staged_nodes,
        expected_purpose_import_fingerprint,
        None,
        control,
        PurposeImportLimits::default(),
    )
}

/// Recheck a full scan while reusing purpose rows from exact unchanged source nodes.
fn revalidate_staged_publication_inputs_with_purpose_snapshot(
    plan: &ScanRuntimePlan,
    staged_nodes: &[Node],
    purpose_import: Option<&PurposeImportSnapshot>,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    revalidate_staged_publication_inputs_controlled_with_limits(
        plan,
        staged_nodes,
        purpose_import.map(|snapshot| snapshot.fingerprint.as_str()),
        purpose_import.map(|snapshot| snapshot.records.as_slice()),
        control,
        PurposeImportLimits::default(),
    )
}

/// Recheck a staged batch under explicit purpose-input limits used by tests.
fn revalidate_staged_publication_inputs_controlled_with_limits(
    plan: &ScanRuntimePlan,
    staged_nodes: &[Node],
    expected_purpose_import_fingerprint: Option<&str>,
    reusable_purpose_records: Option<&[atlas_map::ImportedPurposeRecord]>,
    control: &IndexWorkControl,
    purpose_limits: PurposeImportLimits,
) -> Result<(), CliError> {
    control.check(IndexWorkStage::Publication)?;
    let current_plan = plan
        .reload_controlled_with_limits(control, purpose_limits)
        .map_err(|source| publication_input_error(&plan.root, source))?;
    control.check(IndexWorkStage::Publication)?;
    let staged_fingerprint = plan.publication_contract_fingerprint();
    let current_fingerprint = current_plan.publication_contract_fingerprint();
    if staged_fingerprint != current_fingerprint {
        return Err(verification_incomplete(
            &plan.root,
            IndexVerificationReason::PublicationContractMismatch,
            &CliError::InvalidInput(
                "derived index policy changed while publication was being built; retry the refresh"
                    .to_string(),
            ),
        ));
    }
    let current_nodes = scan_repo_controlled(
        &current_plan.root,
        &current_plan.scan_options,
        ScanLimits::default(),
        control,
    )
    .map_err(|source| source_inspection_error(&current_plan.root, source))?;
    verify_source_nodes_match(&current_plan.root, &current_nodes, staged_nodes)?;
    if let Some(expected_fingerprint) = expected_purpose_import_fingerprint {
        let current_fingerprint = if let Some(records) = reusable_purpose_records {
            current_plan
                .purpose_import_fingerprint_for_records_controlled_with_limits(
                    records,
                    control,
                    purpose_limits,
                )
                .map_err(|source| publication_input_error(&plan.root, source))?
        } else {
            current_plan
                .purpose_import_snapshot_controlled_with_limits(
                    &current_nodes,
                    control,
                    purpose_limits,
                )
                .map_err(|source| publication_input_error(&plan.root, source))?
                .fingerprint
        };
        if current_fingerprint != expected_fingerprint {
            return Err(verification_incomplete(
                &plan.root,
                IndexVerificationReason::PublicationContractMismatch,
                &CliError::InvalidInput(
                    "purpose-import inputs changed while publication was being built; retry the refresh"
                        .to_string(),
                ),
            ));
        }
    }
    control.check(IndexWorkStage::Publication)?;
    Ok(())
}

/// Preserve typed work failures while adapting authored-input uncertainty.
fn publication_input_error(root: &Path, source: CliError) -> CliError {
    match source {
        source @ CliError::IndexWork(_) => source,
        other => verification_incomplete(root, IndexVerificationReason::PolicyUnavailable, &other),
    }
}

/// Preserve typed work failures while adapting ordinary scan uncertainty.
fn source_inspection_error(root: &Path, source: FsError) -> CliError {
    match source {
        FsError::IndexWork(failure) => failure.into(),
        FsError::RepositoryBoundary { .. } => {
            CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
                project_root: normalize_native_path_display(root),
                status: IndexReadStatus::VerificationIncomplete,
                reason: IndexVerificationReason::PolicyUnavailable,
                scope: IndexRefreshScope::Full,
                message: source.to_string(),
            }))
        }
        other => CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
            project_root: normalize_native_path_display(root),
            status: IndexReadStatus::VerificationIncomplete,
            reason: IndexVerificationReason::SourceInspectionFailed,
            scope: IndexRefreshScope::Full,
            message: other.to_string(),
        })),
    }
}

/// Commit only after the shared work boundary still permits publication.
fn complete_index_publication(
    publication: IndexPublicationGuard<'_>,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    control.check(IndexWorkStage::Publication)?;
    publication.complete()?;
    Ok(())
}

/// Add one unambiguous field/value pair to a derived-index fingerprint.
fn hash_index_contract_value(hasher: &mut Hasher, field: &str, value: &str) {
    hasher.update(field.as_bytes());
    hasher.update(&[0]);
    hasher.update(value.as_bytes());
    hasher.update(&[0xff]);
}

/// Convert a policy/root preflight failure into a non-destructive read refusal.
fn verification_incomplete(
    root: &Path,
    reason: IndexVerificationReason,
    source: &CliError,
) -> CliError {
    CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
        project_root: normalize_native_path_display(root),
        status: IndexReadStatus::VerificationIncomplete,
        reason,
        scope: IndexRefreshScope::Full,
        message: source.to_string(),
    }))
}

/// Compare source-derived node identity while ignoring non-semantic mtimes.
fn same_indexed_source(current: &Node, indexed: &Node) -> bool {
    current.path == indexed.path
        && current.kind == indexed.kind
        && current.parent_path == indexed.parent_path
        && current.extension == indexed.extension
        && current.language == indexed.language
        && current.size_bytes == indexed.size_bytes
        && current.content_hash == indexed.content_hash
}

/// Refuse publication when source bytes no longer match their staged node.
fn source_changed_during_derivation(root: &Path, path: &str) -> CliError {
    CliError::RefreshRequired(Box::new(IndexRefreshRequired {
        project_root: normalize_native_path_display(root),
        status: IndexReadStatus::RefreshRequired,
        reason: IndexRefreshReason::SourceChanged,
        scope: IndexRefreshScope::Full,
        changed: 1,
        added: 0,
        removed: 0,
        modified: 1,
        sample_paths: vec![path.to_string()],
    }))
}

impl<'a> PurposeInputReader<'a> {
    /// Create one reader whose cancellation and limits belong to the scan operation.
    fn new(
        plan: &ScanRuntimePlan,
        control: &'a IndexWorkControl,
        limits: PurposeImportLimits,
    ) -> Self {
        let mut complete_paths = BTreeSet::new();
        if let Some(path) = &plan.selected_config_path {
            complete_paths.insert(path.clone());
        }
        if let Some(config) = &plan.config {
            complete_paths.insert(config.map_path.clone());
            complete_paths.insert(config.nonsource_files_path.clone());
        }
        Self::for_complete_paths(
            control,
            limits,
            complete_paths,
            plan.config.as_ref().map_or_else(
                || ".purpose".to_string(),
                |config| config.purpose_filename().to_string(),
            ),
        )
    }

    /// Create a bounded reader before a complete runtime plan is available.
    fn for_complete_paths(
        control: &'a IndexWorkControl,
        limits: PurposeImportLimits,
        complete_paths: BTreeSet<PathBuf>,
        purpose_filename: String,
    ) -> Self {
        Self {
            control,
            limits,
            complete_paths,
            purpose_filename,
            complete_digests: BTreeMap::new(),
        }
    }

    /// Read one UTF-8 input, treating non-UTF-8 source headers as purpose-free.
    fn read_text(&mut self, path: &Path) -> Result<String, CliError> {
        self.control.check(IndexWorkStage::Publication)?;
        let is_complete = self.complete_paths.contains(path)
            || path
                .file_name()
                .is_some_and(|name| name == self.purpose_filename.as_str());
        let file_limit = if is_complete {
            self.limits.complete_file_bytes
        } else {
            self.limits.header_bytes
        };
        let mut file = fs::File::open(path).map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let bytes = self.read_bytes(path, &mut file, file_limit, is_complete)?;
        if is_complete {
            self.complete_digests.insert(
                path.to_path_buf(),
                blake3::hash(&bytes).to_hex().to_string(),
            );
        }
        match String::from_utf8(bytes) {
            Ok(content) => Ok(content),
            Err(source) if !is_complete && source.utf8_error().error_len().is_some() => {
                Ok(String::new())
            }
            Err(source) if !is_complete && source.utf8_error().error_len().is_none() => {
                let valid_up_to = source.utf8_error().valid_up_to();
                String::from_utf8(source.into_bytes()[..valid_up_to].to_vec()).map_err(|source| {
                    CliError::InvalidInput(format!(
                        "purpose header input is not valid UTF-8 for {}: {source}",
                        normalize_native_path_display(path)
                    ))
                })
            }
            Err(source) => Err(CliError::InvalidInput(format!(
                "purpose input is not valid UTF-8 for {}: {source}",
                normalize_native_path_display(path)
            ))),
        }
    }

    /// Read bytes with inter-chunk cancellation and aggregate accounting.
    fn read_bytes<R: Read>(
        &mut self,
        path: &Path,
        reader: &mut R,
        file_limit: u64,
        require_complete: bool,
    ) -> Result<Vec<u8>, CliError> {
        let initial_capacity = usize::try_from(file_limit)
            .unwrap_or(usize::MAX)
            .min(CONTROLLED_SOURCE_READ_BUFFER_BYTES);
        let mut bytes = Vec::with_capacity(initial_capacity);
        let mut buffer = [0_u8; CONTROLLED_SOURCE_READ_BUFFER_BYTES];
        loop {
            self.control.check(IndexWorkStage::Publication)?;
            let file_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let remaining = file_limit.saturating_sub(file_bytes);
            if remaining == 0 {
                if !require_complete {
                    break;
                }
                let read = reader
                    .read(&mut buffer[..1])
                    .map_err(|source| CliError::Io {
                        path: path.to_path_buf(),
                        source,
                    })?;
                if read == 0 {
                    break;
                }
                return Err(IndexWorkFailure::resource_limit(
                    IndexWorkStage::Publication,
                    IndexWorkResource::PurposeBytes,
                    file_limit,
                    file_limit.saturating_add(1),
                )
                .into());
            }
            let read_limit = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(buffer.len());
            let read = reader
                .read(&mut buffer[..read_limit])
                .map_err(|source| CliError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
            if read == 0 {
                break;
            }
            self.control.consume_purpose_bytes(
                self.limits.total_bytes,
                u64::try_from(read).unwrap_or(u64::MAX),
            )?;
            bytes.extend_from_slice(&buffer[..read]);
        }
        self.control.check(IndexWorkStage::Publication)?;
        Ok(bytes)
    }

    /// Return the digest of one complete input already read through this boundary.
    fn complete_digest(&self, path: &Path) -> Option<&str> {
        self.complete_digests.get(path).map(String::as_str)
    }
}

impl ScanRuntimePlan {
    /// Resolve scan policy for one project path.
    pub(crate) fn for_path(
        config_path: Option<&Path>,
        path: &Path,
        text_index_max_bytes: Option<u64>,
    ) -> Result<Self, CliError> {
        let control = standalone_index_work_control();
        Self::for_path_controlled(config_path, path, text_index_max_bytes, &control)
    }

    /// Resolve scan policy through the operation-owned bounded config reader.
    pub(crate) fn for_path_controlled(
        config_path: Option<&Path>,
        path: &Path,
        text_index_max_bytes: Option<u64>,
        control: &IndexWorkControl,
    ) -> Result<Self, CliError> {
        Self::for_path_controlled_with_limits(
            config_path,
            path,
            text_index_max_bytes,
            control,
            PurposeImportLimits::default(),
        )
    }

    /// Resolve scan policy under explicit authored-input limits used by focused tests.
    fn for_path_controlled_with_limits(
        config_path: Option<&Path>,
        path: &Path,
        text_index_max_bytes: Option<u64>,
        control: &IndexWorkControl,
        purpose_limits: PurposeImportLimits,
    ) -> Result<Self, CliError> {
        control.check(IndexWorkStage::Publication)?;
        let root = canonical_source_project_root(path)?;
        let selected_config_path = selected_scan_import_config_path(config_path, &root)?;
        let config = if let Some(path) = selected_config_path.as_deref() {
            let mut complete_paths = BTreeSet::new();
            complete_paths.insert(path.to_path_buf());
            let mut reader = PurposeInputReader::for_complete_paths(
                control,
                purpose_limits,
                complete_paths,
                ".purpose".to_string(),
            );
            let text = reader.read_text(path)?;
            let config = load_atlas_config_from_text(path, &text)?;
            let config_root = canonical_project_root(&config.root)?;
            if config_root != root {
                return Err(config_root_mismatch_error(path, &config_root, &root));
            }
            Some(config)
        } else {
            None
        };
        control.check(IndexWorkStage::Publication)?;
        let scan_options = config.as_ref().map_or_else(
            ScanOptions::default,
            atlas_map::AtlasMapConfig::scan_options,
        );
        let text_options = text_index_options(config.as_ref(), text_index_max_bytes);
        #[cfg(feature = "optional-parser-supervisor")]
        let optional_parser_selection =
            OptionalParserPackLifecycle::new(&root, None)?.derive_project_selection()?;
        #[cfg(feature = "optional-parser-supervisor")]
        let scan_options = {
            let mut scan_options = scan_options;
            scan_options.admit_optional_languages =
                optional_parser_selection.selection_key().is_some();
            scan_options
        };
        Ok(Self {
            root,
            config,
            selected_config_path,
            config_path_override: config_path.map(Path::to_path_buf),
            scan_options,
            text_options,
            text_index_max_bytes_override: text_index_max_bytes,
            #[cfg(feature = "optional-parser-supervisor")]
            optional_parser_selection,
        })
    }

    /// Reload the effective filesystem and text policy from current local state.
    fn reload(&self) -> Result<Self, CliError> {
        Self::for_path(
            self.config_path_override.as_deref(),
            &self.root,
            self.text_index_max_bytes_override,
        )
    }

    /// Reload effective policy through the operation-owned bounded input reader.
    fn reload_controlled(&self, control: &IndexWorkControl) -> Result<Self, CliError> {
        self.reload_controlled_with_limits(control, PurposeImportLimits::default())
    }

    /// Reload effective policy under explicit authored-input limits used by focused tests.
    fn reload_controlled_with_limits(
        &self,
        control: &IndexWorkControl,
        purpose_limits: PurposeImportLimits,
    ) -> Result<Self, CliError> {
        Self::for_path_controlled_with_limits(
            self.config_path_override.as_deref(),
            &self.root,
            self.text_index_max_bytes_override,
            control,
            purpose_limits,
        )
    }

    /// Hash the durable parser and configured source/index policy contract.
    ///
    /// Request-scoped text limits control one scan or watcher operation but do
    /// not become project compatibility state that later reads must repeat.
    fn publication_contract_fingerprint(&self) -> String {
        let configured_text_options = text_index_options(self.config.as_ref(), None);
        index_derivation_fingerprint(
            &self.scan_options,
            configured_text_options,
            #[cfg(feature = "optional-parser-supervisor")]
            &self.optional_parser_selection,
        )
    }

    /// Capture every purpose input from one existing controlled repository scan.
    fn purpose_import_snapshot_controlled(
        &self,
        nodes: &[Node],
        control: &IndexWorkControl,
    ) -> Result<PurposeImportSnapshot, CliError> {
        self.purpose_import_snapshot_controlled_with_limits(
            nodes,
            control,
            PurposeImportLimits::default(),
        )
    }

    /// Capture purpose inputs under explicit limits used by focused tests.
    fn purpose_import_snapshot_controlled_with_limits(
        &self,
        nodes: &[Node],
        control: &IndexWorkControl,
        limits: PurposeImportLimits,
    ) -> Result<PurposeImportSnapshot, CliError> {
        control.check(IndexWorkStage::Publication)?;
        let mut hasher = Hasher::new();
        hash_index_contract_value(&mut hasher, "purpose_import_version", "2");
        let Some(config) = self.config.as_ref() else {
            hash_index_contract_value(&mut hasher, "selected_config", "absent");
            return Ok(PurposeImportSnapshot {
                records: Vec::new(),
                fingerprint: hasher.finalize().to_hex().to_string(),
            });
        };
        let selected_config_path = self.selected_config_path.as_deref().ok_or_else(|| {
            CliError::InvalidInput(
                "purpose import has normalized configuration without a selected config path"
                    .to_string(),
            )
        })?;
        if u64::try_from(nodes.len()).unwrap_or(u64::MAX) > limits.records {
            return Err(IndexWorkFailure::resource_limit(
                IndexWorkStage::Publication,
                IndexWorkResource::PurposeRecords,
                limits.records,
                u64::try_from(nodes.len()).unwrap_or(u64::MAX),
            )
            .into());
        }
        let mut reader = PurposeInputReader::new(self, control, limits);
        hash_publication_input_file_controlled(
            &mut hasher,
            "selected_config",
            selected_config_path,
            &mut reader,
        )?;
        let records = atlas_map::imported_purpose_records_from_nodes(config, nodes, &mut |path| {
            reader.read_text(path)
        })?;
        let fingerprint = finish_purpose_import_fingerprint_controlled(
            &mut hasher,
            config,
            &records,
            &mut reader,
            control,
            limits,
        )?;
        Ok(PurposeImportSnapshot {
            records,
            fingerprint,
        })
    }

    /// Recheck external purpose inputs while reusing records from exact unchanged source nodes.
    fn purpose_import_fingerprint_for_records_controlled_with_limits(
        &self,
        records: &[atlas_map::ImportedPurposeRecord],
        control: &IndexWorkControl,
        limits: PurposeImportLimits,
    ) -> Result<String, CliError> {
        control.check(IndexWorkStage::Publication)?;
        let mut hasher = Hasher::new();
        hash_index_contract_value(&mut hasher, "purpose_import_version", "2");
        let Some(config) = self.config.as_ref() else {
            hash_index_contract_value(&mut hasher, "selected_config", "absent");
            return Ok(hasher.finalize().to_hex().to_string());
        };
        let selected_config_path = self.selected_config_path.as_deref().ok_or_else(|| {
            CliError::InvalidInput(
                "purpose import has normalized configuration without a selected config path"
                    .to_string(),
            )
        })?;
        let mut reader = PurposeInputReader::new(self, control, limits);
        hash_publication_input_file_controlled(
            &mut hasher,
            "selected_config",
            selected_config_path,
            &mut reader,
        )?;
        finish_purpose_import_fingerprint_controlled(
            &mut hasher,
            config,
            records,
            &mut reader,
            control,
            limits,
        )
    }
}

/// Finish one purpose-import fingerprint from already normalized source records.
fn finish_purpose_import_fingerprint_controlled(
    hasher: &mut Hasher,
    config: &atlas_map::AtlasMapConfig,
    records: &[atlas_map::ImportedPurposeRecord],
    reader: &mut PurposeInputReader<'_>,
    control: &IndexWorkControl,
    limits: PurposeImportLimits,
) -> Result<String, CliError> {
    let record_count = u64::try_from(records.len()).unwrap_or(u64::MAX);
    if record_count > limits.records {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::Publication,
            IndexWorkResource::PurposeRecords,
            limits.records,
            record_count,
        )
        .into());
    }
    hash_publication_input_file_controlled(hasher, "legacy_map", &config.map_path, reader)?;
    hash_publication_input_file_controlled(
        hasher,
        "nonsource_purposes",
        &config.nonsource_files_path,
        reader,
    )?;
    for record in records {
        control.check(IndexWorkStage::Publication)?;
        hash_index_contract_value(hasher, "purpose_path", &record.path);
        hash_index_contract_value(hasher, "purpose_summary", &record.summary);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Resolve the exact config file selected for a scan plan without loading it.
fn selected_scan_import_config_path(
    config_path: Option<&Path>,
    root: &Path,
) -> Result<Option<PathBuf>, CliError> {
    if let Some(config_path) = config_path {
        return absolute_path(config_path).map(Some);
    }
    Ok(config_candidates_for_root(root)
        .into_iter()
        .find(|candidate| candidate.exists()))
}

/// Bind one optional file's identity and exact bytes to publication input state.
fn hash_publication_input_file_controlled(
    hasher: &mut Hasher,
    role: &str,
    path: &Path,
    reader: &mut PurposeInputReader<'_>,
) -> Result<(), CliError> {
    hash_index_contract_value(hasher, role, &normalize_native_path_display(path));
    if !path.exists() {
        hash_index_contract_value(hasher, "input_state", "missing");
        return Ok(());
    }
    if reader.complete_digest(path).is_none() {
        let _ = reader.read_text(path)?;
    }
    let digest = reader.complete_digest(path).ok_or_else(|| {
        CliError::InvalidInput(format!(
            "purpose publication input was not read completely: {}",
            normalize_native_path_display(path)
        ))
    })?;
    hash_index_contract_value(hasher, "input_state", "present");
    hash_index_contract_value(hasher, "input_digest", digest);
    Ok(())
}

/// Scan command report shared by CLI and MCP adapters.
#[derive(Debug, Serialize)]
pub(crate) struct ScanReport {
    /// Repository overview after scan.
    pub(crate) overview: Overview,
    /// Legacy purpose records imported into the current index.
    pub(crate) purpose_import: PurposeImportReport,
    /// Persisted text search index report.
    pub(crate) text_index: TextIndexReport,
    /// Structural summaries refreshed for declaration-light files.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Symbol graph build report.
    pub(crate) symbols: SymbolBuildReport,
}

/// Legacy purpose import counts from a scan.
#[derive(Debug, Default, Serialize)]
pub(crate) struct PurposeImportReport {
    /// Purpose records imported into indexed nodes.
    pub(crate) imported: usize,
    /// Legacy purpose records skipped because the path is no longer indexed.
    pub(crate) skipped_stale: usize,
    /// Legacy purpose records skipped because a curated purpose already exists.
    pub(crate) skipped_existing: usize,
}

/// Options for the first-run initialization bootstrap.
pub(crate) struct InitBootstrapOptions {
    /// Skip the scan/index phase.
    pub(crate) no_scan: bool,
    /// Force a scan even when future freshness checks would skip it.
    pub(crate) force_rescan: bool,
    /// Optional text index byte limit override.
    pub(crate) text_index_max_bytes: Option<u64>,
}

/// Project initialization phase status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InitPhaseStatus {
    /// Resource was created during this run.
    Created,
    /// Resource already existed before this run.
    Exists,
    /// Resource was verified or phase completed.
    Verified,
    /// Phase was explicitly skipped.
    Skipped,
    /// Phase failed before the report could finish.
    Failed,
}

/// First-run init report shared by CLI and MCP adapters.
#[derive(Debug, Serialize)]
pub(crate) struct InitSetupReport {
    /// Whether every init phase completed successfully.
    pub(crate) ok: bool,
    /// Canonical project root initialized by this command.
    pub(crate) root: String,
    /// Project-local directory status.
    pub(crate) project_dir: InitPathStatus,
    /// Project-local config status.
    pub(crate) config: InitPathStatus,
    /// Project-local non-source registry status.
    pub(crate) nonsource_files: InitPathStatus,
    /// Durable `SQLite` DB status.
    pub(crate) db: InitPathStatus,
    /// Generated host MCP config files.
    pub(crate) host_configs: Vec<InitHostConfigStatus>,
    /// Scan/index phase result.
    pub(crate) scan: InitScanPhase,
    /// Agent harness purpose curation handoff.
    pub(crate) purpose_handoff: PurposeCuratorHandoff,
    /// Human/agent next steps.
    pub(crate) next_steps: Vec<String>,
}

/// Status for one path managed by init.
#[derive(Debug, Serialize)]
pub(crate) struct InitPathStatus {
    /// Path status.
    pub(crate) status: InitPhaseStatus,
    /// Normalized native display path.
    pub(crate) path: String,
}

/// Status for one generated host integration config.
#[derive(Debug, Serialize)]
pub(crate) struct InitHostConfigStatus {
    /// Harness/config shape name.
    pub(crate) harness: &'static str,
    /// File status.
    pub(crate) status: InitPhaseStatus,
    /// Normalized native display path.
    pub(crate) path: String,
    /// Error text when this host config could not be generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// Scan/index phase result for init.
#[derive(Debug, Serialize)]
pub(crate) struct InitScanPhase {
    /// Scan phase status.
    pub(crate) status: InitPhaseStatus,
    /// Whether scan was requested by this run.
    pub(crate) requested: bool,
    /// Whether force-rescan was requested.
    pub(crate) force_rescan: bool,
    /// Scan report when the scan ran.
    pub(crate) report: Option<ScanReport>,
    /// Error text when the scan/index phase failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// Purpose curation handoff for agent/plugin harnesses.
#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "serialized host policy exposes independent capabilities and guarantees"
)]
pub(crate) struct PurposeCuratorHandoff {
    /// Whether this report is intended for an agent harness.
    pub(crate) agent_harness_expected: bool,
    /// Curator execution is owned by the agent host, never the Rust server.
    pub(crate) execution_owner: &'static str,
    /// Recommended subagent reasoning selection.
    pub(crate) recommended_subagent_reasoning: &'static str,
    /// Whether the current main agent may process the same bounded batch.
    pub(crate) main_agent_fallback: bool,
    /// Explicitly records that `ProjectAtlas` did not spawn a host agent.
    pub(crate) server_started_curator: bool,
    /// Successful maintenance should not add ordinary conversation output.
    pub(crate) silent_on_success: bool,
    /// Purpose queue page for initial curation.
    pub(crate) queue: PurposeCurationPage,
    /// Handoff instructions for plugin/agent harnesses.
    pub(crate) instructions: Vec<String>,
}

/// Return a canonical absolute project root.
pub(crate) fn canonical_project_root(root: &Path) -> Result<PathBuf, CliError> {
    root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })
}

/// Return a canonical checked-out source root, rejecting bare Git control roots.
pub(crate) fn canonical_source_project_root(root: &Path) -> Result<PathBuf, CliError> {
    let root = canonical_project_root(root)?;
    if is_bare_git_control_root(&root)? {
        return Err(CliError::WorktreeRequired(Box::new(
            ProjectWorktreeRequired {
                project_root: normalize_native_path_display(&root),
                status: IndexReadStatus::WorktreeRequired,
            },
        )));
    }
    Ok(root)
}

/// Return a typed, non-mutating first-use handoff for one selected root.
pub(crate) fn index_init_required(root: &Path, database: &Path) -> CliError {
    CliError::InitRequired(Box::new(IndexInitRequired {
        project_root: normalize_native_path_display(root),
        database: normalize_native_path_display(database),
        status: IndexReadStatus::InitRequired,
    }))
}

/// Recognize a bare repository or selected common Git control directory.
fn is_bare_git_control_root(root: &Path) -> Result<bool, CliError> {
    let git = root.join(".git");
    let control_root = match fs::metadata(&git) {
        Ok(metadata) if metadata.is_file() => return Ok(false),
        Ok(metadata) if metadata.is_dir() => git,
        Ok(_) => return Ok(false),
        Err(source) if source.kind() == io::ErrorKind::NotFound => root.to_path_buf(),
        Err(source) => {
            return Err(CliError::Io { path: git, source });
        }
    };
    let head = control_root.join("HEAD");
    let config = control_root.join("config");
    let objects = control_root.join("objects");
    let refs = control_root.join("refs");
    for path in [&head, &config, &objects, &refs] {
        if !path.try_exists().map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })? {
            return Ok(false);
        }
    }
    let structurally_git = fs::metadata(&head)
        .map_err(|source| CliError::Io { path: head, source })?
        .is_file()
        && fs::metadata(&config)
            .map_err(|source| CliError::Io {
                path: config.clone(),
                source,
            })?
            .is_file()
        && fs::metadata(&objects)
            .map_err(|source| CliError::Io {
                path: objects,
                source,
            })?
            .is_dir()
        && fs::metadata(&refs)
            .map_err(|source| CliError::Io { path: refs, source })?
            .is_dir();
    if !structurally_git {
        return Ok(false);
    }
    if control_root == root {
        return Ok(true);
    }

    effective_git_config_bare_setting(&control_root, &config).map(|bare| bare.unwrap_or(false))
}

/// Query Git's effective local `core.bare` value, including configured includes.
fn effective_git_config_bare_setting(
    control_root: &Path,
    config: &Path,
) -> Result<Option<bool>, CliError> {
    let mut child = StdCommand::new("git")
        .arg("--git-dir")
        .arg(normalize_native_path_display(control_root))
        .args([
            "config",
            "--local",
            "--includes",
            "--get",
            "--bool",
            "core.bare",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| CliError::Io {
            path: config.to_path_buf(),
            source,
        })?;
    let deadline = Instant::now() + GIT_CONFIG_QUERY_TIMEOUT;
    loop {
        match child.try_wait().map_err(|source| CliError::Io {
            path: config.to_path_buf(),
            source,
        })? {
            Some(_) => break,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => {
                child.kill().map_err(|source| CliError::Io {
                    path: config.to_path_buf(),
                    source,
                })?;
                child.wait().map_err(|source| CliError::Io {
                    path: config.to_path_buf(),
                    source,
                })?;
                return Err(CliError::Io {
                    path: config.to_path_buf(),
                    source: io::Error::new(
                        io::ErrorKind::TimedOut,
                        "effective Git config query exceeded its deadline",
                    ),
                });
            }
        }
    }
    let output = child.wait_with_output().map_err(|source| CliError::Io {
        path: config.to_path_buf(),
        source,
    })?;
    if output.stdout.len().saturating_add(output.stderr.len()) > MAX_GIT_CONFIG_QUERY_OUTPUT_BYTES {
        return Err(CliError::Io {
            path: config.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "effective Git config query exceeded its output bound",
            ),
        });
    }
    if output.status.success() {
        return match std::str::from_utf8(&output.stdout)
            .map(str::trim)
            .map_err(|source| CliError::Io {
                path: config.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            })? {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            value => Err(CliError::Io {
                path: config.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Git returned invalid core.bare value {value:?}"),
                ),
            }),
        };
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    Err(CliError::Io {
        path: config.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "effective Git config query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ),
    })
}

/// Load map configuration for purpose import during scan.
pub(crate) fn load_scan_import_config(
    config_path: Option<&Path>,
    scan_path: &Path,
) -> Result<Option<atlas_map::AtlasMapConfig>, CliError> {
    if let Some(config_path) = config_path {
        return Ok(Some(load_atlas_config(Some(config_path))?));
    }
    let project_config = scan_path.join(".projectatlas").join("config.toml");
    if project_config.exists() {
        return Ok(Some(load_atlas_config(Some(&project_config))?));
    }
    let flat_config = scan_path.join("projectatlas.toml");
    if flat_config.exists() {
        return Ok(Some(load_atlas_config(Some(&flat_config))?));
    }
    Ok(None)
}

/// Open or create a durable index bound to one selected project root.
pub(crate) fn open_atlas_store_for_project(
    path: &Path,
    root: &Path,
) -> Result<AtlasStore, CliError> {
    open_atlas_store_for_project_with_location_validator(path, root, validate_database_location)
}

/// Validate storage before creating the database parent and opening the store.
fn open_atlas_store_for_project_with_location_validator<F>(
    path: &Path,
    root: &Path,
    validate_location: F,
) -> Result<AtlasStore, CliError>
where
    F: FnOnce(&Path) -> projectatlas_db::DbResult<()>,
{
    validate_location(path).map_err(project_store_error)?;
    ensure_parent_dir(path)?;
    AtlasStore::open_for_project(path, root).map_err(project_store_error)
}

/// Open a current durable index read snapshot bound to one selected root.
pub(crate) fn open_atlas_store_read_only_for_project(
    path: &Path,
    root: &Path,
) -> Result<AtlasStore, CliError> {
    AtlasStore::open_read_only_for_project(path, root).map_err(project_store_error)
}

/// Preserve typed selected-root mismatch diagnostics across store adapters.
pub(crate) fn project_store_error(source: projectatlas_db::DbError) -> CliError {
    match source {
        projectatlas_db::DbError::ProjectRootMismatch { expected, found } => {
            CliError::ProjectMismatch(Box::new(IndexProjectMismatch {
                status: IndexReadStatus::ProjectMismatch,
                selected_project_root: expected,
                indexed_project_root: found,
            }))
        }
        other => CliError::Db(other),
    }
}

/// Return the config path init should preserve or create for a project root.
pub(crate) fn init_config_path(root: &Path, explicit: Option<&Path>) -> PathBuf {
    if let Some(config_path) = explicit {
        return if config_path.is_absolute() {
            config_path.to_path_buf()
        } else {
            root.join(config_path)
        };
    }
    let nested_config = root.join(".projectatlas").join("config.toml");
    if nested_config.exists() {
        return nested_config;
    }
    let flat_config = root.join("projectatlas.toml");
    if flat_config.exists() {
        return flat_config;
    }
    nested_config
}

/// Run the one-call first-run init bootstrap.
pub(crate) fn run_init_bootstrap(
    root: &Path,
    db_path: &Path,
    config_path: Option<&Path>,
    options: &InitBootstrapOptions,
) -> Result<InitSetupReport, CliError> {
    let root = canonical_source_project_root(root)?;
    let project_dir = root.join(".projectatlas");
    let config_file = init_config_path(&root, config_path);
    let nonsource_file = project_dir.join("projectatlas-nonsource-files.toon");
    let project_dir_existed = project_dir.exists();
    let config_existed = config_file.exists();
    let nonsource_existed = nonsource_file.exists();
    let db_existed = db_path.exists();

    init_project_with_config(&root, Some(&config_file))?;
    let mut store = open_atlas_store_for_project(db_path, &root)?;

    let mut ok = true;
    let (scan_status, scan_report, scan_error) = if options.no_scan {
        (InitPhaseStatus::Skipped, None, None)
    } else {
        let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
        let control = index_work_control(&symbol_options);
        match ScanRuntimePlan::for_path_controlled(
            config_path,
            &root,
            options.text_index_max_bytes,
            &control,
        )
        .and_then(|plan| run_scan_pipeline_controlled(&mut store, &plan, &symbol_options, &control))
        {
            Ok(report) => (InitPhaseStatus::Verified, Some(report), None),
            Err(error) => {
                ok = false;
                (InitPhaseStatus::Failed, None, Some(error.to_string()))
            }
        }
    };

    let purpose_query = HealthQuery {
        start_index: 0,
        limit: DEFAULT_HEALTH_LIMIT,
        category: None,
        severity: None,
        path_prefix: None,
        summary_only: false,
        scope: HealthScope::purpose_default(),
    };
    let purpose_queue = purpose_curation_page(&store, &purpose_query, "project-init")?;
    let next_steps = init_next_steps(options.no_scan, scan_error.is_some(), purpose_queue.total);

    Ok(InitSetupReport {
        ok,
        root: normalize_native_path_display(&root),
        project_dir: InitPathStatus {
            status: init_path_status(project_dir_existed),
            path: normalize_native_path_display(project_dir),
        },
        config: InitPathStatus {
            status: init_path_status(config_existed),
            path: normalize_native_path_display(config_file),
        },
        nonsource_files: InitPathStatus {
            status: init_path_status(nonsource_existed),
            path: normalize_native_path_display(nonsource_file),
        },
        db: InitPathStatus {
            status: init_path_status(db_existed),
            path: normalize_native_path_display(db_path),
        },
        host_configs: Vec::new(),
        scan: InitScanPhase {
            status: scan_status,
            requested: !options.no_scan,
            force_rescan: options.force_rescan,
            report: scan_report,
            error: scan_error,
        },
        purpose_handoff: purpose_curator_handoff(purpose_queue),
        next_steps,
    })
}

/// Return created/existing status for a path.
pub(crate) fn init_path_status(existed: bool) -> InitPhaseStatus {
    if existed {
        InitPhaseStatus::Exists
    } else {
        InitPhaseStatus::Created
    }
}

/// Return stable purpose handoff instructions for agent harnesses.
fn purpose_handoff_instructions() -> Vec<String> {
    vec![
        "If the host supports isolated subagents, delegate this actionable low-scope batch at the lowest reasoning tier the host can enforce; otherwise let the main agent process the same bounded rows without blocking navigation.".to_string(),
        "Inspect only bounded current summary, graph, outline, or exact-slice context, then copy task, work_key, and state_token into atlas_purpose_review or projectatlas purpose review --apply; never edit SQLite directly.".to_string(),
        "Skip accepted purposes unless an agent or user explicitly assigns a correction; use atlas_purpose_set or projectatlas purpose set for that deliberate correction path.".to_string(),
        "Keep successful curator maintenance out of normal conversation; ProjectAtlas reports a handoff and never claims that the Rust server spawned an agent.".to_string(),
    ]
}

/// Build one host-owned purpose-curator handoff shared by init and session brief.
pub(crate) fn purpose_curator_handoff(queue: PurposeCurationPage) -> PurposeCuratorHandoff {
    PurposeCuratorHandoff {
        agent_harness_expected: true,
        execution_owner: "agent_host",
        recommended_subagent_reasoning: "lowest_host_enforced",
        main_agent_fallback: true,
        server_started_curator: false,
        silent_on_success: true,
        queue,
        instructions: purpose_handoff_instructions(),
    }
}

/// Return concise next steps for humans and agents.
fn init_next_steps(
    scan_skipped: bool,
    scan_failed: bool,
    purpose_queue_total: usize,
) -> Vec<String> {
    let mut steps = Vec::new();
    if scan_skipped {
        steps.push("Run projectatlas scan when you are ready to build the deep index.".to_string());
    } else if scan_failed {
        steps.push(
            "Fix the scan/index error and rerun projectatlas init or projectatlas scan."
                .to_string(),
        );
    }
    if purpose_queue_total > 0 {
        steps.push(
            "Use the purpose_handoff queue to delegate purpose creation/correction at the lowest reasoning tier the host can enforce."
                .to_string(),
        );
    } else {
        steps.push("Purpose queue is empty for the default high-impact scope.".to_string());
    }
    steps.push("Run projectatlas overview to confirm repository orientation.".to_string());
    steps
}

/// Create the parent directory for a path when it has one.
pub(crate) fn ensure_parent_dir(path: &Path) -> Result<(), CliError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|source| CliError::Io {
        path: parent.to_path_buf(),
        source,
    })
}

/// Build the standard config/root mismatch error.
pub(crate) fn config_root_mismatch_error(
    config_path: &Path,
    config_root: &Path,
    selected_root: &Path,
) -> CliError {
    CliError::InvalidInput(format!(
        "ProjectAtlas config '{}' resolves project root '{}' outside selected project root '{}'",
        config_path.display(),
        config_root.display(),
        selected_root.display()
    ))
}

/// Resolve the default MCP project root without trusting the process cwd.
pub(crate) fn default_mcp_project_root(
    db: &Path,
    config_path: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if let Some(config_path) = config_path {
        let config = load_atlas_config(Some(config_path))?;
        let config_root = canonical_source_project_root(&config.root)?;
        if let Some(db_root) = project_root_from_db_path(db) {
            let db_root = canonical_source_project_root(&db_root)?;
            if config_root != db_root {
                return Err(config_root_mismatch_error(
                    config_path,
                    &config_root,
                    &db_root,
                ));
            }
        }
        return Ok(config_root);
    }
    if db.exists()
        && let Some(project_root) = read_project_root_read_only(db)?
    {
        return canonical_source_project_root(Path::new(&project_root));
    }
    if let Some(project_root) = project_root_from_db_path(db) {
        return canonical_source_project_root(&project_root);
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    canonical_source_project_root(&current_dir)
}

/// Resolve the default CLI project root before opening an implicit database.
pub(crate) fn default_cli_project_root(
    db: &Path,
    config_path: Option<&Path>,
    database_path_is_explicit: bool,
) -> Result<PathBuf, CliError> {
    if !database_path_is_explicit
        && config_path.is_none()
        && let Some(project_root) = project_root_from_db_path(db)
    {
        return canonical_source_project_root(&project_root);
    }
    default_mcp_project_root(db, config_path)
}

/// Resolve a CLI repository-root argument, using indexed state for the default `.`.
pub(crate) fn defaultable_cli_project_root(
    path: &Path,
    db: &Path,
    config_path: Option<&Path>,
    database_path_is_explicit: bool,
) -> Result<PathBuf, CliError> {
    if path == Path::new(".") {
        return default_cli_project_root(db, config_path, database_path_is_explicit);
    }
    Ok(path.to_path_buf())
}

/// Infer a project root from a default `.projectatlas/projectatlas.db` path.
fn project_root_from_db_path(db: &Path) -> Option<PathBuf> {
    let parent = db.parent()?;
    let cache_dir_name = parent.file_name()?;
    if cache_dir_name != ".projectatlas" {
        return None;
    }
    parent
        .parent()
        .filter(|root| !root.as_os_str().is_empty())
        .map_or_else(|| Some(PathBuf::from(".")), |root| Some(root.to_path_buf()))
}

/// Load scan options for a project root from `ProjectAtlas` config when present.
pub(crate) fn scan_options_for_root(
    config_path: Option<&Path>,
    root: &Path,
) -> Result<ScanOptions, CliError> {
    Ok(load_scan_import_config(config_path, root)?
        .as_ref()
        .map_or_else(
            ScanOptions::default,
            atlas_map::AtlasMapConfig::scan_options,
        ))
}

/// Resolve text-index persistence options from command override and config.
pub(crate) fn text_index_options(
    config: Option<&atlas_map::AtlasMapConfig>,
    max_bytes_override: Option<u64>,
) -> TextIndexOptions {
    let max_bytes = max_bytes_override
        .filter(|value| *value > 0)
        .or_else(|| config.map(atlas_map::AtlasMapConfig::text_index_max_bytes))
        .unwrap_or(atlas_map::DEFAULT_TEXT_INDEX_MAX_BYTES);
    TextIndexOptions::new(max_bytes)
}

/// Capture the last complete generation used as a publication compare-and-swap anchor.
fn publication_base_generation(store: &AtlasStore) -> Result<IndexGeneration, CliError> {
    Ok(store
        .index_publication()?
        .map_or(IndexGeneration::ZERO, |publication| publication.generation))
}

/// Prepare a complete source/index batch without acquiring the `SQLite` writer.
fn stage_full_index_publication(
    store: &AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    reuse_unchanged_symbols: bool,
    import_legacy_purposes: bool,
    control: &IndexWorkControl,
) -> Result<IndexPublicationBatch, CliError> {
    let base_generation = publication_base_generation(store)?;
    let contract_fingerprint = plan.publication_contract_fingerprint();
    let previous_hashes = reuse_unchanged_symbols
        .then(|| indexed_file_hashes(store))
        .transpose()?;
    let nodes = scan_repo_controlled(
        &plan.root,
        &plan.scan_options,
        ScanLimits::default(),
        control,
    )
    .map_err(|source| source_inspection_error(&plan.root, source))?;
    control.check(IndexWorkStage::Publication)?;
    let purpose_import = import_legacy_purposes
        .then(|| plan.purpose_import_snapshot_controlled(&nodes, control))
        .transpose()?;
    let protected_purpose_paths = protected_purpose_paths(&nodes, purpose_import.as_ref());
    let text_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    let text = stage_text_index_for_changed_paths_controlled(
        &plan.root,
        &nodes,
        plan.text_options,
        control,
    )?;
    let retained_before_symbols =
        staged_publication_identity_bytes(&plan.root, &contract_fingerprint)
            .saturating_add(staged_string_bytes(&text_paths))
            .saturating_add(staged_node_bytes(&nodes))
            .saturating_add(staged_text_bytes(&text))
            .saturating_add(staged_purpose_bytes(purpose_import.as_ref()));
    let symbol_limits = symbol_limits_with_remaining_staging_bytes(retained_before_symbols)?;
    let symbols = stage_symbols_for_nodes_with_limits(
        store,
        &plan.root,
        #[cfg(feature = "optional-parser-supervisor")]
        &plan.optional_parser_selection,
        &nodes,
        symbol_options,
        previous_hashes.as_ref(),
        None,
        &protected_purpose_paths,
        control,
        symbol_limits,
    )?;
    let graph = graph_projection::stage_full_repository_graph(
        store,
        &plan.root,
        base_generation,
        &nodes,
        &symbols,
        control,
    )?;
    let structural_summaries = stage_structural_summaries_for_nodes_controlled(
        store,
        &nodes,
        &text.rows,
        Some(&symbols),
        &protected_purpose_paths,
        symbol_options.effective_workers(),
        control,
    )?;
    enforce_publication_staging_budget(
        retained_before_symbols
            .saturating_add(symbols.retained_bytes)
            .saturating_add(graph.retained_bytes())
            .saturating_add(structural_summaries.retained_bytes),
    )?;
    Ok(IndexPublicationBatch {
        base_generation,
        contract_fingerprint,
        root: plan.root.clone(),
        nodes: NodePublicationBatch::Full { nodes },
        purpose_import,
        text_paths,
        text,
        symbols,
        graph,
        structural_summaries,
    })
}

/// Return paths whose reviewed or built-in purpose must suppress generated suggestions.
fn protected_purpose_paths(
    nodes: &[Node],
    purpose_import: Option<&PurposeImportSnapshot>,
) -> HashSet<String> {
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    let mut protected = BUILTIN_PROJECTATLAS_PURPOSES
        .iter()
        .filter(|(path, _purpose)| indexed_paths.contains(*path))
        .map(|(path, _purpose)| (*path).to_string())
        .collect::<HashSet<_>>();
    if let Some(snapshot) = purpose_import {
        protected.extend(
            snapshot
                .records
                .iter()
                .filter(|record| indexed_paths.contains(record.path.as_str()))
                .map(|record| record.path.clone()),
        );
    }
    protected
}

/// Count retained node string bytes for one bounded in-memory publication batch.
fn staged_node_bytes(nodes: &[Node]) -> u64 {
    nodes.iter().fold(0_u64, |bytes, node| {
        bytes
            .saturating_add(node.path.len() as u64)
            .saturating_add(
                node.parent_path
                    .as_ref()
                    .map_or(0, |value| value.len() as u64),
            )
            .saturating_add(
                node.extension
                    .as_ref()
                    .map_or(0, |value| value.len() as u64),
            )
            .saturating_add(node.language.as_ref().map_or(0, |value| value.len() as u64))
            .saturating_add(
                node.content_hash
                    .as_ref()
                    .map_or(0, |value| value.len() as u64),
            )
    })
}

/// Count retained persisted-text strings for one staged batch.
fn staged_text_bytes(text: &TextIndexRefresh) -> u64 {
    text.rows.iter().fold(0_u64, |bytes, row| {
        let bytes = bytes.saturating_add(row.path.len() as u64);
        row.text.as_ref().map_or(bytes, |text| {
            bytes
                .saturating_add(text.path.len() as u64)
                .saturating_add(
                    text.content_hash
                        .as_ref()
                        .map_or(0, |value| value.len() as u64),
                )
                .saturating_add(text.content.len() as u64)
        })
    })
}

/// Count retained legacy-purpose strings for one staged batch.
fn staged_purpose_bytes(purpose_import: Option<&PurposeImportSnapshot>) -> u64 {
    purpose_import.map_or(0, |snapshot| {
        snapshot
            .records
            .iter()
            .fold(snapshot.fingerprint.len() as u64, |bytes, record| {
                bytes
                    .saturating_add(record.path.len() as u64)
                    .saturating_add(record.summary.len() as u64)
            })
    })
}

/// Count retained strings duplicated into one publication batch.
fn staged_string_bytes(values: &[String]) -> u64 {
    values.iter().fold(0_u64, |bytes, value| {
        bytes.saturating_add(value.len() as u64)
    })
}

/// Count the selected root and derivation identity retained by one batch.
fn staged_publication_identity_bytes(root: &Path, contract_fingerprint: &str) -> u64 {
    (normalize_native_path_display(root).len() as u64)
        .saturating_add(contract_fingerprint.len() as u64)
}

/// Restrict parser output to the remaining aggregate publication-staging budget.
fn symbol_limits_with_remaining_staging_bytes(
    retained_bytes: u64,
) -> Result<SymbolPublicationLimits, CliError> {
    enforce_publication_staging_budget(retained_bytes)?;
    Ok(SymbolPublicationLimits {
        output_bytes: SymbolPublicationLimits::STANDARD
            .output_bytes
            .min(MAX_PUBLICATION_STAGING_BYTES.saturating_sub(retained_bytes)),
        ..SymbolPublicationLimits::STANDARD
    })
}

/// Fail before writer acquisition when retained publication state exceeds its budget.
fn enforce_publication_staging_budget(retained_bytes: u64) -> Result<(), CliError> {
    if retained_bytes > MAX_PUBLICATION_STAGING_BYTES {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::Publication,
            IndexWorkResource::OutputBytes,
            MAX_PUBLICATION_STAGING_BYTES,
            retained_bytes,
        )
        .into());
    }
    Ok(())
}

/// Build the complete expected source state after one normalized incremental delta.
fn expected_nodes_after_incremental(
    baseline_nodes: Vec<Node>,
    changed_nodes: &[Node],
    absent_paths: &[String],
) -> Vec<Node> {
    let absent_paths = absent_paths
        .iter()
        .map(String::as_str)
        .filter(|path| !matches!(*path, "" | "."))
        .collect::<HashSet<_>>();
    let mut expected = baseline_nodes
        .into_iter()
        .filter(|node| !repository_path_is_absent(&node.path, &absent_paths))
        .map(|node| (node.path.clone(), node))
        .collect::<BTreeMap<_, _>>();
    for node in changed_nodes {
        expected.insert(node.path.clone(), node.clone());
    }
    expected.into_values().collect()
}

/// Match an exact absent repository key or one of its slash-delimited ancestors.
fn repository_path_is_absent(path: &str, absent_paths: &HashSet<&str>) -> bool {
    absent_paths.contains(path)
        || path
            .match_indices('/')
            .any(|(separator, _)| absent_paths.contains(&path[..separator]))
}

/// Apply one fully prepared index batch in a short generation-checked transaction.
fn publish_index_batch(
    store: &mut AtlasStore,
    batch: IndexPublicationBatch,
    control: &IndexWorkControl,
) -> Result<IndexPublicationOutcome, CliError> {
    control.check(IndexWorkStage::Publication)?;
    let IndexPublicationBatch {
        base_generation,
        contract_fingerprint,
        root,
        nodes,
        purpose_import,
        text_paths,
        text,
        symbols,
        graph,
        structural_summaries,
    } = batch;
    let mut publication =
        store.begin_index_publication_from(&contract_fingerprint, base_generation)?;
    publication.set_project_root(&root)?;
    let indexed_nodes = match nodes {
        NodePublicationBatch::Full { nodes } => {
            publication.begin_scan_replacement()?;
            for batch in nodes.chunks(PUBLICATION_NODE_BATCH_SIZE) {
                control.check(IndexWorkStage::Publication)?;
                publication.upsert_scan_node_batch(batch)?;
            }
            control.check(IndexWorkStage::Publication)?;
            publication.finish_scan_replacement()?;
            nodes
        }
        NodePublicationBatch::Incremental {
            nodes,
            absent_paths,
            expected_nodes: _,
        } => {
            for batch in nodes.chunks(PUBLICATION_NODE_BATCH_SIZE) {
                control.check(IndexWorkStage::Publication)?;
                publication.upsert_scan_node_batch(batch)?;
            }
            for batch in absent_paths.chunks(PUBLICATION_PATH_BATCH_SIZE) {
                control.check(IndexWorkStage::Publication)?;
                publication.mark_paths_absent(batch)?;
            }
            nodes
        }
    };
    seed_builtin_projectatlas_purposes(&publication, &indexed_nodes)?;
    apply_text_index_stage(&mut publication, &text_paths, &text, control)?;
    let purpose_import = purpose_import.map_or_else(
        || Ok(PurposeImportReport::default()),
        |snapshot| apply_purpose_import_snapshot(&publication, &indexed_nodes, &snapshot, control),
    )?;
    apply_symbol_build_stage(&mut publication, &symbols, control)?;
    graph.apply(&mut publication, control)?;
    apply_structural_summary_stage(&mut publication, &structural_summaries, control)?;
    complete_index_publication(publication, control)?;
    Ok(IndexPublicationOutcome {
        purpose_import,
        text_index: text.report,
        structural_summaries: structural_summaries.report,
        symbols: symbols.report,
    })
}

/// Apply staged legacy-purpose rows without overwriting current reviewed intent.
fn apply_purpose_import_snapshot(
    store: &AtlasStore,
    nodes: &[Node],
    snapshot: &PurposeImportSnapshot,
    control: &IndexWorkControl,
) -> Result<PurposeImportReport, CliError> {
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    let mut report = PurposeImportReport::default();
    for record in &snapshot.records {
        control.check(IndexWorkStage::Publication)?;
        if !indexed_paths.contains(record.path.as_str()) {
            report.skipped_stale += 1;
            continue;
        }
        let Some(indexed) = store.load_node_by_path(&record.path)? else {
            report.skipped_stale += 1;
            continue;
        };
        if matches!(
            indexed.purpose.status,
            PurposeStatus::Approved | PurposeStatus::Stale
        ) {
            report.skipped_existing += 1;
            continue;
        }
        store.set_purpose(&record.path, &record.summary, PurposeSource::Imported)?;
        report.imported += 1;
    }
    Ok(report)
}

/// Execute the full scan/index/symbol pipeline for a resolved project plan.
#[cfg(test)]
pub(crate) fn run_scan_pipeline(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
) -> Result<ScanReport, CliError> {
    let control = index_work_control(symbol_options);
    run_scan_pipeline_controlled(store, plan, symbol_options, &control)
}

/// Execute the full pipeline under one cancellation and resource boundary.
pub(crate) fn run_scan_pipeline_controlled(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<ScanReport, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    let batch = stage_full_index_publication(store, plan, symbol_options, false, true, control)?;
    revalidate_staged_publication_inputs_with_purpose_snapshot(
        plan,
        batch.nodes.expected_nodes(),
        batch.purpose_import.as_ref(),
        control,
    )?;
    let outcome = publish_index_batch(store, batch, control)?;
    let overview = store.overview()?;
    Ok(ScanReport {
        overview,
        purpose_import: outcome.purpose_import,
        text_index: outcome.text_index,
        structural_summaries: outcome.structural_summaries,
        symbols: outcome.symbols,
    })
}

/// Rebuild symbol projections while keeping incomplete work non-queryable.
#[cfg(test)]
pub(crate) fn run_symbol_build_pipeline(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
) -> Result<SymbolBuildReport, CliError> {
    let control = index_work_control(symbol_options);
    run_symbol_build_pipeline_controlled(store, plan, symbol_options, previous_hashes, &control)
}

/// Rebuild symbol projections under one cancellation and publication boundary.
pub(crate) fn run_symbol_build_pipeline_controlled(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
    control: &IndexWorkControl,
) -> Result<SymbolBuildReport, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    control.check(IndexWorkStage::SymbolParsing)?;
    verify_index_project_root(store, &plan.root)?;
    verify_index_publication(store, plan)?;
    let base_generation = publication_base_generation(store)?;
    let nodes = store
        .load_nodes()?
        .into_iter()
        .map(|indexed| indexed.node)
        .collect::<Vec<_>>();
    let contract_fingerprint = plan.publication_contract_fingerprint();
    let retained_before_symbols =
        staged_publication_identity_bytes(&plan.root, &contract_fingerprint)
            .saturating_add(staged_node_bytes(&nodes));
    let symbol_limits = symbol_limits_with_remaining_staging_bytes(retained_before_symbols)?;
    let staged = stage_symbols_for_nodes_with_limits(
        store,
        &plan.root,
        #[cfg(feature = "optional-parser-supervisor")]
        &plan.optional_parser_selection,
        &nodes,
        symbol_options,
        previous_hashes,
        None,
        &HashSet::new(),
        control,
        symbol_limits,
    )?;
    let graph = graph_projection::stage_full_repository_graph(
        store,
        &plan.root,
        base_generation,
        &nodes,
        &staged,
        control,
    )?;
    enforce_publication_staging_budget(
        retained_before_symbols
            .saturating_add(staged.retained_bytes)
            .saturating_add(graph.retained_bytes()),
    )?;
    revalidate_staged_publication_inputs_controlled(plan, &nodes, None, control)?;
    control.check(IndexWorkStage::Publication)?;
    let mut publication =
        store.begin_index_projection_refresh_from(&contract_fingerprint, base_generation)?;
    apply_symbol_build_stage(&mut publication, &staged, control)?;
    graph.apply(&mut publication, control)?;
    complete_index_publication(publication, control)?;
    Ok(staged.report)
}

/// One optional telemetry identity owned by a CLI invocation or MCP process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsageRuntimeInstance {
    /// Opaque runtime-owned identity persisted with usage events.
    id: UsageInstanceId,
    /// Adapter lifecycle that owns sealing this identity.
    owner: UsageInstanceOwner,
}

impl UsageRuntimeInstance {
    /// Create an opaque runtime identity when operating-system entropy is available.
    #[must_use]
    pub(crate) fn new(owner: UsageInstanceOwner) -> Option<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        UsageInstanceId::from_bytes(bytes)
            .ok()
            .map(|id| Self { id, owner })
    }

    /// Record one event using the lifecycle implied by this runtime owner.
    fn record(
        self,
        store: &AtlasStore,
        event: &projectatlas_core::telemetry::UsageEvent,
    ) -> Result<(), CliError> {
        store.record_usage_for_instance(
            self.id,
            self.owner,
            event,
            matches!(self.owner, UsageInstanceOwner::CliInvocation),
        )?;
        Ok(())
    }

    /// Seal this runtime instance in one selected project database.
    pub(crate) fn seal(self, store: &AtlasStore) -> Result<(), CliError> {
        store.seal_usage_instance(self.id)?;
        Ok(())
    }
}

/// Record a usage event from a fast baseline estimate and actual atlas payload.
pub(crate) fn record_usage_estimate(
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    record_usage_estimate_with_context(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        projectatlas_text,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_CONFIDENCE_INFERRED,
    )
}

/// Record a usage event from a fast baseline estimate and explicit baseline semantics.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_usage_estimate_with_context(
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
    token_savings_bucket: &str,
    baseline_kind: &str,
    confidence: &str,
) -> Result<(), CliError> {
    let Some(usage_instance) = usage_instance.filter(|_| !telemetry_disabled()) else {
        return Ok(());
    };
    store.finish_index_read_snapshot()?;
    usage_instance.record(
        store,
        &usage_from_estimates_with_context(
            session,
            command,
            path,
            query,
            estimated_without_projectatlas,
            estimate_tokens(projectatlas_text),
            token_savings_bucket,
            baseline_kind,
            confidence,
        ),
    )?;
    Ok(())
}

/// Record a broad directory-walk avoidance estimate.
pub(crate) fn record_directory_walk_usage_estimate(
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    record_usage_estimate_with_context(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        projectatlas_text,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_DIRECTORY_WALK,
        TOKEN_CONFIDENCE_POLICY_ESTIMATE,
    )
}

/// Record a usage event from baseline and emitted text unless telemetry is disabled.
pub(crate) fn record_usage_text(
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    let Some(usage_instance) = usage_instance.filter(|_| !telemetry_disabled()) else {
        return Ok(());
    };
    store.finish_index_read_snapshot()?;
    usage_instance.record(
        store,
        &usage_from_text(
            session,
            command,
            path,
            query,
            baseline_text,
            projectatlas_text,
        ),
    )?;
    Ok(())
}

/// Return whether telemetry writes are disabled for read-only review contexts.
pub(crate) fn telemetry_disabled() -> bool {
    truthy_env("PROJECTATLAS_NO_TELEMETRY")
}

/// Estimate broad source tokens represented by indexed files with SQL aggregates.
pub(crate) fn estimated_source_tokens_for_indexed_files(
    store: &AtlasStore,
    folder: Option<&str>,
    file_pattern: Option<&str>,
) -> Result<usize, CliError> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    let mut total = 0usize;
    store.visit_file_token_estimates(folder, |path, size_bytes| {
        if matcher.is_match(&path) {
            total =
                total.saturating_add(estimated_source_tokens_for_file_metadata(&path, size_bytes));
        }
        Ok(true)
    })?;
    Ok(total)
}

/// Estimate source tokens for one indexed file without reading it.
pub(crate) fn estimated_source_tokens_for_file_node(node: &Node) -> usize {
    estimated_source_tokens_for_file_metadata(&node.path, node.size_bytes)
}

/// Estimate source tokens for persisted file metadata.
pub(crate) fn estimated_source_tokens_for_file_metadata(
    path: &str,
    size_bytes: Option<u64>,
) -> usize {
    size_bytes.map_or_else(|| estimate_tokens(path), byte_size_to_tokens)
}

/// Estimate source tokens from a byte count with the shared token heuristic.
pub(crate) fn byte_size_to_tokens(bytes: u64) -> usize {
    let token_estimate = bytes.div_ceil(4);
    usize::try_from(token_estimate).unwrap_or(usize::MAX)
}

/// Estimate source tokens from a searched byte count.
pub(crate) fn byte_count_to_tokens(bytes: usize) -> usize {
    if bytes == 0 { 0 } else { bytes.div_ceil(4) }
}

/// Load ranked folder nodes with concise reasons.
pub(crate) fn ranked_folder_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    limit: usize,
) -> Result<Vec<projectatlas_core::RankedNode>, CliError> {
    Ok(load_ranked_folder_nodes_with_reasons(store, query, limit)?)
}

/// Load ranked file nodes with concise reasons.
pub(crate) fn ranked_file_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
    include_content: bool,
) -> Result<Vec<projectatlas_core::RankedNode>, CliError> {
    Ok(load_ranked_file_nodes_with_reasons(
        store,
        query,
        folder,
        file_pattern,
        limit,
        include_content,
    )?)
}

/// Build a next-step recommendation report from indexed metadata.
pub(crate) fn next_step_report(
    store: &AtlasStore,
    query: &str,
    limit: Option<usize>,
) -> Result<NextStepReport, CliError> {
    Ok(build_next_report(store, query, limit)?)
}

/// Build the flattened agent-facing next-step payload.
pub(crate) fn next_step_report_payload(report: &NextStepReport) -> Value {
    json!({
        "query": &report.query,
        "folders": render_ranked_node_rows("folders", &report.folders),
        "files": render_ranked_node_rows("files", &report.files),
        "suggestions": &report.suggestions,
    })
}

/// Agent-facing purpose curation queue with bounded health metadata.
#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "serialized queue paging and scope fields are independent wire facts"
)]
pub(crate) struct PurposeCurationPage {
    /// Selected project identity bound into every work key.
    pub(crate) project_instance_id: String,
    /// Active generation bound into every work key and state token.
    pub(crate) active_generation: u64,
    /// Host-owned task label for this bounded batch.
    pub(crate) task: String,
    /// Deterministic identity for the complete returned batch.
    pub(crate) work_key: String,
    /// Whether this page contains work a host or main agent can process.
    pub(crate) actionable: bool,
    /// Purpose policy scope; automatic handoffs are always low scope.
    pub(crate) curation_scope: &'static str,
    /// Findings after filters are applied.
    pub(crate) total: usize,
    /// Findings before filters are applied, after resolved findings are removed.
    pub(crate) unfiltered_total: usize,
    /// Findings returned in this page.
    pub(crate) returned: usize,
    /// Pagination start index used for this page.
    pub(crate) start_index: usize,
    /// Maximum findings requested for this page.
    pub(crate) limit: usize,
    /// Maximum allowed page size.
    pub(crate) max_limit: usize,
    /// Next start index when more rows are available.
    pub(crate) next_start_index: Option<usize>,
    /// Whether more rows are available.
    pub(crate) truncated: bool,
    /// Whether the queue is restricted to source-relevant paths.
    pub(crate) source_only: bool,
    /// Folder scope included in the queue.
    pub(crate) folder_scope: String,
    /// File scope included in the queue.
    pub(crate) file_scope: String,
    /// Applied category filter.
    pub(crate) category: String,
    /// Applied severity filter.
    pub(crate) severity: String,
    /// Applied path-prefix filter.
    pub(crate) path_prefix: String,
    /// Whether rows were intentionally omitted.
    pub(crate) summary_only: bool,
    /// Queue items that need agent inspection or approval.
    pub(crate) items: Vec<PurposeCurationItem>,
}

/// One path that needs purpose curation.
#[derive(Debug, Serialize)]
pub(crate) struct PurposeCurationItem {
    /// Deterministic project/generation/task/path identity for duplicate coalescing.
    pub(crate) work_key: String,
    /// Opaque current-row token required for stale-safe conditional review.
    pub(crate) state_token: String,
    /// Finding severity.
    pub(crate) severity: String,
    /// Stable health finding id.
    pub(crate) id: String,
    /// Health finding category.
    pub(crate) category: String,
    /// Indexed repository-relative path.
    pub(crate) path: String,
    /// Related path for structural findings.
    pub(crate) related_path: String,
    /// Node kind when the path is still indexed.
    pub(crate) kind: String,
    /// Detected language for source files.
    pub(crate) language: String,
    /// Current approved or suggested purpose text.
    pub(crate) purpose: String,
    /// Purpose lifecycle status.
    pub(crate) purpose_status: String,
    /// Purpose source.
    pub(crate) purpose_source: String,
    /// Whether an agent explicitly reviewed or set this purpose.
    pub(crate) purpose_agent_reviewed: bool,
    /// Priority for agent curation.
    pub(crate) review_priority: String,
    /// Stable reason explaining the priority.
    pub(crate) review_reason: String,
    /// Current deterministic content summary.
    pub(crate) content_summary: String,
    /// Recommended agent action.
    pub(crate) recommendation: String,
}

/// One agent-reviewed purpose update requested by a batch review.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PurposeReviewRequest {
    /// Indexed repository-relative path.
    pub(crate) path: String,
    /// Agent-reviewed purpose one-liner. Required for generated suggestions.
    #[serde(default)]
    pub(crate) purpose: Option<String>,
    /// Confirm the currently stored non-generated purpose after inspection.
    #[serde(default)]
    pub(crate) confirm_existing: bool,
    /// Queue task copied from the selected purpose-curation batch.
    #[serde(default)]
    pub(crate) task: Option<String>,
    /// Queue item work key copied without modification.
    #[serde(default)]
    pub(crate) work_key: Option<String>,
    /// Queue item state token copied without modification.
    #[serde(default)]
    pub(crate) state_token: Option<String>,
}

/// Batch purpose review result.
#[derive(Debug, Serialize)]
pub(crate) struct PurposeReviewReport {
    /// Whether the review changed the database.
    pub(crate) applied: bool,
    /// Number of requested review rows.
    pub(crate) total: usize,
    /// Number of rows changed when applied or that would change in dry-run.
    pub(crate) changed: usize,
    /// Number of rows skipped because they were already agent-reviewed with the same purpose.
    pub(crate) skipped: usize,
    /// Number of accepted, stale, or unavailable rows left unchanged.
    pub(crate) conflicts: usize,
    /// Number of rows that could not be reviewed.
    pub(crate) failed: usize,
    /// Per-path review details.
    pub(crate) items: Vec<PurposeReviewItem>,
}

/// Per-path batch review result.
#[derive(Debug, Serialize)]
pub(crate) struct PurposeReviewItem {
    /// Indexed repository-relative path.
    pub(crate) path: String,
    /// Action selected for this path.
    pub(crate) action: PurposeReviewAction,
    /// Current purpose lifecycle status.
    pub(crate) current_status: String,
    /// Current purpose source.
    pub(crate) current_source: String,
    /// Purpose that will be or was written.
    pub(crate) purpose: String,
    /// Validation or persistence error.
    pub(crate) error: String,
}

/// Stable purpose-review action values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PurposeReviewAction {
    /// The item failed validation.
    Error,
    /// The existing reviewed purpose already matches.
    Skip,
    /// The reviewed purpose was applied.
    Review,
    /// The reviewed purpose would be applied in preview mode.
    WouldReview,
    /// Project, generation, task, path, or row state changed after queue selection.
    Stale,
    /// The path now carries accepted authored intent and was not overwritten.
    Accepted,
    /// The selected path is no longer active in the index.
    Unavailable,
}

/// Validate and optionally apply a batch of agent-reviewed purpose records.
pub(crate) fn review_purposes(
    store: &AtlasStore,
    requests: &[PurposeReviewRequest],
    apply: bool,
) -> Result<PurposeReviewReport, CliError> {
    validate_purpose_review_admission(requests)?;
    let has_conditional_fields = requests.iter().any(has_conditional_purpose_review_field);
    if apply
        && has_conditional_fields
        && !requests.iter().all(is_complete_conditional_purpose_review)
    {
        return Err(CliError::InvalidInput(
            "an applied purpose review batch must be entirely conditional or entirely explicit correction; conditional rows require task, work_key, state_token, and a reviewed purpose"
                .to_string(),
        ));
    }

    // Explicit correction remains item-oriented. Preflight every row and the
    // complete report before the first write so an admission failure cannot
    // partially apply an otherwise valid batch.
    if apply && !has_conditional_fields {
        let preview = collect_purpose_reviews(store, requests, false)?;
        validate_purpose_review_report(&preview)?;
    }

    let report = if apply && has_conditional_fields {
        apply_conditional_purpose_reviews(store, requests)?
    } else {
        collect_purpose_reviews(store, requests, apply)?
    };
    validate_purpose_review_report(&report)?;
    Ok(report)
}

/// Enforce shared CLI/MCP purpose-review request limits before database work.
pub(crate) fn validate_purpose_review_admission(
    requests: &[PurposeReviewRequest],
) -> Result<(), CliError> {
    if requests.is_empty() {
        return Err(CliError::InvalidInput(
            "purpose review input must contain at least one item".to_string(),
        ));
    }
    if requests.len() > MAX_PURPOSE_CURATION_BATCH_ROWS {
        return Err(CliError::InvalidInput(format!(
            "purpose review input contains {} items; maximum is {}",
            requests.len(),
            MAX_PURPOSE_CURATION_BATCH_ROWS
        )));
    }

    let mut aggregate_bytes = 0usize;
    for (index, request) in requests.iter().enumerate() {
        validate_purpose_review_field(index, "path", &request.path, MAX_PURPOSE_REVIEW_PATH_BYTES)?;
        aggregate_bytes = aggregate_bytes
            .checked_add(request.path.len())
            .ok_or_else(|| purpose_review_input_too_large(usize::MAX))?;
        for (name, value) in [
            ("purpose", request.purpose.as_deref()),
            ("task", request.task.as_deref()),
            ("work_key", request.work_key.as_deref()),
            ("state_token", request.state_token.as_deref()),
        ] {
            if let Some(value) = value {
                validate_purpose_review_field(index, name, value, MAX_PURPOSE_REVIEW_FIELD_BYTES)?;
                aggregate_bytes = aggregate_bytes
                    .checked_add(value.len())
                    .ok_or_else(|| purpose_review_input_too_large(usize::MAX))?;
            }
        }
        if aggregate_bytes > MAX_PURPOSE_REVIEW_INPUT_BYTES {
            return Err(purpose_review_input_too_large(aggregate_bytes));
        }
    }
    Ok(())
}

/// Validate one caller-controlled purpose-review string before retaining output.
fn validate_purpose_review_field(
    index: usize,
    name: &str,
    value: &str,
    maximum: usize,
) -> Result<(), CliError> {
    if value.len() > maximum {
        return Err(CliError::InvalidInput(format!(
            "purpose review item {index} field {name} contains {} bytes; maximum is {maximum}",
            value.len()
        )));
    }
    Ok(())
}

/// Build the stable aggregate-byte admission failure.
fn purpose_review_input_too_large(actual: usize) -> CliError {
    CliError::InvalidInput(format!(
        "purpose review input contains {actual} aggregate string bytes; maximum is {MAX_PURPOSE_REVIEW_INPUT_BYTES}"
    ))
}

/// Review one admitted item-oriented batch while bounding retained report data.
fn collect_purpose_reviews(
    store: &AtlasStore,
    requests: &[PurposeReviewRequest],
    apply: bool,
) -> Result<PurposeReviewReport, CliError> {
    let mut items = Vec::with_capacity(requests.len());
    let mut retained_bytes = 0usize;
    for request in requests {
        let item = review_purpose_request(store, request, apply)?;
        retained_bytes = retained_bytes
            .checked_add(purpose_review_item_bytes(&item)?)
            .ok_or_else(|| purpose_review_report_too_large(usize::MAX))?;
        if retained_bytes > MAX_PURPOSE_REVIEW_REPORT_BYTES {
            return Err(purpose_review_report_too_large(retained_bytes));
        }
        items.push(item);
    }
    Ok(summarize_purpose_review(requests.len(), apply, items))
}

/// Return retained string bytes for one report row after per-field admission.
fn purpose_review_item_bytes(item: &PurposeReviewItem) -> Result<usize, CliError> {
    let mut total = 0usize;
    for (name, value, maximum) in [
        ("path", item.path.as_str(), MAX_PURPOSE_REVIEW_PATH_BYTES),
        (
            "current_status",
            item.current_status.as_str(),
            MAX_PURPOSE_REVIEW_FIELD_BYTES,
        ),
        (
            "current_source",
            item.current_source.as_str(),
            MAX_PURPOSE_REVIEW_FIELD_BYTES,
        ),
        (
            "purpose",
            item.purpose.as_str(),
            MAX_PURPOSE_REVIEW_FIELD_BYTES,
        ),
        (
            PURPOSE_REVIEW_REPORT_ERROR_FIELD,
            item.error.as_str(),
            MAX_PURPOSE_REVIEW_FIELD_BYTES,
        ),
    ] {
        if value.len() > maximum {
            return Err(CliError::InvalidInput(format!(
                "purpose review report field {name} contains {} bytes; maximum is {maximum}",
                value.len()
            )));
        }
        total = total
            .checked_add(value.len())
            .ok_or_else(|| purpose_review_report_too_large(usize::MAX))?;
    }
    Ok(total)
}

/// Enforce exact supported adapter output caps for one completed report.
fn validate_purpose_review_report(report: &PurposeReviewReport) -> Result<(), CliError> {
    let json_bytes = serde_json::to_string_pretty(report)?
        .len()
        .checked_add(1)
        .ok_or_else(|| purpose_review_report_too_large(usize::MAX))?;
    if json_bytes > MAX_PURPOSE_REVIEW_REPORT_BYTES {
        return Err(purpose_review_report_too_large(json_bytes));
    }
    let toon_bytes = render_purpose_review_report(report).len();
    if toon_bytes > MAX_PURPOSE_REVIEW_REPORT_BYTES {
        return Err(purpose_review_report_too_large(toon_bytes));
    }
    Ok(())
}

/// Build the stable purpose-review report/output limit failure.
fn purpose_review_report_too_large(actual: usize) -> CliError {
    CliError::InvalidInput(format!(
        "purpose review report contains {actual} bytes; maximum is {MAX_PURPOSE_REVIEW_REPORT_BYTES}"
    ))
}

/// Apply one host-selected conditional batch with one database writer transaction.
fn apply_conditional_purpose_reviews(
    store: &AtlasStore,
    requests: &[PurposeReviewRequest],
) -> Result<PurposeReviewReport, CliError> {
    let prepared = requests
        .iter()
        .map(|request| {
            let path = validated_repo_node_key(Path::new(&request.path)).map_err(|source| {
                CliError::InvalidInput(format!(
                    "invalid purpose review path {:?}: {source}",
                    request.path
                ))
            })?;
            let purpose = request
                .purpose
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    CliError::InvalidInput(
                        "conditional purpose review requires an explicit reviewed purpose"
                            .to_string(),
                    )
                })?
                .to_string();
            let task = request.task.clone().ok_or_else(|| {
                CliError::InvalidInput(
                    "conditional purpose review requires task, work_key, and state_token together"
                        .to_string(),
                )
            })?;
            let work_key = request.work_key.clone().ok_or_else(|| {
                CliError::InvalidInput(
                    "conditional purpose review requires task, work_key, and state_token together"
                        .to_string(),
                )
            })?;
            let state_token = request.state_token.clone().ok_or_else(|| {
                CliError::InvalidInput(
                    "conditional purpose review requires task, work_key, and state_token together"
                        .to_string(),
                )
            })?;
            Ok((
                path.clone(),
                purpose.clone(),
                PurposeConditionalApplyRequest {
                    task,
                    path,
                    work_key,
                    state_token,
                    purpose,
                },
            ))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    let database_requests = prepared
        .iter()
        .map(|(_, _, request)| request.clone())
        .collect::<Vec<_>>();
    let results = store.conditionally_set_purposes(&database_requests)?;
    let items = prepared
        .into_iter()
        .zip(results)
        .map(|((path, purpose, _), result)| {
            debug_assert_eq!(path, result.path);
            PurposeReviewItem {
                path,
                action: conditional_purpose_review_action(result.state, true),
                current_status: result
                    .current_purpose
                    .as_ref()
                    .map(|purpose| purpose.status.to_string())
                    .unwrap_or_default(),
                current_source: result
                    .current_purpose
                    .as_ref()
                    .map(|purpose| purpose.source.to_string())
                    .unwrap_or_default(),
                purpose,
                error: String::new(),
            }
        })
        .collect::<Vec<_>>();
    Ok(summarize_purpose_review(requests.len(), true, items))
}

/// Return whether a row carries one complete queue-bound conditional review.
fn is_complete_conditional_purpose_review(request: &PurposeReviewRequest) -> bool {
    request.task.is_some()
        && request.work_key.is_some()
        && request.state_token.is_some()
        && request
            .purpose
            .as_deref()
            .is_some_and(|purpose| !purpose.trim().is_empty())
}

/// Return whether a row attempts to use queue-bound conditional review.
fn has_conditional_purpose_review_field(request: &PurposeReviewRequest) -> bool {
    request.task.is_some() || request.work_key.is_some() || request.state_token.is_some()
}

/// Aggregate stable batch counters from per-path review outcomes.
fn summarize_purpose_review(
    total: usize,
    applied: bool,
    items: Vec<PurposeReviewItem>,
) -> PurposeReviewReport {
    let changed = items
        .iter()
        .filter(|item| {
            matches!(
                item.action,
                PurposeReviewAction::Review | PurposeReviewAction::WouldReview
            )
        })
        .count();
    let skipped = items
        .iter()
        .filter(|item| {
            matches!(
                item.action,
                PurposeReviewAction::Skip | PurposeReviewAction::Accepted
            )
        })
        .count();
    let conflicts = items
        .iter()
        .filter(|item| {
            matches!(
                item.action,
                PurposeReviewAction::Stale
                    | PurposeReviewAction::Accepted
                    | PurposeReviewAction::Unavailable
            )
        })
        .count();
    let failed = items.iter().filter(|item| !item.error.is_empty()).count();
    PurposeReviewReport {
        applied,
        total,
        changed,
        skipped,
        conflicts,
        failed,
        items,
    }
}

/// Validate and optionally apply one agent-reviewed purpose record.
fn review_purpose_request(
    store: &AtlasStore,
    request: &PurposeReviewRequest,
    apply: bool,
) -> Result<PurposeReviewItem, CliError> {
    let path = validated_repo_node_key(Path::new(&request.path)).map_err(|source| {
        CliError::InvalidInput(format!(
            "invalid purpose review path {:?}: {source}",
            request.path
        ))
    })?;
    let conditional = match (
        request.task.as_deref(),
        request.work_key.as_deref(),
        request.state_token.as_deref(),
    ) {
        (Some(task), Some(work_key), Some(state_token)) => Some((task, work_key, state_token)),
        (None, None, None) => None,
        _ => {
            return Ok(PurposeReviewItem {
                path,
                action: PurposeReviewAction::Error,
                current_status: String::new(),
                current_source: String::new(),
                purpose: request.purpose.clone().unwrap_or_default(),
                error:
                    "conditional purpose review requires task, work_key, and state_token together"
                        .to_string(),
            });
        }
    };
    if let Some((task, work_key, state_token)) = conditional {
        let reviewed_purpose = request
            .purpose
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(reviewed_purpose) = reviewed_purpose else {
            return Ok(PurposeReviewItem {
                path,
                action: PurposeReviewAction::Error,
                current_status: String::new(),
                current_source: String::new(),
                purpose: String::new(),
                error: "conditional purpose review requires an explicit reviewed purpose"
                    .to_string(),
            });
        };
        let state = if apply {
            store.conditionally_set_purpose(task, &path, work_key, state_token, reviewed_purpose)?
        } else {
            preview_conditional_purpose_review(store, task, &path, work_key, state_token)?
        };
        let current = store.load_node_by_path(&path)?;
        return Ok(PurposeReviewItem {
            path,
            action: conditional_purpose_review_action(state, apply),
            current_status: current
                .as_ref()
                .map(|node| node.purpose.status.to_string())
                .unwrap_or_default(),
            current_source: current
                .as_ref()
                .map(|node| node.purpose.source.to_string())
                .unwrap_or_default(),
            purpose: reviewed_purpose.to_string(),
            error: String::new(),
        });
    }
    let Some(indexed) = store.load_node_by_path(&path)? else {
        return Ok(PurposeReviewItem {
            path,
            action: PurposeReviewAction::Error,
            current_status: String::new(),
            current_source: String::new(),
            purpose: request.purpose.clone().unwrap_or_default(),
            error: "path is not indexed".to_string(),
        });
    };
    let current_status = indexed.purpose.status.to_string();
    let current_source = indexed.purpose.source.to_string();
    let current_purpose = indexed.purpose.purpose.clone().unwrap_or_default();
    let explicit_purpose = request
        .purpose
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(reviewed_purpose) = explicit_purpose.or_else(|| {
        request
            .confirm_existing
            .then_some(current_purpose.as_str())
            .filter(|value| !value.trim().is_empty())
    }) else {
        return Ok(PurposeReviewItem {
            path,
            action: PurposeReviewAction::Error,
            current_status,
            current_source,
            purpose: String::new(),
            error: "provide a reviewed purpose or set confirm_existing=true".to_string(),
        });
    };

    if request.confirm_existing
        && explicit_purpose.is_none()
        && (indexed.purpose.status == PurposeStatus::Suggested
            || indexed.purpose.source == PurposeSource::Generated)
    {
        return Ok(PurposeReviewItem {
            path,
            action: PurposeReviewAction::Error,
            current_status,
            current_source,
            purpose: current_purpose,
            error: "generated suggestions require an explicit reviewed purpose".to_string(),
        });
    }

    let reviewed_purpose = reviewed_purpose.trim().to_string();
    let action = if indexed.purpose.agent_reviewed() && current_purpose == reviewed_purpose {
        PurposeReviewAction::Skip
    } else if apply {
        store.set_purpose(&path, &reviewed_purpose, PurposeSource::Agent)?;
        PurposeReviewAction::Review
    } else {
        PurposeReviewAction::WouldReview
    };
    Ok(PurposeReviewItem {
        path,
        action,
        current_status,
        current_source,
        purpose: reviewed_purpose,
        error: String::new(),
    })
}

/// Preview one conditional review against the current queue state without writing.
fn preview_conditional_purpose_review(
    store: &AtlasStore,
    task: &str,
    path: &str,
    work_key: &str,
    state_token: &str,
) -> Result<PurposeConditionalApplyState, CliError> {
    let batch = store.load_purpose_curation_batch(task, &[path.to_string()])?;
    if let Some(candidate) = batch.items.first() {
        return Ok(
            if candidate.work_key == work_key && candidate.state_token == state_token {
                PurposeConditionalApplyState::Applied
            } else {
                PurposeConditionalApplyState::Stale
            },
        );
    }
    Ok(store
        .load_node_by_path(path)?
        .map_or(PurposeConditionalApplyState::PathUnavailable, |_node| {
            PurposeConditionalApplyState::Accepted
        }))
}

/// Map database conditional-apply state into the stable review action contract.
const fn conditional_purpose_review_action(
    state: PurposeConditionalApplyState,
    apply: bool,
) -> PurposeReviewAction {
    match state {
        PurposeConditionalApplyState::Applied if apply => PurposeReviewAction::Review,
        PurposeConditionalApplyState::Applied => PurposeReviewAction::WouldReview,
        PurposeConditionalApplyState::Stale => PurposeReviewAction::Stale,
        PurposeConditionalApplyState::Accepted => PurposeReviewAction::Accepted,
        PurposeConditionalApplyState::PathUnavailable => PurposeReviewAction::Unavailable,
    }
}

/// Build a purpose curation queue from the bounded health page.
pub(crate) fn purpose_curation_page(
    store: &AtlasStore,
    query: &HealthQuery,
    task: &str,
) -> Result<PurposeCurationPage, CliError> {
    let page = store.purpose_curation_findings_page_current(query)?;
    let paths = page
        .findings
        .iter()
        .map(|finding| finding.path.clone())
        .collect::<Vec<_>>();
    let batch = store.load_purpose_curation_batch(task, &paths)?;
    let project_instance_id = batch.project_instance_id.to_string();
    let active_generation = batch.active_generation.get();
    let task = batch.task.clone();
    let work_key = batch.work_key.clone();
    let candidates = batch
        .items
        .into_iter()
        .map(|candidate| (candidate.node.node.path.clone(), candidate))
        .collect::<HashMap<_, _>>();
    let items = page
        .findings
        .iter()
        .filter_map(|finding| {
            let candidate = candidates.get(&finding.path)?;
            let node = &candidate.node;
            let review_signal = purpose_review_signal(&node.node, &node.purpose);
            Some(PurposeCurationItem {
                work_key: candidate.work_key.clone(),
                state_token: candidate.state_token.clone(),
                severity: health_severity_name(finding.severity).to_string(),
                id: finding.id.clone(),
                category: finding.category.clone(),
                path: finding.path.clone(),
                related_path: finding.related_path.clone().unwrap_or_default(),
                kind: node.node.kind.to_string(),
                language: node.node.language.clone().unwrap_or_default(),
                purpose: node.purpose.purpose.clone().unwrap_or_default(),
                purpose_status: node.purpose.status.to_string(),
                purpose_source: node.purpose.source.to_string(),
                purpose_agent_reviewed: node.purpose.agent_reviewed(),
                review_priority: review_signal.priority.to_string(),
                review_reason: review_signal.reason.to_string(),
                content_summary: node.summary.clone().unwrap_or_default(),
                recommendation: "Inspect bounded context, then use conditional purpose review with this task, work_key, and state_token."
                    .to_string(),
            })
        })
        .collect::<Vec<_>>();
    let returned = items.len();
    Ok(PurposeCurationPage {
        project_instance_id,
        active_generation,
        task,
        work_key,
        actionable: returned > 0,
        curation_scope: purpose_queue_curation_scope(query),
        total: page.total,
        unfiltered_total: page.unfiltered_total,
        returned,
        start_index: page.start_index,
        limit: page.limit,
        max_limit: MAX_HEALTH_LIMIT,
        next_start_index: health_next_start_index(&page),
        truncated: health_next_start_index(&page).is_some(),
        source_only: query.scope.is_source_focused(),
        folder_scope: purpose_queue_folder_scope(query).to_string(),
        file_scope: purpose_queue_file_scope(query).to_string(),
        category: query.category.clone().unwrap_or_default(),
        severity: query.severity.map_or("", health_severity_name).to_string(),
        path_prefix: query.path_prefix.clone().unwrap_or_default(),
        summary_only: query.summary_only,
        items,
    })
}

/// Render a bounded health page as compact TOON.
pub(crate) fn render_health_page(page: &HealthFindingsPage, query: &HealthQuery) -> String {
    let rows = page
        .findings
        .iter()
        .map(|finding| {
            json!({
                "severity": health_severity_name(finding.severity),
                "id": finding.id,
                "category": finding.category,
                "path": finding.path,
                "related_path": finding.related_path.as_deref().unwrap_or(""),
                "message": finding.message,
                "recommendation": finding.recommendation,
            })
        })
        .collect::<Vec<_>>();
    encode_agent_payload(&json!({
        "health": {
            "total": page.total,
            "unfiltered_total": page.unfiltered_total,
            "returned": page.returned,
            "start_index": page.start_index,
            "limit": page.limit,
            "max_limit": MAX_HEALTH_LIMIT,
            "next_start_index": health_next_start_index(page),
            "truncated": health_next_start_index(page).is_some(),
            "summary_only": query.summary_only,
            "source_only": query.scope.is_source_focused(),
            "category": query.category.as_deref().unwrap_or(""),
            "severity": query.severity.map_or("", health_severity_name),
            "path_prefix": query.path_prefix.as_deref().unwrap_or(""),
        },
        "health_findings": rows,
    }))
}

/// Render one bounded current coverage page as compact TOON.
pub(crate) fn render_coverage_report(report: &CoverageDiscoveryReport) -> String {
    encode_agent_payload(&json!({ "coverage": report }))
}

/// Render a purpose curation queue as compact TOON.
pub(crate) fn render_purpose_curation_page(page: &PurposeCurationPage) -> String {
    encode_agent_payload(&json!({
        "purpose_curation": {
            "project_instance_id": page.project_instance_id,
            "active_generation": page.active_generation,
            "task": page.task,
            "work_key": page.work_key,
            "actionable": page.actionable,
            "curation_scope": page.curation_scope,
            "total": page.total,
            "unfiltered_total": page.unfiltered_total,
            "returned": page.returned,
            "start_index": page.start_index,
            "limit": page.limit,
            "max_limit": page.max_limit,
            "next_start_index": page.next_start_index,
            "truncated": page.truncated,
            "source_only": page.source_only,
            "folder_scope": page.folder_scope,
            "file_scope": page.file_scope,
            "category": page.category,
            "severity": page.severity,
            "path_prefix": page.path_prefix,
            "summary_only": page.summary_only,
        },
        "purpose_curation_items": page.items,
    }))
}

/// Render a batch purpose review report as compact TOON.
pub(crate) fn render_purpose_review_report(report: &PurposeReviewReport) -> String {
    encode_agent_payload(&json!({
        "purpose_review": {
            "applied": report.applied,
            "total": report.total,
            "changed": report.changed,
            "skipped": report.skipped,
            "conflicts": report.conflicts,
            "failed": report.failed,
        },
        "purpose_review_items": report.items,
    }))
}

/// Return the folder inclusion scope for purpose curation metadata.
fn purpose_queue_folder_scope(query: &HealthQuery) -> &'static str {
    match query.scope {
        HealthScope::SourceOnly | HealthScope::PurposeWithSourceFiles => "source_relevant",
        _ => "all",
    }
}

/// Return the file inclusion scope for purpose curation metadata.
fn purpose_queue_file_scope(query: &HealthQuery) -> &'static str {
    match query.scope {
        HealthScope::PurposeDefault => "high_impact",
        HealthScope::PurposeWithAssets => "high_impact_and_assets",
        HealthScope::SourceOnly | HealthScope::PurposeWithSourceFiles => "all_source",
        HealthScope::All | HealthScope::PurposeStrict => "all",
    }
}

/// Return the truthful curation tier selected by explicit queue scope flags.
fn purpose_queue_curation_scope(query: &HealthQuery) -> &'static str {
    match query.scope {
        HealthScope::PurposeDefault => "low",
        HealthScope::PurposeWithAssets => "low_with_assets",
        HealthScope::SourceOnly | HealthScope::PurposeWithSourceFiles => "medium",
        HealthScope::All | HealthScope::PurposeStrict => "strict",
    }
}

/// Return a stable lowercase severity name.
pub(crate) fn health_severity_name(severity: Severity) -> &'static str {
    severity.as_str()
}

/// Return the next start index for a bounded health page.
fn health_next_start_index(page: &HealthFindingsPage) -> Option<usize> {
    let page_width = page.limit.min(page.total.saturating_sub(page.start_index));
    let page_end = page.start_index.saturating_add(page_width);
    if page_width == 0 || page_end >= page.total {
        None
    } else {
        Some(page_end)
    }
}

/// Estimate source tokens for repository paths referenced by symbols/relations.
pub(crate) fn estimated_source_tokens_for_paths<'a>(
    store: &AtlasStore,
    paths: impl Iterator<Item = &'a str>,
) -> Result<usize, CliError> {
    let mut seen = HashSet::new();
    let mut total = 0usize;
    for path in paths {
        if seen.insert(path.to_string()) {
            total = total.saturating_add(estimated_source_tokens_for_path(store, path)?);
        }
    }
    Ok(total)
}

/// Estimate source tokens for one indexed path, falling back safely for stale rows.
pub(crate) fn estimated_source_tokens_for_path(
    store: &AtlasStore,
    path: &str,
) -> Result<usize, CliError> {
    if let Some(indexed) = store.load_node_by_path(path)?
        && indexed.node.kind == NodeKind::File
    {
        return Ok(estimated_source_tokens_for_file_node(&indexed.node));
    }
    Ok(read_indexed_file_content(store, path).map_or_else(
        |_| estimate_tokens(path),
        |content| estimate_tokens(&content),
    ))
}

/// Persisted file-text index report.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TextIndexReport {
    /// File nodes considered for indexed text.
    pub(crate) candidates: usize,
    /// UTF-8 files persisted for `SQLite`-backed search.
    pub(crate) indexed: usize,
    /// Files skipped because text could not be decoded as UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Files skipped because they exceeded the configured text-index size cap.
    pub(crate) too_large: usize,
    /// Total files skipped from the persisted text index.
    pub(crate) skipped: usize,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) max_bytes: u64,
    /// Source bytes stored in the text index.
    pub(crate) bytes: usize,
}

/// Deterministic structural-summary refresh report.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StructuralSummaryReport {
    /// Indexed files considered for structural summaries.
    pub(crate) candidates: usize,
    /// Files whose observed summaries were refreshed.
    pub(crate) summarized: usize,
    /// Existing observed summaries cleared because current content was not summarizable.
    pub(crate) cleared: usize,
    /// Files skipped because they exceeded the parser size limit.
    pub(crate) too_large: usize,
    /// Files skipped because content was not valid UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Generated purpose suggestions that still need agent review.
    pub(crate) purpose_suggestions: usize,
}

/// Options controlling full-text persistence for `SQLite` search.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextIndexOptions {
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) max_bytes: u64,
}

impl TextIndexOptions {
    /// Create text-index options from config and command overrides.
    pub(crate) fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }
}

/// Outcome of considering one file for persisted text search.
#[derive(Clone, Debug)]
pub(crate) struct TextIndexRow {
    /// Repository-relative path considered for text indexing.
    path: String,
    /// Persistable text row when the file is search-indexed.
    text: Option<IndexedFileText>,
    /// Indexing outcome for reporting.
    reason: TextIndexSkipReason,
}

/// Persisted text refresh result plus rows reused by structural summarizers.
pub(crate) struct TextIndexRefresh {
    /// Aggregate report rendered to callers.
    pub(crate) report: TextIndexReport,
    /// Per-file text outcomes from the same scan batch.
    pub(crate) rows: Vec<TextIndexRow>,
}

/// Text-index outcome categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextIndexSkipReason {
    /// File text was persisted for search.
    Indexed,
    /// File exceeded the configured text-index size cap.
    TooLarge,
    /// File was binary or not valid UTF-8.
    BinaryOrNonUtf8,
}

/// Symbol graph build report.
#[derive(Debug, Serialize)]
pub(crate) struct SymbolBuildReport {
    /// Indexed file candidates considered for symbols.
    pub(crate) candidates: usize,
    /// Files parsed during this build.
    pub(crate) parsed: usize,
    /// Files skipped because they were unchanged and already had symbols.
    pub(crate) unchanged: usize,
    /// Files skipped because they exceeded the configured size limit.
    pub(crate) too_large: usize,
    /// Files skipped because content was not valid UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Files skipped because the build deadline was reached.
    pub(crate) timed_out: usize,
    /// Worker thread count used for parser work.
    pub(crate) max_workers: usize,
    /// Optional timeout seconds requested for parser work.
    pub(crate) timeout_seconds: Option<u64>,
    /// Symbols persisted.
    pub(crate) symbols: usize,
    /// Relations persisted.
    pub(crate) relations: usize,
    /// Node summaries refreshed from symbol graphs.
    pub(crate) summaries: usize,
    /// Generated purpose suggestions that still need agent review.
    pub(crate) purpose_suggestions: usize,
}

/// Filesystem and derived facts prepared before acquiring the `SQLite` writer.
struct IndexPublicationBatch {
    /// Complete publication generation observed before source preparation.
    base_generation: IndexGeneration,
    /// Derivation contract bound to every staged projection.
    contract_fingerprint: String,
    /// Canonical selected source root.
    root: PathBuf,
    /// Full or affected-path node mutation plus the final expected source state.
    nodes: NodePublicationBatch,
    /// Optional legacy-purpose inputs consumed by a full scan.
    purpose_import: Option<PurposeImportSnapshot>,
    /// Repository paths whose persisted source text must be replaced.
    text_paths: Vec<String>,
    /// Prepared persisted source-text rows and report.
    text: TextIndexRefresh,
    /// Prepared symbol graph, summary, and suggestion mutations.
    symbols: SymbolBuildStage,
    /// Prepared normalized repository graph and canonical resolution-key mutation.
    graph: graph_projection::StagedRepositoryGraph,
    /// Prepared structural-summary and suggestion mutations.
    structural_summaries: StructuralSummaryStage,
}

/// Node mutations owned by one full or incremental publication.
enum NodePublicationBatch {
    /// Replace the complete observed source tree.
    Full {
        /// Complete staged node set.
        nodes: Vec<Node>,
    },
    /// Apply a bounded changed-path delta.
    Incremental {
        /// Added or modified nodes.
        nodes: Vec<Node>,
        /// Deleted paths and descendants to mark absent.
        absent_paths: Vec<String>,
        /// Complete expected source state used only for pre-publication revalidation.
        expected_nodes: Vec<Node>,
    },
}

impl NodePublicationBatch {
    /// Return the complete source state that must still exist before publication.
    fn expected_nodes(&self) -> &[Node] {
        match self {
            Self::Full { nodes } => nodes,
            Self::Incremental { expected_nodes, .. } => expected_nodes,
        }
    }
}

/// Reports produced after a staged batch commits successfully.
struct IndexPublicationOutcome {
    /// Legacy-purpose import decisions made against current authored state.
    purpose_import: PurposeImportReport,
    /// Persisted source-text report.
    text_index: TextIndexReport,
    /// Deterministic structural-summary report.
    structural_summaries: StructuralSummaryReport,
    /// Deep symbol graph report.
    symbols: SymbolBuildReport,
}

/// Symbol mutations retained outside the `SQLite` writer transaction.
struct SymbolBuildStage {
    /// Aggregate symbol build report.
    report: SymbolBuildReport,
    /// Deterministically ordered projection changes.
    changes: Vec<SymbolProjectionChange>,
    /// Retained parser-output string bytes admitted by the resource boundary.
    retained_bytes: u64,
}

/// One closed symbol projection mutation.
enum SymbolProjectionChange {
    /// Persist one successfully parsed graph and its derived metadata.
    Parsed(SymbolParseSuccess),
    /// Clear stale symbol output for a skipped source file.
    Clear {
        /// Repository-relative path.
        path: String,
        /// Detected language used to preserve structural summaries where applicable.
        language: Option<String>,
    },
}

/// Structural summary mutations retained outside the `SQLite` writer transaction.
struct StructuralSummaryStage {
    /// Aggregate structural-summary report.
    report: StructuralSummaryReport,
    /// Deterministically ordered summary changes.
    changes: Vec<StructuralSummaryChange>,
    /// Retained summary and suggestion string bytes.
    retained_bytes: u64,
}

/// One file's closed structural-summary derivation.
#[derive(Default)]
struct StructuralSummaryDerivation {
    /// Optional persistence mutation for this file.
    change: Option<StructuralSummaryChange>,
    /// Observed summaries derived or reused.
    summarized: usize,
    /// Existing observed summaries cleared.
    cleared: usize,
    /// Files cleared because they exceeded the parser limit.
    too_large: usize,
    /// Files cleared because their content was not valid text.
    binary_or_non_utf8: usize,
    /// Unapproved purpose suggestions derived from observed summaries.
    purpose_suggestions: usize,
    /// String bytes retained until publication.
    retained_bytes: u64,
}

/// One closed structural-summary projection mutation.
enum StructuralSummaryChange {
    /// Replace one observed summary and optional unreviewed purpose suggestion.
    Set {
        /// Repository-relative path.
        path: String,
        /// Deterministic observed summary.
        summary: String,
        /// Optional generated purpose suggestion.
        purpose_suggestion: Option<String>,
    },
    /// Clear a stale observed summary.
    Clear {
        /// Repository-relative path.
        path: String,
    },
}

/// Watch command report.
#[derive(Debug, Serialize)]
pub(crate) struct WatchReport {
    /// Watcher mode.
    pub(crate) mode: String,
    /// Completed refresh cycles.
    pub(crate) cycles: usize,
    /// Whether the command ran a single refresh and exited.
    pub(crate) once: bool,
    /// Reason the watcher fell back from event mode, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_reason: Option<String>,
    /// Last persisted text search index report.
    pub(crate) text_index: TextIndexReport,
    /// Last structural summary refresh report.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Last symbol refresh report.
    pub(crate) last_symbols: SymbolBuildReport,
}

/// Debounced filesystem changes observed by watcher mode.
#[derive(Debug, Default)]
pub(crate) struct WatchChangeSet {
    /// Whether a full scan is required for correctness.
    requires_full_scan: bool,
    /// Relevant native paths from event batches.
    paths: HashSet<PathBuf>,
}

impl WatchChangeSet {
    /// Return whether there is work to refresh.
    fn has_changes(&self) -> bool {
        self.requires_full_scan || !self.paths.is_empty()
    }

    /// Merge another event batch into this set.
    fn merge(&mut self, other: Self) {
        self.requires_full_scan |= other.requires_full_scan;
        self.paths.extend(other.paths);
    }
}

/// Legacy purpose cleanup report.
#[derive(Debug, Serialize)]
pub(crate) struct LegacyPurposeReport {
    /// Whether files were modified.
    pub(crate) applied: bool,
    /// Number of `.purpose` files found.
    pub(crate) purpose_files_found: usize,
    /// Number of `.purpose` files removed.
    pub(crate) purpose_files_removed: usize,
    /// Source header candidates found.
    pub(crate) source_header_candidates: Vec<String>,
    /// Legacy purpose file paths.
    pub(crate) purpose_files: Vec<String>,
}

/// Local settings report.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsReport {
    /// Runtime cache directory that owns local `ProjectAtlas` state.
    pub(crate) cache_dir: PathStatus,
    /// `SQLite` database file status.
    pub(crate) db: PathStatus,
    /// `SQLite` write-ahead log file status.
    pub(crate) db_wal: PathStatus,
    /// `SQLite` shared-memory sidecar file status.
    pub(crate) db_shm: PathStatus,
    /// `SQLite` rollback journal sidecar file status.
    pub(crate) db_journal: PathStatus,
    /// Project-local MCP configuration file status.
    pub(crate) mcp_config: PathStatus,
    /// Config file used for map/lint/scan imports, when discovered.
    pub(crate) config_path: Option<String>,
    /// Repository root used by map/lint config.
    pub(crate) repo_root: String,
    /// Source that selected the repository root.
    pub(crate) root_detection_source: String,
    /// Whether config and DB root metadata agree.
    pub(crate) root_verified: bool,
    /// Root mismatches that should be fixed before trusting the binding.
    pub(crate) root_mismatches: Vec<String>,
    /// Generated map path.
    pub(crate) map_path: String,
    /// Non-source summary path.
    pub(crate) nonsource_files_path: String,
    /// Default output format.
    pub(crate) default_format: String,
    /// Default search case sensitivity.
    pub(crate) default_search_case_sensitive: bool,
    /// Source used by search commands.
    pub(crate) search_source: String,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) text_index_max_bytes: u64,
    /// Watcher runtime status.
    pub(crate) watcher: WatchStatusReport,
    /// Current index statistics, if the index exists.
    pub(crate) index: Option<SettingsIndexStats>,
    /// Content-free telemetry retention and maintenance state, when the index exists.
    pub(crate) telemetry: Option<TelemetryRetentionState>,
    /// Read-only schema, publication, coverage, and `SQLite` operating diagnostics.
    pub(crate) database: DatabaseSettingsReport,
    /// Content-free language capability registry identity and derived counts.
    pub(crate) language_registry: LanguageRegistryReport,
    /// Digest of the currently implemented provider-backed relation contract.
    pub(crate) semantic_relation_contract_digest: String,
    /// Versioned accepted relation-family inventory and lifecycle state.
    pub(crate) relation_family_inventory: RelationFamilyInventoryReport,
    /// Typed search-mode readiness without an implicit index build.
    pub(crate) search: SettingsSearchReport,
    /// Content-free optional parser-pack lifecycle state.
    pub(crate) optional_parser_pack: OptionalParserSettingsReport,
}

/// Readiness of one settings capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SettingsCapabilityState {
    /// The capability has usable persisted state.
    Ready,
    /// The capability is implemented but the selected index has no rows yet.
    Empty,
    /// The capability is not available in the current runtime/index combination.
    Unavailable,
}

/// Typed settings projection for one search mode.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsSearchModeReport {
    /// Current readiness.
    pub(crate) state: SettingsCapabilityState,
}

/// Typed lexical-search settings projection.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsLexicalSearchReport {
    /// Current readiness.
    pub(crate) state: SettingsCapabilityState,
    /// Authoritative source searched by the correctness path.
    pub(crate) source: &'static str,
    /// Whether returned candidates are verified against persisted exact text.
    pub(crate) exact_verification: bool,
}

/// Content-free readiness for supported and planned search modes.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsSearchReport {
    /// Compatible default when no explicit mode is supplied.
    pub(crate) default_mode: &'static str,
    /// Deterministic persisted-text search.
    pub(crate) lexical: SettingsLexicalSearchReport,
    /// Optional FTS candidate acceleration.
    pub(crate) fts: SettingsSearchModeReport,
    /// Optional semantic retrieval.
    pub(crate) semantic: SettingsSearchModeReport,
    /// Optional lexical-complete hybrid retrieval.
    pub(crate) hybrid: SettingsSearchModeReport,
}

/// Optional parser state present in both feature-enabled and default-core-only builds.
#[derive(Debug, Serialize)]
pub(crate) struct OptionalParserSettingsReport {
    /// Whether the supervised optional-parser lifecycle is compiled into this binary.
    pub(crate) compiled: bool,
    /// Bounded lifecycle metadata when the supervisor feature is present.
    #[cfg(feature = "optional-parser-supervisor")]
    pub(crate) lifecycle: OptionalParserPackLifecycleReport,
    /// Honest state for a binary compiled without the optional lifecycle.
    #[cfg(not(feature = "optional-parser-supervisor"))]
    pub(crate) state: &'static str,
}

/// Filesystem status for a diagnostic path.
#[derive(Debug, Serialize)]
pub(crate) struct PathStatus {
    /// Normalized native path.
    pub(crate) path: String,
    /// Whether the path exists.
    pub(crate) exists: bool,
    /// File size in bytes when the path is an existing file.
    pub(crate) size_bytes: Option<u64>,
}

/// Indexed state summary for settings diagnostics.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsIndexStats {
    /// Canonical project root stored in the index metadata.
    pub(crate) project_root: Option<String>,
    /// Indexed file count.
    pub(crate) files: usize,
    /// Indexed folder count.
    pub(crate) folders: usize,
    /// Missing purpose count.
    pub(crate) missing_purposes: usize,
    /// Stale purpose count.
    pub(crate) stale_purposes: usize,
    /// Suggested purpose count.
    pub(crate) suggested_purposes: usize,
    /// Persisted searchable text rows.
    pub(crate) indexed_text_files: usize,
    /// Persisted searchable text bytes.
    pub(crate) indexed_text_bytes: usize,
    /// Persisted symbol count.
    pub(crate) symbols: usize,
    /// Persisted symbol relation count.
    pub(crate) relations: usize,
    /// Token telemetry event count.
    pub(crate) token_calls: usize,
    /// Unresolved structural health finding count.
    pub(crate) health_findings: usize,
}

/// Watcher status report.
#[derive(Debug, Serialize)]
pub(crate) struct WatchStatusReport {
    /// Whether a watcher implementation is available in this binary.
    pub(crate) available: bool,
    /// Whether a watcher is active.
    pub(crate) active: bool,
    /// Watcher mode.
    pub(crate) mode: String,
    /// Whether event-backed watching is available.
    pub(crate) event_backend_available: bool,
    /// Operational recommendation.
    pub(crate) recommendation: String,
}

/// Runtime index/cache cleanup report.
#[derive(Debug, Serialize)]
pub(crate) struct ResetIndexReport {
    /// Whether files were modified.
    pub(crate) applied: bool,
    /// Whether the command only previewed paths.
    pub(crate) dry_run: bool,
    /// Runtime files selected for cleanup.
    files: Vec<PathStatus>,
    /// Number of selected files removed.
    pub(crate) removed: usize,
}

/// Build settings diagnostics shared by CLI and MCP.
pub(crate) fn build_settings_report(
    db: &Path,
    config_path: Option<&Path>,
    format: OutputFormat,
) -> Result<SettingsReport, CliError> {
    let absolute_db = absolute_path(db)?;
    let resolved_config = resolved_mcp_config_path(&absolute_db, config_path)?;
    let config = if let Some(config_path) = resolved_config.as_deref() {
        load_atlas_config(Some(config_path))?
    } else {
        let project_root = default_mcp_project_root(&absolute_db, None)?;
        load_atlas_config_for_root(&project_root)?
    };
    let cache_dir = absolute_db
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let database = database_settings_report(&absolute_db)?;
    let (index, telemetry, file_text_fts) =
        if database.schema.compatibility == DatabaseSchemaCompatibility::Current {
            let store = AtlasStore::open_read_only(&absolute_db)?;
            let snapshot_publication = store.index_publication()?;
            if settings_publication_matches(
                database.publication.as_ref(),
                snapshot_publication.as_ref(),
            ) {
                (
                    Some(settings_index_stats(&store)?),
                    Some(store.telemetry_retention_state()?),
                    Some(store.file_text_fts_state()?),
                )
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };
    let repo_root = normalize_display_path(&config.root);
    let db_project_root = index
        .as_ref()
        .and_then(|stats| stats.project_root.as_ref())
        .cloned();
    let mut root_mismatches = Vec::new();
    if let Some(db_root) = db_project_root.as_ref()
        && db_root != &repo_root
    {
        root_mismatches.push(format!(
            "db root {db_root:?} does not match config root {repo_root:?}"
        ));
    }
    let root_detection_source = if resolved_config.is_some() {
        "config"
    } else if db_project_root.is_some() {
        "db"
    } else {
        "db-path-or-cwd"
    }
    .to_string();
    let lexical_publication_ready = database.publication.as_ref().is_some_and(|publication| {
        publication.state == IndexPublicationState::Complete
            && publication.generation != IndexGeneration::ZERO
            && publication.contract_fingerprint_state == DatabasePublicationContractState::Valid
    });
    let lexical_state = match (lexical_publication_ready, index.as_ref()) {
        (true, Some(stats)) if stats.indexed_text_files == 0 => SettingsCapabilityState::Empty,
        (true, Some(_)) => SettingsCapabilityState::Ready,
        _ => SettingsCapabilityState::Unavailable,
    };
    let fts_state = match (lexical_publication_ready, file_text_fts.as_ref()) {
        (true, Some(state)) if !state.synchronized => SettingsCapabilityState::Unavailable,
        (true, Some(state)) if state.source_rows == 0 => SettingsCapabilityState::Empty,
        (true, Some(_)) => SettingsCapabilityState::Ready,
        _ => SettingsCapabilityState::Unavailable,
    };
    let search = SettingsSearchReport {
        default_mode: "lexical",
        lexical: SettingsLexicalSearchReport {
            state: lexical_state,
            source: "persisted_text",
            exact_verification: true,
        },
        fts: SettingsSearchModeReport { state: fts_state },
        semantic: SettingsSearchModeReport {
            state: SettingsCapabilityState::Unavailable,
        },
        hybrid: SettingsSearchModeReport {
            state: SettingsCapabilityState::Unavailable,
        },
    };
    #[cfg(feature = "optional-parser-supervisor")]
    let optional_parser_pack = OptionalParserSettingsReport {
        compiled: true,
        lifecycle: OptionalParserPackLifecycle::new(&config.root, None)?.status()?,
    };
    #[cfg(not(feature = "optional-parser-supervisor"))]
    let optional_parser_pack = OptionalParserSettingsReport {
        compiled: false,
        state: "compiled_unavailable",
    };
    Ok(SettingsReport {
        cache_dir: path_status(&cache_dir)?,
        db: path_status(&absolute_db)?,
        db_wal: path_status(&db_sidecar_path(&absolute_db, "wal"))?,
        db_shm: path_status(&db_sidecar_path(&absolute_db, "shm"))?,
        db_journal: path_status(&db_sidecar_path(&absolute_db, "journal"))?,
        mcp_config: path_status(&mcp_config_path_for_db(&absolute_db))?,
        config_path: resolved_config.map(|path| normalize_display_path(&path)),
        repo_root,
        root_detection_source,
        root_verified: root_mismatches.is_empty(),
        root_mismatches,
        map_path: normalize_display_path(&config.map_path),
        nonsource_files_path: normalize_display_path(&config.nonsource_files_path),
        default_format: format!("{format:?}").to_ascii_lowercase(),
        default_search_case_sensitive: false,
        search_source: "sqlite-file-text".to_string(),
        text_index_max_bytes: config.text_index_max_bytes(),
        watcher: watcher_status_report(false),
        index,
        telemetry,
        database,
        language_registry: language_registry_report(),
        semantic_relation_contract_digest: semantic_resolution_contract_digest(),
        relation_family_inventory: relation_family_inventory_report(),
        search,
        optional_parser_pack,
    })
}

/// Reject mixed settings projections when publication changed between read snapshots.
fn settings_publication_matches(
    diagnostic: Option<&DatabasePublicationReport>,
    snapshot: Option<&IndexPublication>,
) -> bool {
    match (diagnostic, snapshot) {
        (None, None) => true,
        (Some(diagnostic), Some(snapshot))
            if diagnostic.state == snapshot.state
                && diagnostic.generation == snapshot.generation =>
        {
            match diagnostic.contract_fingerprint_state {
                DatabasePublicationContractState::Missing => {
                    snapshot.contract_fingerprint.is_none()
                }
                DatabasePublicationContractState::Valid => {
                    diagnostic.contract_fingerprint.as_deref()
                        == snapshot.contract_fingerprint.as_deref()
                }
                DatabasePublicationContractState::Invalid => false,
            }
        }
        _ => false,
    }
}

/// Build index statistics for settings diagnostics.
pub(crate) fn settings_index_stats(store: &AtlasStore) -> Result<SettingsIndexStats, CliError> {
    let overview = store.overview()?;
    let health_findings = store.unresolved_health_finding_count_current()?;
    Ok(SettingsIndexStats {
        project_root: store
            .project_root()?
            .map(|path| normalize_native_path_display_str(&path)),
        files: overview.files,
        folders: overview.folders,
        missing_purposes: overview.missing_purposes,
        stale_purposes: overview.stale_purposes,
        suggested_purposes: overview.suggested_purposes,
        indexed_text_files: store.file_text_count()?,
        indexed_text_bytes: store.file_text_byte_count()?,
        symbols: store.symbol_count()?,
        relations: store.symbol_relation_count()?,
        token_calls: store.token_overview(None)?.calls,
        health_findings,
    })
}

/// Preview or remove local runtime index/cache files.
pub(crate) fn reset_index_files(
    db: &Path,
    apply: bool,
    dry_run: bool,
    include_mcp_config: bool,
) -> Result<ResetIndexReport, CliError> {
    let absolute_db = absolute_path(db)?;
    let mut targets = vec![
        absolute_db.clone(),
        db_sidecar_path(&absolute_db, "wal"),
        db_sidecar_path(&absolute_db, "shm"),
        db_sidecar_path(&absolute_db, "journal"),
    ];
    if include_mcp_config {
        targets.push(mcp_config_path_for_db(&absolute_db));
    }
    targets.sort();
    targets.dedup();
    let files = targets
        .iter()
        .map(|path| path_status(path))
        .collect::<Result<Vec<_>, _>>()?;
    let should_apply = apply && !dry_run;
    let mut removed = 0;
    if should_apply {
        for target in &targets {
            if target.is_file() {
                fs::remove_file(target).map_err(|source| CliError::Io {
                    path: target.clone(),
                    source,
                })?;
                removed += 1;
            }
        }
    }
    Ok(ResetIndexReport {
        applied: should_apply,
        dry_run: !should_apply,
        files,
        removed,
    })
}

/// Resolve the config path that should travel with generated MCP configs.
pub(crate) fn resolved_mcp_config_path(
    db: &Path,
    config: Option<&Path>,
) -> Result<Option<PathBuf>, CliError> {
    if let Some(path) = config {
        return Ok(Some(absolute_path(path)?));
    }
    let mut candidate_roots = Vec::new();
    if db.exists()
        && let Some(project_root) = read_project_root_read_only(db)?
    {
        candidate_roots.push(PathBuf::from(project_root));
    }
    let absolute_db = absolute_path(db)?;
    if let Some(project_root) = project_root_from_db_path(&absolute_db) {
        candidate_roots.push(project_root);
    }
    for root in candidate_roots {
        for candidate in config_candidates_for_root(&root) {
            if candidate.exists() {
                return Ok(Some(absolute_path(&candidate)?));
            }
        }
    }
    Ok(None)
}

/// Return supported config paths for one project root.
fn config_candidates_for_root(root: &Path) -> [PathBuf; 2] {
    [
        root.join(".projectatlas").join("config.toml"),
        root.join("projectatlas.toml"),
    ]
}

/// Return an absolute path without requiring the target to exist.
pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    Ok(current_dir.join(path))
}

/// Return a diagnostic status for one path.
pub(crate) fn path_status(path: &Path) -> Result<PathStatus, CliError> {
    let absolute = absolute_path(path)?;
    let metadata = fs::metadata(&absolute).ok();
    Ok(PathStatus {
        path: normalize_display_path(&absolute),
        exists: metadata.is_some(),
        size_bytes: metadata
            .as_ref()
            .and_then(|metadata| metadata.is_file().then_some(metadata.len())),
    })
}

/// Return the path to a `SQLite` sidecar file.
pub(crate) fn db_sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}-{suffix}", db.display()))
}

/// Return the project-local MCP config path associated with a database path.
pub(crate) fn mcp_config_path_for_db(db: &Path) -> PathBuf {
    db.parent().map_or_else(
        || PathBuf::from("projectatlas.mcp.json"),
        |parent| parent.join("projectatlas.mcp.json"),
    )
}

/// Normalize a path for JSON/TOON diagnostics.
pub(crate) fn normalize_display_path(path: &Path) -> String {
    normalize_native_path_display(path)
}

/// Build a watcher status report from a lightweight runtime probe.
pub(crate) fn watcher_status_report(active: bool) -> WatchStatusReport {
    let notify_available = notify_runtime_available();
    let mode = if notify_available {
        WATCH_MODE_NOTIFY
    } else {
        WATCH_MODE_POLLING
    };
    let recommendation = if notify_available {
        "Run `projectatlas watch --once` for one refresh or `projectatlas watch` for event-backed refresh with portable polling fallback."
    } else {
        "Run `projectatlas watch --once` for one refresh or `projectatlas watch` for portable polling refresh."
    };
    WatchStatusReport {
        available: true,
        active,
        mode: mode.to_string(),
        event_backend_available: notify_available,
        recommendation: recommendation.to_string(),
    }
}

/// Build lint output for an existing `SQLite` index.
pub(crate) fn lint_database_if_present(
    db: &Path,
    root: &Path,
    config_path: Option<&Path>,
    purpose_level: PurposeLintLevel,
) -> Result<(String, i32), CliError> {
    match db.try_exists() {
        Ok(false) => return Ok((String::new(), 0)),
        Ok(true) => {}
        Err(source) => {
            return Err(CliError::Io {
                path: db.to_path_buf(),
                source,
            });
        }
    }
    let store = open_fresh_atlas_store_for_project(db, root, config_path)?;
    let query = purpose_level.health_query();
    let page = store.unresolved_health_findings_page_current(&query)?;
    let blocking = page
        .findings
        .iter()
        .filter(|finding| purpose_level.blocks_category(finding.category.as_str()))
        .collect::<Vec<_>>();
    if blocking.is_empty() {
        return Ok((String::new(), 0));
    }
    let mut report = format!(
        "ProjectAtlas SQLite index health findings (purpose-level {}, showing {} of {}):\n",
        purpose_level.as_str(),
        blocking.len(),
        page.total
    );
    for finding in blocking {
        writeln!(
            &mut report,
            "- [{}] {}: {}",
            finding.category, finding.path, finding.recommendation
        )
        .map_err(|source| CliError::Output(io::Error::other(source.to_string())))?;
    }
    Ok((report, 1))
}

/// Purpose curation strictness used by `projectatlas lint`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PurposeLintLevel {
    /// Advisory first-pass curation scope for folders and high-impact files.
    Low,
    /// Also require agent review for all source files.
    Medium,
    /// Require agent review for every indexed file and folder.
    Strict,
}

impl PurposeLintLevel {
    /// Stable CLI/report label.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::Strict => "strict",
        }
    }

    /// Convert lint strictness into the bounded DB health query.
    fn health_query(self) -> HealthQuery {
        let scope = match self {
            Self::Low => HealthScope::purpose_default(),
            Self::Medium => HealthScope::purpose_with_source_files(),
            Self::Strict => HealthScope::purpose_strict(),
        };
        HealthQuery {
            start_index: 0,
            limit: MAX_HEALTH_LIMIT,
            category: None,
            severity: Some(Severity::Warning),
            path_prefix: None,
            summary_only: false,
            scope,
        }
    }

    /// Return whether a category should make lint fail at this strictness.
    fn blocks_category(self, category: &str) -> bool {
        match category {
            CATEGORY_STALE_PURPOSE
            | CATEGORY_DUPLICATE_PURPOSE
            | CATEGORY_REPEATED_TEMPORARY_FOLDER => true,
            CATEGORY_MISSING_PURPOSE
            | CATEGORY_SUGGESTED_PURPOSE_REVIEW
            | CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED => self != Self::Low,
            _ => false,
        }
    }
}

/// Return whether the platform watcher can be constructed in this process.
pub(crate) fn notify_runtime_available() -> bool {
    let (sender, _receiver) = mpsc::channel();
    RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if sender.send(result).is_err() {
                // Receiver shutdown only means this status probe is done.
            }
        },
        Config::default(),
    )
    .is_ok()
}

/// Options controlling source parsing during symbol graph builds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SymbolBuildOptions {
    /// Maximum file size parsed for symbols.
    pub(crate) max_bytes: u64,
    /// Optional maximum worker threads for parser work.
    max_workers: Option<usize>,
    /// Optional deadline for starting parser work.
    timeout: Option<Duration>,
    /// Serialized timeout value for reports.
    pub(crate) timeout_seconds: Option<u64>,
}

impl SymbolBuildOptions {
    /// Create symbol build options from CLI/MCP values.
    pub(crate) fn new(
        max_bytes: u64,
        max_workers: Option<usize>,
        timeout_seconds: Option<u64>,
    ) -> Self {
        Self {
            max_bytes: max_bytes.min(MAX_SYMBOL_FILE_BYTES),
            max_workers: max_workers.filter(|workers| *workers > 0),
            timeout: timeout_seconds.map(Duration::from_secs),
            timeout_seconds,
        }
    }

    /// Apply a worker ceiling without weakening a tighter caller limit.
    #[must_use]
    pub(crate) fn with_worker_ceiling(mut self, max_workers: usize) -> Self {
        let ceiling = max_workers.max(1);
        self.max_workers = Some(
            self.max_workers
                .map_or(ceiling, |workers| workers.min(ceiling)),
        );
        self
    }

    /// Return the worker count that will be reported.
    pub(crate) fn reported_workers(self) -> usize {
        self.effective_workers()
    }

    /// Derive the worker count from caller policy, host availability, and the safety ceiling.
    fn effective_workers(self) -> usize {
        let available = thread::available_parallelism().map_or(1, usize::from);
        self.max_workers
            .unwrap_or(available)
            .min(available)
            .min(INDEX_WORKER_SAFE_CEILING)
    }

    /// Return whether the parser build deadline has elapsed.
    pub(crate) fn is_timed_out(self, started_at: Instant) -> bool {
        self.timeout
            .is_some_and(|timeout| started_at.elapsed() >= timeout)
    }
}

/// Bound a worker pool by its work cardinality and runtime ceiling.
fn worker_count_for_work(work_items: usize, max_workers: usize) -> usize {
    work_items.min(max_workers.clamp(1, INDEX_WORKER_SAFE_CEILING))
}

/// Aggregate rows and retained string bytes admitted by one symbol publication.
#[derive(Clone, Copy, Debug)]
struct SymbolPublicationLimits {
    /// Maximum symbol rows persisted by the operation.
    symbol_rows: u64,
    /// Maximum relation rows persisted by the operation.
    relation_rows: u64,
    /// Maximum retained parser-output string bytes persisted by the operation.
    output_bytes: u64,
}

impl SymbolPublicationLimits {
    /// Durable process-safe limits used by CLI and MCP indexing operations.
    const STANDARD: Self = Self {
        symbol_rows: 2_000_000,
        relation_rows: 8_000_000,
        output_bytes: MAX_PUBLICATION_STAGING_BYTES,
    };
}

/// Create one indexing boundary with the caller timeout capped by the safe default.
pub(crate) fn index_work_control(options: &SymbolBuildOptions) -> IndexWorkControl {
    IndexWorkControl::new(IndexCancellation::new(), options.timeout)
        .with_timeout_ceiling(DEFAULT_INDEX_WORK_TIMEOUT)
        .with_worker_ceiling(options.effective_workers())
}

/// Create a bounded work boundary for runtime paths without symbol options.
pub(crate) fn standalone_index_work_control() -> IndexWorkControl {
    IndexWorkControl::new(IndexCancellation::new(), Some(DEFAULT_INDEX_WORK_TIMEOUT))
}

/// Apply the runtime's safe whole-operation deadline without weakening caller bounds.
fn bounded_index_work_control(control: &IndexWorkControl) -> IndexWorkControl {
    control.with_timeout_ceiling(DEFAULT_INDEX_WORK_TIMEOUT)
}

/// Source file queued for symbol parsing.
#[derive(Clone, Debug)]
pub(crate) struct SymbolParseJob {
    /// Repository-relative file path.
    pub(crate) path: String,
    /// Native absolute file path.
    native_path: PathBuf,
    /// Content hash captured by the staged filesystem scan.
    expected_content_hash: String,
    /// Detected language name.
    language: Option<String>,
    /// Existing node summary fallback.
    fallback_summary: Option<String>,
    /// Whether a generated purpose suggestion should be written or refreshed.
    purpose_needs_suggestion: bool,
}

/// Successful parser output waiting for sequential DB persistence.
#[derive(Debug)]
pub(crate) struct SymbolParseSuccess {
    /// Repository-relative file path.
    pub(crate) path: String,
    /// Extracted symbol graph.
    graph: SymbolGraph,
    /// File-level parser kept independent from fact-level parser provenance.
    source_parser: ParserKind,
    /// Observed one-line source summary.
    summary: String,
    /// Whether the existing parser worker derived the summary through the structural adapter.
    summary_is_structural: bool,
    /// Optional generated purpose suggestion.
    purpose_suggestion: Option<String>,
}

/// Outcome from one parser worker.
#[derive(Debug)]
pub(crate) enum SymbolParseOutcome {
    /// Source parsed successfully.
    Parsed(SymbolParseSuccess),
    /// File was skipped because it was not UTF-8 source text.
    BinaryOrNonUtf8 {
        /// Repository-relative file path.
        path: String,
    },
    /// Source bytes changed after the staged filesystem scan.
    SourceChanged {
        /// Repository-relative file path.
        path: String,
    },
    /// Source read failed.
    Io {
        /// Native path that failed to read.
        path: PathBuf,
        /// Source IO error.
        source: io::Error,
    },
    /// Cooperative parsing work was canceled or reached its deadline.
    IndexWork(IndexWorkFailure),
}

/// Failure from one cancellation-aware bounded source read.
#[derive(Debug)]
enum SourceReadFailure {
    /// The source file could not be opened or read.
    Io(io::Error),
    /// The shared indexing operation was canceled or reached its deadline.
    IndexWork(IndexWorkFailure),
    /// The source grew beyond the caller's admitted byte count.
    LimitExceeded {
        /// First observed byte count beyond the limit.
        observed: u64,
    },
}

/// Read at most one admitted source-byte budget while checking cooperative stop state.
fn read_source_bytes_controlled(
    path: &Path,
    max_bytes: u64,
    stage: IndexWorkStage,
    control: &IndexWorkControl,
) -> Result<Vec<u8>, SourceReadFailure> {
    control.check(stage).map_err(SourceReadFailure::IndexWork)?;
    let mut file = fs::File::open(path).map_err(SourceReadFailure::Io)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; CONTROLLED_SOURCE_READ_BUFFER_BYTES];
    loop {
        control.check(stage).map_err(SourceReadFailure::IndexWork)?;
        let count = file.read(&mut buffer).map_err(SourceReadFailure::Io)?;
        if count == 0 {
            break;
        }
        let observed = u64::try_from(bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(count as u64);
        if observed > max_bytes {
            return Err(SourceReadFailure::LimitExceeded { observed });
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    control.check(stage).map_err(SourceReadFailure::IndexWork)?;
    Ok(bytes)
}

/// Build selected symbol graphs under explicit aggregate publication limits.
#[cfg(test)]
fn build_symbols_for_paths_with_limits(
    store: &mut AtlasStore,
    root: &Path,
    options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
    target_paths: Option<&HashSet<String>>,
    control: &IndexWorkControl,
    limits: SymbolPublicationLimits,
) -> Result<SymbolBuildReport, CliError> {
    let nodes = if let Some(paths) = target_paths {
        let mut sorted_paths = paths.iter().cloned().collect::<Vec<_>>();
        sorted_paths.sort();
        store
            .load_nodes_by_paths(&sorted_paths)?
            .into_iter()
            .map(|indexed| indexed.node)
            .collect::<Vec<_>>()
    } else {
        store
            .load_nodes()?
            .into_iter()
            .map(|indexed| indexed.node)
            .collect::<Vec<_>>()
    };
    #[cfg(feature = "optional-parser-supervisor")]
    let optional_parser_selection =
        OptionalParserPackLifecycle::new(root, None)?.derive_project_selection()?;
    let staged = stage_symbols_for_nodes_with_limits(
        store,
        root,
        #[cfg(feature = "optional-parser-supervisor")]
        &optional_parser_selection,
        &nodes,
        options,
        previous_hashes,
        target_paths,
        &HashSet::new(),
        control,
        limits,
    )?;
    apply_symbol_build_stage(store, &staged, control)?;
    Ok(staged.report)
}

/// Build selected symbol mutations without acquiring the `SQLite` writer.
#[allow(clippy::too_many_arguments)]
fn stage_symbols_for_nodes_with_limits(
    store: &AtlasStore,
    root: &Path,
    #[cfg(feature = "optional-parser-supervisor")]
    optional_parser_selection: &OptionalParserPackProjectSelection,
    nodes: &[Node],
    options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
    target_paths: Option<&HashSet<String>>,
    protected_purpose_paths: &HashSet<String>,
    control: &IndexWorkControl,
    limits: SymbolPublicationLimits,
) -> Result<SymbolBuildStage, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    #[cfg(feature = "optional-parser-supervisor")]
    let admit_optional_languages = optional_parser_selection.selection_key().is_some();
    #[cfg(not(feature = "optional-parser-supervisor"))]
    let admit_optional_languages = false;
    let root = root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let considered_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| target_paths.is_none_or(|paths| paths.contains(&node.path)))
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    let previously_parsed_paths = store.source_parse_metadata_paths_for_paths(&considered_paths)?;
    let mut candidate_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| target_paths.is_none_or(|paths| paths.contains(&node.path)))
        .filter(|node| {
            is_symbol_candidate_for_admission(
                &node.path,
                node.language.as_deref(),
                admit_optional_languages,
            )
        })
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    candidate_paths.sort();
    let existing_nodes = store
        .load_nodes_by_paths(&candidate_paths)?
        .into_iter()
        .map(|indexed| (indexed.node.path.clone(), indexed))
        .collect::<HashMap<_, _>>();
    let symbol_counts = store.symbol_counts_for_paths(&candidate_paths)?;
    let mut report = SymbolBuildReport {
        candidates: 0,
        parsed: 0,
        unchanged: 0,
        too_large: 0,
        binary_or_non_utf8: 0,
        timed_out: 0,
        max_workers: options.reported_workers(),
        timeout_seconds: options.timeout_seconds,
        symbols: 0,
        relations: 0,
        summaries: 0,
        purpose_suggestions: 0,
    };
    let mut jobs = Vec::new();
    let mut changes = Vec::new();
    let mut output_bytes = 0_u64;
    for node in nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| target_paths.is_none_or(|paths| paths.contains(&node.path)))
        .filter(|node| {
            !is_symbol_candidate_for_admission(
                &node.path,
                node.language.as_deref(),
                admit_optional_languages,
            ) && previously_parsed_paths.contains(&node.path)
        })
    {
        output_bytes = checked_symbol_publication_usage(
            output_bytes,
            node.path.len() as u64 + node.language.as_ref().map_or(0, String::len) as u64,
            limits.output_bytes,
            IndexWorkResource::OutputBytes,
        )?;
        changes.push(SymbolProjectionChange::Clear {
            path: node.path.clone(),
            language: node.language.clone(),
        });
    }
    for node in nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| target_paths.is_none_or(|paths| paths.contains(&node.path)))
        .filter(|node| {
            is_symbol_candidate_for_admission(
                &node.path,
                node.language.as_deref(),
                admit_optional_languages,
            )
        })
    {
        control.check(IndexWorkStage::SymbolParsing)?;
        report.candidates += 1;
        if node.size_bytes.is_some_and(|size| size > options.max_bytes) {
            output_bytes = checked_symbol_publication_usage(
                output_bytes,
                node.path.len() as u64 + node.language.as_ref().map_or(0, String::len) as u64,
                limits.output_bytes,
                IndexWorkResource::OutputBytes,
            )?;
            changes.push(SymbolProjectionChange::Clear {
                path: node.path.clone(),
                language: node.language.clone(),
            });
            report.too_large += 1;
            continue;
        }
        let symbol_count = symbol_counts.get(&node.path).copied().unwrap_or_default();
        if node.content_hash.as_ref().is_some_and(|hash| {
            previous_hashes.and_then(|hashes| hashes.get(&node.path)) == Some(hash)
        }) {
            let has_source_index =
                symbol_count > 0 || store.load_source_parse_metadata(&node.path)?.is_some();
            if has_source_index {
                report.unchanged += 1;
                continue;
            }
        }
        let existing = existing_nodes.get(&node.path);
        jobs.push(SymbolParseJob {
            path: node.path.clone(),
            native_path: root.join(repo_path_to_native(&node.path)),
            expected_content_hash: node
                .content_hash
                .clone()
                .ok_or_else(|| source_changed_during_derivation(&root, &node.path))?,
            language: node.language.clone(),
            fallback_summary: existing.and_then(|indexed| indexed.summary.clone()),
            purpose_needs_suggestion: !protected_purpose_paths.contains(&node.path)
                && existing.is_none_or(|indexed| {
                    matches!(
                        indexed.purpose.status,
                        PurposeStatus::Missing | PurposeStatus::Suggested
                    )
                }),
        });
        if jobs.len() > MAX_SYMBOL_PARSE_JOBS {
            return Err(IndexWorkFailure::resource_limit(
                IndexWorkStage::SymbolParsing,
                IndexWorkResource::SymbolJobs,
                MAX_SYMBOL_PARSE_JOBS as u64,
                jobs.len() as u64,
            )
            .into());
        }
    }
    report.max_workers = worker_count_for_work(jobs.len(), report.max_workers);
    if !jobs.is_empty() {
        let pool = ThreadPoolBuilder::new()
            .num_threads(report.max_workers)
            .build()
            .map_err(|source| {
                CliError::InvalidInput(format!("symbol worker pool failed: {source}"))
            })?;
        #[cfg(feature = "optional-parser-supervisor")]
        let outcomes = optional_parser_runtime::parse_symbol_jobs_controlled(
            &root,
            optional_parser_selection,
            &pool,
            &jobs,
            options,
            control,
        )?;
        #[cfg(not(feature = "optional-parser-supervisor"))]
        let outcomes = parse_symbol_job_batches_controlled(&pool, &jobs, options, control)?;
        for outcome in outcomes {
            match outcome {
                SymbolParseOutcome::Parsed(parsed) => {
                    let next_symbols = checked_symbol_publication_usage(
                        report.symbols as u64,
                        parsed.graph.symbols.len() as u64,
                        limits.symbol_rows,
                        IndexWorkResource::SymbolRows,
                    )?;
                    let next_relations = checked_symbol_publication_usage(
                        report.relations as u64,
                        parsed.graph.relations.len() as u64,
                        limits.relation_rows,
                        IndexWorkResource::RelationRows,
                    )?;
                    let next_output_bytes = checked_symbol_publication_usage(
                        output_bytes,
                        symbol_parse_output_bytes(&parsed),
                        limits.output_bytes,
                        IndexWorkResource::OutputBytes,
                    )?;
                    report.summaries += 1;
                    if parsed.purpose_suggestion.is_some() {
                        report.purpose_suggestions += 1;
                    }
                    report.parsed += 1;
                    report.symbols = next_symbols as usize;
                    report.relations = next_relations as usize;
                    output_bytes = next_output_bytes;
                    changes.push(SymbolProjectionChange::Parsed(parsed));
                }
                SymbolParseOutcome::BinaryOrNonUtf8 { path } => {
                    let language = nodes
                        .iter()
                        .find(|node| node.path == path)
                        .and_then(|node| node.language.clone());
                    output_bytes = checked_symbol_publication_usage(
                        output_bytes,
                        path.len() as u64 + language.as_ref().map_or(0, String::len) as u64,
                        limits.output_bytes,
                        IndexWorkResource::OutputBytes,
                    )?;
                    changes.push(SymbolProjectionChange::Clear { path, language });
                    report.binary_or_non_utf8 += 1;
                }
                SymbolParseOutcome::SourceChanged { path } => {
                    return Err(source_changed_during_derivation(&root, &path));
                }
                SymbolParseOutcome::Io { path, source } => {
                    return Err(CliError::Io { path, source });
                }
                SymbolParseOutcome::IndexWork(failure) => return Err(failure.into()),
            }
        }
    }
    control.check(IndexWorkStage::SymbolParsing)?;
    Ok(SymbolBuildStage {
        report,
        changes,
        retained_bytes: output_bytes,
    })
}

/// Parse all built-in symbol jobs in bounded Rayon batches.
#[cfg(not(feature = "optional-parser-supervisor"))]
fn parse_symbol_job_batches_controlled(
    pool: &rayon::ThreadPool,
    jobs: &[SymbolParseJob],
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<Vec<SymbolParseOutcome>, CliError> {
    let mut outcomes = Vec::with_capacity(jobs.len());
    for batch in jobs.chunks(SYMBOL_PARSE_BATCH_SIZE) {
        control.check(IndexWorkStage::SymbolParsing)?;
        outcomes.extend(parse_symbol_jobs_controlled(pool, batch, options, control)?);
    }
    Ok(outcomes)
}

/// Apply prepared symbol mutations inside the parent publication transaction.
fn apply_symbol_build_stage(
    store: &mut AtlasStore,
    staged: &SymbolBuildStage,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    for change in &staged.changes {
        control.check(IndexWorkStage::Publication)?;
        match change {
            SymbolProjectionChange::Parsed(parsed) => {
                store.set_node_summary(&parsed.path, &parsed.summary)?;
                if let Some(suggestion) = parsed.purpose_suggestion.as_deref() {
                    store.set_suggested_purpose(&parsed.path, suggestion)?;
                }
                let mut metadata = SourceParseMetadata::from_graph(&parsed.graph);
                metadata.parser = parsed.source_parser;
                store.replace_symbol_graph_with_metadata(&parsed.graph, &metadata)?;
            }
            SymbolProjectionChange::Clear { path, language } => {
                clear_skipped_symbol_index(store, path, language.as_deref())?;
            }
        }
    }
    control.check(IndexWorkStage::Publication)?;
    Ok(())
}

/// Parse one bounded symbol batch under the shared work boundary.
#[cfg(not(feature = "optional-parser-supervisor"))]
fn parse_symbol_jobs_controlled(
    pool: &rayon::ThreadPool,
    jobs: &[SymbolParseJob],
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<Vec<SymbolParseOutcome>, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    Ok(pool.install(|| {
        jobs.par_iter()
            .map(|job| parse_symbol_job_controlled(job, options, control))
            .collect::<Vec<_>>()
    }))
}

/// Admit one aggregate symbol-publication resource before persistence.
fn checked_symbol_publication_usage(
    current: u64,
    added: u64,
    limit: u64,
    resource: IndexWorkResource,
) -> Result<u64, CliError> {
    let observed = current.saturating_add(added);
    if observed > limit {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::SymbolParsing,
            resource,
            limit,
            observed,
        )
        .into());
    }
    Ok(observed)
}

/// Count retained string bytes in one parser output without serializing a second copy.
fn symbol_parse_output_bytes(parsed: &SymbolParseSuccess) -> u64 {
    let graph = &parsed.graph;
    let mut bytes = graph.path.len() as u64
        + graph.language.as_ref().map_or(0, String::len) as u64
        + parsed.summary.len() as u64
        + parsed.purpose_suggestion.as_ref().map_or(0, String::len) as u64;
    for symbol in &graph.symbols {
        bytes = bytes.saturating_add(
            symbol.path.len() as u64
                + symbol.language.as_ref().map_or(0, String::len) as u64
                + symbol.name.len() as u64
                + symbol.signature.len() as u64
                + symbol.documentation.as_ref().map_or(0, String::len) as u64
                + symbol.parent.as_ref().map_or(0, String::len) as u64
                + symbol.detail.as_ref().map_or(0, String::len) as u64,
        );
    }
    for relation in &graph.relations {
        bytes = bytes.saturating_add(
            relation.path.len() as u64
                + relation.source_name.len() as u64
                + relation.target_name.len() as u64
                + relation.context.len() as u64,
        );
    }
    bytes
}

/// Parse one source file into a symbol graph.
#[cfg(test)]
pub(crate) fn parse_symbol_job(
    job: &SymbolParseJob,
    options: &SymbolBuildOptions,
    started_at: Instant,
) -> SymbolParseOutcome {
    let control = options
        .timeout
        .and_then(|timeout| started_at.checked_add(timeout))
        .map_or_else(
            || IndexWorkControl::new(IndexCancellation::new(), None),
            |deadline| IndexWorkControl::with_deadline(IndexCancellation::new(), deadline),
        );
    parse_symbol_job_controlled(job, options, &control)
}

/// Parse one source file while observing cancellation before and during parsing.
fn parse_symbol_job_controlled(
    job: &SymbolParseJob,
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> SymbolParseOutcome {
    if options.is_timed_out(control.started_at()) {
        return SymbolParseOutcome::IndexWork(IndexWorkFailure::DeadlineExceeded {
            stage: IndexWorkStage::SymbolParsing,
        });
    }
    if let Err(failure) = control.check(IndexWorkStage::SymbolParsing) {
        return SymbolParseOutcome::IndexWork(failure);
    }
    let content = match admit_symbol_job_source(job, options, control) {
        Ok(content) => content,
        Err(outcome) => return *outcome,
    };
    parse_admitted_symbol_job(job, &content, None, options, control)
}

/// Read, bound, hash-check, and decode one source exactly once for symbol staging.
fn admit_symbol_job_source(
    job: &SymbolParseJob,
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<String, Box<SymbolParseOutcome>> {
    let bytes = match read_source_bytes_controlled(
        &job.native_path,
        options.max_bytes,
        IndexWorkStage::SymbolParsing,
        control,
    ) {
        Ok(bytes) => bytes,
        Err(SourceReadFailure::Io(source)) => {
            return Err(Box::new(SymbolParseOutcome::Io {
                path: job.native_path.clone(),
                source,
            }));
        }
        Err(SourceReadFailure::IndexWork(failure)) => {
            return Err(Box::new(SymbolParseOutcome::IndexWork(failure)));
        }
        Err(SourceReadFailure::LimitExceeded { observed }) => {
            return Err(Box::new(SymbolParseOutcome::IndexWork(
                IndexWorkFailure::resource_limit(
                    IndexWorkStage::SymbolParsing,
                    IndexWorkResource::SourceBytes,
                    options.max_bytes,
                    observed,
                ),
            )));
        }
    };
    if let Err(failure) = control.check(IndexWorkStage::SymbolParsing) {
        return Err(Box::new(SymbolParseOutcome::IndexWork(failure)));
    }
    if blake3::hash(&bytes).to_hex().as_str() != job.expected_content_hash {
        return Err(Box::new(SymbolParseOutcome::SourceChanged {
            path: job.path.clone(),
        }));
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Err(Box::new(SymbolParseOutcome::BinaryOrNonUtf8 {
            path: job.path.clone(),
        }));
    };
    Ok(content)
}

/// Extract conservative facts from admitted source and retain independent source provenance.
fn parse_admitted_symbol_job(
    job: &SymbolParseJob,
    content: &str,
    source_parser: Option<ParserKind>,
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> SymbolParseOutcome {
    if options.is_timed_out(control.started_at()) {
        return SymbolParseOutcome::IndexWork(IndexWorkFailure::DeadlineExceeded {
            stage: IndexWorkStage::SymbolParsing,
        });
    }
    let graph =
        match extract_symbol_graph_controlled(&job.path, job.language.as_deref(), content, control)
        {
            Ok(graph) => graph,
            Err(failure) => return SymbolParseOutcome::IndexWork(failure),
        };
    let source_parser = source_parser.unwrap_or(graph.parser);
    let structural_summary = graph
        .symbols
        .is_empty()
        .then(|| structural_summary_for_path(&job.path, job.language.as_deref(), content));
    let structural_summary = structural_summary.flatten();
    let summary_is_structural = structural_summary.is_some();
    let summary = structural_summary
        .unwrap_or_else(|| summarize_symbol_graph(&graph, job.fallback_summary.as_deref()));
    let purpose_suggestion = job
        .purpose_needs_suggestion
        .then(|| suggest_file_purpose(&job.path, &summary));
    SymbolParseOutcome::Parsed(SymbolParseSuccess {
        path: job.path.clone(),
        graph,
        source_parser,
        summary,
        summary_is_structural,
        purpose_suggestion,
    })
}

/// Return an empty symbol build report.
pub(crate) fn empty_symbol_build_report() -> SymbolBuildReport {
    SymbolBuildReport {
        candidates: 0,
        parsed: 0,
        unchanged: 0,
        too_large: 0,
        binary_or_non_utf8: 0,
        timed_out: 0,
        max_workers: 0,
        timeout_seconds: None,
        symbols: 0,
        relations: 0,
        summaries: 0,
        purpose_suggestions: 0,
    }
}

/// Return an empty text-index report for a no-op refresh.
fn empty_text_index_report(options: TextIndexOptions) -> TextIndexReport {
    TextIndexReport {
        candidates: 0,
        indexed: 0,
        binary_or_non_utf8: 0,
        too_large: 0,
        skipped: 0,
        max_bytes: options.max_bytes,
        bytes: 0,
    }
}

/// Create a deterministic one-line content summary from extracted symbols.
pub(crate) fn summarize_symbol_graph(graph: &SymbolGraph, fallback: Option<&str>) -> String {
    if graph.symbols.is_empty() {
        if let Some(fallback) = fallback.filter(|summary| !is_scanner_fallback_summary(summary)) {
            return fallback.to_string();
        }
        let language = observed_language_label(graph.language.as_deref());
        return format!("{language} source file with no declarations found.");
    }
    let language = observed_language_label(graph.language.as_deref());
    let primary_names = primary_symbol_names(graph, 4);
    let primary_kinds = primary_symbol_kinds(graph);
    let imports = relation_targets(graph, RelationKind::Imports, 2);
    let dependencies = relation_targets(graph, RelationKind::DependsOn, 3);
    if !dependencies.is_empty() {
        let subject = observed_manifest_subject(&language);
        return format!(
            "{subject} declaring {} and depending on {}.",
            primary_names.join(", "),
            dependencies.join(", ")
        );
    }
    if !imports.is_empty() {
        return format!(
            "{language} source defining {} {} with imports {}.",
            primary_kinds,
            primary_names.join(", "),
            imports.join(", ")
        );
    }
    format!(
        "{language} source defining {} {}.",
        primary_kinds,
        primary_names.join(", ")
    )
}

/// Return a readable language label for agent-facing content summaries.
fn observed_language_label(language: Option<&str>) -> String {
    match language.unwrap_or("source") {
        "cargo-manifest" => "cargo manifest".to_string(),
        "cargo-lock" => "cargo lock".to_string(),
        "rust-build-script" => "rust build script".to_string(),
        "objective-c" => "Objective-C".to_string(),
        "csharp" => "C#".to_string(),
        "cpp" => "C++".to_string(),
        other => other.replace('-', " "),
    }
}

/// Return the subject phrase for manifest-style content summaries.
fn observed_manifest_subject(language: &str) -> String {
    if language.contains("manifest") {
        language.to_string()
    } else {
        format!("{language} manifest")
    }
}

/// Return a compact phrase describing the most important symbol kinds.
pub(crate) fn primary_symbol_kinds(graph: &SymbolGraph) -> String {
    let mut function_like = 0_usize;
    let mut type_like = 0_usize;
    let mut manifest_like = 0_usize;
    let mut value_like = 0_usize;
    for symbol in &graph.symbols {
        match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => function_like += 1,
            SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
            | SymbolKind::Type => type_like += 1,
            SymbolKind::Package | SymbolKind::Workspace | SymbolKind::Dependency => {
                manifest_like += 1;
            }
            SymbolKind::Value => value_like += 1,
            SymbolKind::Module | SymbolKind::Import | SymbolKind::Unknown => {}
        }
    }
    if manifest_like > 0 && function_like == 0 && type_like == 0 {
        return "manifest entries".to_string();
    }
    if value_like > 0 && function_like == 0 && type_like == 0 {
        return value_only_symbol_kind_label(graph, value_like);
    }
    match (type_like, function_like) {
        (0, 0) => "symbols".to_string(),
        (0, 1) => "function".to_string(),
        (0, _) => "functions".to_string(),
        (1, 0) => "type".to_string(),
        (_, 0) => "types".to_string(),
        (1, 1) => "type and function".to_string(),
        (1, _) => "type and functions".to_string(),
        (_, 1) => "types and function".to_string(),
        (_, _) => "types and functions".to_string(),
    }
}

/// Return the right value-only summary noun for the indexed language.
pub(crate) fn value_only_symbol_kind_label(graph: &SymbolGraph, count: usize) -> String {
    let language = graph.language.as_deref().unwrap_or_default();
    let binding_language = matches!(
        language,
        "javascript" | "typescript" | "tsx" | "vue" | "svelte"
    ) || graph
        .symbols
        .iter()
        .any(|symbol| symbol.detail.as_deref() == Some("fallback-composition-binding"));
    let singular = if binding_language { "binding" } else { "value" };
    let plural = if binding_language {
        "bindings"
    } else {
        "values"
    };
    if count == 1 {
        singular.to_string()
    } else {
        plural.to_string()
    }
}

/// Return stable names for the most important declaration symbols.
pub(crate) fn primary_symbol_names(graph: &SymbolGraph, limit: usize) -> Vec<String> {
    let has_primary_definitions = graph.symbols.iter().any(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Class
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Trait
                | SymbolKind::Interface
                | SymbolKind::Type
        )
    });
    let mut names = graph
        .symbols
        .iter()
        .filter(|symbol| {
            if has_primary_definitions && symbol.kind == SymbolKind::Value {
                return false;
            }
            !matches!(
                symbol.kind,
                SymbolKind::Import
                    | SymbolKind::Dependency
                    | SymbolKind::Module
                    | SymbolKind::Unknown
            )
        })
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    if names.is_empty() {
        names = graph
            .symbols
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect::<Vec<_>>();
    }
    names.sort();
    names.dedup();
    names.truncate(limit);
    if names.is_empty() {
        vec!["indexed symbols".to_string()]
    } else {
        names
    }
}

/// Return relation targets for one relation kind.
pub(crate) fn relation_targets(
    graph: &SymbolGraph,
    kind: RelationKind,
    limit: usize,
) -> Vec<String> {
    let mut targets = graph
        .relations
        .iter()
        .filter(|relation| relation.kind == kind)
        .map(|relation| relation.target_name.clone())
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets.truncate(limit);
    targets
}

/// Create a generated file-purpose suggestion from a path and content summary.
pub(crate) fn suggest_file_purpose(path: &str, summary: &str) -> String {
    let subject = path_context_subject(path);
    if summary.contains("dataset manifest") {
        if let Some(datasets) = summary_between(summary, " including ", " and keys") {
            format!("Define the {subject} dataset manifest for {datasets}.")
        } else {
            format!("Define the {subject} dataset manifest.")
        }
    } else if let Some(workflow) = summary_between(summary, "yaml workflow ", " triggered") {
        format!("Define the {workflow} workflow.")
    } else if summary.contains("manifest") {
        if let Some(package) = summary_between(summary, " manifest for ", " with ") {
            format!("Define the {package} manifest.")
        } else {
            format!("Define the {subject} manifest.")
        }
    } else if let Some(title) = summary_between(summary, "document titled ", " with ") {
        format!("Document {title}.")
    } else if summary.contains("stylesheet") {
        format!("Style the {subject} stylesheet.")
    } else if summary.contains("config") {
        format!("Configure the {subject}.")
    } else if is_gradle_build_script(path) {
        if let Some(declarations) = summary_primary_declarations(summary) {
            format!("Define Gradle build tasks around {declarations}.")
        } else {
            "Configure the Gradle build.".to_string()
        }
    } else if let Some(declarations) = summary_primary_declarations(summary) {
        format!("Implement the {subject} source around {declarations}.")
    } else if summary.contains("source") {
        format!("Implement the {subject} source.")
    } else {
        format!("Implement the {subject}.")
    }
}

/// Return whether a path is a Gradle build script rather than ordinary Kotlin/Groovy source.
fn is_gradle_build_script(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.ends_with("build.gradle") || normalized.ends_with("build.gradle.kts")
}

/// Return a path-aware subject phrase for a generated purpose suggestion.
fn path_context_subject(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    let Some(file_name) = segments.pop() else {
        return "path".to_string();
    };
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let stem_words = path_segment_words(stem);
    let parent_words = segments
        .iter()
        .rev()
        .find(|segment| !is_generic_context_segment(segment))
        .map(|segment| path_segment_words(segment));
    match parent_words {
        Some(parent) if !parent.is_empty() && parent != stem_words => {
            format!("{parent} {stem_words}")
        }
        _ => stem_words,
    }
}

/// Convert one path segment into readable lowercase words.
fn path_segment_words(segment: &str) -> String {
    let mut words = String::new();
    let mut previous_lowercase = false;
    for character in segment.chars() {
        if character == '-' || character == '_' || character == '.' {
            push_word_space(&mut words);
            previous_lowercase = false;
            continue;
        }
        if character.is_uppercase() && previous_lowercase {
            push_word_space(&mut words);
        }
        words.extend(character.to_lowercase());
        previous_lowercase = character.is_lowercase() || character.is_ascii_digit();
    }
    let words = words.trim();
    if words.is_empty() {
        "path".to_string()
    } else {
        words.to_string()
    }
}

/// Append one word separator when the phrase already has content.
fn push_word_space(words: &mut String) {
    if !words.ends_with(' ') && !words.is_empty() {
        words.push(' ');
    }
}

/// Return whether a path segment is too generic to add useful purpose context.
fn is_generic_context_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "src"
            | "source"
            | "sources"
            | "app"
            | "apps"
            | "lib"
            | "libs"
            | "crate"
            | "crates"
            | "package"
            | "packages"
            | "test"
            | "tests"
            | "spec"
            | "specs"
            | "fixture"
            | "fixtures"
            | "example"
            | "examples"
            | "script"
            | "scripts"
    )
}

/// Extract primary declaration names from a deterministic content summary.
fn summary_primary_declarations(summary: &str) -> Option<String> {
    let after_marker = summary
        .split_once(" source defining ")
        .map(|(_, value)| value)
        .or_else(|| summary.split_once(" declaring ").map(|(_, value)| value))?;
    let declaration_clause = trim_summary_clause(after_marker);
    let names = strip_declaration_kind_prefix(declaration_clause)
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .take(3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if names.is_empty() {
        None
    } else {
        Some(join_human_names(&names))
    }
}

/// Trim trailing summary details from a declaration clause.
fn trim_summary_clause(value: &str) -> &str {
    value
        .split(" with imports ")
        .next()
        .unwrap_or(value)
        .split(" and depending on ")
        .next()
        .unwrap_or(value)
        .trim_end_matches('.')
        .trim()
}

/// Remove the deterministic symbol-kind phrase before the primary names.
fn strip_declaration_kind_prefix(value: &str) -> &str {
    const PREFIXES: &[&str] = &[
        "types and functions ",
        "type and functions ",
        "types and function ",
        "type and function ",
        "manifest entries ",
        "functions ",
        "function ",
        "types ",
        "type ",
        "bindings ",
        "binding ",
        "values ",
        "value ",
        "symbols ",
    ];
    PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value)
}

/// Join declaration names as a compact human phrase.
fn join_human_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, second, third, ..] => format!("{first}, {second}, and {third}"),
    }
}

/// Return a non-empty substring between two markers.
fn summary_between<'a>(summary: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let after_start = summary.split_once(start)?.1;
    let value = after_start.split_once(end)?.0.trim();
    (!value.is_empty()).then_some(value)
}

/// Return whether a language should be parsed for symbols.
pub(crate) fn is_symbol_candidate(path: &str, language: Option<&str>) -> bool {
    let Some(language) = language else {
        return path.ends_with("Cargo.toml")
            || path.ends_with("Cargo.lock")
            || Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    ["vue", "ps1", "psm1", "psd1"]
                        .iter()
                        .any(|expected| extension.eq_ignore_ascii_case(expected))
                });
    };
    language_capability(language)
        .is_none_or(|capability| capability.symbol_parser != SymbolParserOwner::Unavailable)
}

/// Apply the project-selected optional-language boundary to symbol work admission.
fn is_symbol_candidate_for_admission(
    path: &str,
    language: Option<&str>,
    admit_optional_languages: bool,
) -> bool {
    if !admit_optional_languages
        && language
            .and_then(language_capability)
            .is_some_and(|capability| capability.optional_pack.is_some())
    {
        return false;
    }
    is_symbol_candidate(path, language)
}

/// Clear stale symbol output while preserving structural summaries when present.
fn clear_skipped_symbol_index(
    store: &AtlasStore,
    path: &str,
    language: Option<&str>,
) -> Result<(), CliError> {
    if is_structural_summary_candidate(path, language) {
        store.clear_symbol_graph_for_path(path)?;
    } else {
        store.clear_source_index_for_path(path)?;
    }
    Ok(())
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
pub(crate) fn validated_file_key(file: &Path) -> Result<String, CliError> {
    validated_repo_file_key(file).map_err(|source| CliError::InvalidInput(source.to_string()))
}

/// Normalize a folder filter into the repository path convention.
pub(crate) fn normalized_folder_filter(folder: &str) -> Result<String, CliError> {
    let trimmed = folder.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "." {
        return Ok(".".to_string());
    }
    validated_file_key(Path::new(trimmed)).map_err(|_error| {
        CliError::InvalidInput(format!(
            "folder filter {folder:?} must be a project-relative path"
        ))
    })
}

/// Validate that a path belongs to the indexed project file set.
pub(crate) fn validated_indexed_file_key(
    store: &AtlasStore,
    file: &Path,
) -> Result<String, CliError> {
    let file_key = validated_file_key(file)?;
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| CliError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    if indexed.node.kind != NodeKind::File {
        return Err(CliError::InvalidInput(format!(
            "path {file_key:?} is not an indexed file"
        )));
    }
    Ok(file_key)
}

/// Load the project root recorded by the latest scan.
pub(crate) fn indexed_project_root(store: &AtlasStore) -> Result<PathBuf, CliError> {
    store.project_root()?.map(PathBuf::from).ok_or_else(|| {
        CliError::InvalidInput(
            "indexed project root is missing; run projectatlas scan <project-root> first"
                .to_string(),
        )
    })
}

/// Build an absolute native path for a previously validated indexed file key.
pub(crate) fn indexed_native_path(store: &AtlasStore, file_key: &str) -> Result<PathBuf, CliError> {
    Ok(indexed_project_root(store)?.join(repo_path_to_native(file_key)))
}

/// Read content for a previously validated indexed file key.
pub(crate) fn read_indexed_file_content(
    store: &AtlasStore,
    file_key: &str,
) -> Result<String, CliError> {
    let native = indexed_native_path(store, file_key)?;
    let indexed = store.load_node_by_path(file_key)?.ok_or_else(|| {
        CliError::InvalidInput(format!("indexed file {file_key:?} was not found"))
    })?;
    let project_root = normalize_native_path_display(indexed_project_root(store)?);
    let metadata = match fs::metadata(&native) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(CliError::RefreshRequired(Box::new(IndexRefreshRequired {
                project_root,
                status: IndexReadStatus::RefreshRequired,
                reason: IndexRefreshReason::PathsChanged,
                scope: IndexRefreshScope::Full,
                changed: 1,
                added: 0,
                removed: 1,
                modified: 0,
                sample_paths: vec![file_key.to_string()],
            })));
        }
        Err(source) => {
            return Err(CliError::VerificationIncomplete(Box::new(
                IndexVerificationIncomplete {
                    project_root,
                    status: IndexReadStatus::VerificationIncomplete,
                    reason: IndexVerificationReason::SourceInspectionFailed,
                    scope: IndexRefreshScope::Full,
                    message: format!("failed to read '{}': {source}", native.display()),
                },
            )));
        }
    };
    if indexed
        .node
        .size_bytes
        .is_some_and(|indexed_bytes| indexed_bytes != metadata.len())
    {
        return Err(CliError::RefreshRequired(Box::new(IndexRefreshRequired {
            project_root,
            status: IndexReadStatus::RefreshRequired,
            reason: IndexRefreshReason::SourceChanged,
            scope: IndexRefreshScope::Full,
            changed: 1,
            added: 0,
            removed: 0,
            modified: 1,
            sample_paths: vec![file_key.to_string()],
        })));
    }
    if metadata.len() > MAX_INDEXED_NAVIGATION_SOURCE_BYTES {
        return Err(CliError::VerificationIncomplete(Box::new(
            IndexVerificationIncomplete {
                project_root,
                status: IndexReadStatus::VerificationIncomplete,
                reason: IndexVerificationReason::SourceTooLarge,
                scope: IndexRefreshScope::Full,
                message: format!(
                    "indexed file {file_key:?} contains {} bytes; bounded navigation reads admit at most {MAX_INDEXED_NAVIGATION_SOURCE_BYTES} bytes",
                    metadata.len()
                ),
            },
        )));
    }
    let file = fs::File::open(&native).map_err(|source| {
        CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
            project_root: project_root.clone(),
            status: IndexReadStatus::VerificationIncomplete,
            reason: IndexVerificationReason::SourceInspectionFailed,
            scope: IndexRefreshScope::Full,
            message: format!("failed to open '{}': {source}", native.display()),
        }))
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_INDEXED_NAVIGATION_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| {
            CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
                project_root: project_root.clone(),
                status: IndexReadStatus::VerificationIncomplete,
                reason: IndexVerificationReason::SourceInspectionFailed,
                scope: IndexRefreshScope::Full,
                message: format!("failed to read '{}': {source}", native.display()),
            }))
        })?;
    if bytes.len() as u64 != metadata.len()
        || bytes.len() as u64 > MAX_INDEXED_NAVIGATION_SOURCE_BYTES
    {
        return Err(CliError::RefreshRequired(Box::new(IndexRefreshRequired {
            project_root,
            status: IndexReadStatus::RefreshRequired,
            reason: IndexRefreshReason::SourceChanged,
            scope: IndexRefreshScope::Full,
            changed: 1,
            added: 0,
            removed: 0,
            modified: 1,
            sample_paths: vec![file_key.to_string()],
        })));
    }
    let current_hash = blake3::hash(&bytes).to_hex().to_string();
    if indexed.node.content_hash.as_deref() != Some(current_hash.as_str()) {
        return Err(CliError::RefreshRequired(Box::new(IndexRefreshRequired {
            project_root,
            status: IndexReadStatus::RefreshRequired,
            reason: IndexRefreshReason::SourceChanged,
            scope: IndexRefreshScope::Full,
            changed: 1,
            added: 0,
            removed: 0,
            modified: 1,
            sample_paths: vec![file_key.to_string()],
        })));
    }
    String::from_utf8(bytes).map_err(|source| {
        CliError::VerificationIncomplete(Box::new(IndexVerificationIncomplete {
            project_root,
            status: IndexReadStatus::VerificationIncomplete,
            reason: IndexVerificationReason::SourceInspectionFailed,
            scope: IndexRefreshScope::Full,
            message: format!("indexed file {file_key:?} is not valid UTF-8: {source}"),
        }))
    })
}

/// Run the watcher refresh loop.
pub(crate) fn run_watch_loop(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    once: bool,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
) -> Result<WatchReport, CliError> {
    if once {
        return run_single_watch_refresh(store, plan, symbol_options);
    }
    run_watch_with_polling_fallback(
        store,
        plan,
        poll_seconds,
        max_cycles,
        symbol_options,
        |store| run_notify_watch_loop(store, plan, poll_seconds, max_cycles, symbol_options),
    )
}

/// Run an event-backed watcher and preserve current changes through polling fallback.
fn run_watch_with_polling_fallback<F>(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    run_notify: F,
) -> Result<WatchReport, CliError>
where
    F: FnOnce(&mut AtlasStore) -> Result<WatchReport, CliError>,
{
    match run_notify(store) {
        Ok(report) => Ok(report),
        Err(error @ CliError::RefreshRequired(_)) => Err(error),
        Err(error) => run_polling_watch_loop(
            store,
            plan,
            poll_seconds,
            max_cycles,
            symbol_options,
            Some(error.to_string()),
        ),
    }
}

/// Run one deterministic watcher refresh and exit.
pub(crate) fn run_single_watch_refresh(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
) -> Result<WatchReport, CliError> {
    let control = index_work_control(symbol_options);
    run_single_watch_refresh_controlled(store, plan, symbol_options, &control)
}

/// Run one watcher refresh under one cancellation and publication boundary.
pub(crate) fn run_single_watch_refresh_controlled(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<WatchReport, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    control.check(IndexWorkStage::RepositoryTraversal)?;
    let current_plan = plan.reload_controlled(control)?;
    let last_refresh = refresh_index_controlled(store, &current_plan, symbol_options, control)?;
    Ok(WatchReport {
        mode: WATCH_MODE_ONCE.to_string(),
        cycles: 1,
        once: true,
        fallback_reason: None,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Run an event-backed watcher loop with `notify`.
pub(crate) fn run_notify_watch_loop(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
) -> Result<WatchReport, CliError> {
    let mut current_plan = plan.reload()?;
    let watch_root = current_plan
        .root
        .canonicalize()
        .map_err(|source| CliError::Io {
            path: current_plan.root.clone(),
            source,
        })?;
    let (sender, receiver) = mpsc::sync_channel(WATCH_EVENT_QUEUE_CAPACITY);
    let continuity_lost = Arc::new(AtomicBool::new(false));
    let callback_continuity_lost = Arc::clone(&continuity_lost);
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            match sender.try_send(result) {
                Ok(()) => {}
                Err(TrySendError::Full(_result)) => {
                    callback_continuity_lost.store(true, Ordering::Release);
                }
                Err(TrySendError::Disconnected(_result)) => {
                    // Receiver shutdown means the command is exiting.
                }
            }
        },
        Config::default(),
    )
    .map_err(|source| CliError::Watcher(source.to_string()))?;
    watcher
        .watch(&watch_root, RecursiveMode::Recursive)
        .map_err(|source| CliError::Watcher(source.to_string()))?;
    let debounce = Duration::from_secs(poll_seconds.max(1));
    let mut cycles = 0;
    let mut last_refresh = refresh_index(store, &current_plan, symbol_options)?;
    cycles += 1;
    while max_cycles == 0 || cycles < max_cycles {
        let changes = wait_for_index_event_with_continuity(
            &receiver,
            &watch_root,
            debounce,
            &current_plan.scan_options,
            &continuity_lost,
        )?;
        if changes.has_changes() {
            current_plan = plan.reload()?;
            last_refresh =
                refresh_index_for_changes(store, &current_plan, &changes, symbol_options)?;
            cycles += 1;
        }
    }
    Ok(WatchReport {
        mode: WATCH_MODE_NOTIFY.to_string(),
        cycles,
        once: false,
        fallback_reason: None,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Wait for one bounded event batch and preserve local queue-overflow uncertainty.
fn wait_for_index_event_with_continuity(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    root: &Path,
    debounce: Duration,
    scan_options: &ScanOptions,
    continuity_lost: &AtomicBool,
) -> Result<WatchChangeSet, CliError> {
    let mut changes = notify_result_changes(
        root,
        scan_options,
        receiver.recv().map_err(|source| {
            CliError::Watcher(format!("watch event channel disconnected: {source}"))
        })?,
    )?;
    loop {
        match receiver.recv_timeout(debounce) {
            Ok(result) => {
                changes.merge(notify_result_changes(root, scan_options, result)?);
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(CliError::Watcher(
                    "watch event channel disconnected".to_string(),
                ));
            }
        }
    }
    if continuity_lost.swap(false, Ordering::AcqRel) {
        changes.requires_full_scan = true;
    }
    Ok(changes)
}

/// Convert a `notify` result into index-relevant changes.
pub(crate) fn notify_result_changes(
    root: &Path,
    scan_options: &ScanOptions,
    result: notify::Result<Event>,
) -> Result<WatchChangeSet, CliError> {
    let event = result.map_err(|source| CliError::Watcher(source.to_string()))?;
    Ok(notify_event_changes(root, scan_options, &event))
}

/// Convert a `notify` event into index-relevant changes.
pub(crate) fn notify_event_changes(
    root: &Path,
    scan_options: &ScanOptions,
    event: &Event,
) -> WatchChangeSet {
    if !event_kind_affects_index(event.kind) {
        return WatchChangeSet::default();
    }
    let mut changes = WatchChangeSet {
        requires_full_scan: event.need_rescan(),
        paths: HashSet::new(),
    };
    for path in &event.paths {
        let candidate = absolute_watch_path(root, path);
        if watch_path_requires_full_scan(root, &candidate) {
            changes.requires_full_scan = true;
            changes.paths.insert(candidate);
            continue;
        }
        let Some(index_path) = normalized_watch_index_path(root, path, scan_options) else {
            continue;
        };
        if matches!(
            event.kind,
            EventKind::Modify(notify::event::ModifyKind::Name(_))
        ) || watch_path_requires_full_scan(root, &index_path)
        {
            changes.requires_full_scan = true;
        }
        changes.paths.insert(index_path);
    }
    changes
}

/// Return whether a `notify` event kind can change indexed content.
pub(crate) fn event_kind_affects_index(kind: EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

/// Return whether a native event path belongs to indexed repository content.
#[cfg(test)]
pub(crate) fn watch_path_affects_index(
    root: &Path,
    path: &Path,
    scan_options: &ScanOptions,
) -> bool {
    normalized_watch_index_path(root, path, scan_options).is_some()
}

/// Return one repository-contained native path after watcher normalization and policy checks.
fn normalized_watch_index_path(
    root: &Path,
    path: &Path,
    scan_options: &ScanOptions,
) -> Option<PathBuf> {
    let candidate = absolute_watch_path(root, path);
    let relative = safe_watch_relative_path(root, &candidate)?;
    if relative == "." {
        return Some(root.to_path_buf());
    }
    let policy_path = if candidate.strip_prefix(root).is_ok() {
        candidate.clone()
    } else {
        match candidate.try_exists() {
            Ok(true) => candidate.clone(),
            Ok(false) => root.join(repo_path_to_native(&relative)),
            Err(_) => return None,
        }
    };
    // Unknown ignore state should not admit a path into the incremental index.
    let Ok(gitignore_ignored) = gitignore_excludes_path(root, &policy_path) else {
        return None;
    };
    if gitignore_ignored {
        return None;
    }
    if relative.split('/').any(|component| component == ".purpose")
        || scan_options.excludes_relative_path(&relative)
    {
        return None;
    }
    Some(candidate)
}

/// Return a safe normalized repository path for a watcher event.
fn safe_watch_relative_path(root: &Path, candidate: &Path) -> Option<String> {
    let relative = normalize_repo_path(root, candidate)
        .ok()
        .or_else(|| native_display_relative_path(root, candidate))?;
    valid_watch_relative_path(relative)
}

/// Reconcile equivalent native paths when Windows extended prefixes differ.
fn native_display_relative_path(root: &Path, candidate: &Path) -> Option<String> {
    let root = normalize_native_path_display_str(root.to_str()?);
    let candidate = normalize_native_path_display_str(candidate.to_str()?);
    let root = if root == "/" {
        root.as_str()
    } else {
        root.trim_end_matches('/')
    };
    if candidate == root || cfg!(windows) && candidate.eq_ignore_ascii_case(root) {
        return Some(".".to_string());
    }
    let prefix = if root == "/" {
        "/".to_string()
    } else {
        format!("{root}/")
    };
    if let Some(relative) = candidate.strip_prefix(&prefix) {
        return Some(relative.to_string());
    }
    #[cfg(windows)]
    {
        let prefix_candidate = candidate.get(..prefix.len())?;
        if prefix_candidate.eq_ignore_ascii_case(&prefix) {
            return candidate.get(prefix.len()..).map(ToOwned::to_owned);
        }
    }
    None
}

/// Reject empty, current-directory, and parent traversal path components.
fn valid_watch_relative_path(relative: String) -> Option<String> {
    if relative == "." {
        return Some(relative);
    }
    if relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return None;
    }
    Some(relative)
}

/// Return an absolute path for a watcher event path.
pub(crate) fn absolute_watch_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Return whether a path event requires a full scan for correctness.
pub(crate) fn watch_path_requires_full_scan(root: &Path, path: &Path) -> bool {
    let Some(relative) = safe_watch_relative_path(root, path) else {
        return false;
    };
    if relative == "." {
        return true;
    }
    path.is_dir()
        || matches!(relative.rsplit('/').next(), Some(".gitignore" | ".ignore"))
        || relative == ".git"
        || relative.ends_with("/.git")
        || relative == ".git/info/exclude"
        || relative.ends_with("/.git/info/exclude")
        || matches!(
            relative.rsplit('/').next(),
            Some("tsconfig.json" | "jsconfig.json")
        )
        || index_policy_path(relative.as_str())
}

/// Return whether one repository-relative path owns derived-index policy.
fn index_policy_path(relative: &str) -> bool {
    CORE_INDEX_POLICY_PATHS.contains(&relative)
        || cfg!(feature = "optional-parser-supervisor") && {
            #[cfg(feature = "optional-parser-supervisor")]
            {
                relative == OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH
            }
            #[cfg(not(feature = "optional-parser-supervisor"))]
            {
                false
            }
        }
}

/// Run the portable polling watcher fallback loop.
pub(crate) fn run_polling_watch_loop(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    fallback_reason: Option<String>,
) -> Result<WatchReport, CliError> {
    let mut cycles = 0;
    let mut current_plan = plan.reload()?;
    let mut last_refresh = refresh_index(store, &current_plan, symbol_options)?;
    cycles += 1;
    while max_cycles == 0 || cycles < max_cycles {
        thread::sleep(Duration::from_secs(poll_seconds.max(1)));
        current_plan = plan.reload()?;
        last_refresh = refresh_index(store, &current_plan, symbol_options)?;
        cycles += 1;
    }
    Ok(WatchReport {
        mode: WATCH_MODE_POLLING.to_string(),
        cycles,
        once: false,
        fallback_reason,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Combined refresh output for watcher and one-shot refresh paths.
pub(crate) struct IndexRefreshReport {
    /// Persisted text search index refresh report.
    pub(crate) text_index: TextIndexReport,
    /// Structural summary refresh report.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Deep symbol graph refresh report.
    symbols: SymbolBuildReport,
}

/// Refresh filesystem and symbol state.
pub(crate) fn refresh_index(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
) -> Result<IndexRefreshReport, CliError> {
    let control = index_work_control(symbol_options);
    refresh_index_controlled(store, plan, symbol_options, &control)
}

/// Refresh every derived projection under one cancellation boundary.
pub(crate) fn refresh_index_controlled(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<IndexRefreshReport, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    let reuse_unchanged_symbols = publication_contract_matches(store, plan)?;
    if reuse_unchanged_symbols
        && detect_index_freshness_controlled(store, plan, ScanLimits::default(), control)?
            .delta
            .is_none()
    {
        graph_projection::cleanup_abandoned_repository_graph_staging(store, &plan.root, control)?;
        return Ok(empty_index_refresh_report(plan.text_options));
    }
    let batch = stage_full_index_publication(
        store,
        plan,
        symbol_options,
        reuse_unchanged_symbols,
        false,
        control,
    )?;
    revalidate_staged_publication_inputs_controlled(
        plan,
        batch.nodes.expected_nodes(),
        None,
        control,
    )?;
    if staged_full_refresh_is_unchanged(store, &batch)? {
        return Ok(empty_index_refresh_report(plan.text_options));
    }
    let outcome = publish_index_batch(store, batch, control)?;
    Ok(IndexRefreshReport {
        text_index: outcome.text_index,
        structural_summaries: outcome.structural_summaries,
        symbols: outcome.symbols,
    })
}

/// Return whether one fully staged watcher refresh matches complete durable state.
fn staged_full_refresh_is_unchanged(
    store: &AtlasStore,
    batch: &IndexPublicationBatch,
) -> Result<bool, CliError> {
    let NodePublicationBatch::Full { nodes: expected } = &batch.nodes else {
        return Ok(false);
    };
    if batch.purpose_import.is_some() {
        return Ok(false);
    }
    let Some(publication) = store.index_publication()? else {
        return Ok(false);
    };
    if publication.state != IndexPublicationState::Complete
        || publication.generation != batch.base_generation
        || publication.contract_fingerprint.as_deref() != Some(batch.contract_fingerprint.as_str())
    {
        return Ok(false);
    }
    let current = store
        .load_nodes()?
        .into_iter()
        .map(|indexed| indexed.node)
        .collect::<Vec<_>>();
    Ok(current.len() == expected.len()
        && current
            .iter()
            .zip(expected)
            .all(|(current, expected)| same_indexed_source(current, expected)))
}

/// Build the stable report for a verified watcher no-op.
fn empty_index_refresh_report(text_options: TextIndexOptions) -> IndexRefreshReport {
    IndexRefreshReport {
        text_index: empty_text_index_report(text_options),
        structural_summaries: StructuralSummaryReport::default(),
        symbols: empty_symbol_build_report(),
    }
}

/// Refresh filesystem and symbol state for a debounced event batch.
pub(crate) fn refresh_index_for_changes(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    changes: &WatchChangeSet,
    symbol_options: &SymbolBuildOptions,
) -> Result<IndexRefreshReport, CliError> {
    let control = index_work_control(symbol_options);
    refresh_index_for_changes_controlled(store, plan, changes, symbol_options, &control)
}

/// Refresh one watcher batch under one cancellation and publication boundary.
pub(crate) fn refresh_index_for_changes_controlled(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    changes: &WatchChangeSet,
    symbol_options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<IndexRefreshReport, CliError> {
    let bounded_control = bounded_index_work_control(control);
    let control = &bounded_control;
    control.check(IndexWorkStage::RepositoryTraversal)?;
    if changes.requires_full_scan || !publication_contract_matches(store, plan)? {
        return refresh_index_controlled(store, plan, symbol_options, control);
    }
    if changes.paths.len() > MAX_INCREMENTAL_CHANGED_PATHS {
        return Err(IndexWorkFailure::resource_limit(
            IndexWorkStage::RepositoryTraversal,
            IndexWorkResource::Entries,
            MAX_INCREMENTAL_CHANGED_PATHS as u64,
            changes.paths.len() as u64,
        )
        .into());
    }
    let root = &plan.root;
    let base_generation = publication_base_generation(store)?;
    let baseline_nodes = store
        .load_nodes()?
        .into_iter()
        .map(|indexed| indexed.node)
        .collect::<Vec<_>>();
    let baseline_by_path = baseline_nodes
        .iter()
        .map(|node| (node.path.clone(), node))
        .collect::<HashMap<_, _>>();
    let mut nodes = Vec::new();
    let mut absent_paths = Vec::new();
    let mut source_bytes = 0_u64;
    let scan_policy = RootScanPolicy::discover(root, &plan.scan_options, control)
        .map_err(|source| source_inspection_error(root, source))?;
    for path in sorted_watch_paths(&changes.paths) {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        match path.try_exists() {
            Ok(true) => {
                let remaining_source_bytes =
                    MAX_INCREMENTAL_SOURCE_BYTES.saturating_sub(source_bytes);
                if let Some(node) = scan_path_with_policy_controlled(
                    &scan_policy,
                    &path,
                    ScanLimits::new(1, remaining_source_bytes, 1),
                    control,
                )
                .map_err(|source| source_inspection_error(root, source))?
                {
                    source_bytes = source_bytes
                        .checked_add(node.size_bytes.unwrap_or_default())
                        .ok_or_else(|| {
                            IndexWorkFailure::resource_limit(
                                IndexWorkStage::SourceMetadata,
                                IndexWorkResource::SourceBytes,
                                MAX_INCREMENTAL_SOURCE_BYTES,
                                u64::MAX,
                            )
                        })?;
                    if source_bytes > MAX_INCREMENTAL_SOURCE_BYTES {
                        return Err(IndexWorkFailure::resource_limit(
                            IndexWorkStage::SourceMetadata,
                            IndexWorkResource::SourceBytes,
                            MAX_INCREMENTAL_SOURCE_BYTES,
                            source_bytes,
                        )
                        .into());
                    }
                    nodes.push(node);
                } else if let Some(path_key) = normalized_deleted_path(root, &path)? {
                    absent_paths.push(path_key);
                }
            }
            Ok(false) => {
                if let Some(path_key) = normalized_deleted_path(root, &path)? {
                    absent_paths.push(path_key);
                }
            }
            Err(source) => {
                return Err(CliError::VerificationIncomplete(Box::new(
                    IndexVerificationIncomplete {
                        project_root: normalize_native_path_display(root),
                        status: IndexReadStatus::VerificationIncomplete,
                        reason: IndexVerificationReason::SourceInspectionFailed,
                        scope: IndexRefreshScope::Full,
                        message: format!("failed to inspect '{}': {source}", path.display()),
                    },
                )));
            }
        }
    }
    absent_paths.sort();
    absent_paths.dedup();
    let candidate_paths = nodes
        .iter()
        .map(|node| node.path.clone())
        .chain(absent_paths.iter().cloned())
        .collect::<HashSet<_>>();
    let mut sorted_candidate_paths = candidate_paths.into_iter().collect::<Vec<_>>();
    sorted_candidate_paths.sort();
    let existing_nodes = sorted_candidate_paths
        .iter()
        .filter_map(|path| {
            baseline_by_path
                .get(path)
                .map(|node| (path.clone(), (*node).clone()))
        })
        .collect::<HashMap<_, _>>();
    if absent_paths.iter().any(|path| {
        existing_nodes
            .get(path)
            .is_some_and(|node| node.kind == NodeKind::Folder)
    }) {
        return refresh_index_controlled(store, plan, symbol_options, control);
    }
    graph_projection::cleanup_abandoned_repository_graph_staging(store, root, control)?;
    let direct_graph_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .chain(absent_paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    nodes.retain(|node| {
        existing_nodes
            .get(&node.path)
            .is_none_or(|indexed| !same_indexed_source(node, indexed))
    });
    absent_paths.retain(|path| existing_nodes.contains_key(path));
    if nodes.is_empty() && absent_paths.is_empty() {
        revalidate_staged_publication_inputs_controlled(plan, &baseline_nodes, None, control)?;
        return Ok(empty_index_refresh_report(plan.text_options));
    }
    drop(existing_nodes);
    drop(baseline_by_path);
    let changed_paths = nodes
        .iter()
        .map(|node| node.path.clone())
        .chain(absent_paths.iter().cloned())
        .collect::<HashSet<_>>();
    let previous_hashes = indexed_file_hashes_for_paths(store, &changed_paths)?;
    let mut text_paths = changed_paths.iter().cloned().collect::<Vec<_>>();
    text_paths.sort();
    let text =
        stage_text_index_for_changed_paths_controlled(root, &nodes, plan.text_options, control)?;
    let protected_purpose_paths = protected_purpose_paths(&nodes, None);
    let target_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<HashSet<_>>();
    let expected_nodes = expected_nodes_after_incremental(baseline_nodes, &nodes, &absent_paths);
    let contract_fingerprint = plan.publication_contract_fingerprint();
    let retained_before_symbols = staged_publication_identity_bytes(root, &contract_fingerprint)
        .saturating_add(staged_string_bytes(&text_paths))
        .saturating_add(staged_string_bytes(&absent_paths))
        .saturating_add(staged_node_bytes(&expected_nodes))
        .saturating_add(staged_node_bytes(&nodes))
        .saturating_add(staged_text_bytes(&text));
    let symbol_limits = symbol_limits_with_remaining_staging_bytes(retained_before_symbols)?;
    let symbols = stage_symbols_for_nodes_with_limits(
        store,
        root,
        #[cfg(feature = "optional-parser-supervisor")]
        &plan.optional_parser_selection,
        &nodes,
        symbol_options,
        Some(&previous_hashes),
        Some(&target_paths),
        &protected_purpose_paths,
        control,
        symbol_limits,
    )?;
    let graph = graph_projection::stage_incremental_repository_graph(
        store,
        root,
        base_generation,
        &expected_nodes,
        &direct_graph_paths.into_iter().collect::<Vec<_>>(),
        &symbols,
        control,
    )?;
    let structural_summaries = stage_structural_summaries_for_nodes_controlled(
        store,
        &nodes,
        &text.rows,
        Some(&symbols),
        &protected_purpose_paths,
        symbol_options.effective_workers(),
        control,
    )?;
    enforce_publication_staging_budget(
        retained_before_symbols
            .saturating_add(symbols.retained_bytes)
            .saturating_add(graph.retained_bytes())
            .saturating_add(structural_summaries.retained_bytes),
    )?;
    let batch = IndexPublicationBatch {
        base_generation,
        contract_fingerprint,
        root: root.clone(),
        nodes: NodePublicationBatch::Incremental {
            nodes,
            absent_paths,
            expected_nodes,
        },
        purpose_import: None,
        text_paths,
        text,
        symbols,
        graph,
        structural_summaries,
    };
    revalidate_staged_publication_inputs_controlled(
        plan,
        batch.nodes.expected_nodes(),
        None,
        control,
    )?;
    let outcome = publish_index_batch(store, batch, control)?;
    Ok(IndexRefreshReport {
        text_index: outcome.text_index,
        structural_summaries: outcome.structural_summaries,
        symbols: outcome.symbols,
    })
}

/// Seed built-in purposes for reserved `ProjectAtlas` metadata nodes when needed.
pub(crate) fn seed_builtin_projectatlas_purposes(
    store: &AtlasStore,
    nodes: &[Node],
) -> Result<(), CliError> {
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    for (path, purpose) in BUILTIN_PROJECTATLAS_PURPOSES {
        if !indexed_paths.contains(path) {
            continue;
        }
        let Some(indexed) = store.load_node_by_path(path)? else {
            continue;
        };
        if !matches!(
            indexed.purpose.status,
            PurposeStatus::Approved | PurposeStatus::Stale
        ) {
            store.set_purpose(path, purpose, PurposeSource::Imported)?;
        }
    }
    Ok(())
}

/// Refresh structural summaries while observing the operation work boundary.
#[cfg(test)]
pub(crate) fn refresh_structural_summaries_for_nodes(
    store: &mut AtlasStore,
    nodes: &[Node],
    text_rows: &[TextIndexRow],
) -> Result<StructuralSummaryReport, CliError> {
    let control = standalone_index_work_control();
    refresh_structural_summaries_for_nodes_controlled(store, nodes, text_rows, &control)
}

/// Refresh structural summaries while observing the operation work boundary.
#[cfg(test)]
fn refresh_structural_summaries_for_nodes_controlled(
    store: &mut AtlasStore,
    nodes: &[Node],
    text_rows: &[TextIndexRow],
    control: &IndexWorkControl,
) -> Result<StructuralSummaryReport, CliError> {
    let staged = stage_structural_summaries_for_nodes_controlled(
        store,
        nodes,
        text_rows,
        None,
        &HashSet::new(),
        2,
        control,
    )?;
    apply_structural_summary_stage(store, &staged, control)?;
    Ok(staged.report)
}

/// Derive structural summary mutations without acquiring the `SQLite` writer.
fn stage_structural_summaries_for_nodes_controlled(
    store: &AtlasStore,
    nodes: &[Node],
    text_rows: &[TextIndexRow],
    symbols: Option<&SymbolBuildStage>,
    protected_purpose_paths: &HashSet<String>,
    max_workers: usize,
    control: &IndexWorkControl,
) -> Result<StructuralSummaryStage, CliError> {
    control.check(IndexWorkStage::TextIndex)?;
    let candidates = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| is_structural_summary_candidate(&node.path, node.language.as_deref()))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(StructuralSummaryStage {
            report: StructuralSummaryReport::default(),
            changes: Vec::new(),
            retained_bytes: 0,
        });
    }
    let paths = candidates
        .iter()
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    let indexed_nodes = store
        .load_nodes_by_paths(&paths)?
        .into_iter()
        .map(|indexed| (indexed.node.path.clone(), indexed))
        .collect::<HashMap<_, _>>();
    let symbol_counts = store.symbol_counts_for_paths(&paths)?;
    let text_by_path = text_rows
        .iter()
        .filter_map(|row| row.text.as_ref().map(|text| (text.path.as_str(), text)))
        .collect::<HashMap<_, _>>();
    let reason_by_path = text_rows
        .iter()
        .map(|row| (row.path.as_str(), row.reason))
        .collect::<HashMap<_, _>>();
    let mut staged_symbol_counts = HashMap::new();
    let mut staged_symbol_summaries = HashMap::new();
    let mut staged_structural_summaries = HashMap::new();
    if let Some(symbols) = symbols {
        for change in &symbols.changes {
            match change {
                SymbolProjectionChange::Parsed(parsed) => {
                    staged_symbol_counts.insert(parsed.path.as_str(), parsed.graph.symbols.len());
                    staged_symbol_summaries.insert(parsed.path.as_str(), parsed.summary.as_str());
                    if parsed.summary_is_structural {
                        staged_structural_summaries
                            .insert(parsed.path.as_str(), parsed.purpose_suggestion.is_some());
                    }
                }
                SymbolProjectionChange::Clear { path, .. } => {
                    staged_symbol_counts.insert(path.as_str(), 0);
                }
            }
        }
    }
    let mut report = StructuralSummaryReport {
        candidates: paths.len(),
        ..StructuralSummaryReport::default()
    };
    let worker_count = worker_count_for_work(candidates.len(), max_workers);
    let pool = ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .build()
        .map_err(|source| {
            CliError::InvalidInput(format!("structural summary worker pool failed: {source}"))
        })?;
    let derivations = pool.install(|| {
        candidates
            .par_iter()
            .map(|node| -> Result<StructuralSummaryDerivation, CliError> {
                control.check(IndexWorkStage::TextIndex)?;
                let existing = indexed_nodes.get(&node.path);
                if reason_by_path.get(node.path.as_str()) == Some(&TextIndexSkipReason::TooLarge)
                    || node
                        .size_bytes
                        .is_some_and(|size_bytes| size_bytes > MAX_SYMBOL_FILE_BYTES)
                {
                    return Ok(StructuralSummaryDerivation {
                        change: Some(StructuralSummaryChange::Clear {
                            path: node.path.clone(),
                        }),
                        cleared: 1,
                        too_large: 1,
                        retained_bytes: node.path.len() as u64,
                        ..StructuralSummaryDerivation::default()
                    });
                }
                let Some(text) = text_by_path.get(node.path.as_str()) else {
                    return Ok(StructuralSummaryDerivation {
                        change: Some(StructuralSummaryChange::Clear {
                            path: node.path.clone(),
                        }),
                        cleared: 1,
                        binary_or_non_utf8: usize::from(
                            reason_by_path.get(node.path.as_str())
                                == Some(&TextIndexSkipReason::BinaryOrNonUtf8),
                        ),
                        retained_bytes: node.path.len() as u64,
                        ..StructuralSummaryDerivation::default()
                    });
                };
                if let Some(purpose_suggested) = staged_structural_summaries.get(node.path.as_str())
                {
                    return Ok(StructuralSummaryDerivation {
                        summarized: 1,
                        purpose_suggestions: usize::from(*purpose_suggested),
                        ..StructuralSummaryDerivation::default()
                    });
                }
                let symbol_count = staged_symbol_counts
                    .get(node.path.as_str())
                    .copied()
                    .or_else(|| symbol_counts.get(node.path.as_str()).copied())
                    .unwrap_or_default();
                let effective_summary = staged_symbol_summaries
                    .get(node.path.as_str())
                    .copied()
                    .or_else(|| existing.and_then(|indexed| indexed.summary.as_deref()));
                if symbol_count > 0
                    && effective_summary.is_some_and(|summary| {
                        !summary.trim().is_empty() && !is_scanner_fallback_summary(summary)
                    })
                {
                    return Ok(StructuralSummaryDerivation::default());
                }
                let Some(summary) = structural_summary_for_path(
                    &node.path,
                    node.language.as_deref(),
                    &text.content,
                ) else {
                    return Ok(StructuralSummaryDerivation {
                        change: Some(StructuralSummaryChange::Clear {
                            path: node.path.clone(),
                        }),
                        cleared: 1,
                        retained_bytes: node.path.len() as u64,
                        ..StructuralSummaryDerivation::default()
                    });
                };
                let purpose_needs_suggestion = !protected_purpose_paths.contains(&node.path)
                    && existing.is_none_or(|indexed| {
                        matches!(
                            indexed.purpose.status,
                            PurposeStatus::Missing | PurposeStatus::Suggested
                        )
                    });
                let purpose_suggestion =
                    purpose_needs_suggestion.then(|| suggest_file_purpose(&node.path, &summary));
                let purpose_suggestions = usize::from(purpose_suggestion.is_some());
                let retained_bytes = (node.path.len() as u64)
                    .saturating_add(summary.len() as u64)
                    .saturating_add(
                        purpose_suggestion
                            .as_ref()
                            .map_or(0, |suggestion| suggestion.len() as u64),
                    );
                control.check(IndexWorkStage::TextIndex)?;
                Ok(StructuralSummaryDerivation {
                    change: Some(StructuralSummaryChange::Set {
                        path: node.path.clone(),
                        summary,
                        purpose_suggestion,
                    }),
                    summarized: 1,
                    purpose_suggestions,
                    retained_bytes,
                    ..StructuralSummaryDerivation::default()
                })
            })
            .collect::<Result<Vec<_>, CliError>>()
    })?;
    let mut changes = Vec::new();
    let mut retained_bytes = 0_u64;
    for derivation in derivations {
        report.summarized += derivation.summarized;
        report.cleared += derivation.cleared;
        report.too_large += derivation.too_large;
        report.binary_or_non_utf8 += derivation.binary_or_non_utf8;
        report.purpose_suggestions += derivation.purpose_suggestions;
        retained_bytes = retained_bytes.saturating_add(derivation.retained_bytes);
        if let Some(change) = derivation.change {
            changes.push(change);
        }
    }
    Ok(StructuralSummaryStage {
        report,
        changes,
        retained_bytes,
    })
}

/// Apply prepared structural summaries inside the parent publication transaction.
fn apply_structural_summary_stage(
    store: &mut AtlasStore,
    staged: &StructuralSummaryStage,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    for change in &staged.changes {
        control.check(IndexWorkStage::Publication)?;
        match change {
            StructuralSummaryChange::Set {
                path,
                summary,
                purpose_suggestion,
            } => {
                store.set_node_summary(path, summary)?;
                if let Some(suggestion) = purpose_suggestion.as_deref() {
                    store.set_suggested_purpose(path, suggestion)?;
                }
            }
            StructuralSummaryChange::Clear { path } => store.clear_node_summary(path)?,
        }
    }
    control.check(IndexWorkStage::Publication)?;
    Ok(())
}

/// Refresh the persisted text index for every scanned file node.
#[cfg(test)]
pub(crate) fn refresh_text_index_for_nodes(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<TextIndexReport, CliError> {
    let control = standalone_index_work_control();
    Ok(
        refresh_text_index_for_nodes_with_rows_controlled(store, root, nodes, options, &control)?
            .report,
    )
}

/// Refresh all text rows under one cancellation and staging-byte boundary.
#[cfg(test)]
pub(crate) fn refresh_text_index_for_nodes_with_rows(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<TextIndexRefresh, CliError> {
    let control = standalone_index_work_control();
    refresh_text_index_for_nodes_with_rows_controlled(store, root, nodes, options, &control)
}

/// Refresh all text rows under one cancellation and staging-byte boundary.
#[cfg(test)]
fn refresh_text_index_for_nodes_with_rows_controlled(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
    control: &IndexWorkControl,
) -> Result<TextIndexRefresh, CliError> {
    let file_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    refresh_text_index_for_changed_paths_with_rows_controlled(
        store,
        root,
        &file_paths,
        nodes,
        options,
        control,
    )
}

/// Refresh selected text rows under one cancellation and staging-byte boundary.
#[cfg(test)]
fn refresh_text_index_for_changed_paths_with_rows_controlled(
    store: &mut AtlasStore,
    root: &Path,
    considered_paths: &[String],
    nodes: &[Node],
    options: TextIndexOptions,
    control: &IndexWorkControl,
) -> Result<TextIndexRefresh, CliError> {
    let staged = stage_text_index_for_changed_paths_controlled(root, nodes, options, control)?;
    apply_text_index_stage(store, considered_paths, &staged, control)?;
    Ok(staged)
}

/// Build selected persisted-text rows without acquiring the `SQLite` writer.
fn stage_text_index_for_changed_paths_controlled(
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
    control: &IndexWorkControl,
) -> Result<TextIndexRefresh, CliError> {
    control.check(IndexWorkStage::TextIndex)?;
    let text_rows = indexed_file_texts_for_nodes_controlled(root, nodes, options, control)?;
    let indexed = text_rows.iter().filter(|row| row.text.is_some()).count();
    let indexed_bytes = text_rows
        .iter()
        .filter_map(|row| row.text.as_ref())
        .map(|text| text.byte_count)
        .fold(0usize, usize::saturating_add);
    let file_candidates = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .count();
    let binary_or_non_utf8 = text_rows
        .iter()
        .filter(|row| row.reason == TextIndexSkipReason::BinaryOrNonUtf8)
        .count();
    let too_large = text_rows
        .iter()
        .filter(|row| row.reason == TextIndexSkipReason::TooLarge)
        .count();
    let report = TextIndexReport {
        candidates: file_candidates,
        indexed,
        binary_or_non_utf8,
        too_large,
        skipped: file_candidates.saturating_sub(indexed),
        max_bytes: options.max_bytes,
        bytes: indexed_bytes,
    };
    control.check(IndexWorkStage::TextIndex)?;
    Ok(TextIndexRefresh {
        report,
        rows: text_rows,
    })
}

/// Apply prepared persisted-text rows inside the parent publication transaction.
fn apply_text_index_stage(
    store: &mut AtlasStore,
    considered_paths: &[String],
    staged: &TextIndexRefresh,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let text_by_path = staged
        .rows
        .iter()
        .filter_map(|row| row.text.as_ref().map(|text| (text.path.as_str(), text)))
        .collect::<HashMap<_, _>>();
    for paths in considered_paths.chunks(PUBLICATION_TEXT_BATCH_SIZE) {
        control.check(IndexWorkStage::Publication)?;
        store.replace_file_texts_for_paths(
            paths,
            paths
                .iter()
                .filter_map(|path| text_by_path.get(path.as_str()).copied()),
        )?;
    }
    control.check(IndexWorkStage::Publication)?;
    Ok(())
}

/// Build indexed text rows for UTF-8 scanned files with size caps.
#[cfg(test)]
pub(crate) fn indexed_file_texts_for_nodes(
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<Vec<TextIndexRow>, CliError> {
    let control = standalone_index_work_control();
    indexed_file_texts_for_nodes_controlled(root, nodes, options, &control)
}

/// Build bounded UTF-8 text rows while observing cancellation between files.
fn indexed_file_texts_for_nodes_controlled(
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
    control: &IndexWorkControl,
) -> Result<Vec<TextIndexRow>, CliError> {
    indexed_file_texts_for_nodes_with_limit(root, nodes, options, MAX_STAGED_TEXT_BYTES, control)
}

/// Build UTF-8 text rows under an explicit aggregate staging-byte limit.
fn indexed_file_texts_for_nodes_with_limit(
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
    max_staged_bytes: u64,
    control: &IndexWorkControl,
) -> Result<Vec<TextIndexRow>, CliError> {
    let mut rows = Vec::new();
    let mut staged_bytes = 0_u64;
    for node in nodes.iter().filter(|node| node.kind == NodeKind::File) {
        control.check(IndexWorkStage::TextIndex)?;
        if node
            .size_bytes
            .is_some_and(|size_bytes| size_bytes > options.max_bytes)
        {
            rows.push(TextIndexRow {
                path: node.path.clone(),
                text: None,
                reason: TextIndexSkipReason::TooLarge,
            });
            continue;
        }
        let remaining_staged_bytes = max_staged_bytes.saturating_sub(staged_bytes);
        if node
            .size_bytes
            .is_some_and(|size_bytes| size_bytes > remaining_staged_bytes)
        {
            return Err(IndexWorkFailure::resource_limit(
                IndexWorkStage::TextIndex,
                IndexWorkResource::TextBytes,
                max_staged_bytes,
                staged_bytes.saturating_add(node.size_bytes.unwrap_or_default()),
            )
            .into());
        }
        let native_path = root.join(repo_path_to_native(&node.path));
        let read_limit = options.max_bytes.min(remaining_staged_bytes);
        let aggregate_limit_is_narrower = remaining_staged_bytes <= options.max_bytes;
        let bytes = match read_source_bytes_controlled(
            &native_path,
            read_limit,
            IndexWorkStage::TextIndex,
            control,
        ) {
            Ok(bytes) => bytes,
            Err(SourceReadFailure::Io(source)) => {
                return Err(CliError::Io {
                    path: native_path,
                    source,
                });
            }
            Err(SourceReadFailure::IndexWork(failure)) => return Err(failure.into()),
            Err(SourceReadFailure::LimitExceeded { observed }) if aggregate_limit_is_narrower => {
                return Err(IndexWorkFailure::resource_limit(
                    IndexWorkStage::TextIndex,
                    IndexWorkResource::TextBytes,
                    max_staged_bytes,
                    staged_bytes.saturating_add(observed),
                )
                .into());
            }
            Err(SourceReadFailure::LimitExceeded { .. }) => {
                return Err(source_changed_during_derivation(root, &node.path));
            }
        };
        control.check(IndexWorkStage::TextIndex)?;
        let current_hash = blake3::hash(&bytes).to_hex().to_string();
        if node.content_hash.as_deref() != Some(current_hash.as_str()) {
            return Err(source_changed_during_derivation(root, &node.path));
        }
        let Ok(content) = String::from_utf8(bytes) else {
            rows.push(TextIndexRow {
                path: node.path.clone(),
                text: None,
                reason: TextIndexSkipReason::BinaryOrNonUtf8,
            });
            continue;
        };
        staged_bytes = staged_bytes.saturating_add(content.len() as u64);
        rows.push(TextIndexRow {
            path: node.path.clone(),
            reason: TextIndexSkipReason::Indexed,
            text: Some(IndexedFileText {
                path: node.path.clone(),
                content_hash: node.content_hash.clone(),
                byte_count: content.len(),
                line_count: content.lines().count(),
                content,
            }),
        });
    }
    Ok(rows)
}

/// Load indexed file hashes for incremental refresh comparison.
pub(crate) fn indexed_file_hashes(store: &AtlasStore) -> Result<HashMap<String, String>, CliError> {
    Ok(store
        .load_nodes()?
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter_map(|node| node.node.content_hash.map(|hash| (node.node.path, hash)))
        .collect::<HashMap<_, _>>())
}

/// Load indexed file hashes for selected repository paths.
pub(crate) fn indexed_file_hashes_for_paths(
    store: &AtlasStore,
    paths: &HashSet<String>,
) -> Result<HashMap<String, String>, CliError> {
    let mut sorted_paths = paths.iter().cloned().collect::<Vec<_>>();
    sorted_paths.sort();
    Ok(store
        .load_nodes_by_paths(&sorted_paths)?
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter_map(|node| node.node.content_hash.map(|hash| (node.node.path, hash)))
        .collect::<HashMap<_, _>>())
}

/// Return event paths in deterministic order.
pub(crate) fn sorted_watch_paths(paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut paths = paths.iter().cloned().collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Normalize a deleted path if it belongs to the watched repository.
pub(crate) fn normalized_deleted_path(
    root: &Path,
    path: &Path,
) -> Result<Option<String>, CliError> {
    match normalize_repo_path(root, path) {
        Ok(path) => Ok(valid_watch_relative_path(path)),
        Err(projectatlas_core::CoreError::PathOutsideRoot { .. }) => {
            Ok(native_display_relative_path(root, path).and_then(valid_watch_relative_path))
        }
        Err(source) => Err(CliError::InvalidInput(source.to_string())),
    }
}

/// Inspect and optionally remove legacy `.purpose` files.
pub(crate) fn strip_legacy_purpose(
    root: &Path,
    config_path: Option<&Path>,
    apply: bool,
    dry_run: bool,
    strip_source_headers: bool,
) -> Result<LegacyPurposeReport, CliError> {
    let root = root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let scan_options = scan_options_for_root(config_path, &root)?;
    let nodes = scan_repo(&root, &scan_options)?;
    let effective_dry_run = dry_run || !apply;
    let purpose_files = indexed_purpose_files(&root, &nodes);
    let mut removed = 0;
    if !effective_dry_run {
        for path in &purpose_files {
            let native = root.join(repo_path_to_native(path));
            fs::remove_file(&native).map_err(|source| CliError::Io {
                path: native,
                source,
            })?;
            removed += 1;
        }
    }
    let source_header_candidates = if strip_source_headers {
        purpose_header_candidates(&root, &nodes)?
    } else {
        Vec::new()
    };
    Ok(LegacyPurposeReport {
        applied: !effective_dry_run,
        purpose_files_found: purpose_files.len(),
        purpose_files_removed: removed,
        source_header_candidates,
        purpose_files,
    })
}

/// Collect `.purpose` files only from folders included in the normal index.
pub(crate) fn indexed_purpose_files(root: &Path, nodes: &[Node]) -> Vec<String> {
    let mut purpose_files = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == NodeKind::Folder) {
        let relative = if node.path == "." {
            ".purpose".to_string()
        } else {
            format!("{}/.purpose", node.path)
        };
        let native = root.join(repo_path_to_native(&relative));
        if native.exists() {
            purpose_files.push(relative);
        }
    }
    purpose_files.sort();
    purpose_files
}

/// Return source files that appear to start with legacy Purpose headers.
pub(crate) fn purpose_header_candidates(
    root: &Path,
    nodes: &[Node],
) -> Result<Vec<String>, CliError> {
    let mut candidates = Vec::new();
    for node in nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| is_symbol_candidate(&node.path, node.language.as_deref()))
    {
        let path = root.join(repo_path_to_native(&node.path));
        let content = fs::read_to_string(&path).map_err(|source| CliError::Io { path, source })?;
        if content
            .lines()
            .take(3)
            .any(|line| line.trim_start().contains("Purpose:"))
        {
            candidates.push(node.path.clone());
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::graph::{GraphRelationKind, RelationResolution, RepositoryNodePath};
    use projectatlas_db::RepositoryGraphRelationQuery;
    use std::error::Error;
    use std::fmt::Debug;

    #[test]
    fn worker_pools_respect_work_cardinality_and_runtime_ceiling() {
        for (work_items, max_workers, expected) in [
            (0, 16, 0),
            (1, 16, 1),
            (8, 16, 8),
            (64, 16, 16),
            (64, usize::MAX, INDEX_WORKER_SAFE_CEILING),
            (8, 0, 1),
        ] {
            assert_eq!(
                worker_count_for_work(work_items, max_workers),
                expected,
                "work_items={work_items}, max_workers={max_workers}"
            );
        }
    }

    #[test]
    fn settings_publication_identity_rejects_mixed_snapshots() {
        let fingerprint = "a".repeat(64);
        let diagnostic = DatabasePublicationReport {
            state: IndexPublicationState::Complete,
            contract_fingerprint: Some(fingerprint.clone()),
            contract_fingerprint_state: DatabasePublicationContractState::Valid,
            generation: IndexGeneration::new(4),
        };
        let matching = IndexPublication {
            state: IndexPublicationState::Complete,
            contract_fingerprint: Some(fingerprint),
            generation: IndexGeneration::new(4),
        };
        assert!(settings_publication_matches(
            Some(&diagnostic),
            Some(&matching)
        ));

        let next_generation = IndexPublication {
            generation: IndexGeneration::new(5),
            ..matching.clone()
        };
        assert!(!settings_publication_matches(
            Some(&diagnostic),
            Some(&next_generation)
        ));

        let invalid = DatabasePublicationReport {
            contract_fingerprint: None,
            contract_fingerprint_state: DatabasePublicationContractState::Invalid,
            ..diagnostic
        };
        assert!(!settings_publication_matches(
            Some(&invalid),
            Some(&matching)
        ));
    }

    #[test]
    fn supplied_language_controls_symbol_candidate_owner() {
        for path in ["Cargo.toml", "src/App.vue", "scripts/Get-Atlas.ps1"] {
            assert!(!is_symbol_candidate(path, Some("toon")), "{path}");
        }
        assert!(is_symbol_candidate("data/report.toon", Some("rust")));
        assert!(is_symbol_candidate(
            "data/report.toon",
            Some("cargo-manifest")
        ));
        assert!(is_symbol_candidate("data/report.toon", Some("vue")));
        assert!(is_symbol_candidate("data/report.toon", Some("powershell")));
        assert!(!is_symbol_candidate("data/report.toon", Some("toon")));
    }

    #[test]
    fn optional_catalog_symbol_work_requires_effective_admission() {
        assert!(is_symbol_candidate("scripts/report.awk", Some("awk")));
        assert!(!is_symbol_candidate_for_admission(
            "scripts/report.awk",
            Some("awk"),
            false,
        ));
        assert!(is_symbol_candidate_for_admission(
            "scripts/report.awk",
            Some("awk"),
            true,
        ));
        assert!(is_symbol_candidate_for_admission(
            "src/lib.rs",
            Some("rust"),
            false,
        ));
    }

    #[test]
    fn missing_language_preserves_specialized_symbol_candidate_inference() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "src/App.vue",
            "src/App.VUE",
            "scripts/Get-Atlas.ps1",
            "scripts/Atlas.psm1",
            "scripts/Atlas.psd1",
        ] {
            assert!(is_symbol_candidate(path, None), "{path}");
        }
        assert!(!is_symbol_candidate("data/report.toon", None));
    }

    #[test]
    fn rejected_database_location_does_not_create_or_change_database() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;

        for uncertain in [false, true] {
            let database_parent = temp
                .path()
                .join(if uncertain {
                    "uncertain"
                } else {
                    "unsupported"
                })
                .join("nested");
            let database = database_parent.join("projectatlas.db");
            let rejected_path = database.clone();
            let result = open_atlas_store_for_project_with_location_validator(
                &database,
                &root,
                move |_path| {
                    if uncertain {
                        Err(projectatlas_db::DbError::DatabaseFilesystemUncertain {
                            path: rejected_path,
                            mount_point: None,
                            filesystem_type: None,
                            reason: "injected filesystem uncertainty".to_string(),
                        })
                    } else {
                        Err(projectatlas_db::DbError::DatabaseFilesystemUnsupported {
                            path: rejected_path,
                            mount_point: None,
                            filesystem_type: Some("nfs".to_string()),
                        })
                    }
                },
            );
            let rejected = matches!(
                result,
                Err(CliError::Db(
                    projectatlas_db::DbError::DatabaseFilesystemUnsupported { .. }
                        | projectatlas_db::DbError::DatabaseFilesystemUncertain { .. }
                ))
            );
            require_eq(&rejected, &true, "typed location rejection")?;
            require_eq(
                &database_parent.exists(),
                &false,
                "rejected database parent absence",
            )?;
            require_eq(&database.exists(), &false, "rejected database absence")?;

            let existing_database = temp.path().join(if uncertain {
                "uncertain-existing.db"
            } else {
                "unsupported-existing.db"
            });
            let original_bytes = b"existing database bytes stay untouched";
            fs::write(&existing_database, original_bytes)?;
            let rejected_path = existing_database.clone();
            let existing_result = open_atlas_store_for_project_with_location_validator(
                &existing_database,
                &root,
                move |_path| {
                    if uncertain {
                        Err(projectatlas_db::DbError::DatabaseFilesystemUncertain {
                            path: rejected_path,
                            mount_point: None,
                            filesystem_type: None,
                            reason: "injected filesystem uncertainty".to_string(),
                        })
                    } else {
                        Err(projectatlas_db::DbError::DatabaseFilesystemUnsupported {
                            path: rejected_path,
                            mount_point: None,
                            filesystem_type: Some("nfs".to_string()),
                        })
                    }
                },
            );
            require_eq(
                &matches!(
                    existing_result,
                    Err(CliError::Db(
                        projectatlas_db::DbError::DatabaseFilesystemUnsupported { .. }
                            | projectatlas_db::DbError::DatabaseFilesystemUncertain { .. }
                    ))
                ),
                &true,
                "typed existing location rejection",
            )?;
            require_eq(
                &fs::read(&existing_database)?,
                &original_bytes.to_vec(),
                "rejected existing database bytes",
            )?;
        }
        Ok(())
    }

    #[test]
    fn operation_deadline_starts_with_default_or_shorter_explicit_timeout() {
        for (timeout_seconds, expected) in [
            (None, DEFAULT_INDEX_WORK_TIMEOUT),
            (Some(1), Duration::from_secs(1)),
            (
                Some(DEFAULT_INDEX_WORK_TIMEOUT.as_secs() + 1),
                DEFAULT_INDEX_WORK_TIMEOUT,
            ),
        ] {
            let options = SymbolBuildOptions::new(1_024, Some(1), timeout_seconds);
            let control = index_work_control(&options);
            assert_eq!(
                control
                    .deadline()
                    .map(|deadline| deadline.duration_since(control.started_at())),
                Some(expected)
            );
        }
    }

    #[test]
    fn normal_read_refresh_delta_enforces_path_and_byte_budgets() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let node = |path: String, size_bytes: u64| Node {
            path,
            kind: NodeKind::File,
            parent_path: None,
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(size_bytes),
            mtime_ns: Some(1),
            content_hash: Some("current-content".to_string()),
        };
        let path_bounded_nodes = (0..=NORMAL_READ_REFRESH_MAX_PATHS)
            .map(|index| node(format!("src/file_{index}.rs"), 1))
            .collect::<Vec<_>>();
        let path_delta = source_node_delta(temp.path(), &path_bounded_nodes, &[])
            .ok_or_else(|| io::Error::other("path-bounded freshness delta was missing"))?;
        require_eq(
            &path_delta.report.scope,
            &IndexRefreshScope::Full,
            "path-bounded refresh scope",
        )?;
        require_eq(
            &path_delta.report.changed,
            &(NORMAL_READ_REFRESH_MAX_PATHS + 1),
            "path-bounded change count",
        )?;
        require_eq(
            &path_delta.report.sample_paths.len(),
            &INDEX_FRESHNESS_SAMPLE_LIMIT,
            "bounded freshness sample",
        )?;

        let byte_bounded_nodes = vec![node(
            "src/large.rs".to_string(),
            NORMAL_READ_REFRESH_MAX_BYTES + 1,
        )];
        let byte_delta = source_node_delta(temp.path(), &byte_bounded_nodes, &[])
            .ok_or_else(|| io::Error::other("byte-bounded freshness delta was missing"))?;
        require_eq(
            &byte_delta.report.scope,
            &IndexRefreshScope::Full,
            "byte-bounded refresh scope",
        )?;
        Ok(())
    }

    #[test]
    fn controlled_freshness_plan_uses_the_callers_cancellation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        fs::write(root.join("lib.rs"), "pub fn indexed() {}\n")?;
        let db_path = root.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &root, None)?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        drop(store);

        let invalid_config = root.join("invalid-config.toml");
        fs::write(&invalid_config, "[invalid")?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        control.cancel();
        let result = open_fresh_atlas_store_for_project_controlled(
            &db_path,
            &root,
            Some(&invalid_config),
            &control,
        );
        if !matches!(
            result,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::Publication,
            }))
        ) {
            return Err(io::Error::other(
                "controlled freshness parsed policy outside the caller cancellation boundary",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn automatic_read_refresh_returns_typed_state_when_writer_is_unavailable()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let source_path = root.join("lib.rs");
        let indexed_source = "pub fn indexed() {}\n";
        let current_source = "pub fn current() {}\n";
        fs::write(&source_path, indexed_source)?;
        let db_path = root.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &root, None)?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let mut initial_store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(&mut initial_store, &plan, &symbol_options)?;
        let initial_generation = initial_store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?
            .generation;
        drop(initial_store);

        fs::write(&source_path, current_source)?;
        let mut blocking_writer = open_atlas_store_for_project(&db_path, &plan.root)?;
        let publication =
            blocking_writer.begin_index_publication(&plan.publication_contract_fingerprint())?;
        let refresh = open_fresh_atlas_store_for_project(&db_path, &plan.root, None);
        let Err(CliError::RefreshRequired(report)) = refresh else {
            return Err(io::Error::other(
                "contended automatic refresh did not return typed refresh_required",
            )
            .into());
        };
        require_eq(
            &report.scope,
            &IndexRefreshScope::Incremental,
            "contended automatic refresh scope",
        )?;
        require_eq(
            &report.reason,
            &IndexRefreshReason::SourceChanged,
            "contended automatic refresh reason",
        )?;

        let last_valid = open_atlas_store_read_only_for_project(&db_path, &plan.root)?;
        require_eq(
            &last_valid
                .index_publication()?
                .ok_or_else(|| io::Error::other("last-valid publication missing"))?
                .generation,
            &initial_generation,
            "last-valid generation during contention",
        )?;
        require_eq(
            &last_valid
                .load_file_text("lib.rs")?
                .ok_or_else(|| io::Error::other("last-valid source text missing"))?
                .content,
            &indexed_source.to_string(),
            "last-valid source during contention",
        )?;
        drop(last_valid);
        drop(publication);

        let repaired = open_fresh_atlas_store_for_project(&db_path, &plan.root, None)?;
        require_eq(
            &repaired
                .index_publication()?
                .ok_or_else(|| io::Error::other("repaired publication missing"))?
                .generation,
            &initial_generation
                .checked_next()
                .ok_or_else(|| io::Error::other("test generation overflow"))?,
            "repaired generation after contention",
        )?;
        require_eq(
            &repaired
                .load_file_text("lib.rs")?
                .ok_or_else(|| io::Error::other("repaired source text missing"))?
                .content,
            &current_source.to_string(),
            "current source after contention",
        )?;
        Ok(())
    }

    #[test]
    fn derived_source_readers_reject_bytes_outside_staged_hash() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("lib.rs");
        let staged_source = "fn first() {}\n";
        let changed_source = "fn other() {}\n";
        fs::write(&path, staged_source)?;
        let expected_content_hash = blake3::hash(staged_source.as_bytes()).to_hex().to_string();
        let node = Node {
            path: "lib.rs".to_string(),
            kind: NodeKind::File,
            parent_path: None,
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(staged_source.len() as u64),
            mtime_ns: Some(1),
            content_hash: Some(expected_content_hash.clone()),
        };
        fs::write(&path, changed_source)?;

        let text_result = indexed_file_texts_for_nodes(
            temp.path(),
            std::slice::from_ref(&node),
            TextIndexOptions::new(1_024),
        );
        let Err(CliError::RefreshRequired(details)) = text_result else {
            return Err(io::Error::other("text derivation accepted changed source bytes").into());
        };
        require_eq(
            &details.reason,
            &IndexRefreshReason::SourceChanged,
            "text source-change reason",
        )?;
        require_eq(
            &details.sample_paths,
            &vec!["lib.rs".to_string()],
            "text source-change paths",
        )?;

        let symbol_outcome = parse_symbol_job(
            &SymbolParseJob {
                path: node.path,
                native_path: path,
                expected_content_hash,
                language: node.language,
                fallback_summary: None,
                purpose_needs_suggestion: false,
            },
            &SymbolBuildOptions::new(1_024, Some(1), None),
            Instant::now(),
        );
        if !matches!(
            symbol_outcome,
            SymbolParseOutcome::SourceChanged { path } if path == "lib.rs"
        ) {
            return Err(io::Error::other("symbol derivation accepted changed source bytes").into());
        }

        let bounded_path = temp.path().join("bounded.txt");
        fs::write(&bounded_path, "four")?;
        let bounded_node = Node {
            path: "bounded.txt".to_string(),
            kind: NodeKind::File,
            parent_path: None,
            extension: Some(".txt".to_string()),
            language: Some("text".to_string()),
            size_bytes: Some(4),
            mtime_ns: Some(1),
            content_hash: Some(blake3::hash(b"four").to_hex().to_string()),
        };
        let bounded_control = standalone_index_work_control();
        let bounded_result = indexed_file_texts_for_nodes_with_limit(
            temp.path(),
            &[bounded_node],
            TextIndexOptions::new(1_024),
            3,
            &bounded_control,
        );
        if !matches!(
            bounded_result,
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::TextIndex,
                    resource: IndexWorkResource::TextBytes,
                    limit: 3,
                    observed: 4,
                }
            ))
        ) {
            return Err(io::Error::other("text staging accepted bytes beyond its limit").into());
        }

        let bounded_symbol = parse_symbol_job(
            &SymbolParseJob {
                path: "bounded.txt".to_string(),
                native_path: bounded_path,
                expected_content_hash: blake3::hash(b"four").to_hex().to_string(),
                language: Some("text".to_string()),
                fallback_summary: None,
                purpose_needs_suggestion: false,
            },
            &SymbolBuildOptions::new(3, Some(1), None),
            Instant::now(),
        );
        if !matches!(
            bounded_symbol,
            SymbolParseOutcome::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                stage: IndexWorkStage::SymbolParsing,
                resource: IndexWorkResource::SourceBytes,
                limit: 3,
                observed: 4,
            })
        ) {
            return Err(io::Error::other("symbol read accepted bytes beyond its limit").into());
        }
        Ok(())
    }

    #[test]
    fn parser_workers_reuse_structural_summaries_without_touching_approved_purpose()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("package.json");
        let content =
            r#"{"name":"demo","scripts":{"test":"vitest"},"dependencies":{"react":"1.0.0"}}"#;
        fs::write(&path, content)?;
        let node = Node {
            path: "package.json".to_string(),
            kind: NodeKind::File,
            parent_path: None,
            extension: Some(".json".to_string()),
            language: Some("json".to_string()),
            size_bytes: Some(content.len() as u64),
            mtime_ns: Some(1),
            content_hash: Some(blake3::hash(content.as_bytes()).to_hex().to_string()),
        };
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(std::slice::from_ref(&node))?;
        store.set_purpose(
            "package.json",
            "Own the JavaScript package manifest.",
            PurposeSource::Agent,
        )?;
        let text = refresh_text_index_for_nodes_with_rows(
            &mut store,
            temp.path(),
            std::slice::from_ref(&node),
            TextIndexOptions::new(1_024),
        )?;
        let SymbolParseOutcome::Parsed(parsed) = parse_symbol_job(
            &SymbolParseJob {
                path: node.path.clone(),
                native_path: path,
                expected_content_hash: node
                    .content_hash
                    .clone()
                    .ok_or_else(|| io::Error::other("fixture hash missing"))?,
                language: node.language.clone(),
                fallback_summary: None,
                purpose_needs_suggestion: false,
            },
            &SymbolBuildOptions::new(1_024, Some(1), None),
            Instant::now(),
        ) else {
            return Err(io::Error::other("package manifest did not parse").into());
        };
        require_eq(
            &parsed.summary_is_structural,
            &true,
            "parser-owned structural summary",
        )?;
        require_eq(
            &parsed.summary.as_str(),
            &"package manifest for demo with scripts test and 1 dependencies.",
            "structural content summary",
        )?;
        require_eq(
            &parsed.purpose_suggestion.is_none(),
            &true,
            "approved-purpose suggestion suppression",
        )?;
        let retained_bytes = symbol_parse_output_bytes(&parsed);
        let symbols = SymbolBuildStage {
            report: empty_symbol_build_report(),
            changes: vec![SymbolProjectionChange::Parsed(parsed)],
            retained_bytes,
        };
        let protected_purpose_paths = HashSet::from(["package.json".to_string()]);
        let control = standalone_index_work_control();
        let structural = stage_structural_summaries_for_nodes_controlled(
            &store,
            std::slice::from_ref(&node),
            &text.rows,
            Some(&symbols),
            &protected_purpose_paths,
            1,
            &control,
        )?;
        require_eq(
            &structural.report.summarized,
            &1,
            "structural summary report",
        )?;
        require_eq(
            &structural.report.purpose_suggestions,
            &0,
            "structural purpose-suggestion report",
        )?;
        require_eq(
            &structural.changes.is_empty(),
            &true,
            "duplicate structural mutations",
        )?;

        apply_symbol_build_stage(&mut store, &symbols, &control)?;
        apply_structural_summary_stage(&mut store, &structural, &control)?;
        let indexed = store
            .load_node_by_path("package.json")?
            .ok_or_else(|| io::Error::other("indexed package manifest missing"))?;
        require_eq(
            &indexed.summary.as_deref(),
            &Some("package manifest for demo with scripts test and 1 dependencies."),
            "persisted structural content summary",
        )?;
        require_eq(
            &indexed.purpose.purpose.as_deref(),
            &Some("Own the JavaScript package manifest."),
            "approved purpose text",
        )?;
        require_eq(
            &indexed.purpose.status,
            &PurposeStatus::Approved,
            "approved purpose status",
        )?;
        Ok(())
    }

    #[test]
    fn symbol_build_clamps_file_bytes_and_bounds_all_published_output() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        fs::write(
            temp.path().join("lib.rs"),
            "pub fn first() { second(); }\nfn second() {}\n",
        )?;
        let nodes = scan_repo(temp.path(), &ScanOptions::default())?;
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&nodes)?;
        let options = SymbolBuildOptions::new(u64::MAX, Some(INDEX_WORKER_SAFE_CEILING), None);
        require_eq(
            &options.max_bytes,
            &MAX_SYMBOL_FILE_BYTES,
            "effective CLI/MCP symbol file limit",
        )?;
        let control = standalone_index_work_control();
        for resource in [
            IndexWorkResource::SymbolRows,
            IndexWorkResource::RelationRows,
            IndexWorkResource::OutputBytes,
        ] {
            if !matches!(
                checked_symbol_publication_usage(1, 1, 1, resource),
                Err(CliError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded { observed: 2, .. }
                ))
            ) {
                return Err(io::Error::other(format!(
                    "symbol publication did not accumulate {resource}"
                ))
                .into());
            }
        }
        let cases = [
            (
                SymbolPublicationLimits {
                    symbol_rows: 0,
                    relation_rows: u64::MAX,
                    output_bytes: u64::MAX,
                },
                IndexWorkResource::SymbolRows,
            ),
            (
                SymbolPublicationLimits {
                    symbol_rows: u64::MAX,
                    relation_rows: 0,
                    output_bytes: u64::MAX,
                },
                IndexWorkResource::RelationRows,
            ),
            (
                SymbolPublicationLimits {
                    symbol_rows: u64::MAX,
                    relation_rows: u64::MAX,
                    output_bytes: 0,
                },
                IndexWorkResource::OutputBytes,
            ),
        ];
        for (limits, expected_resource) in cases {
            let result = build_symbols_for_paths_with_limits(
                &mut store,
                temp.path(),
                &options,
                None,
                None,
                &control,
                limits,
            );
            if !matches!(
                result,
                Err(CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::SymbolParsing,
                    resource,
                    ..
                })) if resource == expected_resource
            ) {
                return Err(io::Error::other(format!(
                    "symbol publication did not enforce {expected_resource}"
                ))
                .into());
            }
            require_eq(
                &store.symbol_count_for_path("lib.rs")?,
                &0,
                "no over-limit symbol output persisted",
            )?;
        }

        let mut oversized_node = nodes
            .iter()
            .find(|node| node.path == "lib.rs")
            .cloned()
            .ok_or_else(|| io::Error::other("oversized symbol fixture node missing"))?;
        oversized_node.size_bytes = Some(4);
        let clear_bytes = oversized_node.path.len() as u64
            + oversized_node.language.as_ref().map_or(0, String::len) as u64;
        let clear_options = SymbolBuildOptions::new(3, Some(1), None);
        let clear_limits = SymbolPublicationLimits {
            symbol_rows: u64::MAX,
            relation_rows: u64::MAX,
            output_bytes: clear_bytes.saturating_sub(1),
        };
        let clear_result = stage_symbols_for_nodes_with_limits(
            &store,
            temp.path(),
            #[cfg(feature = "optional-parser-supervisor")]
            &OptionalParserPackProjectSelection::Inactive,
            std::slice::from_ref(&oversized_node),
            &clear_options,
            None,
            None,
            &HashSet::new(),
            &control,
            clear_limits,
        );
        if !matches!(
            clear_result,
            Err(CliError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                stage: IndexWorkStage::SymbolParsing,
                resource: IndexWorkResource::OutputBytes,
                limit,
                observed,
            })) if limit == clear_bytes.saturating_sub(1) && observed == clear_bytes
        ) {
            return Err(
                io::Error::other("symbol clear output bypassed its retained-byte limit").into(),
            );
        }
        let clear_stage = stage_symbols_for_nodes_with_limits(
            &store,
            temp.path(),
            #[cfg(feature = "optional-parser-supervisor")]
            &OptionalParserPackProjectSelection::Inactive,
            std::slice::from_ref(&oversized_node),
            &clear_options,
            None,
            None,
            &HashSet::new(),
            &control,
            SymbolPublicationLimits {
                output_bytes: clear_bytes,
                ..SymbolPublicationLimits::STANDARD
            },
        )?;
        require_eq(
            &clear_stage.retained_bytes,
            &clear_bytes,
            "retained symbol clear bytes",
        )?;
        if !matches!(
            clear_stage.changes.as_slice(),
            [SymbolProjectionChange::Clear { path, language }]
                if path == "lib.rs" && language.as_deref() == Some("rust")
        ) {
            return Err(
                io::Error::other("oversized symbol output did not retain one clear").into(),
            );
        }

        let report = build_symbols_for_paths_with_limits(
            &mut store,
            temp.path(),
            &options,
            None,
            None,
            &control,
            SymbolPublicationLimits::STANDARD,
        )?;
        require_eq(&report.parsed, &1, "compatible bounded symbol build")?;
        require_eq(&report.max_workers, &1, "single-job symbol worker count")?;
        if report.symbols == 0 || report.relations == 0 {
            return Err(io::Error::other("bounded symbol build omitted parser output").into());
        }
        Ok(())
    }

    #[test]
    fn purpose_import_skips_non_utf8_source_headers_but_keeps_authored_inputs_strict()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let config_path = init_config_path(temp.path(), None);
        init_project_with_config(temp.path(), Some(&config_path))?;
        let config = load_atlas_config(Some(&config_path))?;
        fs::write(
            temp.path().join("binary.txt"),
            b"// Purpose: Must not be imported from binary source.\n\xff",
        )?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let nodes = scan_repo(&plan.root, &plan.scan_options)?;
        if !nodes.iter().any(|node| node.path == "binary.txt") {
            return Err(io::Error::other("binary source fixture was not scanned").into());
        }
        let snapshot =
            plan.purpose_import_snapshot_controlled(&nodes, &standalone_index_work_control())?;
        if snapshot
            .records
            .iter()
            .any(|record| record.path == "binary.txt")
        {
            return Err(io::Error::other("non-UTF-8 source header imported a purpose").into());
        }

        fs::write(&config.map_path, [0xff])?;
        let strict =
            plan.purpose_import_snapshot_controlled(&nodes, &standalone_index_work_control());
        if !matches!(
            strict,
            Err(CliError::InvalidInput(message))
                if message.contains("purpose input is not valid UTF-8")
        ) {
            return Err(
                io::Error::other("non-UTF-8 authored purpose input was not rejected").into(),
            );
        }
        Ok(())
    }

    #[test]
    fn purpose_import_inputs_observe_cancellation_limits_and_rollback() -> Result<(), Box<dyn Error>>
    {
        struct CancelAfterFirstRead {
            bytes: io::Cursor<Vec<u8>>,
            cancellation: IndexCancellation,
            reads: usize,
        }

        impl Read for CancelAfterFirstRead {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let read = self.bytes.read(buffer)?;
                self.reads += 1;
                if self.reads == 1 {
                    self.cancellation.cancel();
                }
                Ok(read)
            }
        }

        let temp = tempfile::tempdir()?;
        let config_path = init_config_path(temp.path(), None);
        init_project_with_config(temp.path(), Some(&config_path))?;
        let fixture_config = load_atlas_config(Some(&config_path))?;
        fs::write(temp.path().join("lib.rs"), "fn imported() {}\n")?;
        fs::write(
            &fixture_config.map_path,
            "folders[1]:\n  .,Controlled imported repository purpose\n",
        )?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let nodes = scan_repo(&plan.root, &plan.scan_options)?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&plan.root)?;
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        let publication_before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("controlled import publication missing"))?;
        let root_before = store
            .load_node_by_path(".")?
            .ok_or_else(|| io::Error::other("controlled import root missing"))?;
        let text_before = store
            .load_file_text("lib.rs")?
            .ok_or_else(|| io::Error::other("controlled import text missing"))?;
        let symbol_count_before = store.symbol_count()?;
        let relation_count_before = store.symbol_relation_count()?;

        let config_bytes = fs::metadata(&config_path)?.len();
        let map_bytes = fs::metadata(&fixture_config.map_path)?.len();
        let nonsource_bytes = fs::metadata(&fixture_config.nonsource_files_path)?.len();
        let source_bytes = fs::metadata(temp.path().join("lib.rs"))?.len();
        let staged_input_bytes = config_bytes
            .saturating_add(map_bytes)
            .saturating_add(nonsource_bytes)
            .saturating_add(source_bytes);
        let complete_operation_bytes = config_bytes
            .saturating_add(staged_input_bytes)
            .saturating_add(config_bytes)
            .saturating_add(staged_input_bytes);
        let largest_complete_input = config_bytes.max(map_bytes).max(nonsource_bytes);
        let aggregate_limits = PurposeImportLimits {
            total_bytes: complete_operation_bytes,
            complete_file_bytes: largest_complete_input,
            header_bytes: source_bytes,
            records: 100,
        };
        let aggregate_control = standalone_index_work_control();
        let aggregate_plan = ScanRuntimePlan::for_path_controlled_with_limits(
            None,
            temp.path(),
            None,
            &aggregate_control,
            aggregate_limits,
        )?;
        let aggregate_nodes = scan_repo(&aggregate_plan.root, &aggregate_plan.scan_options)?;
        let aggregate_snapshot = aggregate_plan.purpose_import_snapshot_controlled_with_limits(
            &aggregate_nodes,
            &aggregate_control,
            aggregate_limits,
        )?;
        revalidate_index_publication_inputs_controlled_with_limits(
            &store,
            &aggregate_plan,
            Some(&aggregate_snapshot.fingerprint),
            &aggregate_control,
            aggregate_limits,
        )?;

        let cumulative_limit = complete_operation_bytes.saturating_sub(1);
        let cumulative_limits = PurposeImportLimits {
            total_bytes: cumulative_limit,
            ..aggregate_limits
        };
        let cumulative_control = standalone_index_work_control();
        let cumulative_plan = ScanRuntimePlan::for_path_controlled_with_limits(
            None,
            temp.path(),
            None,
            &cumulative_control,
            cumulative_limits,
        )?;
        let cumulative_nodes = scan_repo(&cumulative_plan.root, &cumulative_plan.scan_options)?;
        let cumulative_snapshot = cumulative_plan.purpose_import_snapshot_controlled_with_limits(
            &cumulative_nodes,
            &cumulative_control,
            cumulative_limits,
        )?;
        let cumulative_result = revalidate_index_publication_inputs_controlled_with_limits(
            &store,
            &cumulative_plan,
            Some(&cumulative_snapshot.fingerprint),
            &cumulative_control.with_timeout_ceiling(DEFAULT_INDEX_WORK_TIMEOUT),
            cumulative_limits,
        );
        if !matches!(
            cumulative_result,
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::Publication,
                    resource: IndexWorkResource::PurposeBytes,
                    limit,
                    observed,
                }
            )) if limit == cumulative_limit && observed > limit
        ) {
            return Err(io::Error::other(
                "plan, staging, and revalidation readers did not share one purpose-byte budget",
            )
            .into());
        }

        let control = standalone_index_work_control();
        let staged_snapshot = plan.purpose_import_snapshot_controlled(&nodes, &control)?;
        let small_limits = PurposeImportLimits {
            total_bytes: 64,
            complete_file_bytes: 64,
            header_bytes: 64,
            records: 100,
        };
        let initial_limit_control = standalone_index_work_control();
        let initial_limited = plan.purpose_import_snapshot_controlled_with_limits(
            &nodes,
            &initial_limit_control,
            small_limits,
        );
        if !matches!(
            initial_limited,
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::Publication,
                    resource: IndexWorkResource::PurposeBytes,
                    limit: 64,
                    observed: 65,
                }
            ))
        ) {
            return Err(io::Error::other(
                "initial purpose snapshot exceeded limits without a typed failure",
            )
            .into());
        }
        let initial_cancel = IndexWorkControl::new(IndexCancellation::new(), None);
        initial_cancel.cancel();
        if !matches!(
            plan.purpose_import_snapshot_controlled(&nodes, &initial_cancel),
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::Publication,
            }))
        ) {
            return Err(io::Error::other(
                "initial purpose snapshot ignored operation cancellation",
            )
            .into());
        }

        let contract_fingerprint = plan.publication_contract_fingerprint();
        let mut publication = store.begin_index_publication(&contract_fingerprint)?;
        publication.set_project_root(&plan.root)?;
        publication.replace_scan(&nodes)?;
        publication.set_purpose(".", "Uncommitted purpose input", PurposeSource::Imported)?;
        let publication_limit_control = standalone_index_work_control();
        let limited = revalidate_index_publication_inputs_controlled_with_limits(
            &publication,
            &plan,
            Some(&staged_snapshot.fingerprint),
            &publication_limit_control,
            small_limits,
        );
        if !matches!(
            limited,
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::Publication,
                    resource: IndexWorkResource::PurposeBytes,
                    limit: 64,
                    observed: 65,
                }
            ))
        ) {
            return Err(io::Error::other(
                "publication purpose inputs exceeded limits without a typed failure",
            )
            .into());
        }
        drop(publication);
        require_eq(
            &store.index_publication()?,
            &Some(publication_before.clone()),
            "publication after bounded purpose-input rollback",
        )?;
        require_eq(
            &store.load_node_by_path(".")?,
            &Some(root_before.clone()),
            "authored purpose after bounded input rollback",
        )?;
        require_eq(
            &store.load_file_text("lib.rs")?,
            &Some(text_before.clone()),
            "indexed text after bounded input rollback",
        )?;
        require_eq(
            &store.symbol_count()?,
            &symbol_count_before,
            "symbols after bounded input rollback",
        )?;
        require_eq(
            &store.symbol_relation_count()?,
            &relation_count_before,
            "relations after bounded input rollback",
        )?;

        let mut canceled_publication = store.begin_index_publication(&contract_fingerprint)?;
        canceled_publication.set_project_root(&plan.root)?;
        canceled_publication.replace_scan(&nodes)?;
        canceled_publication.set_purpose(
            ".",
            "Canceled uncommitted purpose input",
            PurposeSource::Imported,
        )?;
        let late_cancel = IndexWorkControl::new(IndexCancellation::new(), None);
        late_cancel.cancel();
        let canceled = revalidate_index_publication_inputs_controlled(
            &canceled_publication,
            &plan,
            Some(&staged_snapshot.fingerprint),
            &late_cancel,
        );
        if !matches!(
            canceled,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::Publication,
            }))
        ) {
            return Err(io::Error::other(
                "late purpose-input revalidation ignored operation cancellation",
            )
            .into());
        }
        drop(canceled_publication);
        require_eq(
            &store.index_publication()?,
            &Some(publication_before),
            "publication after canceled purpose-input rollback",
        )?;
        require_eq(
            &store.load_node_by_path(".")?,
            &Some(root_before),
            "authored purpose after canceled input rollback",
        )?;
        require_eq(
            &store.load_file_text("lib.rs")?,
            &Some(text_before),
            "indexed text after canceled input rollback",
        )?;
        require_eq(
            &store.symbol_count()?,
            &symbol_count_before,
            "symbols after canceled input rollback",
        )?;
        require_eq(
            &store.symbol_relation_count()?,
            &relation_count_before,
            "relations after canceled input rollback",
        )?;

        let record_limited = plan.purpose_import_snapshot_controlled_with_limits(
            &nodes,
            &control,
            PurposeImportLimits {
                records: 0,
                ..PurposeImportLimits::default()
            },
        );
        if !matches!(
            record_limited,
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    stage: IndexWorkStage::Publication,
                    resource: IndexWorkResource::PurposeRecords,
                    limit: 0,
                    observed,
                }
            )) if observed > 0
        ) {
            return Err(io::Error::other("purpose record limit was not enforced").into());
        }

        let cancellation = IndexCancellation::new();
        let cancel_control = IndexWorkControl::new(cancellation.clone(), None);
        let mut reader =
            PurposeInputReader::new(&plan, &cancel_control, PurposeImportLimits::default());
        let mut input = CancelAfterFirstRead {
            bytes: io::Cursor::new(vec![b'x'; CONTROLLED_SOURCE_READ_BUFFER_BYTES * 2]),
            cancellation,
            reads: 0,
        };
        let canceled = reader.read_bytes(
            Path::new("controlled-purpose-input"),
            &mut input,
            u64::try_from(CONTROLLED_SOURCE_READ_BUFFER_BYTES * 2).unwrap_or(u64::MAX),
            true,
        );
        if !matches!(
            canceled,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::Publication,
            }))
        ) {
            return Err(io::Error::other(
                "purpose input did not observe cancellation between chunks",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn publication_revalidation_rejects_source_policy_and_import_drift()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let config_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&config_dir)?;
        let source_path = temp.path().join("lib.rs");
        let staged_source = "fn first() {}\n";
        fs::write(&source_path, staged_source)?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let db_path = config_dir.join("projectatlas.db");
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        let control = standalone_index_work_control();
        let generation_before_contention = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?
            .generation;
        let contract_fingerprint = plan.publication_contract_fingerprint();
        let contended_batch =
            stage_full_index_publication(&store, &plan, &symbol_options, true, false, &control)?;
        revalidate_staged_publication_inputs_controlled(
            &plan,
            contended_batch.nodes.expected_nodes(),
            None,
            &control,
        )?;
        let writer_blocker = rusqlite::Connection::open(&db_path)?;
        writer_blocker.execute_batch("BEGIN IMMEDIATE")?;
        let source_after_contention = "fn later() {}\n";
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let (publish_tx, publish_rx) = std::sync::mpsc::sync_channel(1);
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let publisher = std::thread::spawn(move || {
            if ready_tx.send(()).is_err() || publish_rx.recv().is_err() {
                return;
            }
            let publication_control = standalone_index_work_control();
            let result = publish_index_batch(&mut store, contended_batch, &publication_control);
            drop(result_tx.send((store, result)));
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|source| {
                io::Error::other(format!("publisher did not become ready: {source}"))
            })?;
        fs::write(&source_path, source_after_contention)?;
        publish_tx
            .send(())
            .map_err(|source| io::Error::other(format!("publisher stopped early: {source}")))?;
        let timely_result = result_rx.recv_timeout(Duration::from_millis(500));
        writer_blocker.execute_batch("ROLLBACK")?;
        let (returned_store, contention_result) = match timely_result {
            Ok(result) => result,
            Err(source) => {
                publisher
                    .join()
                    .map_err(|_panic| io::Error::other("publication thread panicked"))?;
                return Err(io::Error::other(format!(
                    "publication waited for a contending writer instead of failing fast: {source}"
                ))
                .into());
            }
        };
        publisher
            .join()
            .map_err(|_panic| io::Error::other("publication thread panicked"))?;
        store = returned_store;
        let Err(CliError::Db(contention)) = contention_result else {
            return Err(io::Error::other(
                "publication waited through contention and accepted stale staged source",
            )
            .into());
        };
        if !contention.is_write_unavailable() {
            return Err(io::Error::other(
                "publication contention did not return typed write-unavailable state",
            )
            .into());
        }
        require_eq(
            &store
                .index_publication()?
                .ok_or_else(|| io::Error::other("publication missing after contention"))?
                .generation,
            &generation_before_contention,
            "publication generation after contention",
        )?;
        require_eq(
            &store
                .load_file_text("lib.rs")?
                .ok_or_else(|| io::Error::other("indexed text missing after contention"))?
                .content,
            &staged_source.to_string(),
            "indexed text after contention",
        )?;
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        require_eq(
            &store
                .load_file_text("lib.rs")?
                .ok_or_else(|| io::Error::other("indexed text missing after retry"))?
                .content,
            &source_after_contention.to_string(),
            "restaged text after contention retry",
        )?;
        let initial_generation = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("retried publication missing"))?
            .generation;
        require_eq(
            &initial_generation,
            &generation_before_contention
                .checked_next()
                .ok_or_else(|| io::Error::other("test generation overflowed"))?,
            "single generation advance after contention retry",
        )?;
        let mut competing_store = open_atlas_store_for_project(&db_path, &plan.root)?;
        let competing_publication = competing_store
            .begin_index_projection_refresh_from(&contract_fingerprint, initial_generation)?;
        competing_publication.set_node_summary("lib.rs", "winning projection")?;
        let generation_conflict_batch =
            stage_full_index_publication(&store, &plan, &symbol_options, true, false, &control)?;
        competing_publication.complete()?;
        let winning_generation = initial_generation
            .checked_next()
            .ok_or_else(|| io::Error::other("test generation overflowed"))?;
        let conflict = publish_index_batch(&mut store, generation_conflict_batch, &control);
        if !matches!(
            conflict,
            Err(CliError::Db(
                projectatlas_db::DbError::PublicationBaseGenerationChanged {
                    expected,
                    found,
                }
            )) if expected == initial_generation && found == winning_generation
        ) {
            return Err(io::Error::other(
                "runtime publication did not reject a generation changed after preparation",
            )
            .into());
        }
        let winning_publication = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("winning publication missing"))?;
        require_eq(
            &winning_publication.generation,
            &winning_generation,
            "winning publication generation",
        )?;
        require_eq(
            &winning_publication.state,
            &projectatlas_db::IndexPublicationState::Complete,
            "winning publication state",
        )?;
        require_eq(
            &store
                .load_node_by_path("lib.rs")?
                .and_then(|node| node.summary),
            &Some("winning projection".to_string()),
            "winning publication summary",
        )?;
        let staged_source_batch =
            stage_full_index_publication(&store, &plan, &symbol_options, true, false, &control)?;

        fs::write(&source_path, "fn other() {}\n")?;
        let source_result = revalidate_staged_publication_inputs_controlled(
            &plan,
            staged_source_batch.nodes.expected_nodes(),
            None,
            &control,
        );
        let Err(CliError::RefreshRequired(details)) = source_result else {
            return Err(io::Error::other("publication accepted changed source state").into());
        };
        require_eq(
            &details.reason,
            &IndexRefreshReason::SourceChanged,
            "publication source-change reason",
        )?;
        require_eq(
            &details.sample_paths,
            &vec!["lib.rs".to_string()],
            "publication source-change paths",
        )?;

        fs::write(&source_path, staged_source)?;
        let staged_policy_batch =
            stage_full_index_publication(&store, &plan, &symbol_options, true, false, &control)?;
        let config_path = config_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"[project]
root = "."
map_path = ".projectatlas/projectatlas.toon"
nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"

[scan]
source_extensions = [".rs"]
exclude_dir_names = [".git", ".projectatlas", "target"]
exclude_dir_suffixes = []
exclude_path_prefixes = []
non_source_path_prefixes = []
text_index_max_bytes = 7

[purpose]
default_style = "line-comment"
line_comment_prefixes = ["//"]

[purpose.styles_by_extension]
".rs" = "line-comment"
"#,
        )?;
        let policy_result = revalidate_staged_publication_inputs_controlled(
            &plan,
            staged_policy_batch.nodes.expected_nodes(),
            None,
            &control,
        );
        let Err(CliError::VerificationIncomplete(details)) = policy_result else {
            return Err(io::Error::other("publication accepted changed effective policy").into());
        };
        require_eq(
            &details.reason,
            &IndexVerificationReason::PublicationContractMismatch,
            "publication policy-change reason",
        )?;

        let configured_plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let request_limited_plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1))?;
        require_eq(
            &configured_plan.text_options.max_bytes,
            &7,
            "configured text-index limit",
        )?;
        require_eq(
            &request_limited_plan.text_options.max_bytes,
            &1,
            "request-scoped text-index limit",
        )?;
        require_eq(
            &configured_plan.publication_contract_fingerprint(),
            &request_limited_plan.publication_contract_fingerprint(),
            "request limit excluded from publication contract",
        )?;
        let reloaded_request_plan = request_limited_plan.reload()?;
        require_eq(
            &reloaded_request_plan.text_options.max_bytes,
            &1,
            "request limit retained across operation reload",
        )?;
        require_eq(
            &request_limited_plan.publication_contract_fingerprint(),
            &reloaded_request_plan.publication_contract_fingerprint(),
            "reloaded operation publication contract",
        )?;

        let changed_config = fs::read_to_string(&config_path)?
            .replace("text_index_max_bytes = 7", "text_index_max_bytes = 8");
        fs::write(&config_path, changed_config)?;
        let configured_cap_changed_plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1))?;
        if configured_plan.publication_contract_fingerprint()
            == configured_cap_changed_plan.publication_contract_fingerprint()
        {
            return Err(io::Error::other(
                "configured text-index limit did not change the publication contract",
            )
            .into());
        }

        let import_repo = temp.path().join("import-repo");
        let import_atlas_dir = import_repo.join(".projectatlas");
        fs::create_dir_all(&import_atlas_dir)?;
        fs::write(import_repo.join("lib.rs"), "fn imported() {}\n")?;
        let import_map_path = import_atlas_dir.join("projectatlas.toon");
        fs::write(
            &import_map_path,
            "folders[1]:\n  .,Original imported repository purpose\n",
        )?;
        let external_config_path = temp.path().join("external-config.toml");
        fs::write(
            &external_config_path,
            r#"[project]
root = "import-repo"
map_path = ".projectatlas/projectatlas.toon"
nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"
"#,
        )?;
        let import_plan =
            ScanRuntimePlan::for_path(Some(&external_config_path), &import_repo, Some(1))?;
        let mut import_store = open_atlas_store_for_project(
            &import_atlas_dir.join("projectatlas.db"),
            &import_plan.root,
        )?;
        run_scan_pipeline(&mut import_store, &import_plan, &symbol_options)?;
        verify_index_freshness(&import_store, &import_repo, Some(&external_config_path))?;
        let normal_import_plan =
            ScanRuntimePlan::for_path(Some(&external_config_path), &import_repo, None)?;
        run_symbol_build_pipeline(
            &mut import_store,
            &normal_import_plan,
            &symbol_options,
            None,
        )?;
        verify_index_publication(&import_store, &normal_import_plan)?;
        let publication_before = import_store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial imported publication missing"))?;
        let root_before = import_store
            .load_node_by_path(".")?
            .ok_or_else(|| io::Error::other("imported root node missing"))?;
        if root_before.purpose.purpose.as_deref() != Some("Original imported repository purpose") {
            return Err(io::Error::other("legacy purpose fixture was not imported").into());
        }

        let import_control = standalone_index_work_control();
        let staged_import_batch = stage_full_index_publication(
            &import_store,
            &import_plan,
            &symbol_options,
            true,
            true,
            &import_control,
        )?;
        let staged_purpose_import = staged_import_batch
            .purpose_import
            .as_ref()
            .ok_or_else(|| io::Error::other("staged purpose import missing"))?;
        if !staged_purpose_import
            .records
            .iter()
            .any(|record| record.summary == "Original imported repository purpose")
        {
            return Err(io::Error::other("legacy purpose fixture was not imported").into());
        }
        fs::write(
            &import_map_path,
            "folders[1]:\n  .,Changed imported repository purpose\n",
        )?;
        let import_result = revalidate_staged_publication_inputs_with_purpose_snapshot(
            &import_plan,
            staged_import_batch.nodes.expected_nodes(),
            Some(staged_purpose_import),
            &import_control,
        );
        let Err(CliError::VerificationIncomplete(details)) = import_result else {
            return Err(io::Error::other(
                "publication accepted changed legacy purpose import inputs",
            )
            .into());
        };
        require_eq(
            &details.reason,
            &IndexVerificationReason::PublicationContractMismatch,
            "publication import-change reason",
        )?;
        require_eq(
            &import_store.index_publication()?,
            &Some(publication_before),
            "publication after purpose-import rollback",
        )?;
        require_eq(
            &import_store.load_node_by_path(".")?,
            &Some(root_before),
            "authored purpose after purpose-import rollback",
        )?;
        Ok(())
    }

    #[test]
    fn semantic_contract_revision_forces_full_projection_refresh() -> Result<(), Box<dyn Error>> {
        const PRE_MODULE_CALLBACK_DIGEST: &str =
            "487625adf2f9ec76f98034d4ef5667e707960b6b8afd280b213021cb64a0f10f";
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        fs::create_dir_all(temp.path().join("src"))?;
        fs::write(
            temp.path().join("src/config.rs"),
            "pub fn load_timeout_millis() -> u64 { 250 }\n",
        )?;
        fs::write(
            temp.path().join("src/handler.rs"),
            "use crate::config;\npub fn health_response() { let _ = config::load_timeout_millis(); }\n",
        )?;
        fs::write(
            temp.path().join("src/router.rs"),
            "use crate::handler;\npub fn dispatch(path: &str) -> Option<()> { (path == \"/health\").then(handler::health_response) }\n",
        )?;

        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        let current_fingerprint = plan.publication_contract_fingerprint();
        let legacy_fingerprint = index_derivation_fingerprint_with_semantic_digest(
            &plan.scan_options,
            text_index_options(plan.config.as_ref(), None),
            #[cfg(feature = "optional-parser-supervisor")]
            &plan.optional_parser_selection,
            PRE_MODULE_CALLBACK_DIGEST,
        );
        if legacy_fingerprint == current_fingerprint {
            return Err(io::Error::other(
                "semantic contract revision did not change the derivation fingerprint",
            )
            .into());
        }

        let current_generation = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("current publication missing"))?
            .generation;
        store
            .begin_index_publication_from(&legacy_fingerprint, current_generation)?
            .complete()?;
        if publication_contract_matches(&store, &plan)? {
            return Err(io::Error::other(
                "prior semantic contract unexpectedly matched the current plan",
            )
            .into());
        }

        let stale_generation = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("stale publication missing"))?
            .generation;
        let control = standalone_index_work_control();
        refresh_index_controlled(&mut store, &plan, &symbol_options, &control)?;
        let refreshed = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("refreshed publication missing"))?;
        if refreshed.generation <= stale_generation
            || refreshed.contract_fingerprint.as_deref() != Some(current_fingerprint.as_str())
        {
            return Err(io::Error::other(
                "semantic contract mismatch did not force a current full publication",
            )
            .into());
        }
        let graphs = store.load_symbol_graphs_for_paths(&["src/router.rs".to_string()])?;
        if !graphs
            .iter()
            .flat_map(|graph| &graph.relations)
            .any(|relation| {
                relation.kind == projectatlas_core::symbols::RelationKind::Calls
                    && relation.target_name == "handler::health_response"
            })
        {
            return Err(io::Error::other(
                "semantic refresh did not publish the comparison-then callback edge",
            )
            .into());
        }
        Ok(())
    }

    #[cfg(feature = "optional-parser-supervisor")]
    #[test]
    fn optional_parser_selection_changes_derivation_and_preserves_prior_generation_on_failure()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let source_path = temp.path().join("main.awk");
        fs::write(&source_path, "BEGIN { print \"atlas\" }\n")?;
        let inactive_plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = open_atlas_store_for_project(&db_path, &inactive_plan.root)?;
        run_scan_pipeline(&mut store, &inactive_plan, &symbol_options)?;
        if inactive_plan.scan_options.admit_optional_languages {
            return Err(io::Error::other(
                "inactive optional pack admitted catalog languages into the scan policy",
            )
            .into());
        }
        let inactive_optional = store
            .load_node_by_path("main.awk")?
            .ok_or_else(|| io::Error::other("inactive optional source node missing"))?;
        if inactive_optional.node.language.is_some() {
            return Err(io::Error::other(
                "inactive optional extension received a catalog language assignment",
            )
            .into());
        }
        let before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?;

        let selection_path = temp.path().join(repo_path_to_native(
            OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH,
        ));
        fs::write(
            &selection_path,
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "pack_id": "broad-parser",
                "selected": {
                    "projectatlas_version": OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION,
                    "artifact": "a".repeat(64),
                }
            }))?,
        )?;
        let selected_plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        if !selected_plan.scan_options.admit_optional_languages {
            return Err(io::Error::other(
                "selected optional pack did not enable catalog language admission",
            )
            .into());
        }
        if inactive_plan.publication_contract_fingerprint()
            == selected_plan.publication_contract_fingerprint()
        {
            return Err(io::Error::other(
                "optional parser selection did not change the derivation contract",
            )
            .into());
        }
        if publication_contract_matches(&store, &selected_plan)? {
            return Err(io::Error::other(
                "selected optional artifact unexpectedly matched the inactive publication",
            )
            .into());
        }
        if !watch_path_requires_full_scan(temp.path(), &selection_path) {
            return Err(io::Error::other(
                "optional parser selection event did not require a full refresh",
            )
            .into());
        }

        let mut changes = WatchChangeSet::default();
        changes.paths.insert(source_path);
        let result =
            refresh_index_for_changes(&mut store, &selected_plan, &changes, &symbol_options);
        if !matches!(result, Err(CliError::ParserPack(_))) {
            return Err(io::Error::other(
                "missing selected artifact did not fail before publication",
            )
            .into());
        }
        require_eq(
            &store
                .index_publication()?
                .ok_or_else(|| io::Error::other("publication disappeared after failure"))?
                .generation,
            &before.generation,
            "generation after selected optional artifact failure",
        )?;

        fs::remove_file(selection_path)?;
        let disabled_plan = selected_plan.reload()?;
        if disabled_plan.scan_options.admit_optional_languages {
            return Err(io::Error::other(
                "disabled optional pack retained catalog language admission",
            )
            .into());
        }
        require_eq(
            &disabled_plan.publication_contract_fingerprint(),
            &inactive_plan.publication_contract_fingerprint(),
            "disabled optional parser derivation contract",
        )?;

        let stale_graph = SymbolGraph {
            path: "main.awk".to_string(),
            language: Some("awk".to_string()),
            parser: ParserKind::Fallback,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let stale_metadata = SourceParseMetadata {
            path: stale_graph.path.clone(),
            language: stale_graph.language.clone(),
            parser: ParserKind::TreeSitter,
            symbol_count: 0,
            relation_count: 0,
        };
        store.replace_symbol_graph_with_metadata(&stale_graph, &stale_metadata)?;
        let selected_publication = store.begin_index_publication_from(
            &selected_plan.publication_contract_fingerprint(),
            before.generation,
        )?;
        selected_publication.complete()?;

        refresh_index(&mut store, &disabled_plan, &symbol_options)?;
        require_eq(
            &store.load_source_parse_metadata("main.awk")?,
            &None,
            "disabled optional parser metadata",
        )?;
        if !publication_contract_matches(&store, &disabled_plan)? {
            return Err(io::Error::other(
                "disabled optional parser refresh did not publish its derivation contract",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn symbol_projection_refresh_republishes_normalized_graph_at_one_generation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        fs::write(
            temp.path().join("lib.rs"),
            "pub fn caller() { target(); }\npub fn target() {}\n",
        )?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let mut store =
            open_atlas_store_for_project(&atlas_dir.join("projectatlas.db"), &plan.root)?;
        refresh_index(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(1_024, Some(1), None),
        )?;
        let before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?;
        let relation_query = RepositoryGraphRelationQuery::Family {
            relation: GraphRelationKind::Legacy(RelationKind::Calls),
        };
        require_eq(
            &store
                .repository_graph_relations(relation_query.clone(), 10)?
                .rows
                .len(),
            &1,
            "initial normalized call relation",
        )?;

        run_symbol_build_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(1, Some(1), None),
            None,
        )?;

        let after = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("symbol publication missing"))?;
        require_eq(
            &after.generation,
            &before
                .generation
                .checked_next()
                .ok_or_else(|| io::Error::other("publication generation overflowed"))?,
            "symbol and graph publication generation",
        )?;
        require_eq(
            &store
                .repository_graph_relations(relation_query, 10)?
                .rows
                .len(),
            &0,
            "cleared symbol relation projection",
        )?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("project identity missing"))?;
        let path = RepositoryNodePath::new(Path::new("lib.rs"))?;
        let entities = store.repository_graph_entities_by_path(project, &path, 10)?;
        require_eq(
            &entities
                .rows
                .iter()
                .all(|entity| entity.generation() == after.generation),
            &true,
            "symbol refresh normalized graph generation",
        )?;
        Ok(())
    }

    #[test]
    fn unchanged_full_and_incremental_refreshes_do_not_advance_generation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let source_path = temp.path().join("lib.rs");
        fs::write(&source_path, "fn stable() {}\n")?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let mut store =
            open_atlas_store_for_project(&atlas_dir.join("projectatlas.db"), &plan.root)?;
        refresh_index(&mut store, &plan, &symbol_options)?;
        let normal_read_plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        verify_index_publication(&store, &normal_read_plan)?;
        let before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("project identity missing"))?;
        let abandoned_full_stage = atlas_dir.join("graph-stage-full-noop");
        fs::create_dir(&abandoned_full_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &abandoned_full_stage.join("projectatlas.db"),
            &plan.root,
            project,
        )?);
        let full_report = refresh_index(&mut store, &plan, &symbol_options)?;
        let after_full = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("publication missing after full no-op refresh"))?;
        require_eq(
            &after_full.generation,
            &before.generation,
            "full no-op publication generation",
        )?;
        require_eq(
            &full_report.text_index.candidates,
            &0,
            "full no-op text candidates",
        )?;
        require_eq(
            &full_report.structural_summaries.candidates,
            &0,
            "full no-op summary candidates",
        )?;
        require_eq(
            &full_report.symbols.candidates,
            &0,
            "full no-op symbol candidates",
        )?;
        require_eq(
            &full_report.symbols.max_workers,
            &0,
            "full no-op symbol workers",
        )?;
        require_eq(
            &abandoned_full_stage.exists(),
            &false,
            "full no-op abandoned graph stage",
        )?;
        let abandoned_incremental_stage = atlas_dir.join("graph-stage-incremental-noop");
        fs::create_dir(&abandoned_incremental_stage)?;
        drop(AtlasStore::create_repository_graph_staging(
            &abandoned_incremental_stage.join("projectatlas.db"),
            &plan.root,
            project,
        )?);
        let mut changes = WatchChangeSet::default();
        changes.paths.insert(source_path);

        let report = refresh_index_for_changes(&mut store, &plan, &changes, &symbol_options)?;
        let after = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("publication missing after no-op refresh"))?;

        require_eq(
            &after.generation,
            &before.generation,
            "no-op publication generation",
        )?;
        require_eq(&report.text_index.candidates, &0, "no-op text candidates")?;
        require_eq(
            &report.structural_summaries.candidates,
            &0,
            "no-op summary candidates",
        )?;
        require_eq(&report.symbols.candidates, &0, "no-op symbol candidates")?;
        require_eq(&report.symbols.max_workers, &0, "no-op symbol workers")?;
        require_eq(
            &abandoned_incremental_stage.exists(),
            &false,
            "incremental no-op abandoned graph stage",
        )?;
        Ok(())
    }

    #[test]
    fn watcher_preserves_explicit_full_refresh_guidance() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("source.rs"), "pub fn source() {}\n")?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let database = temp.path().join(".projectatlas/projectatlas.db");
        let mut store = open_atlas_store_for_project(&database, &plan.root)?;
        refresh_index(&mut store, &plan, &symbol_options)?;
        let before = store.index_publication()?;

        let error =
            run_watch_with_polling_fallback(&mut store, &plan, 0, 1, &symbol_options, |_| {
                Err(CliError::RefreshRequired(Box::new(
                    index_policy_refresh_required(&plan.root),
                )))
            })
            .err()
            .ok_or_else(|| {
                io::Error::other("full-refresh guidance was hidden by polling fallback")
            })?;
        let CliError::RefreshRequired(report) = error else {
            return Err(io::Error::other(format!(
                "unexpected watcher error after full-refresh guidance: {error:?}"
            ))
            .into());
        };
        require_eq(
            &report.scope,
            &IndexRefreshScope::Full,
            "watcher full-refresh scope",
        )?;
        require_eq(
            &store.index_publication()?,
            &before,
            "watcher generation after full-refresh guidance",
        )?;
        Ok(())
    }

    #[test]
    fn canceled_watcher_batch_preserves_last_valid_and_retries_one_generation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let reserved_purpose_path = atlas_dir.join("projectatlas-nonsource-files.toon");
        fs::write(&reserved_purpose_path, "nonsource_files[]:\n")?;
        let changed_path = temp.path().join("changed.rs");
        let deleted_path = temp.path().join("deleted.rs");
        let deleted_dir = temp.path().join("deleted");
        let deleted_descendant = deleted_dir.join("descendant.rs");
        fs::create_dir(&deleted_dir)?;
        fs::write(&changed_path, "pub fn before() {}\n")?;
        fs::write(&deleted_path, "pub fn removed() {}\n")?;
        fs::write(&deleted_descendant, "pub fn descendant() {}\n")?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        refresh_index(&mut store, &plan, &symbol_options)?;
        let before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?;
        let before_node = store
            .load_node_by_path("changed.rs")?
            .ok_or_else(|| io::Error::other("initial changed node missing"))?;
        let reviewed_reserved_purpose = "Describe reviewed non-source atlas responsibilities.";
        store.set_purpose(
            ".projectatlas/projectatlas-nonsource-files.toon",
            reviewed_reserved_purpose,
            PurposeSource::Agent,
        )?;
        let old_reader = open_atlas_store_read_only_for_project(&db_path, &plan.root)?;
        require_eq(
            &old_reader
                .index_publication()?
                .as_ref()
                .map(|state| state.generation),
            &Some(before.generation),
            "old reader generation before staged publication",
        )?;
        let old_text = old_reader
            .load_file_text("changed.rs")?
            .ok_or_else(|| io::Error::other("old reader text missing"))?;

        fs::write(&changed_path, "pub fn after() {}\n")?;
        fs::remove_file(&deleted_path)?;
        fs::remove_dir_all(&deleted_dir)?;
        fs::write(&reserved_purpose_path, "nonsource_files[]:\n\n")?;
        let preparation_control = standalone_index_work_control();
        let staged_batch = stage_full_index_publication(
            &store,
            &plan,
            &symbol_options,
            true,
            false,
            &preparation_control,
        )?;
        revalidate_staged_publication_inputs_controlled(
            &plan,
            staged_batch.nodes.expected_nodes(),
            None,
            &preparation_control,
        )?;
        let IndexPublicationBatch {
            base_generation,
            contract_fingerprint,
            root,
            nodes,
            purpose_import: _,
            text_paths,
            text,
            symbols: _,
            graph: _,
            structural_summaries: _,
        } = staged_batch;
        let mut staged =
            store.begin_index_publication_from(&contract_fingerprint, base_generation)?;
        staged.set_project_root(&root)?;
        let NodePublicationBatch::Full { nodes } = nodes else {
            return Err(io::Error::other("full staging returned an incremental batch").into());
        };
        staged.begin_scan_replacement()?;
        for batch in nodes.chunks(PUBLICATION_NODE_BATCH_SIZE) {
            staged.upsert_scan_node_batch(batch)?;
        }
        staged.finish_scan_replacement()?;
        let late_cancel = IndexWorkControl::new(IndexCancellation::new(), None);
        late_cancel.cancel();
        let late_result = apply_text_index_stage(&mut staged, &text_paths, &text, &late_cancel);
        if !matches!(
            late_result,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::Publication,
            }))
        ) {
            return Err(io::Error::other("late cancellation did not stop publication").into());
        }
        drop(staged);
        require_eq(
            &store
                .index_publication()?
                .as_ref()
                .map(|state| state.generation),
            &Some(before.generation),
            "generation after canceled publication",
        )?;
        require_eq(
            &store.load_node_by_path("changed.rs")?,
            &Some(before_node),
            "last-valid node after canceled publication",
        )?;
        let mut changes = WatchChangeSet::default();
        changes.paths.insert(changed_path);
        changes.paths.insert(deleted_path);
        changes.paths.insert(deleted_dir);
        changes.paths.insert(deleted_descendant);
        changes.paths.insert(reserved_purpose_path);
        let fallback_report =
            run_watch_with_polling_fallback(&mut store, &plan, 0, 1, &symbol_options, |store| {
                let canceled = IndexWorkControl::new(IndexCancellation::new(), None);
                canceled.cancel();
                let Err(error) = refresh_index_for_changes_controlled(
                    store,
                    &plan,
                    &changes,
                    &symbol_options,
                    &canceled,
                ) else {
                    return Err(CliError::InvalidInput(
                        "canceled watcher batch unexpectedly succeeded".to_string(),
                    ));
                };
                let generation = store.index_publication()?.map(|state| state.generation);
                if generation != Some(before.generation) {
                    return Err(CliError::InvalidInput(
                        "canceled watcher batch advanced publication before fallback".to_string(),
                    ));
                }
                Err(error)
            })?;
        require_eq(
            &fallback_report.mode.as_str(),
            &WATCH_MODE_POLLING,
            "canceled notify batch fallback mode",
        )?;
        if fallback_report
            .fallback_reason
            .as_deref()
            .is_none_or(|reason| !reason.contains("canceled"))
        {
            return Err(io::Error::other(
                "polling fallback did not retain the notify failure reason",
            )
            .into());
        }

        let after = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("incremental publication missing"))?;
        require_eq(
            &after.generation,
            &before
                .generation
                .checked_next()
                .ok_or_else(|| io::Error::other("test generation overflowed"))?,
            "one incremental publication generation",
        )?;
        require_eq(
            &store.load_node_by_path("deleted.rs")?.is_none(),
            &true,
            "deleted path is absent",
        )?;
        require_eq(
            &store
                .load_symbols(Some("deleted.rs"), Some("removed"), 10)?
                .is_empty(),
            &true,
            "deleted symbols are invalidated",
        )?;
        require_eq(
            &store.load_node_by_path("deleted/descendant.rs")?.is_none(),
            &true,
            "deleted descendant path is absent",
        )?;
        require_eq(
            &store
                .load_symbols(Some("deleted/descendant.rs"), Some("descendant"), 10)?
                .is_empty(),
            &true,
            "deleted descendant symbols are invalidated",
        )?;
        require_eq(
            &store
                .load_symbols(Some("changed.rs"), Some("after"), 10)?
                .len(),
            &1,
            "changed symbols are published",
        )?;
        require_eq(
            &store
                .load_symbols(Some("changed.rs"), Some("before"), 10)?
                .is_empty(),
            &true,
            "replaced symbols are invalidated",
        )?;
        let reserved = store
            .load_node_by_path(".projectatlas/projectatlas-nonsource-files.toon")?
            .ok_or_else(|| io::Error::other("reserved metadata node missing"))?;
        require_eq(
            &reserved.purpose.purpose.as_deref(),
            &Some(reviewed_reserved_purpose),
            "stale reviewed built-in purpose text",
        )?;
        require_eq(
            &reserved.purpose.status,
            &PurposeStatus::Approved,
            "reviewed built-in purpose state",
        )?;
        require_eq(
            &old_reader
                .index_publication()?
                .as_ref()
                .map(|state| state.generation),
            &Some(before.generation),
            "old reader remains on the complete prior generation",
        )?;
        require_eq(
            &old_reader
                .load_file_text("changed.rs")?
                .map(|text| text.content),
            &Some(old_text.content),
            "old reader remains on prior source text",
        )?;
        let new_reader = open_atlas_store_read_only_for_project(&db_path, &plan.root)?;
        require_eq(
            &new_reader
                .index_publication()?
                .as_ref()
                .map(|state| state.generation),
            &Some(after.generation),
            "new reader sees the complete replacement generation",
        )?;
        require_eq(
            &new_reader
                .load_file_text("changed.rs")?
                .map(|text| text.content),
            &Some("pub fn after() {}\n".to_string()),
            "new reader sees replacement source text",
        )?;
        new_reader.finish_index_read_snapshot()?;
        old_reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn one_sided_notify_rename_events_require_full_verification() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let old_path = temp.path().join("old.rs");
        let new_path = temp.path().join("new.rs");
        for (mode, path) in [
            (notify::event::RenameMode::From, old_path),
            (notify::event::RenameMode::To, new_path),
        ] {
            let event =
                Event::new(EventKind::Modify(notify::event::ModifyKind::Name(mode))).add_path(path);
            let changes = notify_event_changes(temp.path(), &ScanOptions::default(), &event);
            require_eq(
                &changes.requires_full_scan,
                &true,
                "one-sided rename full verification",
            )?;
        }
        Ok(())
    }

    #[test]
    fn notify_rescan_and_ignored_policy_events_require_full_verification()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join(".projectatlas"))?;
        fs::write(temp.path().join(".gitignore"), ".projectatlas/\n")?;
        let config = temp.path().join(".projectatlas/config.toml");
        fs::write(&config, "[project]\nroot = \".\"\n")?;
        let config_event = Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(config.clone());
        let config_changes =
            notify_event_changes(temp.path(), &ScanOptions::default(), &config_event);
        require_eq(
            &config_changes.requires_full_scan,
            &true,
            "ignored ProjectAtlas config full verification",
        )?;
        require_eq(
            &config_changes.paths.contains(&config),
            &true,
            "ignored ProjectAtlas config event path",
        )?;

        let rescan_event = Event::new(EventKind::Any).set_flag(notify::event::Flag::Rescan);
        let rescan_changes =
            notify_event_changes(temp.path(), &ScanOptions::default(), &rescan_event);
        require_eq(
            &rescan_changes.requires_full_scan,
            &true,
            "backend rescan flag full verification",
        )?;
        Ok(())
    }

    #[test]
    fn directory_only_deletion_re_resolves_external_inbound_callers() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let atlas_dir = temp.path().join(".projectatlas");
        let source_dir = temp.path().join("src");
        let removed_dir = source_dir.join("removed");
        fs::create_dir_all(&atlas_dir)?;
        fs::create_dir_all(&removed_dir)?;
        fs::write(
            source_dir.join("caller.rs"),
            "pub fn caller() { target(); }\n",
        )?;
        fs::write(removed_dir.join("target.rs"), "pub fn target() {}\n")?;

        let plan = ScanRuntimePlan::for_path(None, temp.path(), Some(1_024))?;
        let symbol_options = SymbolBuildOptions::new(1_024, Some(1), None);
        let mut store =
            open_atlas_store_for_project(&atlas_dir.join("projectatlas.db"), &plan.root)?;
        refresh_index(&mut store, &plan, &symbol_options)?;
        let before = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("initial publication missing"))?;
        require_caller_resolution(
            &store,
            "src/caller.rs",
            |resolution| matches!(resolution, RelationResolution::Resolved { .. }),
            "initial caller resolution",
        )?;

        fs::remove_dir_all(&removed_dir)?;
        let mut changes = WatchChangeSet::default();
        changes.paths.insert(removed_dir);
        refresh_index_for_changes(&mut store, &plan, &changes, &symbol_options)?;

        let after = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("replacement publication missing"))?;
        require_eq(
            &after.generation,
            &before
                .generation
                .checked_next()
                .ok_or_else(|| io::Error::other("test generation overflowed"))?,
            "directory deletion generation",
        )?;
        require_eq(
            &store.load_node_by_path("src/removed/target.rs")?.is_none(),
            &true,
            "deleted descendant node",
        )?;
        require_eq(
            &store
                .load_symbols(Some("src/removed/target.rs"), Some("target"), 10)?
                .is_empty(),
            &true,
            "deleted descendant symbol",
        )?;
        require_caller_resolution(
            &store,
            "src/caller.rs",
            |resolution| matches!(resolution, RelationResolution::Unresolved { .. }),
            "caller resolution after directory deletion",
        )?;
        Ok(())
    }

    fn require_caller_resolution(
        store: &AtlasStore,
        caller_path: &str,
        expected: impl FnOnce(&RelationResolution) -> bool,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("project identity missing"))?;
        let caller_path = RepositoryNodePath::new(Path::new(caller_path))?;
        let caller_entities =
            store.repository_graph_entities_by_path(project, &caller_path, 100)?;
        let relations = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Calls),
            },
            100,
        )?;
        let mut matching = relations.rows.iter().filter(|relation| {
            caller_entities
                .rows
                .iter()
                .any(|entity| entity.key() == relation.source())
        });
        let relation = matching
            .next()
            .ok_or_else(|| io::Error::other(format!("{label}: caller relation missing")))?;
        if matching.next().is_some() {
            return Err(io::Error::other(format!("{label}: multiple caller relations")).into());
        }
        if !expected(relation.resolution()) {
            return Err(io::Error::other(format!(
                "{label}: unexpected resolution {:?}",
                relation.resolution()
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn purpose_curator_handoff_applies_one_stale_safe_batch() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[
            Node {
                path: "src/main.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(12),
                mtime_ns: Some(10),
                content_hash: Some("hash-main".to_string()),
            },
            Node {
                path: "src/detail.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(12),
                mtime_ns: Some(10),
                content_hash: Some("hash-detail".to_string()),
            },
        ])?;
        store.set_suggested_purpose("src/detail.rs", "Generated detail suggestion")?;
        let task = "runtime-purpose-curator";
        let page = purpose_curation_page(
            &store,
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: None,
                summary_only: false,
                scope: HealthScope::all(),
            },
            task,
        )?;
        require_eq(&page.actionable, &true, "actionable queue")?;
        require_eq(&page.items.len(), &2, "queue item count")?;
        require_eq(&page.task, &task.to_string(), "queue task")?;
        let requests = page
            .items
            .iter()
            .map(|item| PurposeReviewRequest {
                path: item.path.clone(),
                purpose: Some(format!("Reviewed purpose for {}", item.path)),
                confirm_existing: false,
                task: Some(page.task.clone()),
                work_key: Some(item.work_key.clone()),
                state_token: Some(item.state_token.clone()),
            })
            .collect::<Vec<_>>();
        let handoff = purpose_curator_handoff(page);
        require_eq(
            &handoff.execution_owner,
            &"agent_host",
            "host-owned execution",
        )?;
        require_eq(
            &handoff.server_started_curator,
            &false,
            "no server-started curator",
        )?;
        require_eq(
            &handoff.recommended_subagent_reasoning,
            &"lowest_host_enforced",
            "lowest host-enforced reasoning",
        )?;
        require_eq(&handoff.main_agent_fallback, &true, "main-agent fallback")?;

        let mixed = vec![
            requests[0].clone(),
            PurposeReviewRequest {
                path: requests[1].path.clone(),
                purpose: Some("Explicit correction must stay separate".to_string()),
                confirm_existing: false,
                task: None,
                work_key: None,
                state_token: None,
            },
        ];
        match review_purposes(&store, &mixed, true) {
            Err(CliError::InvalidInput(_)) => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "mixed purpose batch returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("mixed purpose batch was accepted").into()),
        }
        let mut partial = requests[1].clone();
        partial.state_token = None;
        match review_purposes(&store, &[requests[0].clone(), partial], true) {
            Err(CliError::InvalidInput(_)) => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "partial conditional batch returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => {
                return Err(io::Error::other("partial conditional batch was accepted").into());
            }
        }
        drop(store);
        store = AtlasStore::open_for_project(&database, &root)?;
        let unchanged =
            store.load_nodes_by_paths(&["src/main.rs".to_string(), "src/detail.rs".to_string()])?;
        require_eq(
            &unchanged.iter().all(|node| !node.purpose.agent_reviewed()),
            &true,
            "rejected batch left every purpose unapproved after reopen",
        )?;

        let applied = review_purposes(&store, &requests, true)?;
        require_eq(&applied.changed, &2, "conditional batch changed count")?;
        require_eq(&applied.conflicts, &0, "conditional batch conflicts")?;
        require_eq(
            &applied
                .items
                .iter()
                .all(|item| item.action == PurposeReviewAction::Review),
            &true,
            "conditional batch actions",
        )?;

        let repeated = review_purposes(&store, &requests, true)?;
        require_eq(&repeated.changed, &0, "accepted repeat changed count")?;
        require_eq(&repeated.conflicts, &2, "accepted repeat conflicts")?;
        require_eq(
            &repeated
                .items
                .iter()
                .all(|item| item.action == PurposeReviewAction::Accepted),
            &true,
            "accepted repeat actions",
        )?;
        let empty = purpose_curation_page(
            &store,
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: None,
                summary_only: false,
                scope: HealthScope::all(),
            },
            task,
        )?;
        require_eq(&empty.actionable, &false, "accepted queue is quiet")?;

        let correction = review_purposes(
            &store,
            &[PurposeReviewRequest {
                path: "src/main.rs".to_string(),
                purpose: Some("Explicit corrected purpose".to_string()),
                confirm_existing: false,
                task: None,
                work_key: None,
                state_token: None,
            }],
            true,
        )?;
        require_eq(
            &correction.items[0].action,
            &PurposeReviewAction::Review,
            "explicit correction action",
        )?;
        let corrected = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("corrected runtime path disappeared"))?;
        require_eq(
            &corrected.purpose.purpose.as_deref(),
            &Some("Explicit corrected purpose"),
            "explicit correction value",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_review_admission_bounds_input_and_prevents_partial_apply()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[
            Node {
                path: "src/first.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(12),
                mtime_ns: Some(10),
                content_hash: Some("hash-first".to_string()),
            },
            Node {
                path: "src/second.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(12),
                mtime_ns: Some(10),
                content_hash: Some("hash-second".to_string()),
            },
        ])?;
        store.set_suggested_purpose("src/first.rs", "Generated first purpose")?;
        store.set_purpose(
            "src/second.rs",
            &"x".repeat(MAX_PURPOSE_REVIEW_FIELD_BYTES + 1),
            PurposeSource::Imported,
        )?;

        let valid_first = PurposeReviewRequest {
            path: "src/first.rs".to_string(),
            purpose: Some("Reviewed café λ purpose".to_string()),
            confirm_existing: false,
            task: None,
            work_key: None,
            state_token: None,
        };
        let oversized_report = PurposeReviewRequest {
            path: "src/second.rs".to_string(),
            purpose: None,
            confirm_existing: true,
            task: None,
            work_key: None,
            state_token: None,
        };
        let Err(error) = review_purposes(&store, &[valid_first.clone(), oversized_report], true)
        else {
            return Err(io::Error::other(
                "oversized retained report field was accepted before apply",
            )
            .into());
        };
        require_eq(
            &error
                .to_string()
                .contains("purpose review report field purpose"),
            &true,
            "oversized report field error",
        )?;
        let unchanged = store
            .load_node_by_path("src/first.rs")?
            .ok_or_else(|| io::Error::other("first review fixture disappeared"))?;
        require_eq(
            &unchanged.purpose.agent_reviewed(),
            &false,
            "report admission failure prevented partial apply",
        )?;

        let oversized_field = PurposeReviewRequest {
            purpose: Some("x".repeat(MAX_PURPOSE_REVIEW_FIELD_BYTES + 1)),
            ..valid_first.clone()
        };
        let Err(error) = review_purposes(&store, &[oversized_field], true) else {
            return Err(io::Error::other("oversized request field passed admission").into());
        };
        require_eq(
            &error.to_string().contains("field purpose"),
            &true,
            "oversized input field error",
        )?;

        let too_many = vec![valid_first.clone(); MAX_PURPOSE_CURATION_BATCH_ROWS + 1];
        let Err(error) = review_purposes(&store, &too_many, true) else {
            return Err(io::Error::other("oversized request count passed admission").into());
        };
        require_eq(
            &error.to_string().contains("maximum is 200"),
            &true,
            "oversized item count error",
        )?;

        let aggregate = (0..9)
            .map(|index| PurposeReviewRequest {
                path: format!("src/{index}.rs"),
                purpose: Some("x".repeat(MAX_PURPOSE_REVIEW_FIELD_BYTES)),
                confirm_existing: false,
                task: None,
                work_key: None,
                state_token: None,
            })
            .collect::<Vec<_>>();
        let Err(error) = review_purposes(&store, &aggregate, false) else {
            return Err(io::Error::other("oversized aggregate request passed admission").into());
        };
        require_eq(
            &error.to_string().contains("aggregate string bytes"),
            &true,
            "oversized aggregate input error",
        )?;

        let preview = review_purposes(&store, &[valid_first], false)?;
        require_eq(
            &preview.items[0].purpose,
            &"Reviewed café λ purpose".to_string(),
            "UTF-8 purpose compatibility",
        )?;
        require_eq(
            &render_purpose_review_report(&preview).contains("Reviewed café λ purpose"),
            &true,
            "UTF-8 TOON compatibility",
        )?;
        Ok(())
    }

    #[test]
    fn indexed_navigation_read_rejects_stale_or_oversized_source_before_allocation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        let source_dir = root.join("src");
        fs::create_dir_all(&source_dir)?;
        let source = source_dir.join("large.rs");
        fs::File::create(&source)?.set_len(MAX_INDEXED_NAVIGATION_SOURCE_BYTES + 1)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[Node {
            path: "src/large.rs".to_string(),
            kind: NodeKind::File,
            parent_path: Some("src".to_string()),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(MAX_INDEXED_NAVIGATION_SOURCE_BYTES + 1),
            mtime_ns: Some(10),
            content_hash: Some("unused-oversized-hash".to_string()),
        }])?;

        let Err(error) = read_indexed_file_content(&store, "src/large.rs") else {
            return Err(
                io::Error::other("oversized indexed source was allocated and accepted").into(),
            );
        };
        let CliError::VerificationIncomplete(details) = error else {
            return Err(io::Error::other("oversized source returned the wrong error type").into());
        };
        require_eq(
            &details.reason,
            &IndexVerificationReason::SourceTooLarge,
            "oversized source reason",
        )?;

        fs::File::create(&source)?.set_len(1)?;
        let Err(error) = read_indexed_file_content(&store, "src/large.rs") else {
            return Err(io::Error::other("changed source size did not require refresh").into());
        };
        let CliError::RefreshRequired(details) = error else {
            return Err(
                io::Error::other("changed source size returned the wrong error type").into(),
            );
        };
        require_eq(
            &details.reason,
            &IndexRefreshReason::SourceChanged,
            "changed source size reason",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_watch_roots_keep_deleted_paths_in_scope() -> Result<(), Box<dyn Error>> {
        let root = Path::new(r"\\?\C:\repo");
        let deleted = Path::new(r"C:\repo\src\deleted.rs");
        require_eq(
            &normalized_deleted_path(root, deleted)?,
            &Some("src/deleted.rs".to_string()),
            "extended root deleted path",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn notify_events_normalize_unicode_paths_against_extended_windows_roots()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let source_dir = temp.path().join("src");
        fs::create_dir(&source_dir)?;
        let deleted = source_dir.join("Über.rs");
        fs::write(&deleted, "pub fn before() {}\n")?;
        fs::remove_file(&deleted)?;

        let root_text = temp
            .path()
            .to_str()
            .ok_or_else(|| io::Error::other("temporary path is not UTF-8"))?;
        let extended_root = if root_text.starts_with(r"\\?\") {
            temp.path().to_path_buf()
        } else {
            PathBuf::from(format!(r"\\?\{root_text}"))
        };
        let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
            .add_path(deleted.clone());
        let changes = notify_event_changes(&extended_root, &ScanOptions::default(), &event);

        require_eq(
            &changes.paths.contains(&deleted),
            &true,
            "native Unicode watcher path",
        )?;
        require_eq(
            &changes.requires_full_scan,
            &false,
            "source removal full-scan policy",
        )?;
        require_eq(
            &normalized_deleted_path(&extended_root, &deleted)?,
            &Some("src/Über.rs".to_string()),
            "normalized Unicode deleted path",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn notify_events_preserve_native_backslash_paths_on_unix() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join(r"src\generated.rs");
        fs::write(&source, "pub fn generated() {}\n")?;
        let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(source.clone());

        let changes = notify_event_changes(temp.path(), &ScanOptions::default(), &event);

        require_eq(
            &changes.paths,
            &HashSet::from([source]),
            "native Unix watcher paths",
        )?;
        Ok(())
    }

    /// Require equal test values without panicking from a fallible test.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }
}
