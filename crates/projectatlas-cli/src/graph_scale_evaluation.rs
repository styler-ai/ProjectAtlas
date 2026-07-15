//! Manifest-bound repository-graph scale evidence with supervised process measurement.

use crate::git_process_policy::{RepositoryGitError, RepositoryGitProbe};
use crate::graph_scale_plan::{GraphScalePlan, GraphScalePlanError};
use crate::graph_scale_process::{
    GraphScaleProcessError, GraphScaleProcessEvidence, run_measured_process,
    validate_process_evidence,
};
use projectatlas_core::graph::{
    GraphContractError, GraphEntityKind, GraphRelationKind, IndexEpoch, PublicationState,
    StructuralSlot,
};
use projectatlas_core::symbols::{
    CodeSymbol, CompactSymbolGraph, CompactSymbolGraphError, ParserKind, RelationKind, SymbolGraph,
    SymbolKind, SymbolRelation,
};
use projectatlas_db::{AtlasStore, DbError, GraphFactCounts, PersistedGraphTarget};
use projectatlas_service::{ServiceError, graph_query};
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags, Rows};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, Stats};
use std::cmp::Ordering as Comparison;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command as StandardCommand;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Frozen evaluation manifest compiled into the evidence executable.
const MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
/// Exact evaluator source compiled into the evidence executable.
const EVALUATOR_SOURCE_BYTES: &[u8] = include_bytes!("graph_scale_evaluation.rs");
/// Exact graph-scale plan source compiled into the evidence executable.
const PLAN_SOURCE_BYTES: &[u8] = include_bytes!("graph_scale_plan.rs");
/// Exact process-measurement source compiled into the evidence executable.
const PROCESS_SOURCE_BYTES: &[u8] = include_bytes!("graph_scale_process.rs");
/// Exact shared process-policy source compiled into the evidence executable.
const PROCESS_POLICY_SOURCE_BYTES: &[u8] = include_bytes!("bounded_process_supervisor.rs");
/// Exact repository-bound Git policy compiled into the evidence executable.
const GIT_POLICY_SOURCE_BYTES: &[u8] = include_bytes!("git_process_policy.rs");
/// Exact workspace dependency policy compiled into the evidence executable.
const WORKSPACE_MANIFEST_BYTES: &[u8] = include_bytes!("../../../Cargo.toml");
/// Exact CLI target and dependency policy compiled into the evidence executable.
const CLI_MANIFEST_BYTES: &[u8] = include_bytes!("../Cargo.toml");
/// Exact core crate dependency policy compiled into the evidence executable.
const CORE_MANIFEST_BYTES: &[u8] = include_bytes!("../../projectatlas-core/Cargo.toml");
/// Exact core crate boundary source compiled into the evidence executable.
const CORE_LIB_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-core/src/lib.rs");
/// Exact default-core budget policy compiled into the evidence executable.
const CORE_BUDGET_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-core/src/budget.rs");
/// Exact database crate dependency policy compiled into the evidence executable.
const DATABASE_MANIFEST_BYTES: &[u8] = include_bytes!("../../projectatlas-db/Cargo.toml");
/// Exact service crate dependency policy compiled into the evidence executable.
const SERVICE_MANIFEST_BYTES: &[u8] = include_bytes!("../../projectatlas-service/Cargo.toml");
/// Exact stable graph identity and publication-domain source compiled into the executable.
const CORE_GRAPH_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-core/src/graph.rs");
/// Exact compact graph layout source compiled into the evidence executable.
const CORE_SYMBOLS_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-core/src/symbols.rs");
/// Exact service facade source compiled into the evidence executable.
const SERVICE_LIB_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-service/src/lib.rs");
/// Exact graph-query service source compiled into the evidence executable.
const SERVICE_QUERY_SOURCE_BYTES: &[u8] =
    include_bytes!("../../projectatlas-service/src/graph_query.rs");
/// Exact database facade source compiled into the evidence executable.
const DATABASE_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-db/src/lib.rs");
/// Exact database schema source compiled into the evidence executable.
const DATABASE_SCHEMA_SOURCE_BYTES: &[u8] = include_bytes!("../../projectatlas-db/src/schema.rs");
/// Exact structural publication source compiled into the evidence executable.
const PUBLICATION_SOURCE_BYTES: &[u8] =
    include_bytes!("../../projectatlas-db/src/structural_publication.rs");
/// Exact example entrypoint source compiled into the evidence executable.
const EXAMPLE_SOURCE_BYTES: &[u8] = include_bytes!("../examples/graph-scale-evidence-runner.rs");
/// Locked dependency graph compiled with the evidence executable.
const CARGO_LOCK_BYTES: &[u8] = include_bytes!("../../../Cargo.lock");
/// Repository-relative source inputs whose exact commit blobs must match the compiled bytes.
const COMPILED_SOURCE_BINDINGS: &[CompiledSourceBinding] = &[
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/src/graph_scale_evaluation.rs",
        bytes: EVALUATOR_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/src/graph_scale_plan.rs",
        bytes: PLAN_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/src/graph_scale_process.rs",
        bytes: PROCESS_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/src/bounded_process_supervisor.rs",
        bytes: PROCESS_POLICY_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/src/git_process_policy.rs",
        bytes: GIT_POLICY_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "Cargo.toml",
        bytes: WORKSPACE_MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/Cargo.toml",
        bytes: CLI_MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-core/Cargo.toml",
        bytes: CORE_MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-core/src/lib.rs",
        bytes: CORE_LIB_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-core/src/budget.rs",
        bytes: CORE_BUDGET_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-core/src/graph.rs",
        bytes: CORE_GRAPH_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-core/src/symbols.rs",
        bytes: CORE_SYMBOLS_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-db/Cargo.toml",
        bytes: DATABASE_MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-service/Cargo.toml",
        bytes: SERVICE_MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-service/src/lib.rs",
        bytes: SERVICE_LIB_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-service/src/graph_query.rs",
        bytes: SERVICE_QUERY_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-db/src/lib.rs",
        bytes: DATABASE_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-db/src/schema.rs",
        bytes: DATABASE_SCHEMA_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-db/src/structural_publication.rs",
        bytes: PUBLICATION_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "crates/projectatlas-cli/examples/graph-scale-evidence-runner.rs",
        bytes: EXAMPLE_SOURCE_BYTES,
    },
    CompiledSourceBinding {
        path: "docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json",
        bytes: MANIFEST_BYTES,
    },
    CompiledSourceBinding {
        path: "Cargo.lock",
        bytes: CARGO_LOCK_BYTES,
    },
];
/// Stable final evidence artifact identity.
const ARTIFACT_KIND: &str = "projectatlas_graph_scale_evaluation";
/// Stable child workload artifact identity.
const WORKLOAD_ARTIFACT_KIND: &str = "projectatlas_graph_scale_workload";
/// Stable successful evidence status.
const STATUS_PASSED: &str = "passed";
/// Stable status for completed reduced diagnostics that cannot satisfy acceptance.
const STATUS_EXPLORATORY: &str = "exploratory_completed";
/// Claim scope for full implementation-scale evidence that is not release evidence.
const CLAIM_STATUS_IMPLEMENTATION_SCALE: &str = "implementation-scale-evidence";
/// Claim scope for reduced diagnostics that cannot satisfy acceptance.
const CLAIM_STATUS_EXPLORATORY: &str = "exploratory-reduced-evidence";
/// Monotonic clock contract used by every retained query cell.
const QUERY_CLOCK: &str = "std::time::Instant-monotonic-high-resolution";
/// Prepared-statement contract used by every retained query cell.
const QUERY_PREPARED_STATE: &str = "rusqlite-prepare_cached-on-one-warmed-connection";
/// Cache-warmup contract used by every retained query cell.
const QUERY_CACHE_STATE: &str = "connection-and-sqlite-page-cache-warmed-by-untimed-requests";
/// Honest limitation for final-size storage observations.
const PHYSICAL_WRITE_ACCOUNTING_STATUS: &str =
    "not-measured-here-final-file-sizes-are-not-physical-writes";
/// Exact allocator accounting scope retained by the workload child.
const RUST_ALLOCATOR_SCOPE: &str = "rust-global-allocator-requests-across-workload-child-threads";
/// Native `SQLite` allocations excluded from the Rust allocator observation.
const SQLITE_NATIVE_ALLOCATION_SCOPE: &str = "not-measured-by-rust-global-allocator";
/// Exact isolated-directory scope for `SQLite` temporary-byte snapshots.
const TEMPORARY_OBSERVATION_STATUS: &str =
    "non-database-regular-files-in-isolated-database-family-directory";
/// Child-only mode marker kept outside the normal option pairs.
const INTERNAL_WORKLOAD_FLAG: &str = "--internal-workload";
/// Manifest option accepted by both supervisor and workload modes.
const MANIFEST_OPTION: &str = "--manifest";
/// Artifact destination option accepted by both supervisor and workload modes.
const OUTPUT_OPTION: &str = "--output";
/// Exact source-commit option accepted by both supervisor and workload modes.
const TESTED_COMMIT_OPTION: &str = "--tested-commit";
/// Evidence-profile option accepted by both supervisor and workload modes.
const PROFILE_OPTION: &str = "--profile";
/// Graph language retained by the deterministic synthetic fixture.
const GRAPH_LANGUAGE: &str = "rust";
/// Graph parser detail retained by the deterministic synthetic fixture.
const GRAPH_DETAIL: &str = "function_item";
/// Source context retained by each deterministic call occurrence.
const CALL_CONTEXT: &str = "target();";
/// Typed graph entity table whose stable-key lookup must remain indexed.
const GRAPH_ENTITIES_TABLE: &str = "graph_entities";
/// Typed graph relation table whose adjacency lookup must remain indexed.
const GRAPH_RELATIONS_TABLE: &str = "graph_relations";
/// Canonical source/kind adjacency index expected for bounded service traversal.
const OUTBOUND_CALL_INDEX: &str = "idx_graph_relations_slot_source_kind_stable_key";

/// Failures at the graph-scale evidence boundary.
#[derive(Debug, Error)]
pub(super) enum GraphScaleEvaluationError {
    /// Command-line arguments did not match the closed evidence surface.
    #[error("invalid graph-scale arguments: {0}")]
    Arguments(String),
    /// The manifest, workload, result, or resource gate was invalid.
    #[error("graph-scale policy failed: {0}")]
    Policy(String),
    /// The closed graph-scale plan was invalid.
    #[error(transparent)]
    Plan(#[from] GraphScalePlanError),
    /// A typed graph publication invariant was invalid.
    #[error(transparent)]
    GraphContract(#[from] GraphContractError),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Retained process or Git text was not exact UTF-8.
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    /// The `SQLite` graph store failed.
    #[error(transparent)]
    Db(#[from] DbError),
    /// A direct evidence-only `SQLite` read failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// The compact graph representation rejected an input.
    #[error(transparent)]
    Compact(#[from] CompactSymbolGraphError),
    /// The shared graph-query service failed.
    #[error(transparent)]
    Service(#[from] ServiceError),
    /// Supervised process measurement failed.
    #[error(transparent)]
    Process(#[from] GraphScaleProcessError),
    /// Repository-bound Git provenance failed.
    #[error(transparent)]
    Git(#[from] RepositoryGitError),
}

/// Parsed supervisor or same-executable workload command.
pub(super) enum GraphScaleCommand {
    /// Parent mode that supervises and measures the workload child.
    Supervisor(GraphScaleArguments),
    /// Internal mode that performs the database workload and writes raw evidence.
    Workload(GraphScaleArguments),
}

/// Exact command arguments for one graph-scale evidence run.
#[derive(Clone, Debug)]
pub(super) struct GraphScaleArguments {
    /// Frozen manifest path.
    manifest: PathBuf,
    /// No-clobber result path.
    output: PathBuf,
    /// Exact base source commit tested by this run.
    tested_commit: String,
    /// Full declared scale or explicitly non-certifying reduced scale.
    profile: EvidenceProfile,
}

/// Closed workload profile that prevents reduced evidence from satisfying the full gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceProfile {
    /// Exact manifest-declared million-entity and multi-million-relation workload.
    Full,
    /// Small real-path workload for focused verification and runtime estimation.
    Reduced,
}

/// One compiled input and its repository-relative commit path.
struct CompiledSourceBinding {
    /// Normalized repository-relative path used by `git show`.
    path: &'static str,
    /// Exact bytes embedded in the running executable.
    bytes: &'static [u8],
}

impl EvidenceProfile {
    /// Parse one exact profile spelling.
    fn parse(value: &str) -> Result<Self, GraphScaleEvaluationError> {
        match value {
            "full" => Ok(Self::Full),
            "reduced" => Ok(Self::Reduced),
            _ => Err(GraphScaleEvaluationError::Arguments(format!(
                "unsupported graph-scale profile {value:?}"
            ))),
        }
    }

    /// Return the stable command spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
        }
    }
}

/// Partial manifest envelope containing the graph-scale plan owner.
#[derive(Debug, Deserialize)]
struct EvaluationManifestEnvelope {
    /// Stable manifest identity.
    manifest_id: String,
    /// Project source and dependency identity.
    projectatlas: EvaluationProjectIdentity,
    /// Registered architecture evaluations.
    architecture_evaluations: ArchitectureEvaluationEnvelope,
}

/// Source identity declared by the frozen evaluation manifest.
#[derive(Debug, Deserialize)]
struct EvaluationProjectIdentity {
    /// SHA-256 of the exact locked dependency graph compiled into the evaluator.
    cargo_lock_sha256: String,
}

/// Graph-scale member of the architecture-evaluation manifest.
#[derive(Debug, Deserialize)]
struct ArchitectureEvaluationEnvelope {
    /// Manifest-owned graph scale plan.
    graph_scale: GraphScalePlan,
}

/// Final no-clobber artifact with a digest over its canonical payload.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleEvidenceArtifact {
    /// Evidence wrapper schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: String,
    /// SHA-256 of the compact JSON serialization of `payload`.
    payload_sha256: String,
    /// Complete evidence payload covered by the digest.
    payload: GraphScaleEvidencePayload,
}

/// Complete retained graph-scale evidence payload.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleEvidencePayload {
    /// Frozen evaluation manifest identity.
    manifest_id: String,
    /// Exact base source commit exercised by the run.
    tested_commit: String,
    /// Operating system that executed the run.
    os: String,
    /// Processor architecture that executed the run.
    arch: String,
    /// Overall implementation-evidence result status.
    status: String,
    /// Explicit scale profile.
    profile: EvidenceProfile,
    /// Manifest-owned or reduced workload plan.
    plan: GraphScalePlan,
    /// Child-owned publication, storage, query, and allocator evidence.
    workload: GraphScaleWorkloadEvidence,
    /// Parent-owned supervised process and process-group memory evidence.
    process: GraphScaleProcessEvidence,
    /// Executable, source, manifest, command, and commit identity.
    provenance: ProvenanceEvidence,
    /// Explicitly limited claim scope for this implementation checkpoint.
    claim_scope: ClaimScope,
}

/// Child workload artifact passed from the measured process to its supervisor.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleWorkloadArtifact {
    /// Workload artifact schema version.
    schema_version: u32,
    /// Stable workload artifact kind.
    artifact_kind: String,
    /// Child-owned evidence.
    evidence: GraphScaleWorkloadEvidence,
}

/// Child-owned workload evidence excluding parent-observed process-tree memory.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleWorkloadEvidence {
    /// Explicit scale profile.
    profile: EvidenceProfile,
    /// Exact plan the child executed.
    plan: GraphScalePlan,
    /// Full staging and publication lifecycle timings.
    lifecycle: LifecycleMeasurements,
    /// Exact producer-order evidence across every completed graph batch.
    completed_batch_order: CompletedBatchOrderEvidence,
    /// Rust global-allocator request observations.
    rust_allocator: RustAllocatorMeasurements,
    /// Complete retained storage lifecycle observations.
    storage: StorageMeasurements,
    /// Warm direct and service query protocol evidence.
    queries: QueryMeasurements,
    /// Atomic structural publication evidence.
    publication: PublicationEvidence,
    /// Expected-versus-observed active graph counts.
    reconciliation: GraphScaleReconciliation,
    /// Stable identity/topology digests and relation/evidence integrity evidence.
    integrity: GraphIntegrityEvidence,
    /// Child-owned implementation checkpoint decisions.
    gates: GraphScaleWorkloadGates,
}

/// Staging and inactive-slot publication timings kept as distinct operations.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LifecycleMeasurements {
    /// Expanded-to-compact worker construction time.
    construction_ns: u64,
    /// Prepared staging-database persistence time.
    staging_persistence_ns: u64,
    /// Staged entity-plus-relation facts per second.
    staging_logical_facts_per_second: u64,
    /// Staging validation and WAL checkpoint time.
    sealing_checkpoint_ns: u64,
    /// Inactive-slot import, reconciliation, and atomic publication time.
    inactive_slot_publication_ns: u64,
    /// Published entity-plus-relation facts per second.
    inactive_slot_publication_logical_facts_per_second: u64,
    /// Live database creation through post-publication count reconciliation.
    total_publication_lifecycle_ns: u64,
}

/// Compact independently verifiable producer-order evidence for completed graph batches.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompletedBatchOrderEvidence {
    /// Number of completed graphs observed before sequential persistence.
    completed_graph_count: usize,
    /// SHA-256 of every graph path in exact encountered producer order.
    producer_path_sequence_sha256: String,
}

/// Honest Rust global-allocator request observations for the workload child.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RustAllocatorMeasurements {
    /// Scope observed by `stats_alloc`.
    scope: String,
    /// Explicit native-allocation limitation.
    sqlite_native_allocation_scope: String,
    /// Rust allocation requests.
    rust_allocation_requests: u64,
    /// Rust deallocation requests.
    rust_deallocation_requests: u64,
    /// Rust reallocation requests.
    rust_reallocation_requests: u64,
    /// Rust allocation plus reallocation requests.
    rust_allocator_requests: u64,
    /// Bytes requested from the Rust global allocator.
    rust_requested_bytes: u64,
    /// Bytes released through the Rust global allocator.
    rust_deallocated_bytes: u64,
    /// Net byte delta requested by Rust reallocations.
    rust_reallocated_bytes: i64,
    /// Allocation plus reallocation requests divided by declared logical graph facts.
    rust_allocator_requests_per_logical_fact: f64,
}

/// Complete lifecycle storage evidence without a physical-write claim.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StorageMeasurements {
    /// Live database immediately before inactive-slot publication.
    live_before_publication: SqliteStorageSnapshot,
    /// Staging database immediately before sealing/checkpoint.
    staging_before_seal: SqliteStorageSnapshot,
    /// Staging database immediately after sealing/checkpoint.
    staging_after_seal: SqliteStorageSnapshot,
    /// Live database immediately after publication and before integrity/query work.
    live_after_publication: SqliteStorageSnapshot,
    /// Final live database family after integrity/query work, checkpoint, and close.
    live_final_after_checkpoint: SqliteStorageSnapshot,
    /// Staging database family after explicit successful cleanup.
    staging_after_cleanup: SqliteFileFamilyObservation,
    /// Retained live database, WAL, and SHM bytes after final checkpoint.
    persistent_live_bytes: u64,
    /// Maximum live-plus-staging bytes across every retained coexistence checkpoint.
    max_observed_live_and_staging_bytes: u64,
    /// Retained live bytes divided by declared logical facts.
    persistent_live_bytes_per_logical_fact: u64,
    /// Final logical index-page bytes divided by declared logical facts.
    logical_index_page_bytes_per_logical_fact: u64,
    /// Explicit limitation: final file sizes are not physical writes.
    physical_write_accounting_status: String,
}

/// One database and sidecar snapshot with `SQLite` page metadata.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SqliteStorageSnapshot {
    /// Main database observation.
    database: FileObservation,
    /// Write-ahead-log observation.
    wal: FileObservation,
    /// Shared-memory sidecar observation.
    shm: FileObservation,
    /// Main plus WAL plus SHM bytes.
    total_bytes: u64,
    /// `SQLite` page size.
    page_size: u64,
    /// `SQLite` page count.
    page_count: u64,
    /// `SQLite` freelist page count.
    freelist_count: u64,
    /// Logical bytes attributed to `SQLite` index pages through `dbstat`.
    logical_index_page_bytes: u64,
    /// Evaluator-directory files not attributable to DB/WAL/SHM.
    sqlite_temporary_bytes_observed: u64,
    /// Exact scope of temporary-byte observation.
    temporary_observation_status: String,
}

/// Presence-sensitive main/WAL/SHM observation used after explicit cleanup.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SqliteFileFamilyObservation {
    /// Main database observation.
    database: FileObservation,
    /// Write-ahead-log observation.
    wal: FileObservation,
    /// Shared-memory sidecar observation.
    shm: FileObservation,
    /// Main plus WAL plus SHM bytes.
    total_bytes: u64,
}

/// Presence-sensitive file-size observation.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileObservation {
    /// Whether the exact file existed at the observation point.
    present: bool,
    /// File length, including a present zero-length file.
    bytes: u64,
}

/// Warm query evidence for the direct adapter and production service operation.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QueryMeasurements {
    /// Monotonic high-resolution clock contract.
    clock: String,
    /// Direct stable-key adapter cell.
    sqlite_stable_key: QueryCellEvidence,
    /// Production bounded three-hop service cell.
    service_bounded_three_hop: QueryCellEvidence,
}

/// Raw request-order evidence for one warmed query cell.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QueryCellEvidence {
    /// Discarded warmups.
    warmups: usize,
    /// Timed requests.
    repetitions: usize,
    /// Every request duration in execution order.
    raw_duration_ns: Vec<u64>,
    /// Every exact result count in execution order.
    raw_result_counts: Vec<u64>,
    /// Every deterministic result digest in execution order.
    raw_result_digests: Vec<String>,
    /// Expected result count established by the warmed path.
    expected_result_count: u64,
    /// Expected deterministic result digest established by the warmed path.
    expected_result_digest: String,
    /// Nearest-rank p50 over a sorted copy of the raw durations.
    p50_ns: u64,
    /// Nearest-rank p95 over a sorted copy of the raw durations.
    p95_ns: u64,
    /// Prepared-statement ownership declaration.
    prepared_state: String,
    /// Cache-state declaration.
    cache_state: String,
    /// Actual production-owned `SQLite` plan and intended-index assertion.
    query_plan: QueryPlanEvidence,
}

/// Retained `EXPLAIN QUERY PLAN` details for one production query.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QueryPlanEvidence {
    /// Semantic planner contract for this query cell.
    requirement: QueryPlanRequirement,
    /// Exact `SQLite` plan detail rows in returned order.
    actual_details: Vec<String>,
}

/// Stable semantic requirement over retained `SQLite` plan rows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum QueryPlanRequirement {
    /// Require an indexed search without binding `SQLite`'s generated index spelling.
    IndexedSearch {
        /// Table that must be searched without a full scan.
        table: String,
    },
    /// Require an indexed search through a deliberately named schema index.
    NamedIndexSearch {
        /// Table that must be searched without a full scan.
        table: String,
        /// Authored schema index that must appear in the plan.
        index: String,
    },
}

impl QueryPlanRequirement {
    /// Stable-key lookup contract without a generated `sqlite_autoindex_*` name.
    fn stable_entity_lookup() -> Self {
        Self::IndexedSearch {
            table: GRAPH_ENTITIES_TABLE.to_owned(),
        }
    }

    /// Outbound typed-adjacency contract through the authored slot/source/kind index.
    fn outbound_call_adjacency() -> Self {
        Self::NamedIndexSearch {
            table: GRAPH_RELATIONS_TABLE.to_owned(),
            index: OUTBOUND_CALL_INDEX.to_owned(),
        }
    }

    /// Recompute the semantic planner decision from exact retained rows.
    fn satisfied_by(&self, actual_details: &[String]) -> bool {
        let (table, required_index) = match self {
            Self::IndexedSearch { table } => (table.as_str(), None),
            Self::NamedIndexSearch { table, index } => (table.as_str(), Some(index.as_str())),
        };
        let table = table.to_ascii_uppercase();
        let required_index = required_index.map(str::to_ascii_uppercase);
        let mut indexed_search = false;
        for detail in actual_details {
            let detail = detail.to_ascii_uppercase();
            if detail.contains("SCAN GRAPH_ENTITIES") || detail.contains("SCAN GRAPH_RELATIONS") {
                return false;
            }
            if detail.contains(&format!("SEARCH {table}"))
                && detail.contains(" USING ")
                && required_index
                    .as_ref()
                    .is_none_or(|index| detail.contains(index))
            {
                indexed_search = true;
            }
        }
        indexed_search
    }
}

/// Atomic publication state before and after the full staging lifecycle.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicationEvidence {
    /// Publication captured before staging.
    before: PublicationSnapshot,
    /// Exact next full-publication state derived from `before`.
    expected_after: PublicationSnapshot,
    /// Publication returned by inactive-slot import.
    after: PublicationSnapshot,
    /// Whether slot and epoch advanced exactly once.
    exact_transition: bool,
}

/// Serializable structural publication tuple.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicationSnapshot {
    /// Active structural slot.
    active_slot: StructuralSlot,
    /// Active structural epoch.
    active_epoch: u64,
}

impl From<PublicationState> for PublicationSnapshot {
    fn from(value: PublicationState) -> Self {
        Self {
            active_slot: value.active_slot,
            active_epoch: value.active_epoch.get(),
        }
    }
}

impl PublicationSnapshot {
    /// Derive the exact next full-publication tuple through the production domain contract.
    fn next_full(self) -> Result<Self, GraphContractError> {
        PublicationState {
            active_slot: self.active_slot,
            active_epoch: IndexEpoch::new(self.active_epoch),
        }
        .next_full()
        .map(Self::from)
    }
}

/// Exact expected-versus-observed active-slot graph counts.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleReconciliation {
    /// Expected entity rows, including one file entity per file graph.
    expected_entities: usize,
    /// Expected logical call-relation rows.
    expected_relations: usize,
    /// Observed active typed graph counts.
    observed: GraphFactCounts,
}

/// Stable identity/topology digests plus one-to-one relation/evidence reconciliation.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphIntegrityEvidence {
    /// Digest of ordered entity identity fields used by this scale checkpoint.
    entity_identity_digest_sha256: String,
    /// Digest of ordered relation identity and topology fields used by this scale checkpoint.
    relation_topology_digest_sha256: String,
    /// Digest of ordered evidence occurrence-identity fields used by this scale checkpoint.
    evidence_occurrence_identity_digest_sha256: String,
    /// Digest binding the three component digests.
    graph_evidence_digest_sha256: String,
    /// Relations with no evidence occurrence.
    relations_missing_evidence: u64,
    /// Relations with more than one evidence occurrence.
    relations_with_duplicate_evidence: u64,
    /// Evidence occurrences whose relation is absent.
    orphan_evidence_occurrences: u64,
}

/// Child-owned decisions excluding parent-observed process memory.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphScaleWorkloadGates {
    /// Exact entity count.
    entity_count: GateDecision,
    /// Exact relation count.
    relation_count: GateDecision,
    /// Exact evidence count.
    evidence_count: GateDecision,
    /// No abstention rows for the fully resolved fixture.
    resolution_abstentions_empty: GateDecision,
    /// One evidence occurrence for every relation and no orphan evidence.
    relation_evidence_integrity: GateDecision,
    /// Atomic inactive-slot publication transition.
    publication_transition: GateDecision,
    /// Rust allocator request ceiling.
    rust_allocator_requests: GateDecision,
    /// Exact deterministic completed-batch producer order.
    completed_batch_order: GateDecision,
    /// Staging throughput floor.
    staging_throughput: GateDecision,
    /// Retained derived-storage ceiling.
    derived_storage: GateDecision,
    /// Logical `SQLite` index-page storage ceiling.
    logical_index_storage: GateDecision,
    /// Full raw direct-query protocol and index plan.
    sqlite_query_protocol: GateDecision,
    /// Full raw production-service query protocol and index plan.
    service_query_protocol: GateDecision,
}

impl GraphScaleWorkloadGates {
    /// Return whether every child-owned gate passed.
    fn all_passed(&self) -> bool {
        [
            self.entity_count,
            self.relation_count,
            self.evidence_count,
            self.resolution_abstentions_empty,
            self.relation_evidence_integrity,
            self.publication_transition,
            self.rust_allocator_requests,
            self.completed_batch_order,
            self.staging_throughput,
            self.derived_storage,
            self.logical_index_storage,
            self.sqlite_query_protocol,
            self.service_query_protocol,
        ]
        .into_iter()
        .all(GateDecision::passed)
    }
}

/// Typed outcome for one implementation evidence gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GateDecision {
    /// The measurement or reconciliation met its declared contract.
    Passed,
    /// The measurement or reconciliation missed its declared contract.
    Failed,
}

impl GateDecision {
    /// Return whether this decision passed.
    const fn passed(self) -> bool {
        matches!(self, Self::Passed)
    }
}

impl From<bool> for GateDecision {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

/// Executable, source, manifest, command, and commit identity.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "these independent source-state observations must remain explicit serialized facts"
)]
struct ProvenanceEvidence {
    /// SHA-256 of the exact running executable.
    executable_sha256: String,
    /// SHA-256 of the frozen manifest bytes.
    manifest_sha256: String,
    /// SHA-256 of the evaluator source bytes.
    graph_scale_evaluator_source_sha256: String,
    /// SHA-256 of the plan source bytes.
    graph_scale_plan_source_sha256: String,
    /// SHA-256 of the process-measurement source bytes.
    graph_scale_process_source_sha256: String,
    /// SHA-256 of the shared process-policy source bytes.
    bounded_process_policy_source_sha256: String,
    /// SHA-256 of the repository-bound Git policy source bytes.
    git_process_policy_source_sha256: String,
    /// SHA-256 of the workspace manifest bytes.
    workspace_manifest_sha256: String,
    /// SHA-256 of the CLI manifest bytes.
    cli_manifest_sha256: String,
    /// SHA-256 of the core crate manifest bytes.
    core_manifest_sha256: String,
    /// SHA-256 of the database crate manifest bytes.
    database_manifest_sha256: String,
    /// SHA-256 of the service crate manifest bytes.
    service_manifest_sha256: String,
    /// SHA-256 of the core crate boundary source bytes.
    core_lib_source_sha256: String,
    /// SHA-256 of the default-core budget policy source bytes.
    core_budget_source_sha256: String,
    /// SHA-256 of the stable graph identity/domain source bytes.
    core_graph_source_sha256: String,
    /// SHA-256 of the compact graph layout source bytes.
    core_symbols_source_sha256: String,
    /// SHA-256 of the service facade source bytes.
    service_lib_source_sha256: String,
    /// SHA-256 of the production service query source bytes.
    service_graph_query_source_sha256: String,
    /// SHA-256 of the database facade source bytes.
    database_source_sha256: String,
    /// SHA-256 of the database schema source bytes.
    database_schema_source_sha256: String,
    /// SHA-256 of the structural publication source bytes.
    structural_publication_source_sha256: String,
    /// SHA-256 of the example entrypoint source bytes.
    example_source_sha256: String,
    /// SHA-256 of the locked dependency graph.
    cargo_lock_sha256: String,
    /// Exact supervisor executable and argument tuple.
    supervisor_command: Vec<String>,
    /// Exact same-executable child and argument tuple.
    workload_command: Vec<String>,
    /// Hard child-process timeout applied by the supervisor.
    external_timeout_seconds: u64,
    /// Canonical Git executable used for every retained source-binding probe.
    git_executable_path: String,
    /// SHA-256 of the exact Git executable bytes.
    git_executable_sha256: String,
    /// Git commit returned immediately before child spawn.
    observed_git_commit_before: String,
    /// Git commit returned after child exit and before artifact promotion.
    observed_git_commit_after: String,
    /// Whether both HEAD observations equal the requested tested commit.
    head_matches_requested_commit: bool,
    /// Whether tracked or untracked source state differed from HEAD before spawn.
    source_worktree_dirty_before: bool,
    /// Whether tracked or untracked source state differed from HEAD after child exit.
    source_worktree_dirty_after: bool,
    /// Whether full evidence is exactly bound to one clean committed source state.
    source_state_commit_bound: bool,
    /// SHA-256 of exact filter-free worktree-state bytes before spawn.
    worktree_state_before_sha256: String,
    /// SHA-256 of exact filter-free worktree-state bytes after child exit.
    worktree_state_after_sha256: String,
    /// Exact compiled-input to tested-commit blob bindings retained only for full evidence.
    source_commit_bindings: Vec<SourceCommitBindingEvidence>,
    /// Whether every compiled input exactly matched its tested-commit blob for full evidence.
    source_commit_verified: bool,
    /// SHA-256 of the serialized authoritative supervisor command tuple.
    command_sha256: String,
    /// SHA-256 of the serialized workload command tuple.
    workload_command_sha256: String,
}

/// One exact compiled-input to tested-commit blob comparison.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceCommitBindingEvidence {
    /// Normalized repository-relative path.
    path: String,
    /// SHA-256 of bytes embedded in the running executable.
    compiled_sha256: String,
    /// SHA-256 of bytes loaded from the tested commit.
    commit_sha256: String,
    /// Whether the byte sequences were exactly equal.
    exact_match: bool,
}

/// Explicit non-release claim boundary.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaimScope {
    /// Implementation evidence is not final release-claim evidence.
    claim_eligible: bool,
    /// Stable claim status.
    claim_status: String,
    /// Whether the exact declared scale was executed.
    full_declared_scale_executed: bool,
    /// Independent exclusions flattened into the claim boundary schema.
    #[serde(flatten)]
    exclusions: ClaimExclusions,
}

/// Independent negative claim statements retained as explicit booleans.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaimExclusions {
    /// No calibrated reference-host claim is made here.
    not_calibrated_reference_host: bool,
    /// In-process service timing is not MCP latency.
    not_mcp_latency: bool,
    /// This artifact cannot establish release superiority.
    not_release_superiority_evidence: bool,
}

/// Parse the closed supervisor or internal workload command surface.
pub(super) fn parse_arguments(
    mut arguments: Vec<OsString>,
) -> Result<GraphScaleCommand, GraphScaleEvaluationError> {
    let internal = arguments
        .first()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == INTERNAL_WORKLOAD_FLAG);
    if internal {
        arguments.remove(0);
    }
    let mut options = BTreeMap::new();
    let mut values = arguments.into_iter();
    while let Some(flag) = values.next() {
        let flag = flag.into_string().map_err(|_value| {
            GraphScaleEvaluationError::Arguments("arguments must be Unicode".into())
        })?;
        let value = values
            .next()
            .ok_or_else(|| {
                GraphScaleEvaluationError::Arguments(format!("missing value for {flag}"))
            })?
            .into_string()
            .map_err(|_value| {
                GraphScaleEvaluationError::Arguments("arguments must be Unicode".into())
            })?;
        require_argument(
            matches!(
                flag.as_str(),
                MANIFEST_OPTION | OUTPUT_OPTION | TESTED_COMMIT_OPTION | PROFILE_OPTION
            ),
            format!("unknown option {flag}"),
        )?;
        require_argument(
            options.insert(flag.clone(), value).is_none(),
            format!("duplicate option {flag}"),
        )?;
    }
    require_argument(
        options.len() == 4,
        "all four graph-scale options are required",
    )?;
    let tested_commit = required_option(&options, TESTED_COMMIT_OPTION)?;
    require_argument(
        is_canonical_git_object_id(&tested_commit),
        "tested commit must be a full lowercase SHA-1 or SHA-256 object identifier",
    )?;
    let arguments = GraphScaleArguments {
        manifest: PathBuf::from(required_option(&options, MANIFEST_OPTION)?),
        output: PathBuf::from(required_option(&options, OUTPUT_OPTION)?),
        tested_commit,
        profile: EvidenceProfile::parse(&required_option(&options, PROFILE_OPTION)?)?,
    };
    if internal {
        Ok(GraphScaleCommand::Workload(arguments))
    } else {
        Ok(GraphScaleCommand::Supervisor(arguments))
    }
}

/// Execute the parsed supervisor or internal workload command.
pub(super) async fn run(command: GraphScaleCommand) -> Result<(), GraphScaleEvaluationError> {
    match command {
        GraphScaleCommand::Supervisor(arguments) => run_supervisor(&arguments).await,
        GraphScaleCommand::Workload(arguments) => run_internal_workload(&arguments),
    }
}

/// Supervise the same executable, sample its process group, and retain final evidence.
async fn run_supervisor(arguments: &GraphScaleArguments) -> Result<(), GraphScaleEvaluationError> {
    let (manifest_bytes, envelope, plan) = load_plan(arguments)?;
    let source_root = source_root()?;
    let git = RepositoryGitProbe::resolve(&source_root)?;
    let observed_git_commit_before = git_output_with(&git, &["rev-parse", "HEAD"]).await?;
    require(
        observed_git_commit_before == arguments.tested_commit,
        format!(
            "tested commit differs from source HEAD before spawn: requested={}, observed={observed_git_commit_before}",
            arguments.tested_commit
        ),
    )?;
    let worktree_state_before = git.worktree_state().await?;
    let source_worktree_dirty_before = !worktree_state_before.is_empty();
    if arguments.profile == EvidenceProfile::Full {
        require(
            !source_worktree_dirty_before,
            "full graph-scale evidence requires a clean committed source worktree",
        )?;
    }
    let (source_commit_bindings, source_commit_verified) =
        if arguments.profile == EvidenceProfile::Full {
            let bindings =
                source_commit_bindings(&git, &arguments.tested_commit, COMPILED_SOURCE_BINDINGS)
                    .await?;
            require(
                bindings.iter().all(|binding| binding.exact_match),
                "one or more compiled graph-scale inputs differ from the tested commit",
            )?;
            (bindings, true)
        } else {
            (Vec::new(), false)
        };

    let executable = fs::canonicalize(std::env::current_exe()?)?;
    let executable_path = path_text(&executable)?;
    let executable_sha256 = sha256_file(&executable)?;
    let directory = tempfile::tempdir()?;
    let child_output = directory.path().join("workload-evidence.json");
    let manifest_path = fs::canonicalize(&arguments.manifest)?;
    let workload_arguments = vec![
        INTERNAL_WORKLOAD_FLAG.to_owned(),
        MANIFEST_OPTION.to_owned(),
        path_text(&manifest_path)?,
        OUTPUT_OPTION.to_owned(),
        path_text(&child_output)?,
        TESTED_COMMIT_OPTION.to_owned(),
        arguments.tested_commit.clone(),
        PROFILE_OPTION.to_owned(),
        arguments.profile.as_str().to_owned(),
    ];
    let workload_command = std::iter::once(executable_path.clone())
        .chain(workload_arguments.iter().cloned())
        .collect::<Vec<_>>();
    let process_outcome = run_measured_process(
        &executable,
        &workload_arguments,
        Duration::from_secs(plan.workload_timeout_seconds),
        plan.process_output_limit_bytes,
        Duration::from_millis(plan.process_sample_interval_ms),
        plan.resource_gates.max_process_group_resident_bytes,
    )
    .await?;
    require(
        sha256_file(&executable)? == executable_sha256,
        "evidence executable changed while the workload ran",
    )?;
    let child_timed_out = process_outcome.evidence.timed_out;
    let child_exit_code = process_outcome.evidence.exit_code;
    let child_output_truncated = process_outcome.evidence.output_truncated;
    let child_stderr = &process_outcome.stderr_diagnostic;

    let observed_git_commit_after = git_output_with(&git, &["rev-parse", "HEAD"]).await?;
    let worktree_state_after = git.worktree_state().await?;
    let source_worktree_dirty_after = !worktree_state_after.is_empty();
    let head_matches_requested_commit = observed_git_commit_before == arguments.tested_commit
        && observed_git_commit_after == arguments.tested_commit;
    if arguments.profile == EvidenceProfile::Full {
        require(
            head_matches_requested_commit && !source_worktree_dirty_after,
            "full graph-scale evidence source state changed while the workload ran",
        )?;
    }
    require(
        process_outcome.evidence.successful_bounded_completion,
        format!(
            "workload child failed closed: exit_code={child_exit_code:?}, timed_out={child_timed_out}, output_truncated={child_output_truncated}, workload_artifact_exists={}, stderr={child_stderr}",
            child_output.exists()
        ),
    )?;
    validate_process_evidence(
        &process_outcome.evidence,
        Duration::from_millis(plan.process_sample_interval_ms),
        Duration::from_secs(plan.workload_timeout_seconds),
        plan.process_output_limit_bytes,
        plan.resource_gates.max_process_group_resident_bytes,
        arguments.profile == EvidenceProfile::Full,
    )?;
    let source_state_commit_bound = arguments.profile == EvidenceProfile::Full
        && head_matches_requested_commit
        && !source_worktree_dirty_before
        && !source_worktree_dirty_after
        && source_commit_verified;

    let workload_artifact: GraphScaleWorkloadArtifact =
        serde_json::from_slice(&fs::read(&child_output)?)?;
    require(
        workload_artifact.schema_version == 1
            && workload_artifact.artifact_kind == WORKLOAD_ARTIFACT_KIND,
        "workload artifact identity drifted",
    )?;
    validate_workload(&workload_artifact.evidence)?;
    require(
        workload_artifact.evidence.profile == arguments.profile,
        "workload profile differs from supervisor profile",
    )?;

    let supervisor_command = std::iter::once(executable_path)
        .chain(unicode_arguments(std::env::args_os().skip(1))?)
        .collect::<Vec<_>>();
    let command_sha256 = sha256_hex(&serde_json::to_vec(&supervisor_command)?);
    let workload_command_sha256 = sha256_hex(&serde_json::to_vec(&workload_command)?);
    let provenance = ProvenanceEvidence {
        executable_sha256,
        manifest_sha256: sha256_hex(&manifest_bytes),
        graph_scale_evaluator_source_sha256: sha256_hex(EVALUATOR_SOURCE_BYTES),
        graph_scale_plan_source_sha256: sha256_hex(PLAN_SOURCE_BYTES),
        graph_scale_process_source_sha256: sha256_hex(PROCESS_SOURCE_BYTES),
        bounded_process_policy_source_sha256: sha256_hex(PROCESS_POLICY_SOURCE_BYTES),
        git_process_policy_source_sha256: sha256_hex(GIT_POLICY_SOURCE_BYTES),
        workspace_manifest_sha256: sha256_hex(WORKSPACE_MANIFEST_BYTES),
        cli_manifest_sha256: sha256_hex(CLI_MANIFEST_BYTES),
        core_manifest_sha256: sha256_hex(CORE_MANIFEST_BYTES),
        database_manifest_sha256: sha256_hex(DATABASE_MANIFEST_BYTES),
        service_manifest_sha256: sha256_hex(SERVICE_MANIFEST_BYTES),
        core_lib_source_sha256: sha256_hex(CORE_LIB_SOURCE_BYTES),
        core_budget_source_sha256: sha256_hex(CORE_BUDGET_SOURCE_BYTES),
        core_graph_source_sha256: sha256_hex(CORE_GRAPH_SOURCE_BYTES),
        core_symbols_source_sha256: sha256_hex(CORE_SYMBOLS_SOURCE_BYTES),
        service_lib_source_sha256: sha256_hex(SERVICE_LIB_SOURCE_BYTES),
        service_graph_query_source_sha256: sha256_hex(SERVICE_QUERY_SOURCE_BYTES),
        database_source_sha256: sha256_hex(DATABASE_SOURCE_BYTES),
        database_schema_source_sha256: sha256_hex(DATABASE_SCHEMA_SOURCE_BYTES),
        structural_publication_source_sha256: sha256_hex(PUBLICATION_SOURCE_BYTES),
        example_source_sha256: sha256_hex(EXAMPLE_SOURCE_BYTES),
        cargo_lock_sha256: sha256_hex(CARGO_LOCK_BYTES),
        supervisor_command,
        workload_command,
        external_timeout_seconds: plan.workload_timeout_seconds,
        git_executable_path: path_text(git.executable())?,
        git_executable_sha256: git.executable_sha256().to_owned(),
        observed_git_commit_before,
        observed_git_commit_after,
        head_matches_requested_commit,
        source_worktree_dirty_before,
        source_worktree_dirty_after,
        source_state_commit_bound,
        worktree_state_before_sha256: sha256_hex(&worktree_state_before),
        worktree_state_after_sha256: sha256_hex(&worktree_state_after),
        source_commit_bindings,
        source_commit_verified,
        command_sha256,
        workload_command_sha256,
    };
    let payload = GraphScaleEvidencePayload {
        manifest_id: envelope.manifest_id,
        tested_commit: arguments.tested_commit.clone(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        status: match arguments.profile {
            EvidenceProfile::Full => STATUS_PASSED,
            EvidenceProfile::Reduced => STATUS_EXPLORATORY,
        }
        .to_owned(),
        profile: arguments.profile,
        plan,
        workload: workload_artifact.evidence,
        process: process_outcome.evidence,
        provenance,
        claim_scope: ClaimScope {
            claim_eligible: false,
            claim_status: match arguments.profile {
                EvidenceProfile::Full => CLAIM_STATUS_IMPLEMENTATION_SCALE,
                EvidenceProfile::Reduced => CLAIM_STATUS_EXPLORATORY,
            }
            .to_owned(),
            full_declared_scale_executed: arguments.profile == EvidenceProfile::Full,
            exclusions: ClaimExclusions {
                not_calibrated_reference_host: true,
                not_mcp_latency: true,
                not_release_superiority_evidence: true,
            },
        },
    };
    let payload_sha256 = sha256_hex(&serde_json::to_vec(&payload)?);
    let artifact = GraphScaleEvidenceArtifact {
        schema_version: 1,
        artifact_kind: ARTIFACT_KIND.to_owned(),
        payload_sha256,
        payload,
    };
    validate_artifact(&artifact)?;
    write_json_create_new(&arguments.output, &artifact)
}

/// Execute and retain one raw child workload artifact.
fn run_internal_workload(arguments: &GraphScaleArguments) -> Result<(), GraphScaleEvaluationError> {
    let (_manifest_bytes, _envelope, plan) = load_plan(arguments)?;
    let evidence = execute_workload(arguments.profile, &plan)?;
    validate_workload(&evidence)?;
    write_json_create_new(
        &arguments.output,
        &GraphScaleWorkloadArtifact {
            schema_version: 1,
            artifact_kind: WORKLOAD_ARTIFACT_KIND.to_owned(),
            evidence,
        },
    )
}

/// Load the exact compiled manifest and choose the requested profile plan.
fn load_plan(
    arguments: &GraphScaleArguments,
) -> Result<(Vec<u8>, EvaluationManifestEnvelope, GraphScalePlan), GraphScaleEvaluationError> {
    let manifest_bytes = fs::read(&arguments.manifest)?;
    require(
        manifest_bytes == MANIFEST_BYTES,
        "manifest bytes differ from the evaluator compiled into this executable",
    )?;
    let envelope: EvaluationManifestEnvelope = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest_envelope(&envelope)?;
    let plan = match arguments.profile {
        EvidenceProfile::Full => envelope.architecture_evaluations.graph_scale.clone(),
        EvidenceProfile::Reduced => {
            reduced_evidence_plan(&envelope.architecture_evaluations.graph_scale)
        }
    };
    plan.validate_shape()?;
    Ok((manifest_bytes, envelope, plan))
}

/// Reconcile manifest-owned graph scale and locked dependency identity.
fn validate_manifest_envelope(
    envelope: &EvaluationManifestEnvelope,
) -> Result<(), GraphScaleEvaluationError> {
    envelope
        .architecture_evaluations
        .graph_scale
        .validate_declared()?;
    validate_sha256_hex(
        &envelope.projectatlas.cargo_lock_sha256,
        "manifest Cargo lock digest",
    )?;
    require(
        envelope.projectatlas.cargo_lock_sha256 == sha256_hex(CARGO_LOCK_BYTES),
        "manifest Cargo lock digest differs from the dependency graph compiled into the evaluator",
    )
}

/// Execute the production staging/publication path and both query cells.
fn execute_workload(
    profile: EvidenceProfile,
    plan: &GraphScalePlan,
) -> Result<GraphScaleWorkloadEvidence, GraphScaleEvaluationError> {
    plan.validate_shape()?;
    if profile == EvidenceProfile::Full {
        plan.validate_declared()?;
    }
    let publication_lifecycle_started = Instant::now();
    let directory = tempfile::tempdir()?;
    let live_directory = directory.path().join("live-database");
    let staging_directory = directory.path().join("staging-database");
    fs::create_dir(&live_directory)?;
    fs::create_dir(&staging_directory)?;
    let live_path = live_directory.join("repository-graph.db");
    let staging_path = staging_directory.join("repository-graph.db");
    let project_root = directory.path().join("repository");
    fs::create_dir(&project_root)?;
    let allocation_region = Region::new(&INSTRUMENTED_SYSTEM);
    let mut live = AtlasStore::open(&live_path)?;
    live.set_project_root(&project_root)?;
    let publication_before = live.publication_state()?;
    let staging = live.create_structural_staging(&live_path, &staging_path, &project_root)?;
    let mut stage = AtlasStore::open(staging.path())?;
    stage.prepare_structural_full_scan()?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(plan.worker_count)
        .build()
        .map_err(|error| {
            GraphScaleEvaluationError::Policy(format!("graph worker pool failed: {error}"))
        })?;

    let mut construction_ns = 0_u64;
    let mut staging_persistence_ns = 0_u64;
    let mut completed_graph_count = 0_usize;
    let mut producer_path_sequence = Sha256::new();
    for batch_start in (0..plan.graph_count).step_by(plan.completed_graphs_per_batch) {
        let batch_end = batch_start
            .saturating_add(plan.completed_graphs_per_batch)
            .min(plan.graph_count);
        let construction_started = Instant::now();
        let completed_results = pool.install(|| {
            (batch_start..batch_end)
                .into_par_iter()
                .map(|index| build_graph(index, plan))
                .collect::<Vec<_>>()
        });
        construction_ns = construction_ns.saturating_add(elapsed_ns(construction_started));
        let completed_batch = completed_results
            .into_iter()
            .collect::<Result<Vec<_>, GraphScaleEvaluationError>>()?;

        let persistence_started = Instant::now();
        for (batch_offset, graph) in completed_batch.iter().enumerate() {
            let graph_index = batch_start.saturating_add(batch_offset);
            require(
                graph_index < batch_end && graph.path() == graph_path(graph_index),
                "completed graph batch did not preserve exact producer order",
            )?;
            hash_field(&mut producer_path_sequence, graph.path().as_bytes());
            completed_graph_count = completed_graph_count.saturating_add(1);
            stage.stage_compact_symbol_graph(graph)?;
        }
        staging_persistence_ns =
            staging_persistence_ns.saturating_add(elapsed_ns(persistence_started));
    }

    let staging_signature = format!(
        "graph-scale:{}:{}",
        profile.as_str(),
        sha256_hex(MANIFEST_BYTES)
    );
    stage.set_staged_structural_state_signature(&staging, &staging_signature)?;
    let staging_before_seal = sqlite_storage_snapshot(&staging_path, &staging_directory)?;
    let sealing_started = Instant::now();
    stage.seal_structural_staging(&staging)?;
    let sealing_checkpoint_ns = elapsed_ns(sealing_started);
    let staging_after_seal = sqlite_storage_snapshot(&staging_path, &staging_directory)?;
    drop(stage);

    let live_before_publication = sqlite_storage_snapshot(&live_path, &live_directory)?;
    let publication_started = Instant::now();
    let publication_after = live.publish_structural_staging(&staging)?;
    let inactive_slot_publication_ns = elapsed_ns(publication_started);
    let expected_publication_after = publication_before.next_full().map_err(|error| {
        GraphScaleEvaluationError::Policy(format!("publication epoch overflowed: {error}"))
    })?;
    let publication = PublicationEvidence {
        before: publication_before.into(),
        expected_after: expected_publication_after.into(),
        after: publication_after.into(),
        exact_transition: publication_after == expected_publication_after,
    };

    let expected_entities = plan.expected_entities()?;
    let expected_relations = plan.expected_relations()?;
    let logical_facts = expected_entities.saturating_add(expected_relations);
    let observed = live.graph_fact_counts()?;
    let total_publication_lifecycle_ns = elapsed_ns(publication_lifecycle_started);
    let live_after_publication = sqlite_storage_snapshot(&live_path, &live_directory)?;
    let integrity = graph_integrity_evidence(&live_path, publication_after)?;
    let queries = run_warm_queries(&live, plan)?;
    drop(live);
    checkpoint_database(&live_path)?;
    let live_final_after_checkpoint = sqlite_storage_snapshot(&live_path, &live_directory)?;
    cleanup_sqlite_file_family(&staging_path)?;
    let staging_after_cleanup = sqlite_file_family_observation(&staging_path)?;
    let persistent_live_bytes = live_final_after_checkpoint.total_bytes;
    let max_observed_live_and_staging_bytes = max_observed_live_and_staging_bytes(
        &live_before_publication,
        &staging_before_seal,
        &staging_after_seal,
        &live_after_publication,
    )?;
    let persistent_live_bytes_per_logical_fact =
        ceiling_ratio(persistent_live_bytes, usize_to_u64(logical_facts));
    let logical_index_page_bytes_per_logical_fact = ceiling_ratio(
        live_final_after_checkpoint.logical_index_page_bytes,
        usize_to_u64(logical_facts),
    );
    let storage = StorageMeasurements {
        live_before_publication,
        staging_before_seal,
        staging_after_seal,
        live_after_publication,
        live_final_after_checkpoint,
        staging_after_cleanup,
        persistent_live_bytes,
        max_observed_live_and_staging_bytes,
        persistent_live_bytes_per_logical_fact,
        logical_index_page_bytes_per_logical_fact,
        physical_write_accounting_status: PHYSICAL_WRITE_ACCOUNTING_STATUS.to_owned(),
    };
    let allocation_stats = allocation_region.change();
    let rust_allocator = allocation_measurements(allocation_stats, logical_facts);
    let staging_logical_facts_per_second = rate_per_second(logical_facts, staging_persistence_ns);
    let inactive_slot_publication_logical_facts_per_second =
        rate_per_second(logical_facts, inactive_slot_publication_ns);
    let completed_batch_order = CompletedBatchOrderEvidence {
        completed_graph_count,
        producer_path_sequence_sha256: format!("{:x}", producer_path_sequence.finalize()),
    };
    let lifecycle = LifecycleMeasurements {
        construction_ns,
        staging_persistence_ns,
        staging_logical_facts_per_second,
        sealing_checkpoint_ns,
        inactive_slot_publication_ns,
        inactive_slot_publication_logical_facts_per_second,
        total_publication_lifecycle_ns,
    };
    let gates = GraphScaleWorkloadGates {
        entity_count: (observed.entities == expected_entities).into(),
        relation_count: (observed.relations == expected_relations).into(),
        evidence_count: (observed.evidence_occurrences == expected_relations).into(),
        resolution_abstentions_empty: (observed.resolution_occurrences == 0
            && observed.resolution_candidates == 0)
            .into(),
        relation_evidence_integrity: (integrity.relations_missing_evidence == 0
            && integrity.relations_with_duplicate_evidence == 0
            && integrity.orphan_evidence_occurrences == 0)
            .into(),
        publication_transition: publication.exact_transition.into(),
        rust_allocator_requests: (rust_allocator.rust_allocator_requests_per_logical_fact
            <= plan
                .resource_gates
                .max_rust_allocator_requests_per_logical_fact)
            .into(),
        completed_batch_order: (completed_batch_order == expected_completed_batch_order(plan))
            .into(),
        staging_throughput: (staging_logical_facts_per_second
            >= plan.resource_gates.min_staging_logical_facts_per_second)
            .into(),
        derived_storage: (persistent_live_bytes_per_logical_fact
            <= plan
                .resource_gates
                .max_derived_storage_bytes_per_logical_fact)
            .into(),
        logical_index_storage: (logical_index_page_bytes_per_logical_fact
            <= plan
                .resource_gates
                .max_derived_storage_bytes_per_logical_fact)
            .into(),
        sqlite_query_protocol: query_cell_passed(&queries.sqlite_stable_key).into(),
        service_query_protocol: query_cell_passed(&queries.service_bounded_three_hop).into(),
    };
    let evidence = GraphScaleWorkloadEvidence {
        profile,
        plan: plan.clone(),
        lifecycle,
        completed_batch_order,
        rust_allocator,
        storage,
        queries,
        publication,
        reconciliation: GraphScaleReconciliation {
            expected_entities,
            expected_relations,
            observed,
        },
        integrity,
        gates,
    };
    validate_workload(&evidence)?;
    Ok(evidence)
}

/// Return an explicitly non-certifying small real-path evidence plan.
fn reduced_evidence_plan(plan: &GraphScalePlan) -> GraphScalePlan {
    let mut plan = plan.clone();
    plan.graph_count = 2;
    plan.declarations_per_graph = 8;
    plan.call_edges_per_declaration = 2;
    plan.extra_call_edges_per_graph = 0;
    plan.worker_count = 2;
    plan.completed_graphs_per_batch = 2;
    plan.query_warmups = 2;
    plan.query_repetitions = 20;
    plan.workload_timeout_seconds = 120;
    plan.resource_gates
        .max_rust_allocator_requests_per_logical_fact = 10_000.0;
    plan.resource_gates.max_process_group_resident_bytes = u64::MAX;
    plan.resource_gates.min_staging_logical_facts_per_second = 1;
    plan.resource_gates
        .max_derived_storage_bytes_per_logical_fact = u64::MAX;
    plan
}

/// Derive the exact graph-path sequence required by the manifest-owned producer range.
fn expected_completed_batch_order(plan: &GraphScalePlan) -> CompletedBatchOrderEvidence {
    let mut sequence = Sha256::new();
    for graph_index in 0..plan.graph_count {
        hash_field(&mut sequence, graph_path(graph_index).as_bytes());
    }
    CompletedBatchOrderEvidence {
        completed_graph_count: plan.graph_count,
        producer_path_sequence_sha256: format!("{:x}", sequence.finalize()),
    }
}

/// Return the stable repository path owned by one synthetic graph index.
fn graph_path(graph_index: usize) -> String {
    format!("src/scale/graph_{graph_index:04}.rs")
}

/// Build one bounded worker-owned graph with deterministic typed declarations and calls.
fn build_graph(
    graph_index: usize,
    plan: &GraphScalePlan,
) -> Result<CompactSymbolGraph, GraphScaleEvaluationError> {
    let path = graph_path(graph_index);
    let mut symbols = Vec::with_capacity(plan.declarations_per_graph);
    let relation_capacity = plan
        .declarations_per_graph
        .saturating_mul(plan.call_edges_per_declaration)
        .saturating_add(plan.extra_call_edges_per_graph);
    let mut relations = Vec::with_capacity(relation_capacity);
    for symbol_index in 0..plan.declarations_per_graph {
        let name = symbol_name(graph_index, symbol_index);
        symbols.push(CodeSymbol {
            path: path.clone(),
            language: Some(GRAPH_LANGUAGE.to_owned()),
            name: name.clone(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            exported: false,
            documentation: None,
            line_start: symbol_index.saturating_add(1),
            line_end: symbol_index.saturating_add(1),
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some(GRAPH_DETAIL.to_owned()),
        });
        for edge_offset in 1..=plan.call_edges_per_declaration {
            let target_index = symbol_index
                .saturating_add(edge_offset)
                .checked_rem(plan.declarations_per_graph)
                .ok_or_else(|| {
                    GraphScaleEvaluationError::Policy("declaration divisor was zero".into())
                })?;
            relations.push(call_relation(
                &path,
                &name,
                &symbol_name(graph_index, target_index),
                symbol_index.saturating_add(1),
            ));
        }
    }
    let source_name = symbol_name(graph_index, 0);
    for offset in 0..plan.extra_call_edges_per_graph {
        let target_index = plan
            .call_edges_per_declaration
            .saturating_add(1)
            .saturating_add(offset)
            .checked_rem(plan.declarations_per_graph)
            .ok_or_else(|| {
                GraphScaleEvaluationError::Policy("declaration divisor was zero".into())
            })?;
        relations.push(call_relation(
            &path,
            &source_name,
            &symbol_name(graph_index, target_index),
            plan.declarations_per_graph
                .saturating_add(offset)
                .saturating_add(1),
        ));
    }
    Ok(CompactSymbolGraph::try_from(SymbolGraph {
        path,
        language: Some(GRAPH_LANGUAGE.to_owned()),
        parser: ParserKind::TreeSitter,
        symbols,
        relations,
    })?)
}

/// Build one deterministic synthetic call occurrence.
fn call_relation(path: &str, source: &str, target: &str, line: usize) -> SymbolRelation {
    SymbolRelation {
        path: path.to_owned(),
        source_name: source.to_owned(),
        target_name: target.to_owned(),
        kind: RelationKind::Calls,
        line,
        context: CALL_CONTEXT.to_owned(),
        parser: ParserKind::TreeSitter,
    }
}

/// Return one globally unique declaration name in stable lexical order.
fn symbol_name(graph_index: usize, symbol_index: usize) -> String {
    format!("graph_{graph_index:04}_symbol_{symbol_index:04}")
}

/// Measure warmed direct adapter and production bounded-three-hop service cells.
fn run_warm_queries(
    store: &AtlasStore,
    plan: &GraphScalePlan,
) -> Result<QueryMeasurements, GraphScaleEvaluationError> {
    let seed = store
        .load_graph_entities_by_qualified_name(GraphEntityKind::Declaration, &symbol_name(0, 0), 1)?
        .into_iter()
        .next()
        .ok_or_else(|| GraphScaleEvaluationError::Policy("query seed entity is missing".into()))?;
    let direct_query_plan = query_plan_evidence(
        store.graph_entity_query_plan(&seed.stable_key_digest)?,
        QueryPlanRequirement::stable_entity_lookup(),
    )?;
    let service_query_plan = query_plan_evidence(
        store.graph_outbound_relations_by_kind_query_plan(
            &seed.stable_key_digest,
            GraphRelationKind::Calls,
            plan.query_limit,
        )?,
        QueryPlanRequirement::outbound_call_adjacency(),
    )?;
    let mut direct_expected = None;
    for _ in 0..plan.query_warmups {
        let result = store.load_graph_entity(&seed.stable_key_digest)?;
        let observation = direct_entity_observation(result.as_ref());
        establish_or_require_query_result(&mut direct_expected, &observation)?;
    }
    let direct_expected = direct_expected.ok_or_else(|| {
        GraphScaleEvaluationError::Policy("direct query warmup result was missing".into())
    })?;
    let mut direct_durations = Vec::with_capacity(plan.query_repetitions);
    let mut direct_counts = Vec::with_capacity(plan.query_repetitions);
    let mut direct_digests = Vec::with_capacity(plan.query_repetitions);
    for _ in 0..plan.query_repetitions {
        let started = Instant::now();
        let result = store.load_graph_entity(&seed.stable_key_digest)?;
        let duration = elapsed_ns(started);
        let observation = direct_entity_observation(result.as_ref());
        require_query_result(&direct_expected, &observation)?;
        direct_durations.push(duration);
        direct_counts.push(observation.0);
        direct_digests.push(observation.1);
    }

    let mut service_expected = None;
    for _ in 0..plan.query_warmups {
        let result = graph_query::bounded_three_hop(
            store,
            seed.stable_key_digest,
            GraphRelationKind::Calls,
            plan.query_limit,
        )?;
        let observation = service_result_observation(&result);
        establish_or_require_query_result(&mut service_expected, &observation)?;
    }
    let service_expected = service_expected.ok_or_else(|| {
        GraphScaleEvaluationError::Policy("service query warmup result was missing".into())
    })?;
    let mut service_durations = Vec::with_capacity(plan.query_repetitions);
    let mut service_counts = Vec::with_capacity(plan.query_repetitions);
    let mut service_digests = Vec::with_capacity(plan.query_repetitions);
    for _ in 0..plan.query_repetitions {
        let started = Instant::now();
        let result = graph_query::bounded_three_hop(
            store,
            seed.stable_key_digest,
            GraphRelationKind::Calls,
            plan.query_limit,
        )?;
        let duration = elapsed_ns(started);
        let observation = service_result_observation(&result);
        require_query_result(&service_expected, &observation)?;
        service_durations.push(duration);
        service_counts.push(observation.0);
        service_digests.push(observation.1);
    }

    Ok(QueryMeasurements {
        clock: QUERY_CLOCK.to_owned(),
        sqlite_stable_key: query_cell(
            plan,
            direct_durations,
            direct_counts,
            direct_digests,
            direct_expected,
            direct_query_plan,
        )?,
        service_bounded_three_hop: query_cell(
            plan,
            service_durations,
            service_counts,
            service_digests,
            service_expected,
            service_query_plan,
        )?,
    })
}

/// Build one complete query-cell artifact without destroying raw execution order.
fn query_cell(
    plan: &GraphScalePlan,
    raw_duration_ns: Vec<u64>,
    raw_result_counts: Vec<u64>,
    raw_result_digests: Vec<String>,
    expected: (u64, String),
    query_plan: QueryPlanEvidence,
) -> Result<QueryCellEvidence, GraphScaleEvaluationError> {
    let p50_ns = nearest_rank_percentile(&raw_duration_ns, 50)?;
    let p95_ns = nearest_rank_percentile(&raw_duration_ns, 95)?;
    Ok(QueryCellEvidence {
        warmups: plan.query_warmups,
        repetitions: plan.query_repetitions,
        raw_duration_ns,
        raw_result_counts,
        raw_result_digests,
        expected_result_count: expected.0,
        expected_result_digest: expected.1,
        p50_ns,
        p95_ns,
        prepared_state: QUERY_PREPARED_STATE.to_owned(),
        cache_state: QUERY_CACHE_STATE.to_owned(),
        query_plan,
    })
}

/// Retain actual plan rows and fail closed when the intended production index is absent.
fn query_plan_evidence(
    actual_details: Vec<String>,
    requirement: QueryPlanRequirement,
) -> Result<QueryPlanEvidence, GraphScaleEvaluationError> {
    require(
        !actual_details.is_empty() && requirement.satisfied_by(&actual_details),
        format!("query plan did not satisfy {requirement:?}: {actual_details:?}"),
    )?;
    Ok(QueryPlanEvidence {
        requirement,
        actual_details,
    })
}

/// Hash one optional direct entity result after timing ends.
fn direct_entity_observation(
    entity: Option<&projectatlas_db::PersistedGraphEntity>,
) -> (u64, String) {
    let Some(entity) = entity else {
        return (0, sha256_hex(b"missing-entity"));
    };
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, &entity.stable_key_digest);
    hash_field(&mut hasher, entity.kind.as_str().as_bytes());
    hash_optional_text(&mut hasher, entity.repository_path.as_deref());
    hash_optional_text(&mut hasher, entity.qualified_name.as_deref());
    hash_optional_text(&mut hasher, entity.signature.as_deref());
    hash_optional_text(&mut hasher, entity.discriminator.as_deref());
    hash_field(&mut hasher, &entity.last_changed_epoch.get().to_le_bytes());
    (1, format!("{:x}", hasher.finalize()))
}

/// Hash one fully materialized production service result after timing ends.
fn service_result_observation(result: &graph_query::BoundedThreeHopResult) -> (u64, String) {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, &result.seed_entity_digest);
    hash_field(
        &mut hasher,
        &result.publication.active_epoch.get().to_le_bytes(),
    );
    hash_field(
        &mut hasher,
        match result.publication.active_slot {
            StructuralSlot::A => b"a",
            StructuralSlot::B => b"b",
        },
    );
    let mut count = 0_u64;
    for hop in &result.hops {
        hash_field(&mut hasher, &[hop.depth]);
        hash_field(&mut hasher, &hop.source_entity_digest);
        for relation in &hop.relations {
            count = count.saturating_add(1);
            hash_persisted_relation(&mut hasher, relation);
        }
    }
    (count, format!("{:x}", hasher.finalize()))
}

/// Hash every typed field that determines one persisted relation result.
fn hash_persisted_relation(
    hasher: &mut Sha256,
    relation: &projectatlas_db::PersistedGraphRelation,
) {
    hash_field(hasher, &relation.stable_key_digest);
    hash_field(hasher, &relation.source_entity_digest);
    hash_field(hasher, relation.kind.as_str().as_bytes());
    match &relation.target {
        PersistedGraphTarget::Internal(target) => {
            hash_field(hasher, b"internal");
            hash_field(hasher, target);
        }
        PersistedGraphTarget::External { namespace, value } => {
            hash_field(hasher, b"external");
            hash_field(hasher, namespace.as_bytes());
            hash_field(hasher, value.as_bytes());
        }
    }
    hash_field(hasher, &relation.last_changed_epoch.get().to_le_bytes());
}

/// Establish one expected result or require exact deterministic agreement.
fn establish_or_require_query_result(
    expected: &mut Option<(u64, String)>,
    observed: &(u64, String),
) -> Result<(), GraphScaleEvaluationError> {
    if let Some(expected) = expected {
        require_query_result(expected, observed)
    } else {
        *expected = Some(observed.clone());
        Ok(())
    }
}

/// Require exact result count and digest equality.
fn require_query_result(
    expected: &(u64, String),
    observed: &(u64, String),
) -> Result<(), GraphScaleEvaluationError> {
    require(
        expected == observed,
        format!("query result drifted: expected={expected:?}, observed={observed:?}"),
    )
}

/// Whether every raw query artifact agrees with its declared protocol.
fn query_cell_passed(cell: &QueryCellEvidence) -> bool {
    cell.warmups > 0
        && cell.repetitions > 0
        && cell.raw_duration_ns.len() == cell.repetitions
        && cell.raw_result_counts.len() == cell.repetitions
        && cell.raw_result_digests.len() == cell.repetitions
        && cell.raw_duration_ns.iter().all(|value| *value > 0)
        && cell
            .raw_result_counts
            .iter()
            .all(|value| *value == cell.expected_result_count)
        && cell
            .raw_result_digests
            .iter()
            .all(|value| value == &cell.expected_result_digest)
        && cell.expected_result_count > 0
        && cell.p50_ns > 0
        && cell.p95_ns > 0
        && cell
            .query_plan
            .requirement
            .satisfied_by(&cell.query_plan.actual_details)
}

/// Recompute every retained query statistic, result, and index-plan assertion.
fn validate_query_cell(
    cell: &QueryCellEvidence,
    plan: &GraphScalePlan,
    requirement: &QueryPlanRequirement,
) -> Result<(), GraphScaleEvaluationError> {
    require(
        cell.warmups == plan.query_warmups
            && cell.repetitions == plan.query_repetitions
            && cell.prepared_state == QUERY_PREPARED_STATE
            && cell.cache_state == QUERY_CACHE_STATE,
        "query cell plan or warm-cache declaration drifted",
    )?;
    require(
        query_cell_passed(cell)
            && cell.p50_ns == nearest_rank_percentile(&cell.raw_duration_ns, 50)?
            && cell.p95_ns == nearest_rank_percentile(&cell.raw_duration_ns, 95)?,
        "query cell raw samples or retained percentiles drifted",
    )?;
    validate_sha256_hex(&cell.expected_result_digest, "query expected result digest")?;
    for digest in &cell.raw_result_digests {
        validate_sha256_hex(digest, "query raw result digest")?;
    }
    require(
        &cell.query_plan.requirement == requirement
            && !cell.query_plan.actual_details.is_empty()
            && requirement.satisfied_by(&cell.query_plan.actual_details),
        format!("query cell did not retain a plan satisfying {requirement:?}"),
    )?;
    Ok(())
}

/// Observe main, WAL, SHM, page metadata, and evaluator-directory temporary bytes.
fn sqlite_storage_snapshot(
    database_path: &Path,
    evaluator_directory: &Path,
) -> Result<SqliteStorageSnapshot, GraphScaleEvaluationError> {
    let connection = read_only_connection(database_path)?;
    let page_size = pragma_u64(&connection, "PRAGMA page_size")?;
    let page_count = pragma_u64(&connection, "PRAGMA page_count")?;
    let freelist_count = pragma_u64(&connection, "PRAGMA freelist_count")?;
    let logical_index_page_bytes = connection.query_row(
        "SELECT COALESCE(SUM(pgsize), 0)
         FROM dbstat
         WHERE name IN (
             SELECT name FROM sqlite_schema WHERE type = 'index'
         )",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let logical_index_page_bytes = u64::try_from(logical_index_page_bytes).map_err(|_error| {
        GraphScaleEvaluationError::Policy(
            "SQLite dbstat returned negative logical index bytes".into(),
        )
    })?;
    drop(connection);
    let database = file_observation(database_path)?;
    let wal_path = sidecar_path(database_path, "-wal");
    let shm_path = sidecar_path(database_path, "-shm");
    let wal = file_observation(&wal_path)?;
    let shm = file_observation(&shm_path)?;
    let total_bytes = database
        .bytes
        .saturating_add(wal.bytes)
        .saturating_add(shm.bytes);
    let sqlite_temporary_bytes_observed =
        temporary_file_bytes(evaluator_directory, &[database_path, &wal_path, &shm_path])?;
    Ok(SqliteStorageSnapshot {
        database,
        wal,
        shm,
        total_bytes,
        page_size,
        page_count,
        freelist_count,
        logical_index_page_bytes,
        sqlite_temporary_bytes_observed,
        temporary_observation_status: TEMPORARY_OBSERVATION_STATUS.to_owned(),
    })
}

/// Checkpoint one closed live database through a short-lived explicit writer.
fn checkpoint_database(path: &Path) -> Result<(), GraphScaleEvaluationError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let busy = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        row.get::<_, i64>(0)
    })?;
    require(
        busy == 0,
        format!("final live WAL checkpoint remained busy: {busy}"),
    )?;
    drop(connection);
    Ok(())
}

/// Remove the exact sealed staging database family after successful publication.
fn cleanup_sqlite_file_family(path: &Path) -> Result<(), GraphScaleEvaluationError> {
    for candidate in [
        path.to_path_buf(),
        sidecar_path(path, "-wal"),
        sidecar_path(path, "-shm"),
    ] {
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Observe an exact `SQLite` file family without opening or recreating it.
fn sqlite_file_family_observation(
    path: &Path,
) -> Result<SqliteFileFamilyObservation, GraphScaleEvaluationError> {
    let database = file_observation(path)?;
    let wal = file_observation(&sidecar_path(path, "-wal"))?;
    let shm = file_observation(&sidecar_path(path, "-shm"))?;
    let total_bytes = database
        .bytes
        .checked_add(wal.bytes)
        .and_then(|value| value.checked_add(shm.bytes))
        .ok_or_else(|| {
            GraphScaleEvaluationError::Policy("SQLite file-family bytes overflowed".into())
        })?;
    Ok(SqliteFileFamilyObservation {
        database,
        wal,
        shm,
        total_bytes,
    })
}

/// Open a query-only `SQLite` connection without creating a database.
fn read_only_connection(path: &Path) -> Result<Connection, GraphScaleEvaluationError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// Read one nonnegative `SQLite` pragma value.
fn pragma_u64(connection: &Connection, pragma: &str) -> Result<u64, GraphScaleEvaluationError> {
    let value = connection.query_row(pragma, [], |row| row.get::<_, i64>(0))?;
    u64::try_from(value).map_err(|_error| {
        GraphScaleEvaluationError::Policy(format!("negative SQLite pragma value: {pragma}={value}"))
    })
}

/// Return presence and byte length without conflating absent and empty sidecars.
fn file_observation(path: &Path) -> Result<FileObservation, GraphScaleEvaluationError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(FileObservation {
            present: true,
            bytes: metadata.len(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileObservation {
            present: false,
            bytes: 0,
        }),
        Err(error) => Err(error.into()),
    }
}

/// Sum regular files not among the exact database/main-sidecar exclusions.
fn temporary_file_bytes(
    directory: &Path,
    excluded: &[&Path],
) -> Result<u64, GraphScaleEvaluationError> {
    let mut total = 0_u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if excluded.iter().any(|candidate| path == **candidate) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

/// Append a `SQLite` sidecar suffix without recoding the database path.
fn sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut value = database_path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

/// Retain canonical entity/relation/evidence digests and reconcile one-to-one evidence.
fn graph_integrity_evidence(
    database_path: &Path,
    publication: PublicationState,
) -> Result<GraphIntegrityEvidence, GraphScaleEvaluationError> {
    let connection = read_only_connection(database_path)?;
    let slot = slot_text(publication.active_slot);
    let entity_identity_digest_sha256 = entity_digest(&connection, slot)?;
    let mut relation_statement = connection.prepare(
        "SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
                target_entity_digest, external_target_namespace, external_target_value
         FROM graph_relations
         WHERE structural_slot = ?1
         ORDER BY stable_key_digest",
    )?;
    let mut evidence_statement = connection.prepare(
        "SELECT relation_digest, stable_key_digest, origin_kind, origin_repository_path,
                source_start_line, source_end_line, resolver_name, resolver_version,
                content_span_fingerprint, occurrence_discriminator
         FROM graph_evidence_occurrences
         WHERE structural_slot = ?1
         ORDER BY relation_digest, stable_key_digest",
    )?;
    let mut relation_rows = relation_statement.query([slot])?;
    let mut evidence_rows = evidence_statement.query([slot])?;
    let mut relation = next_relation_digest_row(&mut relation_rows)?;
    let mut evidence = next_evidence_digest_row(&mut evidence_rows)?;
    let mut relation_hasher = Sha256::new();
    let mut evidence_hasher = Sha256::new();
    let mut missing = 0_u64;
    let mut duplicate = 0_u64;
    let mut orphan = 0_u64;
    while let Some(relation_row) = relation.as_ref() {
        hash_relation_digest_row(&mut relation_hasher, relation_row);
        match evidence
            .as_ref()
            .map(|row| row.relation_digest.cmp(&relation_row.digest))
        {
            None | Some(Comparison::Greater) => {
                missing = missing.saturating_add(1);
                relation = next_relation_digest_row(&mut relation_rows)?;
            }
            Some(Comparison::Less) => {
                if let Some(row) = evidence.as_ref() {
                    hash_evidence_digest_row(&mut evidence_hasher, row);
                }
                orphan = orphan.saturating_add(1);
                evidence = next_evidence_digest_row(&mut evidence_rows)?;
            }
            Some(Comparison::Equal) => {
                let relation_key = relation_row.digest.clone();
                let mut count = 0_u64;
                while evidence
                    .as_ref()
                    .is_some_and(|row| row.relation_digest == relation_key)
                {
                    if let Some(row) = evidence.as_ref() {
                        hash_evidence_digest_row(&mut evidence_hasher, row);
                    }
                    count = count.saturating_add(1);
                    evidence = next_evidence_digest_row(&mut evidence_rows)?;
                }
                if count > 1 {
                    duplicate = duplicate.saturating_add(1);
                }
                relation = next_relation_digest_row(&mut relation_rows)?;
            }
        }
    }
    while let Some(row) = evidence {
        hash_evidence_digest_row(&mut evidence_hasher, &row);
        orphan = orphan.saturating_add(1);
        evidence = next_evidence_digest_row(&mut evidence_rows)?;
    }
    let relation_topology_digest_sha256 = format!("{:x}", relation_hasher.finalize());
    let evidence_occurrence_identity_digest_sha256 = format!("{:x}", evidence_hasher.finalize());
    let graph_evidence_digest_sha256 = graph_evidence_digest(
        &entity_identity_digest_sha256,
        &relation_topology_digest_sha256,
        &evidence_occurrence_identity_digest_sha256,
    );
    Ok(GraphIntegrityEvidence {
        entity_identity_digest_sha256,
        relation_topology_digest_sha256,
        evidence_occurrence_identity_digest_sha256,
        graph_evidence_digest_sha256,
        relations_missing_evidence: missing,
        relations_with_duplicate_evidence: duplicate,
        orphan_evidence_occurrences: orphan,
    })
}

/// Bind the three retained graph-integrity component digests with unambiguous framing.
fn graph_evidence_digest(
    entity_identity_digest_sha256: &str,
    relation_topology_digest_sha256: &str,
    evidence_occurrence_identity_digest_sha256: &str,
) -> String {
    let mut aggregate = Sha256::new();
    hash_field(&mut aggregate, entity_identity_digest_sha256.as_bytes());
    hash_field(&mut aggregate, relation_topology_digest_sha256.as_bytes());
    hash_field(
        &mut aggregate,
        evidence_occurrence_identity_digest_sha256.as_bytes(),
    );
    format!("{:x}", aggregate.finalize())
}

/// Hash every active entity in stable-key order.
fn entity_digest(connection: &Connection, slot: &str) -> Result<String, GraphScaleEvaluationError> {
    let mut statement = connection.prepare(
        "SELECT stable_key_digest, entity_kind, repository_path, qualified_name,
                signature, discriminator
         FROM graph_entities
         WHERE structural_slot = ?1
         ORDER BY stable_key_digest",
    )?;
    let mut rows = statement.query([slot])?;
    let mut hasher = Sha256::new();
    while let Some(row) = rows.next()? {
        hash_field(&mut hasher, &row.get::<_, Vec<u8>>(0)?);
        hash_field(&mut hasher, row.get::<_, String>(1)?.as_bytes());
        hash_optional_text(&mut hasher, row.get::<_, Option<String>>(2)?.as_deref());
        hash_optional_text(&mut hasher, row.get::<_, Option<String>>(3)?.as_deref());
        hash_optional_text(&mut hasher, row.get::<_, Option<String>>(4)?.as_deref());
        hash_optional_text(&mut hasher, row.get::<_, Option<String>>(5)?.as_deref());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Owned relation row used by the streaming integrity merge.
struct RelationDigestRow {
    /// Stable relation digest.
    digest: Vec<u8>,
    /// Stable source entity digest.
    source_digest: Vec<u8>,
    /// Typed relation kind spelling.
    kind: String,
    /// Internal or external target scope.
    target_scope: String,
    /// Internal target entity digest.
    target_digest: Option<Vec<u8>>,
    /// External target namespace.
    external_namespace: Option<String>,
    /// External target value.
    external_value: Option<String>,
}

/// Owned evidence row used by the streaming integrity merge.
struct EvidenceDigestRow {
    /// Stable relation digest owned by this occurrence.
    relation_digest: Vec<u8>,
    /// Stable evidence occurrence digest.
    digest: Vec<u8>,
    /// Typed evidence origin kind.
    origin_kind: String,
    /// Repository path for a path-owned origin.
    origin_path: Option<String>,
    /// Optional one-based source start line.
    start_line: Option<i64>,
    /// Optional one-based source end line.
    end_line: Option<i64>,
    /// Resolver identity.
    resolver_name: String,
    /// Resolver version.
    resolver_version: String,
    /// Content-anchored span fingerprint.
    span_fingerprint: Vec<u8>,
    /// Stable occurrence discriminator.
    occurrence_discriminator: i64,
}

/// Read the next owned relation row from an ordered `SQLite` cursor.
fn next_relation_digest_row(
    rows: &mut Rows<'_>,
) -> Result<Option<RelationDigestRow>, GraphScaleEvaluationError> {
    rows.next()?
        .map(|row| {
            Ok(RelationDigestRow {
                digest: row.get(0)?,
                source_digest: row.get(1)?,
                kind: row.get(2)?,
                target_scope: row.get(3)?,
                target_digest: row.get(4)?,
                external_namespace: row.get(5)?,
                external_value: row.get(6)?,
            })
        })
        .transpose()
}

/// Read the next owned evidence row from an ordered `SQLite` cursor.
fn next_evidence_digest_row(
    rows: &mut Rows<'_>,
) -> Result<Option<EvidenceDigestRow>, GraphScaleEvaluationError> {
    rows.next()?
        .map(|row| {
            Ok(EvidenceDigestRow {
                relation_digest: row.get(0)?,
                digest: row.get(1)?,
                origin_kind: row.get(2)?,
                origin_path: row.get(3)?,
                start_line: row.get(4)?,
                end_line: row.get(5)?,
                resolver_name: row.get(6)?,
                resolver_version: row.get(7)?,
                span_fingerprint: row.get(8)?,
                occurrence_discriminator: row.get(9)?,
            })
        })
        .transpose()
}

/// Hash one canonical relation row.
fn hash_relation_digest_row(hasher: &mut Sha256, row: &RelationDigestRow) {
    hash_field(hasher, &row.digest);
    hash_field(hasher, &row.source_digest);
    hash_field(hasher, row.kind.as_bytes());
    hash_field(hasher, row.target_scope.as_bytes());
    hash_optional_bytes(hasher, row.target_digest.as_deref());
    hash_optional_text(hasher, row.external_namespace.as_deref());
    hash_optional_text(hasher, row.external_value.as_deref());
}

/// Hash one canonical evidence row.
fn hash_evidence_digest_row(hasher: &mut Sha256, row: &EvidenceDigestRow) {
    hash_field(hasher, &row.relation_digest);
    hash_field(hasher, &row.digest);
    hash_field(hasher, row.origin_kind.as_bytes());
    hash_optional_text(hasher, row.origin_path.as_deref());
    hash_optional_i64(hasher, row.start_line);
    hash_optional_i64(hasher, row.end_line);
    hash_field(hasher, row.resolver_name.as_bytes());
    hash_field(hasher, row.resolver_version.as_bytes());
    hash_field(hasher, &row.span_fingerprint);
    hash_field(hasher, &row.occurrence_discriminator.to_le_bytes());
}

/// Convert global allocator statistics into honestly scoped evidence values.
fn allocation_measurements(stats: Stats, logical_facts: usize) -> RustAllocatorMeasurements {
    let denominator = logical_facts.max(1) as f64;
    let allocator_requests = stats.allocations.saturating_add(stats.reallocations);
    RustAllocatorMeasurements {
        scope: RUST_ALLOCATOR_SCOPE.to_owned(),
        sqlite_native_allocation_scope: SQLITE_NATIVE_ALLOCATION_SCOPE.to_owned(),
        rust_allocation_requests: usize_to_u64(stats.allocations),
        rust_deallocation_requests: usize_to_u64(stats.deallocations),
        rust_reallocation_requests: usize_to_u64(stats.reallocations),
        rust_allocator_requests: usize_to_u64(allocator_requests),
        rust_requested_bytes: usize_to_u64(stats.bytes_allocated),
        rust_deallocated_bytes: usize_to_u64(stats.bytes_deallocated),
        rust_reallocated_bytes: isize_to_i64(stats.bytes_reallocated),
        rust_allocator_requests_per_logical_fact: allocator_requests as f64 / denominator,
    }
}

/// Reconcile allocator component counts and the retained per-fact ratio.
fn validate_allocator_measurements(
    evidence: &RustAllocatorMeasurements,
    logical_facts: usize,
) -> Result<(), GraphScaleEvaluationError> {
    let expected_requests = evidence
        .rust_allocation_requests
        .saturating_add(evidence.rust_reallocation_requests);
    let expected_ratio = expected_requests as f64 / logical_facts.max(1) as f64;
    require(
        evidence.scope == RUST_ALLOCATOR_SCOPE
            && evidence.sqlite_native_allocation_scope == SQLITE_NATIVE_ALLOCATION_SCOPE
            && evidence.rust_allocator_requests == expected_requests
            && evidence.rust_allocator_requests_per_logical_fact.to_bits()
                == expected_ratio.to_bits(),
        "Rust allocator evidence components or per-fact ratio drifted",
    )
}

/// Reconcile isolated database-family snapshots and required retained bytes.
fn validate_storage_measurements(
    evidence: &StorageMeasurements,
    logical_facts: usize,
) -> Result<(), GraphScaleEvaluationError> {
    for snapshot in [
        &evidence.live_before_publication,
        &evidence.staging_before_seal,
        &evidence.staging_after_seal,
        &evidence.live_after_publication,
        &evidence.live_final_after_checkpoint,
    ] {
        let observed_total = snapshot
            .database
            .bytes
            .saturating_add(snapshot.wal.bytes)
            .saturating_add(snapshot.shm.bytes);
        require(
            snapshot.database.present
                && snapshot.database.bytes > 0
                && snapshot.total_bytes == observed_total
                && snapshot.page_size > 0
                && snapshot.page_count > 0
                && snapshot.freelist_count <= snapshot.page_count
                && snapshot.logical_index_page_bytes > 0
                && snapshot.logical_index_page_bytes <= snapshot.total_bytes
                && snapshot.sqlite_temporary_bytes_observed == 0
                && snapshot.temporary_observation_status == TEMPORARY_OBSERVATION_STATUS,
            "isolated SQLite storage snapshot is incomplete or internally inconsistent",
        )?;
    }
    let expected_max_observed = max_observed_live_and_staging_bytes(
        &evidence.live_before_publication,
        &evidence.staging_before_seal,
        &evidence.staging_after_seal,
        &evidence.live_after_publication,
    )?;
    require(
        !evidence.staging_after_cleanup.database.present
            && !evidence.staging_after_cleanup.wal.present
            && !evidence.staging_after_cleanup.shm.present
            && evidence.staging_after_cleanup.total_bytes == 0
            && evidence.persistent_live_bytes == evidence.live_final_after_checkpoint.total_bytes
            && evidence.max_observed_live_and_staging_bytes == expected_max_observed
            && evidence.persistent_live_bytes_per_logical_fact
                == ceiling_ratio(evidence.persistent_live_bytes, usize_to_u64(logical_facts))
            && evidence.logical_index_page_bytes_per_logical_fact
                == ceiling_ratio(
                    evidence
                        .live_final_after_checkpoint
                        .logical_index_page_bytes,
                    usize_to_u64(logical_facts),
                )
            && evidence.live_final_after_checkpoint.wal.bytes == 0
            && evidence
                .live_final_after_checkpoint
                .logical_index_page_bytes
                <= evidence.live_final_after_checkpoint.database.bytes
            && evidence.physical_write_accounting_status == PHYSICAL_WRITE_ACCOUNTING_STATUS,
        "persistent, coexistence, index, or staging-cleanup storage evidence drifted",
    )
}

/// Return the largest retained live-plus-staging coexistence observation.
fn max_observed_live_and_staging_bytes(
    live_before_publication: &SqliteStorageSnapshot,
    staging_before_seal: &SqliteStorageSnapshot,
    staging_after_seal: &SqliteStorageSnapshot,
    live_after_publication: &SqliteStorageSnapshot,
) -> Result<u64, GraphScaleEvaluationError> {
    [
        (
            live_before_publication.total_bytes,
            staging_before_seal.total_bytes,
        ),
        (
            live_before_publication.total_bytes,
            staging_after_seal.total_bytes,
        ),
        (
            live_after_publication.total_bytes,
            staging_after_seal.total_bytes,
        ),
    ]
    .into_iter()
    .try_fold(0_u64, |maximum, (live, staging)| {
        live.checked_add(staging)
            .map(|total| maximum.max(total))
            .ok_or_else(|| {
                GraphScaleEvaluationError::Policy(
                    "live/staging coexistence byte count overflowed".into(),
                )
            })
    })
}

/// Reconcile commit, source, command, worktree, and timeout provenance.
fn validate_provenance(
    evidence: &ProvenanceEvidence,
    tested_commit: &str,
    profile: EvidenceProfile,
    workload_timeout_seconds: u64,
) -> Result<(), GraphScaleEvaluationError> {
    validate_sha256_hex(&evidence.executable_sha256, "executable digest")?;
    validate_sha256_hex(&evidence.manifest_sha256, "manifest digest")?;
    validate_sha256_hex(
        &evidence.graph_scale_evaluator_source_sha256,
        "graph-scale evaluator source digest",
    )?;
    validate_sha256_hex(
        &evidence.graph_scale_plan_source_sha256,
        "graph-scale plan source digest",
    )?;
    validate_sha256_hex(
        &evidence.workspace_manifest_sha256,
        "workspace manifest digest",
    )?;
    validate_sha256_hex(&evidence.cli_manifest_sha256, "CLI manifest digest")?;
    validate_sha256_hex(&evidence.core_manifest_sha256, "core manifest digest")?;
    validate_sha256_hex(
        &evidence.database_manifest_sha256,
        "database manifest digest",
    )?;
    validate_sha256_hex(&evidence.service_manifest_sha256, "service manifest digest")?;
    validate_sha256_hex(
        &evidence.core_lib_source_sha256,
        "core crate boundary source digest",
    )?;
    validate_sha256_hex(
        &evidence.core_budget_source_sha256,
        "core budget policy source digest",
    )?;
    validate_sha256_hex(
        &evidence.core_graph_source_sha256,
        "core graph source digest",
    )?;
    validate_sha256_hex(
        &evidence.core_symbols_source_sha256,
        "core symbols source digest",
    )?;
    validate_sha256_hex(
        &evidence.service_lib_source_sha256,
        "service facade source digest",
    )?;
    validate_sha256_hex(
        &evidence.service_graph_query_source_sha256,
        "service graph-query source digest",
    )?;
    validate_sha256_hex(&evidence.database_source_sha256, "database source digest")?;
    validate_sha256_hex(
        &evidence.database_schema_source_sha256,
        "database schema source digest",
    )?;
    validate_sha256_hex(
        &evidence.structural_publication_source_sha256,
        "structural publication source digest",
    )?;
    validate_sha256_hex(&evidence.example_source_sha256, "example source digest")?;
    validate_sha256_hex(&evidence.cargo_lock_sha256, "Cargo lock digest")?;
    validate_sha256_hex(&evidence.git_executable_sha256, "Git executable digest")?;
    validate_sha256_hex(
        &evidence.worktree_state_before_sha256,
        "worktree state before digest",
    )?;
    validate_sha256_hex(
        &evidence.worktree_state_after_sha256,
        "worktree state after digest",
    )?;
    validate_sha256_hex(&evidence.command_sha256, "supervisor command digest")?;
    validate_sha256_hex(&evidence.workload_command_sha256, "workload command digest")?;
    validate_git_commit_hex(
        &evidence.observed_git_commit_before,
        "observed Git commit before",
    )?;
    validate_git_commit_hex(
        &evidence.observed_git_commit_after,
        "observed Git commit after",
    )?;
    validate_command_provenance(evidence, tested_commit, profile)?;
    let evidence_executable = Path::new(&evidence.supervisor_command[0]);
    require(
        evidence.head_matches_requested_commit
            == (evidence.observed_git_commit_before == tested_commit
                && evidence.observed_git_commit_after == tested_commit)
            && evidence.external_timeout_seconds == workload_timeout_seconds
            && workload_timeout_seconds > 0,
        "commit or external-timeout provenance is incomplete",
    )?;
    if profile == EvidenceProfile::Full {
        require(
            evidence.source_state_commit_bound
                && evidence.head_matches_requested_commit
                && !evidence.source_worktree_dirty_before
                && !evidence.source_worktree_dirty_after
                && evidence.source_commit_verified
                && evidence.source_commit_bindings.len() == COMPILED_SOURCE_BINDINGS.len(),
            "full evidence is missing exact tested-commit source bindings",
        )?;
        for (observed, expected) in evidence
            .source_commit_bindings
            .iter()
            .zip(COMPILED_SOURCE_BINDINGS)
        {
            validate_sha256_hex(&observed.compiled_sha256, "compiled source digest")?;
            validate_sha256_hex(&observed.commit_sha256, "commit source digest")?;
            require(
                observed.path == expected.path
                    && observed.compiled_sha256 == sha256_hex(expected.bytes)
                    && observed.compiled_sha256 == observed.commit_sha256
                    && observed.exact_match,
                format!("tested-commit source binding drifted for {}", expected.path),
            )?;
        }
    } else {
        require(
            !evidence.source_state_commit_bound
                && !evidence.source_commit_verified
                && evidence.source_commit_bindings.is_empty(),
            "reduced evidence must not claim tested-commit source binding",
        )?;
    }
    require(
        evidence.manifest_sha256 == sha256_hex(MANIFEST_BYTES)
            && evidence.graph_scale_evaluator_source_sha256 == sha256_hex(EVALUATOR_SOURCE_BYTES)
            && evidence.graph_scale_plan_source_sha256 == sha256_hex(PLAN_SOURCE_BYTES)
            && evidence.graph_scale_process_source_sha256 == sha256_hex(PROCESS_SOURCE_BYTES)
            && evidence.bounded_process_policy_source_sha256
                == sha256_hex(PROCESS_POLICY_SOURCE_BYTES)
            && evidence.git_process_policy_source_sha256 == sha256_hex(GIT_POLICY_SOURCE_BYTES)
            && evidence.workspace_manifest_sha256 == sha256_hex(WORKSPACE_MANIFEST_BYTES)
            && evidence.cli_manifest_sha256 == sha256_hex(CLI_MANIFEST_BYTES)
            && evidence.core_manifest_sha256 == sha256_hex(CORE_MANIFEST_BYTES)
            && evidence.database_manifest_sha256 == sha256_hex(DATABASE_MANIFEST_BYTES)
            && evidence.service_manifest_sha256 == sha256_hex(SERVICE_MANIFEST_BYTES)
            && evidence.core_lib_source_sha256 == sha256_hex(CORE_LIB_SOURCE_BYTES)
            && evidence.core_budget_source_sha256 == sha256_hex(CORE_BUDGET_SOURCE_BYTES)
            && evidence.core_graph_source_sha256 == sha256_hex(CORE_GRAPH_SOURCE_BYTES)
            && evidence.core_symbols_source_sha256 == sha256_hex(CORE_SYMBOLS_SOURCE_BYTES)
            && evidence.service_lib_source_sha256 == sha256_hex(SERVICE_LIB_SOURCE_BYTES)
            && evidence.service_graph_query_source_sha256 == sha256_hex(SERVICE_QUERY_SOURCE_BYTES)
            && evidence.database_source_sha256 == sha256_hex(DATABASE_SOURCE_BYTES)
            && evidence.database_schema_source_sha256 == sha256_hex(DATABASE_SCHEMA_SOURCE_BYTES)
            && evidence.structural_publication_source_sha256
                == sha256_hex(PUBLICATION_SOURCE_BYTES)
            && evidence.example_source_sha256 == sha256_hex(EXAMPLE_SOURCE_BYTES)
            && evidence.cargo_lock_sha256 == sha256_hex(CARGO_LOCK_BYTES)
            && evidence.command_sha256
                == sha256_hex(&serde_json::to_vec(&evidence.supervisor_command)?)
            && evidence.workload_command_sha256
                == sha256_hex(&serde_json::to_vec(&evidence.workload_command)?)
            && evidence_executable.is_absolute()
            && evidence.executable_sha256 == sha256_file(evidence_executable)?
            && Path::new(&evidence.git_executable_path).is_absolute()
            && evidence.git_executable_sha256
                == sha256_file(Path::new(&evidence.git_executable_path))?
            && evidence.worktree_state_before_sha256.len() == 64
            && evidence.worktree_state_after_sha256.len() == 64,
        "compiled input, command, executable, or worktree provenance drifted",
    )?;
    require(
        (evidence.source_worktree_dirty_before
            && evidence.worktree_state_before_sha256 != sha256_hex(&[]))
            || (!evidence.source_worktree_dirty_before
                && evidence.worktree_state_before_sha256 == sha256_hex(&[])),
        "worktree-before dirty flag and digest disagree",
    )?;
    require(
        (evidence.source_worktree_dirty_after
            && evidence.worktree_state_after_sha256 != sha256_hex(&[]))
            || (!evidence.source_worktree_dirty_after
                && evidence.worktree_state_after_sha256 == sha256_hex(&[])),
        "worktree-after dirty flag and digest disagree",
    )?;
    Ok(())
}

/// Validate exact supervisor and same-executable workload argument tuple structure.
fn validate_command_provenance(
    evidence: &ProvenanceEvidence,
    tested_commit: &str,
    profile: EvidenceProfile,
) -> Result<(), GraphScaleEvaluationError> {
    let supervisor = &evidence.supervisor_command;
    let workload = &evidence.workload_command;
    require(
        supervisor.len() == 9
            && workload.len() == 10
            && supervisor[0] == workload[0]
            && Path::new(&supervisor[0]).is_absolute()
            && supervisor[1] == MANIFEST_OPTION
            && supervisor[3] == OUTPUT_OPTION
            && supervisor[5] == TESTED_COMMIT_OPTION
            && supervisor[6] == tested_commit
            && supervisor[7] == PROFILE_OPTION
            && supervisor[8] == profile.as_str()
            && workload[1] == INTERNAL_WORKLOAD_FLAG
            && workload[2] == MANIFEST_OPTION
            && workload[4] == OUTPUT_OPTION
            && workload[6] == TESTED_COMMIT_OPTION
            && workload[7] == tested_commit
            && workload[8] == PROFILE_OPTION
            && workload[9] == profile.as_str()
            && !supervisor[0].is_empty()
            && !supervisor[2].is_empty()
            && !supervisor[4].is_empty()
            && !workload[0].is_empty()
            && !workload[3].is_empty()
            && !workload[5].is_empty()
            && supervisor[4] != workload[5],
        "supervisor or same-executable workload command tuple drifted",
    )
}

/// Validate one child workload before it can be retained or promoted by the supervisor.
fn validate_workload(
    evidence: &GraphScaleWorkloadEvidence,
) -> Result<(), GraphScaleEvaluationError> {
    evidence.plan.validate_shape()?;
    if evidence.profile == EvidenceProfile::Full {
        evidence.plan.validate_declared()?;
    } else {
        require(
            evidence.plan.expected_entities()? < 1_000_000
                && evidence.plan.expected_relations()? < 3_000_000,
            "reduced profile unexpectedly matches declared full scale",
        )?;
    }
    let expected_entities = evidence.plan.expected_entities()?;
    let expected_relations = evidence.plan.expected_relations()?;
    let logical_facts = expected_entities
        .checked_add(expected_relations)
        .ok_or_else(|| GraphScaleEvaluationError::Policy("logical fact count overflowed".into()))?;
    let expected_publication = evidence.publication.before.next_full()?;
    let publication_transition = expected_publication == evidence.publication.expected_after
        && expected_publication == evidence.publication.after;
    require(
        evidence.reconciliation.expected_entities == expected_entities
            && evidence.reconciliation.expected_relations == expected_relations,
        "workload expected counts differ from its plan",
    )?;
    require(
        evidence.publication.exact_transition == publication_transition && publication_transition,
        "workload did not publish exactly one inactive-slot generation",
    )?;
    validate_allocator_measurements(&evidence.rust_allocator, logical_facts)?;
    validate_storage_measurements(&evidence.storage, logical_facts)?;
    require(
        evidence.lifecycle.construction_ns > 0
            && evidence.lifecycle.staging_persistence_ns > 0
            && evidence.lifecycle.sealing_checkpoint_ns > 0
            && evidence.lifecycle.inactive_slot_publication_ns > 0
            && evidence.lifecycle.total_publication_lifecycle_ns > 0
            && evidence.lifecycle.staging_logical_facts_per_second
                == rate_per_second(logical_facts, evidence.lifecycle.staging_persistence_ns)
            && evidence
                .lifecycle
                .inactive_slot_publication_logical_facts_per_second
                == rate_per_second(
                    logical_facts,
                    evidence.lifecycle.inactive_slot_publication_ns,
                ),
        "workload lifecycle timing or throughput evidence drifted",
    )?;
    validate_sha256_hex(
        &evidence.completed_batch_order.producer_path_sequence_sha256,
        "completed-batch producer path sequence digest",
    )?;
    require(
        evidence.completed_batch_order == expected_completed_batch_order(&evidence.plan),
        "completed graph count or producer path order drifted",
    )?;
    let measured_lifecycle_ns = evidence
        .lifecycle
        .construction_ns
        .checked_add(evidence.lifecycle.staging_persistence_ns)
        .and_then(|value| value.checked_add(evidence.lifecycle.sealing_checkpoint_ns))
        .and_then(|value| value.checked_add(evidence.lifecycle.inactive_slot_publication_ns))
        .ok_or_else(|| {
            GraphScaleEvaluationError::Policy("workload lifecycle duration overflowed".into())
        })?;
    require(
        evidence.lifecycle.total_publication_lifecycle_ns >= measured_lifecycle_ns,
        "total publication lifecycle is shorter than its measured components",
    )?;
    validate_query_cell(
        &evidence.queries.sqlite_stable_key,
        &evidence.plan,
        &QueryPlanRequirement::stable_entity_lookup(),
    )?;
    validate_query_cell(
        &evidence.queries.service_bounded_three_hop,
        &evidence.plan,
        &QueryPlanRequirement::outbound_call_adjacency(),
    )?;
    require(
        evidence.queries.clock == QUERY_CLOCK,
        "query clock contract drifted",
    )?;
    validate_sha256_hex(
        &evidence.integrity.entity_identity_digest_sha256,
        "entity identity digest",
    )?;
    validate_sha256_hex(
        &evidence.integrity.relation_topology_digest_sha256,
        "relation topology digest",
    )?;
    validate_sha256_hex(
        &evidence
            .integrity
            .evidence_occurrence_identity_digest_sha256,
        "evidence occurrence identity digest",
    )?;
    validate_sha256_hex(
        &evidence.integrity.graph_evidence_digest_sha256,
        "aggregate graph evidence digest",
    )?;
    require(
        evidence.integrity.graph_evidence_digest_sha256
            == graph_evidence_digest(
                &evidence.integrity.entity_identity_digest_sha256,
                &evidence.integrity.relation_topology_digest_sha256,
                &evidence
                    .integrity
                    .evidence_occurrence_identity_digest_sha256,
            ),
        "aggregate graph evidence digest does not bind its component digests",
    )?;
    let expected_gates = GraphScaleWorkloadGates {
        entity_count: (evidence.reconciliation.observed.entities == expected_entities).into(),
        relation_count: (evidence.reconciliation.observed.relations == expected_relations).into(),
        evidence_count: (evidence.reconciliation.observed.evidence_occurrences
            == expected_relations)
            .into(),
        resolution_abstentions_empty: (evidence.reconciliation.observed.resolution_occurrences
            == 0
            && evidence.reconciliation.observed.resolution_candidates == 0)
            .into(),
        relation_evidence_integrity: (evidence.integrity.relations_missing_evidence == 0
            && evidence.integrity.relations_with_duplicate_evidence == 0
            && evidence.integrity.orphan_evidence_occurrences == 0)
            .into(),
        publication_transition: publication_transition.into(),
        rust_allocator_requests: (evidence
            .rust_allocator
            .rust_allocator_requests_per_logical_fact
            <= evidence
                .plan
                .resource_gates
                .max_rust_allocator_requests_per_logical_fact)
            .into(),
        completed_batch_order: (evidence.completed_batch_order
            == expected_completed_batch_order(&evidence.plan))
        .into(),
        staging_throughput: (evidence.lifecycle.staging_logical_facts_per_second
            >= evidence
                .plan
                .resource_gates
                .min_staging_logical_facts_per_second)
            .into(),
        derived_storage: (evidence.storage.persistent_live_bytes_per_logical_fact
            <= evidence
                .plan
                .resource_gates
                .max_derived_storage_bytes_per_logical_fact)
            .into(),
        logical_index_storage: (evidence.storage.logical_index_page_bytes_per_logical_fact
            <= evidence
                .plan
                .resource_gates
                .max_derived_storage_bytes_per_logical_fact)
            .into(),
        sqlite_query_protocol: query_cell_passed(&evidence.queries.sqlite_stable_key).into(),
        service_query_protocol: query_cell_passed(&evidence.queries.service_bounded_three_hop)
            .into(),
    };
    require(
        evidence.gates == expected_gates && expected_gates.all_passed(),
        format!("one or more workload gates failed: {:?}", evidence.gates),
    )?;
    Ok(())
}

/// Validate payload digest, claim scope, process samples, and full/reduced separation.
fn validate_artifact(
    artifact: &GraphScaleEvidenceArtifact,
) -> Result<(), GraphScaleEvaluationError> {
    require(
        artifact.schema_version == 1 && artifact.artifact_kind == ARTIFACT_KIND,
        "final artifact identity drifted",
    )?;
    require(
        artifact.payload_sha256 == sha256_hex(&serde_json::to_vec(&artifact.payload)?),
        "final artifact payload digest drifted",
    )?;
    require(
        artifact.payload.status
            == match artifact.payload.profile {
                EvidenceProfile::Full => STATUS_PASSED,
                EvidenceProfile::Reduced => STATUS_EXPLORATORY,
            },
        "final artifact status disagrees with its profile",
    )?;
    let manifest: EvaluationManifestEnvelope = serde_json::from_slice(MANIFEST_BYTES)?;
    validate_manifest_envelope(&manifest)?;
    require(
        artifact.payload.manifest_id == manifest.manifest_id
            && artifact.payload.profile == artifact.payload.workload.profile
            && artifact.payload.plan == artifact.payload.workload.plan
            && !artifact.payload.os.is_empty()
            && !artifact.payload.arch.is_empty(),
        "final artifact manifest, profile, plan, or platform identity drifted",
    )?;
    validate_git_commit_hex(&artifact.payload.tested_commit, "tested Git commit")?;
    validate_workload(&artifact.payload.workload)?;
    validate_provenance(
        &artifact.payload.provenance,
        &artifact.payload.tested_commit,
        artifact.payload.profile,
        artifact.payload.plan.workload_timeout_seconds,
    )?;
    validate_process_evidence(
        &artifact.payload.process,
        Duration::from_millis(artifact.payload.plan.process_sample_interval_ms),
        Duration::from_secs(artifact.payload.plan.workload_timeout_seconds),
        artifact.payload.plan.process_output_limit_bytes,
        artifact
            .payload
            .plan
            .resource_gates
            .max_process_group_resident_bytes,
        artifact.payload.profile == EvidenceProfile::Full,
    )?;
    require(
        !artifact.payload.claim_scope.claim_eligible
            && artifact.payload.claim_scope.claim_status
                == match artifact.payload.profile {
                    EvidenceProfile::Full => CLAIM_STATUS_IMPLEMENTATION_SCALE,
                    EvidenceProfile::Reduced => CLAIM_STATUS_EXPLORATORY,
                }
            && artifact
                .payload
                .claim_scope
                .exclusions
                .not_calibrated_reference_host
            && artifact.payload.claim_scope.exclusions.not_mcp_latency
            && artifact
                .payload
                .claim_scope
                .exclusions
                .not_release_superiority_evidence,
        "implementation evidence claim boundary drifted",
    )?;
    require(
        artifact.payload.claim_scope.full_declared_scale_executed
            == (artifact.payload.profile == EvidenceProfile::Full),
        "full-scale claim marker differs from the explicit profile",
    )
}

/// Write one formatted no-clobber JSON artifact.
fn write_json_create_new(
    output: &Path,
    value: &impl Serialize,
) -> Result<(), GraphScaleEvaluationError> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Compare every compiled input with the exact blob stored at the tested commit.
async fn source_commit_bindings(
    git: &RepositoryGitProbe,
    tested_commit: &str,
    bindings: &[CompiledSourceBinding],
) -> Result<Vec<SourceCommitBindingEvidence>, GraphScaleEvaluationError> {
    let mut evidence = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let object = format!("{tested_commit}:{}", binding.path);
        let commit_bytes = git
            .output_bytes(&["cat-file", "blob", object.as_str()])
            .await?;
        evidence.push(SourceCommitBindingEvidence {
            path: binding.path.to_owned(),
            compiled_sha256: sha256_hex(binding.bytes),
            commit_sha256: sha256_hex(&commit_bytes),
            exact_match: binding.bytes == commit_bytes,
        });
    }
    Ok(evidence)
}

/// Run one bounded repository Git query and return strict trimmed UTF-8 stdout.
async fn git_output_with(
    git: &RepositoryGitProbe,
    arguments: &[&str],
) -> Result<String, GraphScaleEvaluationError> {
    Ok(String::from_utf8(git.output_bytes(arguments).await?)?
        .trim()
        .to_owned())
}

/// Return the repository root compiled into this example.
fn source_root() -> Result<PathBuf, GraphScaleEvaluationError> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_root
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| GraphScaleEvaluationError::Policy("source root was unavailable".into()))?;
    Ok(fs::canonicalize(root)?)
}

/// Convert one path to exact retained Unicode command text.
fn path_text(path: &Path) -> Result<String, GraphScaleEvaluationError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        GraphScaleEvaluationError::Arguments(format!(
            "retained command path is not Unicode: {}",
            path.display()
        ))
    })
}

/// Convert retained process arguments without replacement characters.
fn unicode_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Vec<String>, GraphScaleEvaluationError> {
    arguments
        .into_iter()
        .map(|value| {
            value.into_string().map_err(|_value| {
                GraphScaleEvaluationError::Arguments(
                    "retained process arguments must be Unicode".into(),
                )
            })
        })
        .collect()
}

/// Convert a platform allocation count without wrapping.
fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Convert a signed platform reallocation delta without wrapping.
fn isize_to_i64(value: isize) -> i64 {
    i64::try_from(value).unwrap_or(if value.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    })
}

/// Convert elapsed monotonic time to bounded nanoseconds.
fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Compute a throughput floor without floating-point rounding ambiguity.
fn rate_per_second(rows: usize, duration_ns: u64) -> u64 {
    if duration_ns == 0 {
        return u64::MAX;
    }
    usize_to_u64(rows).saturating_mul(1_000_000_000) / duration_ns
}

/// Return an integer ceiling ratio with a nonzero denominator.
fn ceiling_ratio(numerator: u64, denominator: u64) -> u64 {
    let denominator = denominator.max(1);
    numerator.saturating_add(denominator - 1) / denominator
}

/// Return a nearest-rank percentile from a sorted copy of a nonempty sample.
fn nearest_rank_percentile(
    values: &[u64],
    percentile: usize,
) -> Result<u64, GraphScaleEvaluationError> {
    require(!values.is_empty(), "query latency sample is empty")?;
    require(
        (1..=100).contains(&percentile),
        "query percentile must be between one and one hundred",
    )?;
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = sorted.len().saturating_mul(percentile).saturating_add(99) / 100;
    sorted
        .get(rank.saturating_sub(1).min(sorted.len().saturating_sub(1)))
        .copied()
        .ok_or_else(|| GraphScaleEvaluationError::Policy("query percentile is missing".into()))
}

/// Hash one length-prefixed field into a canonical digest.
fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(usize_to_u64(bytes.len()).to_le_bytes());
    hasher.update(bytes);
}

/// Hash one presence-tagged optional byte field.
fn hash_optional_bytes(hasher: &mut Sha256, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            hash_field(hasher, b"some");
            hash_field(hasher, value);
        }
        None => hash_field(hasher, b"none"),
    }
}

/// Hash one presence-tagged optional text field.
fn hash_optional_text(hasher: &mut Sha256, value: Option<&str>) {
    hash_optional_bytes(hasher, value.map(str::as_bytes));
}

/// Hash one presence-tagged optional signed integer.
fn hash_optional_i64(hasher: &mut Sha256, value: Option<i64>) {
    match value {
        Some(value) => {
            hash_field(hasher, b"some");
            hash_field(hasher, &value.to_le_bytes());
        }
        None => hash_field(hasher, b"none"),
    }
}

/// Hash bytes as lowercase SHA-256.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Require one canonical lowercase hexadecimal SHA-256 value.
fn validate_sha256_hex(value: &str, label: &str) -> Result<(), GraphScaleEvaluationError> {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        format!("{label} is not canonical lowercase SHA-256"),
    )
}

/// Return whether one value is a full canonical lowercase Git object identifier.
fn is_canonical_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Require one canonical lowercase SHA-1 or SHA-256 Git object identifier.
fn validate_git_commit_hex(value: &str, label: &str) -> Result<(), GraphScaleEvaluationError> {
    require(
        is_canonical_git_object_id(value),
        format!("{label} is not a canonical lowercase SHA-1 or SHA-256 Git object identifier"),
    )
}

/// Hash one file without loading the executable fully into memory.
fn sha256_file(path: &Path) -> Result<String, GraphScaleEvaluationError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Return the `SQLite` slot spelling.
const fn slot_text(slot: StructuralSlot) -> &'static str {
    match slot {
        StructuralSlot::A => "a",
        StructuralSlot::B => "b",
    }
}

/// Return a policy error when one invariant is false.
fn require(condition: bool, message: impl Into<String>) -> Result<(), GraphScaleEvaluationError> {
    if condition {
        Ok(())
    } else {
        Err(GraphScaleEvaluationError::Policy(message.into()))
    }
}

/// Return an argument error when one command invariant is false.
fn require_argument(
    condition: bool,
    message: impl Into<String>,
) -> Result<(), GraphScaleEvaluationError> {
    if condition {
        Ok(())
    } else {
        Err(GraphScaleEvaluationError::Arguments(message.into()))
    }
}

/// Load one required option without indexing into the map.
fn required_option(
    options: &BTreeMap<String, String>,
    key: &str,
) -> Result<String, GraphScaleEvaluationError> {
    options
        .get(key)
        .cloned()
        .ok_or_else(|| GraphScaleEvaluationError::Arguments(format!("missing option {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_scale_process::{
        ProcessContainmentMechanism, ProcessGateDecision, ProcessMembershipSemantics,
        ProcessStreamEvidence, ResidentMemorySample, ResidentProcessSample,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "runs the committed release-mode million-entity graph-scale evidence workload"]
    async fn task_arri_ut_arri_4_23() -> Result<(), GraphScaleEvaluationError> {
        let root = source_root()?;
        let git = RepositoryGitProbe::resolve(&root)?;
        let tested_commit = git_output_with(&git, &["rev-parse", "HEAD"]).await?;
        require(
            git.worktree_state().await?.is_empty(),
            "ARRI 4.23 full evidence requires a clean source worktree",
        )?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                GraphScaleEvaluationError::Policy(format!(
                    "system clock could not name retained ARRI 4.23 evidence: {error}"
                ))
            })?
            .as_nanos();
        let evidence_directory = root.join("target/arri-4-23-evidence");
        fs::create_dir_all(&evidence_directory)?;
        let artifact_path =
            evidence_directory.join(format!("{tested_commit}-{nonce}-graph-scale-evidence.json"));
        let nested_target = root.join("target/arri-4-23-release-build");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let status = StandardCommand::new(cargo)
            .current_dir(&root)
            .arg("run")
            .arg("--release")
            .arg("--locked")
            .arg("--all-features")
            .arg("--target-dir")
            .arg(&nested_target)
            .arg("-p")
            .arg("projectatlas-cli")
            .arg("--example")
            .arg("graph-scale-evidence-runner")
            .arg("--")
            .arg(MANIFEST_OPTION)
            .arg(root.join("docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json"))
            .arg(OUTPUT_OPTION)
            .arg(&artifact_path)
            .arg(TESTED_COMMIT_OPTION)
            .arg(&tested_commit)
            .arg(PROFILE_OPTION)
            .arg("full")
            .status()?;
        require(
            status.success(),
            "release-mode ARRI 4.23 graph-scale runner failed",
        )?;
        let artifact: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&fs::read(&artifact_path)?)?;
        validate_artifact(&artifact)?;
        require(
            artifact.payload.profile == EvidenceProfile::Full
                && artifact.payload.claim_scope.full_declared_scale_executed
                && artifact.payload.workload.reconciliation.expected_entities == 1_000_000
                && artifact.payload.workload.reconciliation.expected_relations == 3_000_000,
            "retained ARRI 4.23 artifact did not execute the declared full scale",
        )?;
        let artifact_sha256 = sha256_file(&artifact_path)?;
        let receipt_path = artifact_path.with_extension("json.sha256");
        let file_name = artifact_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                GraphScaleEvaluationError::Policy(
                    "retained ARRI 4.23 artifact name was not Unicode".into(),
                )
            })?;
        let mut receipt = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&receipt_path)?;
        let expected_receipt = format!("{artifact_sha256}  {file_name}\n");
        receipt.write_all(expected_receipt.as_bytes())?;
        receipt.sync_all()?;
        drop(receipt);
        require(
            fs::read_to_string(&receipt_path)? == expected_receipt,
            "retained ARRI 4.23 SHA-256 receipt did not read back exactly",
        )?;
        writeln!(
            std::io::stdout().lock(),
            "ARRI 4.23 retained artifact: {}\nARRI 4.23 SHA-256: {artifact_sha256}",
            artifact_path.display()
        )?;
        Ok(())
    }

    #[test]
    fn reduced_graph_scale_exercises_measurement_protocols() -> Result<(), GraphScaleEvaluationError>
    {
        let envelope: EvaluationManifestEnvelope = serde_json::from_slice(MANIFEST_BYTES)?;
        validate_manifest_envelope(&envelope)?;
        let reduced = reduced_evidence_plan(&envelope.architecture_evaluations.graph_scale);
        let evidence = execute_workload(EvidenceProfile::Reduced, &reduced)?;
        require(
            evidence.reconciliation.expected_entities == 18,
            "reduced entity count drifted",
        )?;
        require(
            evidence.reconciliation.expected_relations == 32,
            "reduced relation count drifted",
        )?;
        require(
            evidence.publication.exact_transition,
            "reduced evaluation did not publish through the inactive slot",
        )?;
        require(
            evidence.integrity.relations_missing_evidence == 0
                && evidence.integrity.relations_with_duplicate_evidence == 0
                && evidence.integrity.orphan_evidence_occurrences == 0,
            "reduced graph/evidence integrity drifted",
        )?;
        require(
            evidence.rust_allocator.rust_allocator_requests > 0
                && evidence.rust_allocator.rust_allocator_requests
                    == evidence
                        .rust_allocator
                        .rust_allocation_requests
                        .saturating_add(evidence.rust_allocator.rust_reallocation_requests),
            "reduced evaluation observed no Rust allocation requests",
        )?;
        require(
            evidence.storage.persistent_live_bytes > 0
                && evidence
                    .storage
                    .live_final_after_checkpoint
                    .logical_index_page_bytes
                    > 0
                && evidence.storage.staging_after_cleanup.total_bytes == 0,
            "reduced evaluation observed no retained storage bytes",
        )?;
        require(
            evidence.queries.sqlite_stable_key.raw_duration_ns.len() == reduced.query_repetitions
                && evidence
                    .queries
                    .service_bounded_three_hop
                    .raw_duration_ns
                    .len()
                    == reduced.query_repetitions
                && evidence
                    .queries
                    .sqlite_stable_key
                    .query_plan
                    .requirement
                    .satisfied_by(&evidence.queries.sqlite_stable_key.query_plan.actual_details)
                && evidence
                    .queries
                    .service_bounded_three_hop
                    .query_plan
                    .requirement
                    .satisfied_by(
                        &evidence
                            .queries
                            .service_bounded_three_hop
                            .query_plan
                            .actual_details,
                    ),
            "reduced evaluation did not retain every raw query duration",
        )?;
        validate_workload(&evidence)?;

        let encoded = serde_json::to_vec(&evidence)?;
        let mut forged_count: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_count.reconciliation.observed.entities = forged_count
            .reconciliation
            .observed
            .entities
            .saturating_sub(1);
        require(
            validate_workload(&forged_count).is_err(),
            "forged passing count gate was accepted",
        )?;

        let mut forged_percentile: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_percentile.queries.sqlite_stable_key.p50_ns = forged_percentile
            .queries
            .sqlite_stable_key
            .p50_ns
            .saturating_add(1);
        require(
            validate_workload(&forged_percentile).is_err(),
            "forged query percentile was accepted",
        )?;

        let mut forged_storage: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_storage
            .storage
            .persistent_live_bytes_per_logical_fact = forged_storage
            .storage
            .persistent_live_bytes_per_logical_fact
            .saturating_add(1);
        require(
            validate_workload(&forged_storage).is_err(),
            "forged derived-storage ratio was accepted",
        )?;

        let mut forged_coexistence: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_coexistence
            .storage
            .max_observed_live_and_staging_bytes = forged_coexistence
            .storage
            .max_observed_live_and_staging_bytes
            .saturating_add(1);
        require(
            validate_workload(&forged_coexistence).is_err(),
            "forged maximum observed live/staging bytes were accepted",
        )?;

        let mut forged_order: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_order
            .completed_batch_order
            .producer_path_sequence_sha256 = sha256_hex(b"forged producer order");
        require(
            validate_workload(&forged_order).is_err(),
            "forged completed-batch producer order was accepted",
        )?;

        let mut forged_query_plan: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_query_plan
            .queries
            .sqlite_stable_key
            .query_plan
            .actual_details = vec!["SCAN graph_entities".to_owned()];
        require(
            validate_workload(&forged_query_plan).is_err(),
            "forged whole-table graph query plan was accepted",
        )?;

        let mut forged_publication: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_publication.publication.expected_after = forged_publication.publication.before;
        forged_publication.publication.after = forged_publication.publication.before;
        forged_publication.publication.exact_transition = true;
        require(
            validate_workload(&forged_publication).is_err(),
            "forged same-slot same-epoch publication transition was accepted",
        )?;

        let mut forged_integrity: GraphScaleWorkloadEvidence = serde_json::from_slice(&encoded)?;
        forged_integrity.integrity.graph_evidence_digest_sha256 =
            sha256_hex(b"forged aggregate graph evidence");
        require(
            validate_workload(&forged_integrity).is_err(),
            "forged aggregate graph evidence digest was accepted",
        )?;

        let tested_commit = "0".repeat(40);
        let payload = GraphScaleEvidencePayload {
            manifest_id: envelope.manifest_id,
            tested_commit: tested_commit.clone(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            status: STATUS_EXPLORATORY.to_owned(),
            profile: EvidenceProfile::Reduced,
            plan: reduced.clone(),
            workload: serde_json::from_slice(&encoded)?,
            process: reduced_test_process_evidence(&reduced)?,
            provenance: reduced_test_provenance(&reduced, &tested_commit)?,
            claim_scope: ClaimScope {
                claim_eligible: false,
                claim_status: CLAIM_STATUS_EXPLORATORY.to_owned(),
                full_declared_scale_executed: false,
                exclusions: ClaimExclusions {
                    not_calibrated_reference_host: true,
                    not_mcp_latency: true,
                    not_release_superiority_evidence: true,
                },
            },
        };
        let artifact = GraphScaleEvidenceArtifact {
            schema_version: 1,
            artifact_kind: ARTIFACT_KIND.to_owned(),
            payload_sha256: sha256_hex(&serde_json::to_vec(&payload)?),
            payload,
        };
        validate_artifact(&artifact)?;
        require(
            artifact.payload.status == STATUS_EXPLORATORY
                && !artifact.payload.provenance.source_state_commit_bound
                && !artifact.payload.provenance.source_commit_verified
                && artifact
                    .payload
                    .provenance
                    .source_commit_bindings
                    .is_empty()
                && !artifact.payload.process.complete_tree_claim_eligible
                && !artifact.payload.claim_scope.claim_eligible
                && !artifact.payload.claim_scope.full_declared_scale_executed
                && artifact.payload.workload.reconciliation.expected_entities < 1_000_000
                && artifact.payload.workload.reconciliation.expected_relations < 3_000_000,
            "reduced final artifact retained a certifying field or full-scale marker",
        )?;

        let artifact_bytes = serde_json::to_vec(&artifact)?;
        let mut forged_status: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&artifact_bytes)?;
        forged_status.payload.status = STATUS_PASSED.to_owned();
        forged_status.payload_sha256 = sha256_hex(&serde_json::to_vec(&forged_status.payload)?);
        require(
            validate_artifact(&forged_status).is_err(),
            "reduced final artifact forged a passed status",
        )?;

        let mut forged_binding: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&artifact_bytes)?;
        forged_binding.payload.provenance.source_state_commit_bound = true;
        forged_binding.payload.provenance.source_commit_verified = true;
        forged_binding.payload_sha256 = sha256_hex(&serde_json::to_vec(&forged_binding.payload)?);
        require(
            validate_artifact(&forged_binding).is_err(),
            "reduced final artifact forged clean-commit source binding",
        )?;

        let mut forged_full_scale: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&artifact_bytes)?;
        forged_full_scale
            .payload
            .claim_scope
            .full_declared_scale_executed = true;
        forged_full_scale.payload_sha256 =
            sha256_hex(&serde_json::to_vec(&forged_full_scale.payload)?);
        require(
            validate_artifact(&forged_full_scale).is_err(),
            "reduced final artifact forged the full-scale marker",
        )?;

        let mut forged_executable_digest: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&artifact_bytes)?;
        forged_executable_digest
            .payload
            .provenance
            .executable_sha256 = sha256_hex(b"forged evidence executable");
        forged_executable_digest.payload_sha256 =
            sha256_hex(&serde_json::to_vec(&forged_executable_digest.payload)?);
        require(
            validate_artifact(&forged_executable_digest).is_err(),
            "forged evidence-executable digest was accepted",
        )?;

        let mut forged_executable_path: GraphScaleEvidenceArtifact =
            serde_json::from_slice(&artifact_bytes)?;
        let executable_file_name =
            Path::new(&forged_executable_path.payload.provenance.workload_command[0])
                .file_name()
                .ok_or_else(|| {
                    GraphScaleEvaluationError::Policy(
                        "test evidence executable path has no file name".into(),
                    )
                })?;
        let alternate_executable = source_root()?
            .join("forged-evidence-executable")
            .join(executable_file_name);
        forged_executable_path.payload.provenance.workload_command[0] =
            path_text(&alternate_executable)?;
        forged_executable_path
            .payload
            .provenance
            .workload_command_sha256 = sha256_hex(&serde_json::to_vec(
            &forged_executable_path.payload.provenance.workload_command,
        )?);
        forged_executable_path.payload_sha256 =
            sha256_hex(&serde_json::to_vec(&forged_executable_path.payload)?);
        require(
            validate_artifact(&forged_executable_path).is_err(),
            "mismatched same-file-name evidence executable path was accepted",
        )
    }

    fn reduced_test_process_evidence(
        plan: &GraphScalePlan,
    ) -> Result<GraphScaleProcessEvidence, GraphScaleEvaluationError> {
        let output_limit_bytes =
            u64::try_from(plan.process_output_limit_bytes).map_err(|_error| {
                GraphScaleEvaluationError::Policy("test process output limit overflowed".into())
            })?;
        let timeout_ms = plan
            .workload_timeout_seconds
            .checked_mul(1_000)
            .ok_or_else(|| {
                GraphScaleEvaluationError::Policy("test process timeout overflowed".into())
            })?;
        Ok(GraphScaleProcessEvidence {
            metric_name: "sampled-aggregate-process-group-resident-bytes".to_owned(),
            membership_semantics: ProcessMembershipSemantics::Unknown,
            sample_interval_ms: plan.process_sample_interval_ms,
            timeout_ms,
            output_limit_bytes,
            parent_death_requested: true,
            root_pid: 1,
            containment_mechanism: ProcessContainmentMechanism::Unknown,
            membership_complete: false,
            raw_samples: vec![ResidentMemorySample {
                timestamp_ns: 1,
                aggregate_resident_bytes: 1,
                processes: vec![ResidentProcessSample {
                    pid: 1,
                    start_time_seconds_since_boot: 1,
                    resident_bytes: 1,
                }],
            }],
            successful_sample_count: 1,
            membership_discovery_failures: 0,
            active_member_discovery_failures: 0,
            peak_aggregate_resident_bytes: 1,
            private_committed_bytes: None,
            private_committed_status: "not-available-from-portable-sysinfo-process-sampling"
                .to_owned(),
            shared_page_accounting_policy:
                "sum-per-process-resident-bytes-shared-pages-may-be-double-counted".to_owned(),
            complete_tree_claim_eligible: false,
            resident_ceiling: ProcessGateDecision::Passed,
            duration_ns: 1,
            exit_code: Some(0),
            timed_out: false,
            output_truncated: false,
            stdout: ProcessStreamEvidence {
                retained_bytes: 0,
                retained_sha256: sha256_hex(&[]),
            },
            stderr: ProcessStreamEvidence {
                retained_bytes: 0,
                retained_sha256: sha256_hex(&[]),
            },
            terminal_members_before_teardown: Vec::new(),
            post_teardown_members: Vec::new(),
            sampler_drain_completed: true,
            teardown_completed: true,
            successful_bounded_completion: true,
        })
    }

    fn reduced_test_provenance(
        plan: &GraphScalePlan,
        tested_commit: &str,
    ) -> Result<ProvenanceEvidence, GraphScaleEvaluationError> {
        let git = RepositoryGitProbe::resolve(&source_root()?)?;
        let evidence_executable = std::env::current_exe()?;
        let evidence_executable_path = path_text(&evidence_executable)?;
        let supervisor_command = vec![
            evidence_executable_path.clone(),
            MANIFEST_OPTION.to_owned(),
            "manifest.json".to_owned(),
            OUTPUT_OPTION.to_owned(),
            "final-evidence.json".to_owned(),
            TESTED_COMMIT_OPTION.to_owned(),
            tested_commit.to_owned(),
            PROFILE_OPTION.to_owned(),
            EvidenceProfile::Reduced.as_str().to_owned(),
        ];
        let workload_command = vec![
            evidence_executable_path,
            INTERNAL_WORKLOAD_FLAG.to_owned(),
            MANIFEST_OPTION.to_owned(),
            "manifest.json".to_owned(),
            OUTPUT_OPTION.to_owned(),
            "workload-evidence.json".to_owned(),
            TESTED_COMMIT_OPTION.to_owned(),
            tested_commit.to_owned(),
            PROFILE_OPTION.to_owned(),
            EvidenceProfile::Reduced.as_str().to_owned(),
        ];
        let empty_worktree_digest = sha256_hex(&[]);
        Ok(ProvenanceEvidence {
            executable_sha256: sha256_file(&evidence_executable)?,
            manifest_sha256: sha256_hex(MANIFEST_BYTES),
            graph_scale_evaluator_source_sha256: sha256_hex(EVALUATOR_SOURCE_BYTES),
            graph_scale_plan_source_sha256: sha256_hex(PLAN_SOURCE_BYTES),
            graph_scale_process_source_sha256: sha256_hex(PROCESS_SOURCE_BYTES),
            bounded_process_policy_source_sha256: sha256_hex(PROCESS_POLICY_SOURCE_BYTES),
            git_process_policy_source_sha256: sha256_hex(GIT_POLICY_SOURCE_BYTES),
            workspace_manifest_sha256: sha256_hex(WORKSPACE_MANIFEST_BYTES),
            cli_manifest_sha256: sha256_hex(CLI_MANIFEST_BYTES),
            core_manifest_sha256: sha256_hex(CORE_MANIFEST_BYTES),
            database_manifest_sha256: sha256_hex(DATABASE_MANIFEST_BYTES),
            service_manifest_sha256: sha256_hex(SERVICE_MANIFEST_BYTES),
            core_lib_source_sha256: sha256_hex(CORE_LIB_SOURCE_BYTES),
            core_budget_source_sha256: sha256_hex(CORE_BUDGET_SOURCE_BYTES),
            core_graph_source_sha256: sha256_hex(CORE_GRAPH_SOURCE_BYTES),
            core_symbols_source_sha256: sha256_hex(CORE_SYMBOLS_SOURCE_BYTES),
            service_lib_source_sha256: sha256_hex(SERVICE_LIB_SOURCE_BYTES),
            service_graph_query_source_sha256: sha256_hex(SERVICE_QUERY_SOURCE_BYTES),
            database_source_sha256: sha256_hex(DATABASE_SOURCE_BYTES),
            database_schema_source_sha256: sha256_hex(DATABASE_SCHEMA_SOURCE_BYTES),
            structural_publication_source_sha256: sha256_hex(PUBLICATION_SOURCE_BYTES),
            example_source_sha256: sha256_hex(EXAMPLE_SOURCE_BYTES),
            cargo_lock_sha256: sha256_hex(CARGO_LOCK_BYTES),
            command_sha256: sha256_hex(&serde_json::to_vec(&supervisor_command)?),
            workload_command_sha256: sha256_hex(&serde_json::to_vec(&workload_command)?),
            supervisor_command,
            workload_command,
            external_timeout_seconds: plan.workload_timeout_seconds,
            git_executable_path: path_text(git.executable())?,
            git_executable_sha256: git.executable_sha256().to_owned(),
            observed_git_commit_before: tested_commit.to_owned(),
            observed_git_commit_after: tested_commit.to_owned(),
            head_matches_requested_commit: true,
            source_worktree_dirty_before: false,
            source_worktree_dirty_after: false,
            source_state_commit_bound: false,
            worktree_state_before_sha256: empty_worktree_digest.clone(),
            worktree_state_after_sha256: empty_worktree_digest,
            source_commit_bindings: Vec::new(),
            source_commit_verified: false,
        })
    }

    #[test]
    fn manifest_project_identity_rejects_stale_cargo_lock_digest()
    -> Result<(), GraphScaleEvaluationError> {
        let exact: EvaluationManifestEnvelope = serde_json::from_slice(MANIFEST_BYTES)?;
        validate_manifest_envelope(&exact)?;

        let mut forged: serde_json::Value = serde_json::from_slice(MANIFEST_BYTES)?;
        let cargo_lock_digest = forged
            .pointer_mut("/projectatlas/cargo_lock_sha256")
            .ok_or_else(|| {
                GraphScaleEvaluationError::Policy(
                    "manifest Cargo lock digest field was missing".into(),
                )
            })?;
        *cargo_lock_digest = serde_json::Value::String("0".repeat(64));
        let stale: EvaluationManifestEnvelope = serde_json::from_value(forged)?;
        require(
            validate_manifest_envelope(&stale).is_err(),
            "stale manifest Cargo lock digest was accepted",
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn commit_source_binding_compares_exact_blob_bytes()
    -> Result<(), GraphScaleEvaluationError> {
        let directory = tempfile::tempdir()?;
        git_status(directory.path(), &["init", "--quiet"])?;
        git_status(
            directory.path(),
            &["config", "user.email", "projectatlas@example.invalid"],
        )?;
        git_status(
            directory.path(),
            &["config", "user.name", "ProjectAtlas Test"],
        )?;
        fs::write(directory.path().join("input.txt"), b"committed\n")?;
        git_status(directory.path(), &["add", "input.txt"])?;
        git_status(directory.path(), &["commit", "--quiet", "-m", "test input"])?;
        let git = RepositoryGitProbe::resolve(directory.path())?;
        let tested_commit = git_output_with(&git, &["rev-parse", "HEAD"]).await?;
        let exact = source_commit_bindings(
            &git,
            &tested_commit,
            &[CompiledSourceBinding {
                path: "input.txt",
                bytes: b"committed\n",
            }],
        )
        .await?;
        let stale = source_commit_bindings(
            &git,
            &tested_commit,
            &[CompiledSourceBinding {
                path: "input.txt",
                bytes: b"stale\n",
            }],
        )
        .await?;
        require(
            exact.len() == 1
                && exact[0].exact_match
                && exact[0].compiled_sha256 == exact[0].commit_sha256
                && stale.len() == 1
                && !stale[0].exact_match
                && stale[0].compiled_sha256 != stale[0].commit_sha256,
            "tested-commit source binding did not distinguish exact and stale bytes",
        )
    }

    #[test]
    fn core_compiled_source_bindings_cover_crate_boundary_and_budget_policy()
    -> Result<(), GraphScaleEvaluationError> {
        let expected = [
            ("crates/projectatlas-core/src/lib.rs", CORE_LIB_SOURCE_BYTES),
            (
                "crates/projectatlas-core/src/budget.rs",
                CORE_BUDGET_SOURCE_BYTES,
            ),
        ];

        for (expected_path, expected_bytes) in expected {
            let matching = COMPILED_SOURCE_BINDINGS
                .iter()
                .filter(|binding| binding.path == expected_path)
                .collect::<Vec<_>>();
            require(
                matching.len() == 1 && matching[0].bytes == expected_bytes,
                format!("compiled source binding drifted for {expected_path}"),
            )?;
        }
        Ok(())
    }

    fn git_status(root: &Path, arguments: &[&str]) -> Result<(), GraphScaleEvaluationError> {
        let output = StandardCommand::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .output()?;
        require(
            output.status.success(),
            format!(
                "test git {:?} failed: {}",
                arguments,
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }
}
