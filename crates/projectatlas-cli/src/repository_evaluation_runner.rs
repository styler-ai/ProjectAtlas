//! Test-only runner for isolated, manifest-bound repository evaluation.

use crate::bounded_process_supervisor::{SupervisionError, run_supervised};
use crate::git_process_policy::{
    SanitizedGitWorkspace, SanitizedWorktreeEvidence, WorktreePathState, closed_git_arguments,
    closed_git_environment, git_null_device, index_flags_query, inspect_worktree_path,
    path_from_git_bytes, plan_sanitized_worktree_comparison, raw_head_tree_query, raw_index_query,
    repository_bound_git_arguments, resolve_git_directory, sanitized_hash_query,
    sanitized_index_import_query, sanitized_literal_hash_query, sanitized_untracked_query,
};
use crate::sqlite_architecture_evaluation::{
    ArchitectureEvaluationError, ArchitectureEvaluationPlan, ArchitectureMetrics,
    ArchitectureSampleContext, GLOBAL_SEED_REFERENCE, LEXICAL_FIXTURE_BYTES, ORDERING_ALGORITHM_ID,
    ORDERING_ALGORITHM_VERSION, run_fts_differential, run_sqlite_strategy,
};
use processkit::{Command, Stdin};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;

/// Manifest bytes compiled into the dedicated example.
const MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
/// Evaluator source compiled into this executable for provenance.
const RUNNER_SOURCE_BYTES: &[u8] = include_bytes!("repository_evaluation_runner.rs");
/// Process-supervision adapter source compiled into this executable.
const SUPERVISOR_SOURCE_BYTES: &[u8] = include_bytes!("bounded_process_supervisor.rs");
/// Closed Git subprocess policy source compiled into this executable.
const GIT_POLICY_SOURCE_BYTES: &[u8] = include_bytes!("git_process_policy.rs");
/// `SQLite` architecture evaluator source compiled into this executable.
const ARCHITECTURE_EVALUATOR_SOURCE_BYTES: &[u8] =
    include_bytes!("sqlite_architecture_evaluation.rs");
/// Dedicated example entrypoint source compiled into this executable.
const EXAMPLE_SOURCE_BYTES: &[u8] = include_bytes!("../examples/repository-evaluation-runner.rs");
/// Workspace lockfile used to build this evaluator.
const RUNNER_LOCK_BYTES: &[u8] = include_bytes!("../../../Cargo.lock");
/// Frozen manifest format identifier.
const MANIFEST_FORMAT: &str = "projectatlas.evaluation-manifest";
/// Evidence artifact identifier.
const ARTIFACT_KIND: &str = "projectatlas.repository-evaluation";
/// Private command used to isolate one architecture sample in this executable.
const ARCHITECTURE_SAMPLE_COMMAND: &str = "architecture-sample";
/// Typed architecture-child report schema.
const ARCHITECTURE_SAMPLE_SCHEMA_VERSION: u32 = 1;
/// Maximum retained bytes for each child stream.
const OUTPUT_LIMIT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum bytes read from one tracked materialized entry.
const MATERIALIZED_FILE_BYTE_LIMIT: u64 = 64 * 1024 * 1024;
/// Maximum aggregate bytes read from one tracked checkout.
const MATERIALIZED_CHECKOUT_BYTE_LIMIT: u64 = 256 * 1024 * 1024;
/// Maximum bytes for one persisted JSON record.
const RECORD_LIMIT_BYTES: u64 = 32 * 1024 * 1024;
/// Deadline for local Git identity and clone commands.
const GIT_TIMEOUT_SECONDS: u64 = 120;
/// Number of tool calls in the fixed atlas-first MCP flow.
const MCP_TOOL_CALLS: u64 = 7;
/// Status attached to reduced-repetition evidence.
const PILOT_STATUS: &str = "exploratory-ineligible";
/// Status attached before registered evidence is joined with release provenance.
const REGISTERED_STATUS: &str = "complete-ineligible-pending-host-metrics";
/// Literal used by the lexical-search baseline.
const SEARCH_PATTERN: &str = "fn";
/// Reversible byte change used by one-file refresh.
const MUTATION_MARKER: &[u8] = b"\n// projectatlas repository evaluation refresh\n";
/// Required pinned corpus identities.
const REQUIRED_CORPORA: [&str; 3] = ["serde-json", "projectatlas-self", "rust-analyzer"];
/// Every registered operation, including later experimental operations.
const OPERATION_COUNT: usize = 12;
/// Number of JSON-RPC responses expected from the fixed MCP flow.
const MCP_RESPONSE_COUNT: usize = 8;
/// Cache-state identifier for a scan without an existing database.
const CACHE_DATABASE_ABSENT_NEW_PROCESS: &str = "database-absent-new-process";
/// Cache-state identifier for CLI work against a restored current index.
const CACHE_CURRENT_INDEX_NEW_PROCESS: &str = "current-index-new-process";
/// Cache-state identifier for an MCP flow against a restored current index.
const CACHE_CURRENT_INDEX_NEW_MCP_PROCESS: &str = "current-index-new-mcp-process";
/// Cache label for a supervised architecture child with new `SQLite` connections.
const CACHE_CURRENT_INDEX_SUPERVISED_ARCHITECTURE_CHILD: &str =
    "current-index-supervised-architecture-child";
/// Process-state value shared by every current CLI measurement.
const PROCESS_STATE_NEW: &str = "new-process";
/// Product-process state for supervised architecture experiments.
const PROCESS_STATE_SUPERVISED_CHILD: &str = "supervised-child-process";
/// MCP process-state value for non-MCP measurements.
const MCP_PROCESS_NOT_APPLICABLE: &str = "not-applicable";
/// MCP process-state value for the fixed MCP flow.
const MCP_PROCESS_STATE_NEW: &str = "new-mcp-process";
/// `SQLite` connection state for every current measurement.
const SQLITE_CONNECTION_STATE_NEW: &str = "new-connection";
/// `SQLite` state for each in-process architecture matrix sample.
const SQLITE_CONNECTION_STATE_PER_SAMPLE: &str = "new-connection-per-sample";
/// Host filesystem cache is deliberately uncontrolled by this runner.
const OS_FILE_CACHE_UNCONTROLLED: &str = "uncontrolled";
/// Registered experimental unit for every repository measurement cell.
const EXPERIMENT_SAMPLE_UNIT: &str =
    "one operation on one corpus/profile/environment/runtime tuple";
/// Exact versioned paired-order derivation recorded in the manifest.
const EXPERIMENT_BLOCK_ORDER: &str = "AB or BA from the final-byte low bit of SHA-256(projectatlas.evaluation-order.v2 followed by u64-le length-prefixed decoded-seed, UTF-8 cell-id, u64-le repetition, and UTF-8 pair-id fields); 0 selects AB and 1 selects BA";
/// Required timeout disposition for registered observations.
const TIMEOUT_TREATMENT: &str =
    "retain as failure and worst-direction infinite ratio; never exclude";
/// Required failure disposition for correctness and eligibility failures.
const FAILURE_TREATMENT: &str = "any correctness, integrity, containment, compatibility, or required-platform failure blocks the dimension and aggregate exit";
/// Required outlier disposition for registered observations.
const OUTLIER_POLICY: &str = "retain all preregistered observations; report raw values, median, p50, p95, MAD, and sensitivity without automatic deletion";
/// Required independence rule for repeated requests or model runs.
const INDEPENDENCE_POLICY: &str = "repeated model runs or requests from one fixture or task are one clustered experimental unit, never independent units";
/// Required disposition for invalid denominators and degenerate samples.
const DEGENERATE_SAMPLE_POLICY: &str = "zero denominators, one-class fixtures, bootstrap-degenerate samples, and cells below minimum positive or negative counts are ineligible and never reported as 100 percent";
/// Required rerun policy for failed benchmark cells.
const RERUN_POLICY: &str = "only declared infrastructure failure permits a full cell rerun; retain all attempts and never select the best attempt";
/// Registered repository strata in stable order.
const REGISTERED_STRATA: [&str; 3] = ["small", "medium", "large"];

/// Failures at the isolated evaluation boundary.
#[derive(Debug, Error)]
pub(super) enum EvaluationError {
    /// Command-line arguments did not match the closed surface.
    #[error("invalid arguments: {0}")]
    Arguments(String),
    /// The frozen evaluation contract or an observed invariant drifted.
    #[error("evaluation policy error: {0}")]
    Policy(String),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Process-tree supervision failed.
    #[error(transparent)]
    Supervision(#[from] SupervisionError),
    /// Dev-only `SQLite` architecture evaluation failed.
    #[error(transparent)]
    Architecture(#[from] ArchitectureEvaluationError),
    /// Child output was not UTF-8.
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
}

/// Every operation accepted by the frozen manifest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum OperationId {
    /// Full scan without a pre-existing index.
    ColdFullScan,
    /// Full scan with a current index and warmed host cache.
    WarmFullScan,
    /// Full scan with no repository changes.
    NoChangeScan,
    /// Incremental refresh after one reversible file edit.
    OneFileRefresh,
    /// Indexed literal search.
    LexicalSearch,
    /// Indexed symbol-relation lookup.
    GraphLookup,
    /// Normal atlas-first MCP navigation flow.
    McpCallFlow,
    /// Later full-text-search strategy experiment.
    FtsDifferential,
    /// Later `SQLite` strategy experiment.
    SqliteStrategy,
    /// Later native parser-host experiment.
    ParserHostNative,
    /// Later WebAssembly parser-host experiment.
    ParserHostWasm,
    /// Later optional semantic-retrieval experiment.
    SemanticCandidate,
}

impl OperationId {
    /// Operations required by ARRI-2.6.
    const BASELINE: [Self; 7] = [
        Self::ColdFullScan,
        Self::WarmFullScan,
        Self::NoChangeScan,
        Self::OneFileRefresh,
        Self::LexicalSearch,
        Self::GraphLookup,
        Self::McpCallFlow,
    ];

    /// Baseline plus registered architecture experiments executed by this runner.
    const REGISTERED: [Self; 9] = [
        Self::ColdFullScan,
        Self::WarmFullScan,
        Self::NoChangeScan,
        Self::OneFileRefresh,
        Self::LexicalSearch,
        Self::GraphLookup,
        Self::McpCallFlow,
        Self::FtsDifferential,
        Self::SqliteStrategy,
    ];

    /// Stable manifest identifier.
    const fn id(self) -> &'static str {
        match self {
            Self::ColdFullScan => "cold-full-scan",
            Self::WarmFullScan => "warm-full-scan",
            Self::NoChangeScan => "no-change-scan",
            Self::OneFileRefresh => "one-file-refresh",
            Self::LexicalSearch => "lexical-search",
            Self::GraphLookup => "graph-lookup",
            Self::McpCallFlow => "mcp-call-flow",
            Self::FtsDifferential => "fts-differential",
            Self::SqliteStrategy => "sqlite-strategy",
            Self::ParserHostNative => "parser-host-native",
            Self::ParserHostWasm => "parser-host-wasm",
            Self::SemanticCandidate => "semantic-candidate",
        }
    }
}

/// Frozen corpus fields used by this runner.
#[derive(Clone, Debug, Deserialize)]
struct CorpusSpec {
    /// Stable corpus identifier.
    id: String,
    /// Registered size stratum.
    stratum: String,
    /// Pinned commit identifier.
    commit: String,
    /// Pinned Git tree identifier.
    tree: String,
    /// Whether the materialized checkout must be clean.
    clean_required: bool,
    /// Whether submodules are accepted.
    submodules_allowed: bool,
    /// Whether Git LFS pointers are accepted.
    lfs_allowed: bool,
    /// Registered tracked-file count.
    tracked_files: u64,
    /// Registered sum of tracked blob bytes.
    tracked_logical_bytes: u64,
    /// Registered tracked modes and counts.
    git_modes: BTreeMap<String, u64>,
    /// Materialization verification state.
    materialization_state: String,
}

/// Pinned `ProjectAtlas` baseline and released executable inventory.
#[derive(Debug, Deserialize)]
struct ProjectAtlasSpec {
    /// Commit that produced the baseline runtime.
    baseline_runtime_commit: String,
    /// Tree that produced the baseline runtime.
    baseline_runtime_tree: String,
    /// Lockfile digest for the baseline runtime source.
    baseline_runtime_cargo_lock_sha256: String,
    /// Lockfile digest for the workspace that builds this evaluator.
    cargo_lock_sha256: String,
    /// Released executable artifacts accepted for measurement.
    baseline_release_artifacts: Vec<ReleaseArtifactSpec>,
}

/// One exact released executable accepted on a target host.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct ReleaseArtifactSpec {
    /// Rust target triple.
    target: String,
    /// SHA-256 of the standalone release executable.
    executable_sha256: String,
    /// Exact standalone executable bytes.
    executable_bytes: u64,
    /// Exact `--version` output from the released executable.
    version: String,
    /// Build profile used by the released artifact.
    build_profile: String,
    /// Stable provenance description.
    provenance: String,
}

/// Complete registered statistical plan for repository evaluation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExperimentDesign {
    /// Experimental unit used by every repository measurement cell.
    sample_unit: String,
    /// Fixed repository strata in reporting order.
    strata: Vec<String>,
    /// Warmups performed for each corpus/operation cell.
    warmups: usize,
    /// Measured paired repetitions for non-cold performance cells.
    paired_repetitions: usize,
    /// Minimum eligible paired observations after retained failures.
    minimum_valid_pairs: usize,
    /// Registered index worker-count cells.
    index_worker_counts: Vec<usize>,
    /// Registered query-concurrency cells.
    query_concurrency: Vec<usize>,
    /// Registered mixed read/publication workload.
    mixed_workload: MixedWorkload,
    /// Exact deterministic ordering encoding.
    block_order: String,
    /// Registered deterministic random-source identity.
    rng: ExperimentRng,
    /// Timeout disposition.
    timeout_treatment: String,
    /// Correctness and eligibility failure disposition.
    failure_treatment: String,
    /// Outlier disposition.
    outlier_policy: String,
    /// Registered confidence-interval methods.
    confidence_intervals: ConfidenceIntervalPlans,
    /// Independence rule for repeated observations.
    independence_policy: String,
    /// Degenerate-sample disposition.
    degenerate_sample_policy: String,
    /// Direction of each paired comparison family.
    paired_comparison: PairedComparisonPlan,
    /// Multiple-family correction plan.
    multiplicity: MultiplicityPlan,
    /// Allowed rerun behavior.
    reruns: String,
}

/// Registered mixed publication/read workload.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MixedWorkload {
    /// Concurrent publication tasks.
    publication_tasks: usize,
    /// Concurrent readers.
    concurrent_readers: usize,
}

/// Registered confidence-interval families.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceIntervalPlans {
    /// Paired time, RSS, and geometric-mean interval plan.
    paired_time_rss_and_geometric_means: PairedBootstrapPlan,
    /// Latency percentile interval plan.
    latency_percentiles: LatencyBootstrapPlan,
    /// Accuracy and agent-quality interval plan.
    accuracy_and_agent_metrics: AccuracyBootstrapPlan,
    /// Deterministic bootstrap seed derivation.
    seed_derivation: String,
}

/// Registered clustered paired bootstrap plan.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedBootstrapPlan {
    /// Statistical method.
    method: String,
    /// Clustered experimental unit.
    cluster_unit: String,
    /// Bootstrap resample count.
    resamples: usize,
    /// Confidence level.
    confidence: f64,
    /// Bound used for decisions.
    decision_bound: String,
}

/// Registered hierarchical latency bootstrap plan.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LatencyBootstrapPlan {
    /// Statistical method.
    method: String,
    /// Ordered clustering levels.
    cluster_levels: Vec<String>,
    /// Warmup request minimum per cell.
    warmup_requests_per_cell: usize,
    /// Measured request minimum per cell.
    measured_requests_per_cell: usize,
    /// Bootstrap resample count.
    resamples: usize,
    /// Confidence level.
    confidence: f64,
    /// Bound used for decisions.
    decision_bound: String,
}

/// Registered accuracy and agent-quality bootstrap plan.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccuracyBootstrapPlan {
    /// Statistical method.
    method: String,
    /// Clustered experimental unit.
    cluster_unit: String,
    /// Fixed repository strata.
    repository_strata: Vec<String>,
    /// Frozen integer stratum weights.
    stratum_weights: BTreeMap<String, u64>,
    /// Weight normalization rule.
    weight_normalization: String,
    /// Bootstrap resample count.
    resamples: usize,
    /// Confidence level.
    confidence: f64,
    /// Bound used for decisions.
    decision_bound: String,
}

/// Registered directions for paired comparisons.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedComparisonPlan {
    /// Ratio direction for lower-is-better metrics.
    lower_is_better_ratio: String,
    /// Difference direction for higher-is-better metrics.
    higher_is_better_difference: String,
}

/// Registered multiple-family correction plan.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiplicityPlan {
    /// Independent-family gate rule.
    gate_policy: String,
    /// Correction method.
    claim_family_correction: String,
    /// Families included in the correction.
    applies_to: Vec<String>,
    /// Family-wise alpha.
    family_alpha: f64,
    /// Whether exploratory results can pass claims.
    uncorrected_exploratory_results_cannot_pass_claims: bool,
}

/// Registered deterministic random source used by architecture samples.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExperimentRng {
    /// Registered algorithm identity.
    algorithm: String,
    /// Registered algorithm version.
    version: String,
    /// Pinned global seed.
    seed_hex: String,
}

/// Complete registered benchmark decision functions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionFunctions {
    /// Correctness confidence and denominator policy.
    correctness: CorrectnessDecision,
    /// Non-inferiority limits for retained capabilities and resources.
    non_inferiority: NonInferiorityDecision,
    /// Dimension-specific superiority thresholds.
    superiority: SuperiorityDecision,
    /// Absolute-budget decision rule.
    absolute_budget: TextDecision,
    /// Fail-closed phase-exit rule.
    phase_exit: TextDecision,
}

/// Correctness confidence and denominator contract.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrectnessDecision {
    /// Confidence interval method.
    method: String,
    /// Confidence level.
    confidence: f64,
    /// Minimum positive examples per advertised family.
    minimum_positive_examples_per_family: usize,
    /// Minimum negative examples per advertised family.
    minimum_negative_examples_per_family: usize,
    /// Structural precision floor.
    precision_floor: f64,
    /// Structural recall floor.
    recall_floor: f64,
    /// Optional semantic precision floor.
    semantic_precision_floor: f64,
    /// Optional semantic recall floor.
    semantic_recall_floor: f64,
    /// Fail-closed correctness decision.
    decision: String,
}

/// Independent non-inferiority thresholds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NonInferiorityDecision {
    /// Upper time ratio.
    performance_ratio_upper: f64,
    /// Upper RSS ratio.
    rss_ratio_upper: f64,
    /// Upper persistent-byte ratio.
    bytes_ratio_upper: f64,
    /// Lower paired agent-quality difference.
    agent_quality_difference_lower: f64,
    /// Whether compatibility is mandatory.
    compatibility_required: bool,
}

/// Dimension-specific superiority thresholds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuperiorityDecision {
    /// Maximum corrected cold-index ratio for every required corpus.
    cold_index_per_corpus_ratio_upper: f64,
    /// Maximum corrected cold-index geometric-mean ratio.
    cold_index_geometric_mean_ratio_upper: f64,
    /// Maximum corrected peak-RSS geometric-mean ratio.
    peak_rss_geometric_mean_ratio_upper: f64,
    /// Exclusive upper structural-retrieval p95 ratio.
    structural_retrieval_p95_ratio_upper_exclusive: f64,
    /// Minimum agent-quality point-estimate improvement.
    agent_quality_point_estimate_difference_lower: f64,
    /// Exclusive corrected lower confidence bound for agent quality.
    agent_quality_corrected_bound_lower_exclusive: f64,
    /// Minimum improved task pairs.
    minimum_improved_pairs: usize,
}

/// One exact textual decision rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextDecision {
    /// Decision rule.
    decision: String,
}

impl ExperimentDesign {
    /// Validate every preregistered statistical-plan field before a run starts.
    fn validate(&self) -> Result<(), EvaluationError> {
        require(
            self.sample_unit == EXPERIMENT_SAMPLE_UNIT
                && self.strata.iter().map(String::as_str).eq(REGISTERED_STRATA)
                && self.warmups == 3
                && self.paired_repetitions == 15
                && self.minimum_valid_pairs == 10
                && self.index_worker_counts == [1, 8]
                && self.query_concurrency == [1, 4, 16]
                && self.mixed_workload.publication_tasks == 1
                && self.mixed_workload.concurrent_readers == 8,
            "experimental units, strata, sample counts, or workload cells drifted",
        )?;
        require(
            self.block_order == EXPERIMENT_BLOCK_ORDER
                && self.rng.algorithm == ORDERING_ALGORITHM_ID
                && self.rng.version == ORDERING_ALGORITHM_VERSION
                && is_hex_identifier(&self.rng.seed_hex, 64),
            "deterministic ordering identity or encoding drifted",
        )?;
        require(
            self.timeout_treatment == TIMEOUT_TREATMENT
                && self.failure_treatment == FAILURE_TREATMENT
                && self.outlier_policy == OUTLIER_POLICY
                && self.independence_policy == INDEPENDENCE_POLICY
                && self.degenerate_sample_policy == DEGENERATE_SAMPLE_POLICY
                && self.reruns == RERUN_POLICY,
            "failure, outlier, independence, or rerun policy drifted",
        )?;

        let intervals = &self.confidence_intervals;
        let paired = &intervals.paired_time_rss_and_geometric_means;
        let latency = &intervals.latency_percentiles;
        let accuracy = &intervals.accuracy_and_agent_metrics;
        require(
            paired.method == "deterministic bias-corrected bootstrap of paired log ratios"
                && paired.cluster_unit == "repository/run"
                && paired.resamples == 10_000
                && exact_f64(paired.confidence, 0.95)
                && paired.decision_bound == "one-sided adverse"
                && latency.method == "deterministic hierarchical bootstrap"
                && latency.cluster_levels == ["run", "request"]
                && latency.warmup_requests_per_cell == 100
                && latency.measured_requests_per_cell == 1_000
                && latency.resamples == 10_000
                && exact_f64(latency.confidence, 0.95)
                && latency.decision_bound == "one-sided adverse",
            "paired or latency confidence-interval plan drifted",
        )?;
        require(
            accuracy.method == "deterministic paired bootstrap"
                && accuracy.cluster_unit == "unique fixture/task"
                && accuracy
                    .repository_strata
                    .iter()
                    .map(String::as_str)
                    .eq(REGISTERED_STRATA)
                && accuracy.stratum_weights
                    == BTreeMap::from([
                        ("large".to_string(), 1),
                        ("medium".to_string(), 1),
                        ("small".to_string(), 1),
                    ])
                && accuracy.weight_normalization
                    == "normalize the frozen integer weights to sum to one"
                && accuracy.resamples == 10_000
                && exact_f64(accuracy.confidence, 0.95)
                && accuracy.decision_bound == "one-sided adverse"
                && intervals.seed_derivation == "SHA-256(global seed || metric family || cell ID)",
            "accuracy confidence-interval or frozen-strata plan drifted",
        )?;
        require(
            self.paired_comparison.lower_is_better_ratio == "candidate / baseline"
                && self.paired_comparison.higher_is_better_difference == "candidate - baseline"
                && self.multiplicity.gate_policy
                    == "each required family passes independently; no aggregate compensation"
                && self.multiplicity.claim_family_correction
                    == "Holm step-down family-wise error correction"
                && self.multiplicity.applies_to
                    == [
                        "required corpora",
                        "required languages",
                        "required relation families",
                        "primary metrics",
                    ]
                && exact_f64(self.multiplicity.family_alpha, 0.05)
                && self
                    .multiplicity
                    .uncorrected_exploratory_results_cannot_pass_claims,
            "paired comparison or multiplicity plan drifted",
        )
    }
}

impl DecisionFunctions {
    /// Validate denominator, non-inferiority, superiority, and phase-exit rules.
    fn validate(&self) -> Result<(), EvaluationError> {
        let correctness = &self.correctness;
        require(
            correctness.method == "Wilson score interval"
                && exact_f64(correctness.confidence, 0.95)
                && correctness.minimum_positive_examples_per_family == 20
                && correctness.minimum_negative_examples_per_family == 20
                && exact_f64(correctness.precision_floor, 0.95)
                && exact_f64(correctness.recall_floor, 0.90)
                && exact_f64(correctness.semantic_precision_floor, 0.90)
                && exact_f64(correctness.semantic_recall_floor, 0.80)
                && correctness.decision
                    == "every advertised family lower confidence bound meets its floor",
            "correctness interval, denominator, or family decision drifted",
        )?;
        let non_inferiority = &self.non_inferiority;
        require(
            exact_f64(non_inferiority.performance_ratio_upper, 1.05)
                && exact_f64(non_inferiority.rss_ratio_upper, 1.05)
                && exact_f64(non_inferiority.bytes_ratio_upper, 1.05)
                && exact_f64(non_inferiority.agent_quality_difference_lower, 0.0)
                && non_inferiority.compatibility_required,
            "non-inferiority or compatibility decision drifted",
        )?;
        let superiority = &self.superiority;
        require(
            exact_f64(superiority.cold_index_per_corpus_ratio_upper, 1.10)
                && exact_f64(superiority.cold_index_geometric_mean_ratio_upper, 0.80)
                && exact_f64(superiority.peak_rss_geometric_mean_ratio_upper, 0.80)
                && exact_f64(
                    superiority.structural_retrieval_p95_ratio_upper_exclusive,
                    1.0,
                )
                && exact_f64(
                    superiority.agent_quality_point_estimate_difference_lower,
                    0.05,
                )
                && exact_f64(
                    superiority.agent_quality_corrected_bound_lower_exclusive,
                    0.0,
                )
                && superiority.minimum_improved_pairs == 12,
            "dimension-specific performance or agent-quality superiority decision drifted",
        )?;
        require(
            self.absolute_budget.decision
                == "observed p95 or exact byte count is at or below the preregistered limit"
                && self.phase_exit.decision
                    == "all required cells are present, eligible, compatible, deterministic, contained, and pass their independent decision; missing or ineligible is failure",
            "absolute-budget or fail-closed phase-exit decision drifted",
        )
    }
}

/// Compare manifest-owned floating-point policy values without an approximation window.
fn exact_f64(actual: f64, expected: f64) -> bool {
    actual.to_bits() == expected.to_bits()
}

/// Profile fields relevant to containment.
#[derive(Debug, Deserialize)]
struct ProfileSpec {
    /// Profile identifier.
    id: String,
    /// Whether the profile permits network access.
    network_allowed: bool,
}

/// One exact operation row from the manifest.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationSpec {
    /// Operation identifier.
    id: OperationId,
    /// Corpus selector.
    corpora: String,
    /// Configuration profile.
    profile: String,
    /// Declared cache/process state.
    cache_state: String,
    /// Registered measured repetitions.
    repetitions: usize,
    /// Registered hard deadline.
    timeout_seconds: u64,
    /// Result-schema identifier.
    result_schema: String,
}

/// Minimal typed view of the full evaluation manifest.
#[derive(Debug, Deserialize)]
struct EvaluationManifest {
    /// Manifest schema version.
    schema_version: u32,
    /// Manifest format identifier.
    format: String,
    /// Stable manifest identity.
    manifest_id: String,
    /// Pinned baseline runtime and artifacts.
    projectatlas: ProjectAtlasSpec,
    /// Pinned corpus rows.
    corpora: Vec<CorpusSpec>,
    /// Registered profiles.
    profiles: Vec<ProfileSpec>,
    /// Registered warmup policy.
    experiment_design: ExperimentDesign,
    /// Registered correctness, non-inferiority, and superiority decisions.
    decision_functions: DecisionFunctions,
    /// Registered dev-only `SQLite` architecture experiments.
    architecture_evaluations: ArchitectureEvaluationPlan,
    /// Closed operation inventory.
    operations: Vec<OperationSpec>,
    /// Closed architecture-result field inventories.
    result_schema: ArchitectureResultSchema,
}

/// Manifest-owned field inventories used to reject malformed child metrics.
#[derive(Debug, Deserialize)]
struct ArchitectureResultSchema {
    /// Exact deterministic sample-context fields.
    architecture_sample_context: Vec<String>,
    /// Exact FTS result fields.
    fts_result_metrics: Vec<String>,
    /// Exact `SQLite` strategy aggregate fields.
    sqlite_strategy_result_metrics: Vec<String>,
    /// Exact fields in each `SQLite` strategy cell.
    sqlite_strategy_cell: Vec<String>,
    /// Exact private architecture-child report fields.
    architecture_child_report: Vec<String>,
    /// Exact supervised process-evidence fields.
    architecture_process_evidence: Vec<String>,
    /// Exact raw stdout/stderr artifact fields.
    raw_stream_evidence: Vec<String>,
}

/// Closed command-line arguments.
#[derive(Debug)]
struct RunnerArguments {
    /// Frozen manifest path.
    manifest: PathBuf,
    /// Explicit `ProjectAtlas` executable.
    executable: PathBuf,
    /// Clean source checkout for the pinned baseline runtime.
    source_root: PathBuf,
    /// Explicit Git executable used for all identity operations.
    git: PathBuf,
    /// Root containing local pinned corpus checkouts.
    corpora_root: PathBuf,
    /// Caller-selected ignored output root.
    output_root: PathBuf,
    /// No-clobber run identifier.
    run_id: String,
    /// Optional reduced count for an ineligible pilot.
    pilot_repetitions: Option<usize>,
}

/// Closed invocation modes accepted by the dedicated example.
#[derive(Debug)]
enum RunnerInvocation {
    /// Normal repository-evaluation campaign.
    Evaluation(RunnerArguments),
    /// One internally supervised architecture sample.
    ArchitectureSample(ArchitectureSampleArguments),
}

/// Private, typed arguments for one architecture child.
#[derive(Debug)]
struct ArchitectureSampleArguments {
    /// Frozen manifest path.
    manifest: PathBuf,
    /// Closed architecture operation.
    operation: ArchitectureOperationId,
    /// Pinned corpus identity.
    corpus_id: String,
    /// Whether this is a warmup or measurement.
    sample_kind: SampleKind,
    /// Zero-based outer repetition.
    repetition: usize,
    /// Read-only current-index seed database.
    source_db: PathBuf,
    /// Unique output-owned sample directory, created by the evaluator.
    work_directory: PathBuf,
}

/// Child inputs after manifest, identity, and filesystem validation.
struct ValidatedArchitectureSample {
    /// Compiled and runtime manifest parsed into closed plans.
    manifest: EvaluationManifest,
    /// Canonical read-only current-index seed database.
    source_db: PathBuf,
    /// Canonical output-owned path that must not exist yet.
    work_directory: PathBuf,
    /// Deterministic sample identity consumed by the evaluator.
    sample_context: ArchitectureSampleContext,
}

/// The only operations accepted by the private architecture child.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ArchitectureOperationId {
    /// FTS5 candidate differential.
    FtsDifferential,
    /// `SQLite` load and publication strategy matrix.
    SqliteStrategy,
}

impl ArchitectureOperationId {
    /// Stable manifest and child-protocol identifier.
    const fn id(self) -> &'static str {
        match self {
            Self::FtsDifferential => "fts-differential",
            Self::SqliteStrategy => "sqlite-strategy",
        }
    }

    /// Corresponding manifest operation.
    const fn operation_id(self) -> OperationId {
        match self {
            Self::FtsDifferential => OperationId::FtsDifferential,
            Self::SqliteStrategy => OperationId::SqliteStrategy,
        }
    }

    /// Expected serialized metrics tag.
    const fn result_kind(self) -> &'static str {
        match self {
            Self::FtsDifferential => "fts-result",
            Self::SqliteStrategy => "sqlite-strategy-result",
        }
    }
}

impl TryFrom<OperationId> for ArchitectureOperationId {
    type Error = EvaluationError;

    fn try_from(operation: OperationId) -> Result<Self, Self::Error> {
        match operation {
            OperationId::FtsDifferential => Ok(Self::FtsDifferential),
            OperationId::SqliteStrategy => Ok(Self::SqliteStrategy),
            _ => Err(EvaluationError::Policy(
                "non-architecture operation requested".into(),
            )),
        }
    }
}

impl TryFrom<&str> for ArchitectureOperationId {
    type Error = EvaluationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "fts-differential" => Ok(Self::FtsDifferential),
            "sqlite-strategy" => Ok(Self::SqliteStrategy),
            _ => Err(EvaluationError::Arguments(
                "architecture operation must be fts-differential or sqlite-strategy".into(),
            )),
        }
    }
}

/// One typed JSON document emitted by an architecture child.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchitectureSampleReport {
    /// Child-report schema version.
    schema_version: u32,
    /// Operation actually executed by the child.
    operation_id: ArchitectureOperationId,
    /// Typed evaluator metrics serialized by their owning module.
    metrics: Option<Value>,
    /// Bounded child-side failure description.
    error: Option<String>,
    /// Whether the evaluator result passed every eligibility check.
    success: bool,
}

/// Exact cross-process sample identity expected by the supervising parent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchitectureSampleIdentity {
    /// Manifest field that owns the global seed.
    global_seed_reference: String,
    /// Exact manifest seed value.
    global_seed_hex: String,
    /// Stable corpus/operation/sample identity.
    stable_cell_identity: String,
    /// Zero-based outer repetition.
    repetition: usize,
}

/// Parent-side interpretation that always preserves process evidence separately.
struct ArchitectureSampleOutcome {
    /// Metrics retained from a well-formed child report.
    metrics: Option<Value>,
    /// Bounded validation or child failure.
    error: Option<String>,
    /// Whether process and report both succeeded.
    success: bool,
}

/// Raw environment entry passed only to supervised children.
#[derive(Clone, Debug)]
struct EnvironmentEntry {
    /// Environment-variable name.
    name: String,
    /// Transient value, never persisted directly.
    value: String,
}

/// Observed checkout identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CorpusIdentity {
    /// Observed commit.
    commit: String,
    /// Observed tree.
    tree: String,
    /// Observed tracked-file count.
    tracked_files: u64,
    /// Observed logical blob bytes.
    tracked_logical_bytes: u64,
    /// Observed Git modes and counts.
    git_modes: BTreeMap<String, u64>,
}

/// Runtime-only isolated corpus state.
struct CorpusRuntime {
    /// Frozen corpus row.
    spec: CorpusSpec,
    /// Source and copy identity evidence.
    evidence: Value,
    /// Initial observed Git identity of the isolated copy.
    initial_identity: CorpusIdentity,
    /// Initial digest of the actual materialized checkout bytes.
    initial_materialized_sha256: String,
    /// Isolated checkout root.
    checkout: PathBuf,
    /// Deterministic tracked Rust source used for refresh and MCP calls.
    selected_file: PathBuf,
    /// Output-owned `SQLite` path.
    db: PathBuf,
    /// Frozen current-index seed restored before every non-cold sample.
    seed_db: PathBuf,
}

/// Immutable inputs shared by every operation in one run.
struct ExecutionContext<'a> {
    /// Explicit `ProjectAtlas` executable.
    executable: &'a Path,
    /// This exact release example, reused for supervised architecture samples.
    runner_executable: &'a Path,
    /// Canonical runtime manifest path validated against compiled bytes.
    manifest: &'a Path,
    /// Exact global architecture seed from the compiled manifest.
    global_seed_hex: &'a str,
    /// Run identifier used to isolate telemetry sessions.
    run_id: &'a str,
    /// Credential-free child environment.
    environment: &'a [EnvironmentEntry],
    /// Observed executable package bytes.
    package_bytes: u64,
    /// Pilot or registered evidence status.
    claim_status: &'a str,
    /// Directory that owns bounded raw process streams.
    raw_directory: &'a Path,
}

/// Exact database and `SQLite` sidecar sizes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
struct StorageBytes {
    /// Main database bytes.
    database: u64,
    /// Write-ahead-log bytes.
    wal: u64,
    /// Shared-memory sidecar bytes.
    shm: u64,
    /// Rollback-journal bytes.
    journal: u64,
    /// Sum of all observed sidecars.
    sidecars: u64,
}

impl StorageBytes {
    /// Main database plus observed sidecars.
    const fn total(self) -> u64 {
        self.database.saturating_add(self.sidecars)
    }
}

/// A metric that is either observed with named semantics or explicitly unavailable.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum MetricAvailability<T> {
    /// The metric was observed using the named method.
    Observed {
        /// Observed value.
        value: T,
        /// Measurement method and scope.
        method: &'static str,
    },
    /// The runner cannot truthfully observe the metric.
    Unavailable {
        /// Why the metric is unavailable.
        reason: &'static str,
    },
}

/// Actual process, connection, index, and host-cache state for one sample.
#[derive(Clone, Copy, Debug, Serialize)]
struct CacheStateEvidence {
    /// Product process state at sample start.
    process: &'static str,
    /// MCP process state at sample start.
    mcp_process: &'static str,
    /// `SQLite` connection state at sample start.
    sqlite_connection: &'static str,
    /// Persistent index state at sample start.
    index: &'static str,
    /// Host filesystem-cache state.
    os_file_cache: &'static str,
    /// Whether the state is eligible for a warm-process claim.
    warm_process_claim_eligible: bool,
    /// Whether the state is eligible for a cold-cache claim.
    cold_cache_claim_eligible: bool,
}

/// Closed operation-specific metric schema.
#[derive(Debug, Serialize)]
#[serde(tag = "result_kind", rename_all = "kebab-case")]
enum OperationMetrics {
    /// Full or no-change scan result.
    Index {
        /// Retained database and sidecar bytes after process close.
        retained_storage: StorageBytes,
        /// Signed change in retained file lengths, not bytes written.
        retained_storage_delta_bytes: MetricAvailability<i128>,
        /// Logical write bytes are not exposed by the current runtime.
        logical_written_bytes: MetricAvailability<u64>,
        /// Physical write bytes are not exposed by the current runtime.
        physical_written_bytes: MetricAvailability<u64>,
        /// Indexed files reported by the scan.
        files: u64,
        /// Persisted symbols reported by the scan.
        symbols: u64,
        /// Persisted relations reported by the scan.
        relations: u64,
    },
    /// One-file watcher refresh result.
    Incremental {
        /// Retained database and sidecar bytes after process close.
        retained_storage: StorageBytes,
        /// Signed change in retained file lengths, not bytes written.
        retained_storage_delta_bytes: MetricAvailability<i128>,
        /// Logical write bytes are not exposed by the current runtime.
        logical_written_bytes: MetricAvailability<u64>,
        /// Physical write bytes are not exposed by the current runtime.
        physical_written_bytes: MetricAvailability<u64>,
        /// Deterministically changed source files.
        changed_files: u64,
        /// Symbols refreshed by the watcher.
        symbols: u64,
        /// Relations refreshed by the watcher.
        relations: u64,
    },
    /// Lexical or graph query result.
    Query {
        /// Validated rows returned to the caller.
        returned_rows: u64,
        /// Exact stdout response bytes.
        response_bytes: usize,
    },
    /// Fixed MCP navigation flow result.
    AgentFlow {
        /// Successfully reconciled tool responses.
        mcp_calls_observed: u64,
        /// Successfully reconciled JSON-RPC responses including initialize.
        responses_observed: u64,
        /// Session-isolated estimated tokens emitted with `ProjectAtlas`.
        estimated_tokens_with_projectatlas: u64,
        /// Session-isolated modeled full-file reads avoided.
        likely_file_reads_avoided: u64,
        /// Exact stdout response bytes for the whole flow.
        response_bytes: usize,
        /// Validated JSON-RPC or tool errors.
        errors: u64,
    },
}

/// Minimal typed scan output accepted from the frozen baseline runtime.
#[derive(Debug, Deserialize, Serialize)]
struct ScanOutput {
    /// Repository overview.
    overview: ScanOverview,
    /// Symbol graph build report.
    symbols: SymbolCounts,
}

/// Scan overview fields required by this evaluator.
#[derive(Debug, Deserialize, Serialize)]
struct ScanOverview {
    /// Indexed file count.
    files: u64,
}

/// Symbol counts required from scan/watch output.
#[derive(Debug, Deserialize, Serialize)]
struct SymbolCounts {
    /// Files parsed by the operation.
    parsed: u64,
    /// Persisted symbol count.
    symbols: u64,
    /// Persisted relation count.
    relations: u64,
}

/// Minimal typed one-shot watcher output.
#[derive(Debug, Deserialize)]
struct WatchOutput {
    /// Completed cycles.
    cycles: u64,
    /// Whether this was the requested one-shot execution.
    once: bool,
    /// Last symbol refresh report.
    last_symbols: SymbolCounts,
}

/// Minimal typed lexical-search output.
#[derive(Debug, Deserialize)]
struct SearchOutput {
    /// Declared returned row count.
    returned: u64,
    /// Returned rows.
    results: Vec<SearchResultOutput>,
}

/// Required lexical-search result fields.
#[derive(Debug, Deserialize)]
struct SearchResultOutput {
    /// Repository-relative matched path.
    path: String,
    /// One-based matched line.
    line: u64,
    /// Matched line text.
    text: String,
}

/// Minimal typed relation row used to reject empty or malformed graph output.
#[derive(Debug, Deserialize)]
struct RelationOutput {
    /// Repository-relative path.
    path: String,
    /// Source identity.
    source_name: String,
    /// Target identity.
    target_name: String,
    /// Relation kind.
    kind: String,
    /// One-based source line.
    line: u64,
}

/// Whether a retained sample is a warmup or registered measurement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SampleKind {
    /// Unmeasured cache and runtime preparation retained for audit.
    Warmup,
    /// Registered observation used by later statistical decisions.
    Measurement,
}

impl SampleKind {
    /// Stable evidence and filename identifier.
    const fn id(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measurement => "measurement",
        }
    }
}

impl TryFrom<&str> for SampleKind {
    type Error = EvaluationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "warmup" => Ok(Self::Warmup),
            "measurement" => Ok(Self::Measurement),
            _ => Err(EvaluationError::Arguments(
                "architecture sample kind must be warmup or measurement".into(),
            )),
        }
    }
}

/// No-clobber raw stream artifact referenced by process evidence.
#[derive(Debug, Serialize)]
struct RawStreamEvidence {
    /// Persisted raw stream path.
    path: String,
    /// Exact retained bytes.
    bytes: usize,
    /// SHA-256 over the retained bytes.
    sha256: String,
}

/// Transient process result with persisted evidence and retained stdout.
struct ProcessRun {
    /// Digest-only process evidence.
    evidence: Value,
    /// Retained bytes used only for metrics before being discarded.
    stdout: Vec<u8>,
    /// Whether the child exited successfully within all bounds.
    success: bool,
}

/// One operation record and its success state.
struct MeasurementRun {
    /// Persistable measurement evidence.
    evidence: Value,
    /// Whether measured and restoration processes succeeded.
    success: bool,
}

/// Parsed metrics from the token-report response.
struct McpMetrics {
    /// Successfully reconciled JSON-RPC responses including initialize.
    responses_observed: u64,
    /// Successfully reconciled tool calls.
    calls_observed: u64,
    /// Tokens emitted with `ProjectAtlas` according to session telemetry.
    tokens: u64,
    /// Likely full-file reads avoided according to its telemetry.
    likely_file_reads_avoided: u64,
    /// JSON-RPC or decoding errors observed in the flow.
    errors: u64,
}

/// RAII restoration for the one-file refresh input.
struct FileRestore {
    /// Mutated file path.
    path: PathBuf,
    /// Exact original bytes.
    original: Vec<u8>,
    /// Whether explicit verified restoration completed.
    restored: bool,
}

impl FileRestore {
    /// Apply one deterministic reversible byte change.
    fn mutate(path: &Path) -> Result<Self, EvaluationError> {
        let original = fs::read(path)?;
        let mut changed = original.clone();
        changed.extend_from_slice(MUTATION_MARKER);
        fs::write(path, changed)?;
        Ok(Self {
            path: path.to_owned(),
            original,
            restored: false,
        })
    }

    /// Restore and verify the original bytes.
    fn restore(&mut self) -> Result<(), EvaluationError> {
        fs::write(&self.path, &self.original)?;
        require(
            fs::read(&self.path)? == self.original,
            "refresh input was not restored byte-for-byte",
        )?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for FileRestore {
    fn drop(&mut self) {
        if !self.restored {
            let _ignored = fs::write(&self.path, &self.original);
        }
    }
}

/// Execute the dedicated runner's exact argument surface.
pub(super) async fn run_from_arguments() -> Result<(), EvaluationError> {
    match parse_invocation(env::args_os().skip(1))? {
        RunnerInvocation::Evaluation(arguments) => run_evaluation(arguments).await,
        RunnerInvocation::ArchitectureSample(arguments) => run_architecture_sample(&arguments),
    }
}

/// Execute one normal repository-evaluation campaign without changing its public flags.
async fn run_evaluation(arguments: RunnerArguments) -> Result<(), EvaluationError> {
    fs::create_dir_all(&arguments.output_root)?;
    let output_root = fs::canonicalize(&arguments.output_root)?;
    let corpora_root = fs::canonicalize(&arguments.corpora_root)?;
    require_disjoint(&corpora_root, &output_root)?;
    let run_directory = output_root.join(&arguments.run_id);
    fs::create_dir(&run_directory)?;
    match run_owned(&arguments, &corpora_root, &run_directory).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let failure = json!({
                "schema_version": 1,
                "artifact_kind": ARTIFACT_KIND,
                "run_id": arguments.run_id,
                "error": truncate(&error.to_string(), 4096),
                "failed_unix_ms": unix_millis()?,
                "claim_status": PILOT_STATUS,
            });
            let _ignored = write_json_create_new(&run_directory.join("failure.json"), &failure);
            Err(error)
        }
    }
}

/// Validate and execute exactly one private architecture sample.
fn run_architecture_sample(arguments: &ArchitectureSampleArguments) -> Result<(), EvaluationError> {
    let operation = arguments.operation;
    let validated = validate_architecture_sample(arguments)?;
    let evaluation = match operation {
        ArchitectureOperationId::FtsDifferential => run_fts_differential(
            &validated.source_db,
            &validated.work_directory,
            &validated.manifest.architecture_evaluations,
            &validated.sample_context,
        )
        .map(|result| {
            let success = result.is_eligible();
            (ArchitectureMetrics::FtsResult { result }, success)
        }),
        ArchitectureOperationId::SqliteStrategy => run_sqlite_strategy(
            &validated.source_db,
            &validated.work_directory,
            &validated.manifest.architecture_evaluations,
            &validated.sample_context,
        )
        .map(|result| {
            let success = result.is_eligible();
            (
                ArchitectureMetrics::SqliteStrategyResult { result },
                success,
            )
        }),
    };
    let report = match evaluation {
        Ok((metrics, success)) => ArchitectureSampleReport {
            schema_version: ARCHITECTURE_SAMPLE_SCHEMA_VERSION,
            operation_id: operation,
            metrics: Some(serde_json::to_value(metrics)?),
            error: (!success).then(|| {
                "architecture evaluation retained one or more failed correctness cells".into()
            }),
            success,
        },
        Err(error) => ArchitectureSampleReport {
            schema_version: ARCHITECTURE_SAMPLE_SCHEMA_VERSION,
            operation_id: operation,
            metrics: None,
            error: Some(truncate(&error.to_string(), 4096)),
            success: false,
        },
    };
    emit_architecture_sample_report(&report)?;
    if report.success {
        Ok(())
    } else {
        Err(EvaluationError::Policy(report.error.unwrap_or_else(|| {
            "architecture sample failed without an error".into()
        })))
    }
}

/// Bind a child request to the compiled manifest and its exact sample identity.
fn validate_architecture_sample(
    arguments: &ArchitectureSampleArguments,
) -> Result<ValidatedArchitectureSample, EvaluationError> {
    let manifest_path = fs::canonicalize(&arguments.manifest)?;
    require(
        fs::metadata(&manifest_path)?.is_file(),
        "architecture manifest is not a file",
    )?;
    let manifest_bytes = fs::read(&manifest_path)?;
    require(
        manifest_bytes == MANIFEST_BYTES,
        "architecture runtime manifest differs from compiled bytes",
    )?;
    let manifest = validate_manifest(&manifest_bytes, None)?;
    require(
        manifest
            .corpora
            .iter()
            .any(|corpus| corpus.id == arguments.corpus_id),
        "architecture sample corpus is not registered",
    )?;
    let operation = manifest
        .operations
        .iter()
        .find(|operation| operation.id == arguments.operation.operation_id())
        .ok_or_else(|| {
            EvaluationError::Policy("architecture operation is not registered".into())
        })?;
    let maximum_repetitions = match arguments.sample_kind {
        SampleKind::Warmup => manifest.experiment_design.warmups,
        SampleKind::Measurement => operation.repetitions,
    };
    require(
        maximum_repetitions > 0 && arguments.repetition < maximum_repetitions,
        "architecture sample repetition is outside its registered range",
    )?;
    let source_db = fs::canonicalize(&arguments.source_db)?;
    require(
        fs::metadata(&source_db)?.is_file(),
        "architecture source database is not a file",
    )?;
    let expected_name = architecture_work_directory_name(
        &arguments.corpus_id,
        arguments.operation,
        arguments.sample_kind,
        arguments.repetition,
    );
    require(
        arguments
            .work_directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == expected_name),
        "architecture work-directory identity differs from its sample",
    )?;
    let work_parent = arguments.work_directory.parent().ok_or_else(|| {
        EvaluationError::Policy("architecture work directory has no parent".into())
    })?;
    let work_parent = fs::canonicalize(work_parent)?;
    require(
        fs::metadata(&work_parent)?.is_dir(),
        "architecture work parent is not a directory",
    )?;
    let work_directory = work_parent.join(expected_name);
    require(
        !work_directory.exists(),
        "architecture work directory already exists",
    )?;
    require_disjoint(&source_db, &work_directory)?;
    let stable_cell_identity = architecture_stable_cell_identity(
        &arguments.corpus_id,
        arguments.operation,
        arguments.sample_kind,
    );
    let sample_context = ArchitectureSampleContext::new(
        GLOBAL_SEED_REFERENCE,
        &manifest.experiment_design.rng.seed_hex,
        stable_cell_identity,
        arguments.repetition,
    )?;
    Ok(ValidatedArchitectureSample {
        manifest,
        source_db,
        work_directory,
        sample_context,
    })
}

/// Emit exactly one compact JSON report on the child protocol stream.
fn emit_architecture_sample_report(
    report: &ArchitectureSampleReport,
) -> Result<(), EvaluationError> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, report)?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

/// Own validation, isolation, execution, and final aggregation for one run.
async fn run_owned(
    arguments: &RunnerArguments,
    corpora_root: &Path,
    run_directory: &Path,
) -> Result<(), EvaluationError> {
    let manifest_path = fs::canonicalize(&arguments.manifest)?;
    require(
        fs::metadata(&manifest_path)?.is_file(),
        "runtime manifest is not a file",
    )?;
    let manifest_bytes = fs::read(&manifest_path)?;
    require(
        manifest_bytes == MANIFEST_BYTES,
        "runtime manifest differs from compiled manifest bytes",
    )?;
    let manifest = validate_manifest(&manifest_bytes, arguments.pilot_repetitions)?;
    let source_root = fs::canonicalize(&arguments.source_root)?;
    let git = fs::canonicalize(&arguments.git)?;
    let executable = fs::canonicalize(&arguments.executable)?;
    require(
        fs::metadata(&source_root)?.is_dir(),
        "baseline source root is not a directory",
    )?;
    require(
        fs::metadata(&git)?.is_file(),
        "Git executable is not a file",
    )?;
    require(
        fs::metadata(&executable)?.is_file(),
        "ProjectAtlas release executable is not a file",
    )?;
    require_disjoint(&source_root, run_directory)?;
    require_disjoint(&executable, run_directory)?;
    let control = run_directory.join("control");
    fs::create_dir(&control)?;
    let raw_directory = run_directory.join("raw");
    fs::create_dir(&raw_directory)?;
    let environment = controlled_environment(&control)?;
    let release_artifact = current_release_artifact(&manifest)?.clone();
    let (original_executable_sha256, package_bytes) =
        validate_release_executable(&executable, &release_artifact)?;
    let runtime_directory = run_directory.join("runtime");
    fs::create_dir(&runtime_directory)?;
    let executable_name = executable
        .file_name()
        .ok_or_else(|| EvaluationError::Policy("release executable has no filename".into()))?;
    let owned_executable = runtime_directory.join(executable_name);
    copy_executable_create_new(&executable, &owned_executable)?;
    require(
        sha256_file(&owned_executable)? == release_artifact.executable_sha256
            && fs::metadata(&owned_executable)?.len() == release_artifact.executable_bytes,
        "output-owned release executable copy differs from its pin",
    )?;
    let source_identity = checkout_identity(&git, &source_root, &environment).await?;
    require(
        source_identity.commit == manifest.projectatlas.baseline_runtime_commit
            && source_identity.tree == manifest.projectatlas.baseline_runtime_tree,
        "baseline source commit or tree differs from the manifest",
    )?;
    let source_lock_sha256 = sha256_file(&source_root.join("Cargo.lock"))?;
    require(
        source_lock_sha256 == manifest.projectatlas.baseline_runtime_cargo_lock_sha256,
        "baseline source Cargo.lock differs from the manifest",
    )?;
    let source_materialized_sha256 =
        materialized_checkout_sha256(&git, &source_root, &environment).await?;
    let git_sha256 = sha256_file(&git)?;
    let (git_version, git_version_process) = tool_version(
        &git,
        &["--version".into()],
        run_directory,
        &environment,
        &raw_directory,
        "provenance-git-version",
    )
    .await?;
    let (runtime_version, runtime_version_process) = tool_version(
        &owned_executable,
        &["--version".into()],
        run_directory,
        &environment,
        &raw_directory,
        "provenance-projectatlas-version",
    )
    .await?;
    require(
        runtime_version == release_artifact.version,
        "ProjectAtlas --version differs from the pinned release artifact",
    )?;
    let claim_status = if arguments.pilot_repetitions.is_some() {
        PILOT_STATUS
    } else {
        REGISTERED_STATUS
    };
    let runner_build_profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    if arguments.pilot_repetitions.is_none() {
        require(
            runner_build_profile == "release",
            "registered evaluation requires a release evaluator build",
        )?;
    }
    let corpora =
        materialize_corpora(&manifest, &git, corpora_root, run_directory, &environment).await?;
    let runner_executable = fs::canonicalize(env::current_exe()?)?;
    let operations = registered_operations(&manifest);
    let warmups_per_cell = if arguments.pilot_repetitions.is_some() {
        usize::from(manifest.experiment_design.warmups > 0)
    } else {
        manifest.experiment_design.warmups
    };
    let measured_repetitions =
        |operation: &OperationSpec| arguments.pilot_repetitions.unwrap_or(operation.repetitions);
    let measured_counts = operations
        .iter()
        .map(measured_repetitions)
        .collect::<Vec<_>>();
    let (expected_warmups, expected_measurements) =
        expected_sample_counts(corpora.len(), warmups_per_cell, &measured_counts)?;
    let plan = json!({
        "schema_version": 1,
        "artifact_kind": ARTIFACT_KIND,
        "manifest_id": manifest.manifest_id,
        "manifest_sha256": sha256_hex(&manifest_bytes),
        "run_id": arguments.run_id,
        "pilot_repetitions": arguments.pilot_repetitions,
        "baseline_source": {
            "root": path_text(&source_root)?,
            "identity": source_identity,
            "materialized_sha256": source_materialized_sha256,
            "cargo_lock_sha256": source_lock_sha256,
        },
        "git": {
            "path": path_text(&git)?,
            "sha256": git_sha256,
            "version": git_version,
            "version_process": git_version_process,
        },
        "release_artifact": release_artifact,
        "input_executable": path_text(&executable)?,
        "output_owned_executable": path_text(&owned_executable)?,
        "executable_sha256": original_executable_sha256,
        "package_bytes": package_bytes,
        "evaluator": {
            "executable": path_text(&runner_executable)?,
            "executable_sha256": sha256_file(&runner_executable)?,
            "executable_bytes": fs::metadata(&runner_executable)?.len(),
            "build_profile": runner_build_profile,
            "target": current_target_triple(),
            "runner_source_sha256": sha256_hex(RUNNER_SOURCE_BYTES),
            "supervisor_source_sha256": sha256_hex(SUPERVISOR_SOURCE_BYTES),
            "git_policy_source_sha256": sha256_hex(GIT_POLICY_SOURCE_BYTES),
            "architecture_evaluator_source_sha256": sha256_hex(ARCHITECTURE_EVALUATOR_SOURCE_BYTES),
            "lexical_fixture_sha256": sha256_hex(LEXICAL_FIXTURE_BYTES),
            "example_source_sha256": sha256_hex(EXAMPLE_SOURCE_BYTES),
            "cargo_lock_sha256": sha256_hex(RUNNER_LOCK_BYTES),
        },
        "runtime_version": runtime_version,
        "runtime_version_process": runtime_version_process,
        "environment": retained_environment(&environment),
        "corpora": corpora.iter().map(|corpus| &corpus.evidence).collect::<Vec<_>>(),
        "derived_sample_counts": {
            "expected_warmups": expected_warmups,
            "expected_measurements": expected_measurements,
        },
        "no_network_inputs": true,
        "os_network_isolation_observed": false,
        "claim_status": claim_status,
    });
    write_json_create_new(&run_directory.join("plan.json"), &plan)?;
    let seeds_directory = run_directory.join("seeds");
    fs::create_dir(&seeds_directory)?;
    let measurements_directory = run_directory.join("measurements");
    fs::create_dir(&measurements_directory)?;
    let warmups_directory = run_directory.join("warmups");
    fs::create_dir(&warmups_directory)?;
    let architecture_directory = run_directory.join("architecture-evaluations");
    fs::create_dir(&architecture_directory)?;
    let context = ExecutionContext {
        executable: &owned_executable,
        runner_executable: &runner_executable,
        manifest: &manifest_path,
        global_seed_hex: &manifest.experiment_design.rng.seed_hex,
        run_id: &arguments.run_id,
        environment: &environment,
        package_bytes,
        claim_status,
        raw_directory: &raw_directory,
    };
    let mut failure_count = 0_usize;
    for corpus in &corpora {
        let evidence = match prepare_seed_database(&context, corpus).await {
            Ok(evidence) => evidence,
            Err(error) => {
                failure_count = failure_count.saturating_add(1);
                failed_seed_evidence(corpus, &error)
            }
        };
        write_json_create_new(
            &seeds_directory.join(format!("{}.json", corpus.spec.id)),
            &evidence,
        )?;
    }
    let mut warmups = Vec::new();
    let mut measurements = Vec::new();
    for corpus in &corpora {
        for operation in &operations {
            for (sample_kind, repetitions) in [
                (SampleKind::Warmup, warmups_per_cell),
                (SampleKind::Measurement, measured_repetitions(operation)),
            ] {
                for repetition in 0..repetitions {
                    let execution = match operation.id {
                        OperationId::FtsDifferential | OperationId::SqliteStrategy => {
                            run_architecture_operation(
                                &context,
                                corpus,
                                operation,
                                sample_kind,
                                repetition,
                                &manifest.result_schema,
                                &architecture_directory,
                            )
                            .await
                        }
                        _ => {
                            run_operation(&context, corpus, operation, sample_kind, repetition)
                                .await
                        }
                    };
                    let measurement = match execution {
                        Ok(measurement) => measurement,
                        Err(error) => failed_sample_evidence(
                            corpus,
                            operation,
                            sample_kind,
                            repetition,
                            claim_status,
                            &error,
                        ),
                    };
                    if !measurement.success {
                        failure_count = failure_count.saturating_add(1);
                    }
                    let collection = if sample_kind == SampleKind::Warmup {
                        &mut warmups
                    } else {
                        &mut measurements
                    };
                    let directory = if sample_kind == SampleKind::Warmup {
                        &warmups_directory
                    } else {
                        &measurements_directory
                    };
                    let path = directory.join(format!(
                        "{:04}-{}-{}-{repetition}.json",
                        collection.len(),
                        corpus.spec.id,
                        operation.id.id(),
                    ));
                    write_json_create_new(&path, &measurement.evidence)?;
                    collection.push(measurement.evidence);
                }
            }
        }
    }
    require(
        warmups.len() == expected_warmups && measurements.len() == expected_measurements,
        "retained warmup or measurement count differs from the registered plan",
    )?;
    let mut final_corpora = Vec::new();
    for corpus in &corpora {
        match final_corpus_evidence(&git, corpus, &environment).await {
            Ok(evidence) => final_corpora.push(evidence),
            Err(error) => {
                failure_count = failure_count.saturating_add(1);
                final_corpora.push(json!({
                    "id": corpus.spec.id,
                    "status": "failed",
                    "error": truncate(&error.to_string(), 4096),
                }));
            }
        }
    }
    let final_source = checkout_identity(&git, &source_root, &environment).await;
    let final_source_materialized =
        materialized_checkout_sha256(&git, &source_root, &environment).await;
    let source_verified = final_source
        .as_ref()
        .is_ok_and(|identity| identity == &source_identity)
        && final_source_materialized
            .as_ref()
            .is_ok_and(|digest| digest == &source_materialized_sha256)
        && sha256_file(&source_root.join("Cargo.lock"))? == source_lock_sha256;
    if !source_verified {
        failure_count = failure_count.saturating_add(1);
    }
    let original_executable_final_sha256 = sha256_file(&executable)?;
    let owned_executable_final_sha256 = sha256_file(&owned_executable)?;
    let executable_verified = original_executable_final_sha256 == original_executable_sha256
        && owned_executable_final_sha256 == original_executable_sha256
        && fs::metadata(&owned_executable)?.len() == package_bytes;
    if !executable_verified {
        failure_count = failure_count.saturating_add(1);
    }
    let report = json!({
        "schema_version": 1,
        "artifact_kind": ARTIFACT_KIND,
        "manifest_id": manifest.manifest_id,
        "run_id": arguments.run_id,
        "warmup_count": warmups.len(),
        "measurement_count": measurements.len(),
        "derived_sample_counts": {
            "expected_warmups": expected_warmups,
            "expected_measurements": expected_measurements,
        },
        "failed_samples_or_verifications": failure_count,
        "final_corpora": final_corpora,
        "final_source": {
            "status": if source_verified { "verified" } else { "failed" },
            "identity": final_source.ok(),
            "materialized_sha256": final_source_materialized.ok(),
        },
        "final_executables": {
            "status": if executable_verified { "verified" } else { "failed" },
            "input_sha256": original_executable_final_sha256,
            "output_owned_sha256": owned_executable_final_sha256,
        },
        "claim_eligible": false,
        "residual_blockers": [
            "complete-process-tree peak RSS is unavailable with the current portable dependency set",
            "OS-level network egress isolation is not observed by this runner",
            "source, calibration, and release-environment evidence must be joined externally",
        ],
        "claim_status": claim_status,
    });
    write_json_create_new(&run_directory.join("report.json"), &report)?;
    require(
        failure_count == 0,
        "one or more retained samples or final verifications failed",
    )
}

/// Select the normal public runner or its private supervised sample mode.
fn parse_invocation(
    arguments: impl Iterator<Item = OsString>,
) -> Result<RunnerInvocation, EvaluationError> {
    let mut values = arguments.collect::<Vec<_>>();
    if values
        .first()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == ARCHITECTURE_SAMPLE_COMMAND)
    {
        values.remove(0);
        return parse_architecture_sample_arguments(values.into_iter())
            .map(RunnerInvocation::ArchitectureSample);
    }
    parse_arguments(values.into_iter()).map(RunnerInvocation::Evaluation)
}

/// Parse unique flag/value pairs and reject malformed encodings or duplicates.
fn parse_option_pairs(
    arguments: impl Iterator<Item = OsString>,
) -> Result<BTreeMap<String, String>, EvaluationError> {
    let values = arguments
        .map(|value| {
            value
                .into_string()
                .map_err(|_value| EvaluationError::Arguments("arguments must be Unicode".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    require(
        values.len().is_multiple_of(2),
        "every runner option requires one value",
    )?;
    let mut options = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        require(
            pair[0].starts_with("--") && options.insert(pair[0].clone(), pair[1].clone()).is_none(),
            "runner options must be unique named flags",
        )?;
    }
    Ok(options)
}

/// Parse unique normal-run flags while preserving the established public surface.
fn parse_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<RunnerArguments, EvaluationError> {
    let options = parse_option_pairs(arguments)?;
    let required = [
        "--manifest",
        "--projectatlas",
        "--source-root",
        "--git",
        "--corpora-root",
        "--output-root",
        "--run-id",
    ];
    require(
        required.iter().all(|name| options.contains_key(*name))
            && options
                .keys()
                .all(|name| required.contains(&name.as_str()) || name == "--pilot-repetitions"),
        "runner requires manifest, executable, source root, Git, corpus root, output root, and run id",
    )?;
    let run_id = option(&options, "--run-id")?.to_owned();
    require(
        !run_id.is_empty()
            && run_id.len() <= 80
            && run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "run id must be 1-80 safe ASCII characters",
    )?;
    let pilot_repetitions = options
        .get("--pilot-repetitions")
        .map(|value| {
            value.parse::<usize>().map_err(|error| {
                EvaluationError::Arguments(format!("invalid pilot repetition count: {error}"))
            })
        })
        .transpose()?;
    if let Some(count) = pilot_repetitions {
        require(count > 0, "pilot repetition count must be positive")?;
    }
    Ok(RunnerArguments {
        manifest: PathBuf::from(option(&options, "--manifest")?),
        executable: PathBuf::from(option(&options, "--projectatlas")?),
        source_root: PathBuf::from(option(&options, "--source-root")?),
        git: PathBuf::from(option(&options, "--git")?),
        corpora_root: PathBuf::from(option(&options, "--corpora-root")?),
        output_root: PathBuf::from(option(&options, "--output-root")?),
        run_id,
        pilot_repetitions,
    })
}

/// Parse the exact private architecture-sample argument schema.
fn parse_architecture_sample_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<ArchitectureSampleArguments, EvaluationError> {
    let options = parse_option_pairs(arguments)?;
    let required = [
        "--manifest",
        "--operation",
        "--corpus-id",
        "--sample-kind",
        "--repetition",
        "--source-db",
        "--work-directory",
    ];
    require(
        options.len() == required.len() && required.iter().all(|name| options.contains_key(*name)),
        "architecture sample requires only manifest, operation, corpus, sample kind, repetition, source database, and work directory",
    )?;
    let corpus_id = option(&options, "--corpus-id")?.to_owned();
    require(
        !corpus_id.is_empty()
            && corpus_id.len() <= 80
            && corpus_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "architecture corpus id must be bounded safe ASCII",
    )?;
    let repetition = option(&options, "--repetition")?
        .parse::<usize>()
        .map_err(|error| {
            EvaluationError::Arguments(format!("invalid architecture sample repetition: {error}"))
        })?;
    Ok(ArchitectureSampleArguments {
        manifest: PathBuf::from(option(&options, "--manifest")?),
        operation: ArchitectureOperationId::try_from(option(&options, "--operation")?)?,
        corpus_id,
        sample_kind: SampleKind::try_from(option(&options, "--sample-kind")?)?,
        repetition,
        source_db: PathBuf::from(option(&options, "--source-db")?),
        work_directory: PathBuf::from(option(&options, "--work-directory")?),
    })
}

/// Return one required option.
fn option<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, EvaluationError> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| EvaluationError::Arguments(format!("{name} is required")))
}

/// Validate the corpus and closed operation contracts.
fn validate_manifest(
    bytes: &[u8],
    pilot_repetitions: Option<usize>,
) -> Result<EvaluationManifest, EvaluationError> {
    let manifest: EvaluationManifest = serde_json::from_slice(bytes)?;
    require(
        manifest.schema_version == 1 && manifest.format == MANIFEST_FORMAT,
        "manifest schema or format drifted",
    )?;
    require(!manifest.manifest_id.is_empty(), "manifest id is empty")?;
    require(
        is_hex_identifier(&manifest.projectatlas.baseline_runtime_commit, 40)
            && is_hex_identifier(&manifest.projectatlas.baseline_runtime_tree, 40)
            && is_hex_identifier(
                &manifest.projectatlas.baseline_runtime_cargo_lock_sha256,
                64,
            )
            && manifest.projectatlas.cargo_lock_sha256 == sha256_hex(RUNNER_LOCK_BYTES),
        "baseline source or evaluator lock drifted",
    )?;
    manifest.experiment_design.validate()?;
    manifest.decision_functions.validate()?;
    manifest.architecture_evaluations.validate(
        &sha256_hex(LEXICAL_FIXTURE_BYTES),
        LEXICAL_FIXTURE_BYTES.len(),
        manifest.experiment_design.warmups,
    )?;
    let release_targets = manifest
        .projectatlas
        .baseline_release_artifacts
        .iter()
        .map(|artifact| artifact.target.as_str())
        .collect::<BTreeSet<_>>();
    require(
        !release_targets.is_empty()
            && release_targets.len() == manifest.projectatlas.baseline_release_artifacts.len()
            && manifest
                .projectatlas
                .baseline_release_artifacts
                .iter()
                .all(|artifact| {
                    is_hex_identifier(&artifact.executable_sha256, 64)
                        && artifact.executable_bytes > 0
                        && artifact.build_profile == "release"
                        && !artifact.version.trim().is_empty()
                        && !artifact.provenance.trim().is_empty()
                }),
        "baseline release artifact inventory drifted",
    )?;
    let corpus_ids = manifest
        .corpora
        .iter()
        .map(|corpus| corpus.id.as_str())
        .collect::<BTreeSet<_>>();
    require(
        manifest.corpora.len() == REQUIRED_CORPORA.len()
            && corpus_ids == REQUIRED_CORPORA.into_iter().collect(),
        "required corpus identities drifted",
    )?;
    let mut strata = BTreeSet::new();
    for corpus in &manifest.corpora {
        require(
            is_hex_identifier(&corpus.commit, 40)
                && is_hex_identifier(&corpus.tree, 40)
                && corpus.clean_required
                && !corpus.submodules_allowed
                && !corpus.lfs_allowed
                && corpus.tracked_files > 0
                && corpus.tracked_logical_bytes > 0
                && corpus.git_modes.values().sum::<u64>() == corpus.tracked_files
                && corpus.materialization_state == "verified"
                && strata.insert(corpus.stratum.as_str()),
            "corpus pin or materialization contract drifted",
        )?;
    }
    require(
        strata == ["small", "medium", "large"].into_iter().collect(),
        "corpus strata drifted",
    )?;
    require(
        manifest
            .profiles
            .iter()
            .any(|profile| profile.id == "default-core" && !profile.network_allowed),
        "default-core must remain no-network",
    )?;
    let operation_ids = manifest
        .operations
        .iter()
        .map(|operation| operation.id)
        .collect::<BTreeSet<_>>();
    require(
        manifest.operations.len() == OPERATION_COUNT && operation_ids.len() == OPERATION_COUNT,
        "closed operation inventory drifted",
    )?;
    for id in OperationId::BASELINE {
        let operation = manifest
            .operations
            .iter()
            .find(|operation| operation.id == id)
            .ok_or_else(|| EvaluationError::Policy("baseline operation is missing".into()))?;
        let (cache_state, repetitions, timeout, schema) = expected_baseline(id)
            .ok_or_else(|| EvaluationError::Policy("baseline policy is missing".into()))?;
        require(
            operation.corpora == "all"
                && operation.profile == "default-core"
                && operation.cache_state == cache_state
                && operation.repetitions == repetitions
                && operation.timeout_seconds == timeout
                && operation.result_schema == schema,
            "baseline operation policy drifted",
        )?;
        if let Some(pilot) = pilot_repetitions {
            require(
                pilot <= operation.repetitions,
                "pilot repetitions exceed a registered count",
            )?;
        }
    }
    for id in [OperationId::FtsDifferential, OperationId::SqliteStrategy] {
        let operation = manifest
            .operations
            .iter()
            .find(|operation| operation.id == id)
            .ok_or_else(|| EvaluationError::Policy("architecture operation is missing".into()))?;
        let (profile, repetitions, timeout, schema) = match id {
            OperationId::FtsDifferential => (
                "fts-candidate-lab",
                manifest.architecture_evaluations.fts_repetitions(),
                120,
                "fts-result",
            ),
            OperationId::SqliteStrategy => (
                "default-core",
                manifest.architecture_evaluations.sqlite_repetitions(),
                300,
                "sqlite-strategy-result",
            ),
            _ => {
                return Err(EvaluationError::Policy(
                    "non-architecture operation requested".into(),
                ));
            }
        };
        require(
            operation.corpora == "all"
                && operation.profile == profile
                && operation.cache_state == CACHE_CURRENT_INDEX_SUPERVISED_ARCHITECTURE_CHILD
                && operation.repetitions == repetitions
                && operation.timeout_seconds == timeout
                && operation.result_schema == schema,
            "architecture operation policy drifted",
        )?;
        if let Some(pilot) = pilot_repetitions {
            require(
                pilot <= operation.repetitions,
                "pilot repetitions exceed an architecture count",
            )?;
        }
    }
    Ok(manifest)
}

/// Return the exact cache state, count, deadline, and schema for a baseline operation.
fn expected_baseline(id: OperationId) -> Option<(&'static str, usize, u64, &'static str)> {
    match id {
        OperationId::ColdFullScan => {
            Some((CACHE_DATABASE_ABSENT_NEW_PROCESS, 30, 900, "index-result"))
        }
        OperationId::WarmFullScan => {
            Some((CACHE_CURRENT_INDEX_NEW_PROCESS, 10, 900, "index-result"))
        }
        OperationId::NoChangeScan => {
            Some((CACHE_CURRENT_INDEX_NEW_PROCESS, 10, 300, "index-result"))
        }
        OperationId::OneFileRefresh => Some((
            CACHE_CURRENT_INDEX_NEW_PROCESS,
            10,
            300,
            "incremental-result",
        )),
        OperationId::LexicalSearch | OperationId::GraphLookup => {
            Some((CACHE_CURRENT_INDEX_NEW_PROCESS, 15, 30, "query-result"))
        }
        OperationId::McpCallFlow => Some((
            CACHE_CURRENT_INDEX_NEW_MCP_PROCESS,
            15,
            120,
            "agent-flow-result",
        )),
        _ => None,
    }
}

/// Return baseline rows in their registered order.
#[cfg(test)]
fn baseline_operations(manifest: &EvaluationManifest) -> Vec<OperationSpec> {
    OperationId::BASELINE
        .into_iter()
        .filter_map(|id| {
            manifest
                .operations
                .iter()
                .find(|operation| operation.id == id)
                .cloned()
        })
        .collect()
}

/// Return every operation executed by the dedicated evaluator in registered order.
fn registered_operations(manifest: &EvaluationManifest) -> Vec<OperationSpec> {
    OperationId::REGISTERED
        .into_iter()
        .filter_map(|id| {
            manifest
                .operations
                .iter()
                .find(|operation| operation.id == id)
                .cloned()
        })
        .collect()
}

/// Return the compile target represented by the current evaluator binary.
const fn current_target_triple() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    )))]
    {
        "unsupported-target"
    }
}

/// Select the one pinned release artifact for the current target.
fn current_release_artifact(
    manifest: &EvaluationManifest,
) -> Result<&ReleaseArtifactSpec, EvaluationError> {
    let matches = manifest
        .projectatlas
        .baseline_release_artifacts
        .iter()
        .filter(|artifact| artifact.target == current_target_triple())
        .collect::<Vec<_>>();
    require(
        matches.len() == 1,
        "manifest has no unique pinned release artifact for this target",
    )?;
    matches
        .into_iter()
        .next()
        .ok_or_else(|| EvaluationError::Policy("release artifact selection failed".into()))
}

/// Validate an executable against the exact registered digest and byte length.
fn validate_release_executable(
    executable: &Path,
    artifact: &ReleaseArtifactSpec,
) -> Result<(String, u64), EvaluationError> {
    let sha256 = sha256_file(executable)?;
    let bytes = fs::metadata(executable)?.len();
    require(
        sha256 == artifact.executable_sha256 && bytes == artifact.executable_bytes,
        "ProjectAtlas executable does not match the pinned release artifact",
    )?;
    Ok((sha256, bytes))
}

/// Validate sources, local-clone them, and validate the isolated copies.
async fn materialize_corpora(
    manifest: &EvaluationManifest,
    git: &Path,
    corpora_root: &Path,
    run_directory: &Path,
    environment: &[EnvironmentEntry],
) -> Result<Vec<CorpusRuntime>, EvaluationError> {
    let copies = run_directory.join("corpora");
    let state = run_directory.join("state");
    fs::create_dir(&copies)?;
    fs::create_dir(&state)?;
    let mut corpora = Vec::new();
    for spec in &manifest.corpora {
        let source = fs::canonicalize(corpora_root.join(corpus_directory_name(&spec.id)))?;
        require(source.starts_with(corpora_root), "corpus escaped its root")?;
        let source_observed = checkout_identity(git, &source, environment).await?;
        let source_materialized_sha256 =
            materialized_checkout_sha256(git, &source, environment).await?;
        let expected = CorpusIdentity {
            commit: spec.commit.clone(),
            tree: spec.tree.clone(),
            tracked_files: spec.tracked_files,
            tracked_logical_bytes: spec.tracked_logical_bytes,
            git_modes: spec.git_modes.clone(),
        };
        require(
            source_observed == expected,
            "source corpus identity drifted",
        )?;
        let copy = copies.join(&spec.id);
        git_checked(
            git,
            run_directory,
            &[
                "clone".into(),
                "--local".into(),
                "--no-hardlinks".into(),
                "--no-checkout".into(),
                "--".into(),
                path_text(&source)?,
                path_text(&copy)?,
            ],
            environment,
        )
        .await?;
        git_checked(
            git,
            run_directory,
            &[
                "-C".into(),
                path_text(&copy)?,
                "checkout".into(),
                "--detach".into(),
                "--force".into(),
                spec.commit.clone(),
            ],
            environment,
        )
        .await?;
        let copy = fs::canonicalize(copy)?;
        let copy_observed = checkout_identity(git, &copy, environment).await?;
        require(
            copy_observed == expected,
            "isolated corpus identity drifted",
        )?;
        let copy_materialized_sha256 =
            materialized_checkout_sha256(git, &copy, environment).await?;
        require(
            copy_materialized_sha256 == source_materialized_sha256,
            "isolated corpus materialized bytes differ from the source checkout",
        )?;
        let selected_file = selected_source_file(git, &copy, environment).await?;
        let selected_relative = selected_file
            .strip_prefix(&copy)
            .map_err(|error| EvaluationError::Policy(error.to_string()))?;
        let evidence = json!({
            "id": spec.id,
            "stratum": spec.stratum,
            "source": path_text(&source)?,
            "isolated_copy": path_text(&copy)?,
            "expected": expected,
            "source_observed": source_observed,
            "copy_observed": copy_observed,
            "source_materialized_sha256": source_materialized_sha256,
            "copy_materialized_sha256": copy_materialized_sha256,
            "selected_file": path_text(selected_relative)?,
        });
        let db = state.join(&spec.id).with_extension("db");
        corpora.push(CorpusRuntime {
            spec: spec.clone(),
            evidence,
            initial_identity: copy_observed,
            initial_materialized_sha256: copy_materialized_sha256,
            checkout: copy,
            selected_file,
            seed_db: state.join(&spec.id).with_extension("seed.db"),
            db,
        });
    }
    Ok(corpora)
}

/// Map a manifest identity to the existing ignored materialization directory.
fn corpus_directory_name(id: &str) -> &str {
    if id == "projectatlas-self" {
        "projectatlas"
    } else {
        id
    }
}

/// Read clean commit, tree, file, byte, and mode identity from Git.
async fn checkout_identity(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
) -> Result<CorpusIdentity, EvaluationError> {
    let status = git_worktree_status(git, repository, environment).await?;
    require(status.is_empty(), "corpus checkout is not clean")?;
    let commit = git_output(git, repository, &["rev-parse", "HEAD"], environment).await?;
    let tree = git_output(git, repository, &["rev-parse", "HEAD^{tree}"], environment).await?;
    let rows = git_output_bytes(
        git,
        repository,
        &["ls-tree", "-r", "-l", "-z", "HEAD"],
        environment,
    )
    .await?;
    let (tracked_files, tracked_logical_bytes, git_modes) = parse_ls_tree(&rows)?;
    Ok(CorpusIdentity {
        commit: commit.trim().into(),
        tree: tree.trim().into(),
        tracked_files,
        tracked_logical_bytes,
        git_modes,
    })
}

/// Account for NUL-delimited `git ls-tree -r -l` rows.
fn parse_ls_tree(bytes: &[u8]) -> Result<(u64, u64, BTreeMap<String, u64>), EvaluationError> {
    let mut files = 0_u64;
    let mut logical_bytes = 0_u64;
    let mut modes = BTreeMap::new();
    for row in bytes.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let metadata = row
            .split(|byte| *byte == b'\t')
            .next()
            .ok_or_else(|| EvaluationError::Policy("Git tree row has no metadata".into()))?;
        let fields = std::str::from_utf8(metadata)?
            .split_whitespace()
            .collect::<Vec<_>>();
        require(
            fields.len() == 4 && fields[1] == "blob",
            "invalid Git tree row",
        )?;
        let size = fields[3]
            .parse::<u64>()
            .map_err(|error| EvaluationError::Policy(format!("invalid blob size: {error}")))?;
        files = files.saturating_add(1);
        logical_bytes = logical_bytes.saturating_add(size);
        *modes.entry(fields[0].to_owned()).or_default() += 1;
    }
    Ok((files, logical_bytes, modes))
}

/// Choose the first tracked regular Rust file deterministically.
async fn selected_source_file(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
) -> Result<PathBuf, EvaluationError> {
    let files = git_output_bytes(
        git,
        repository,
        &["ls-files", "-z", "--", ":(glob)**/*.rs"],
        environment,
    )
    .await?;
    for relative in files.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let path = repository.join(std::str::from_utf8(relative)?);
        if fs::symlink_metadata(&path)?.file_type().is_file() {
            return Ok(path);
        }
    }
    Err(EvaluationError::Policy(
        "corpus has no tracked regular Rust source file".into(),
    ))
}

/// Hash the actual tracked working-tree bytes and materialized file kinds.
async fn materialized_checkout_sha256(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
) -> Result<String, EvaluationError> {
    materialized_checkout_sha256_with_limits(
        git,
        repository,
        environment,
        MaterializedReadLimits {
            per_file_bytes: MATERIALIZED_FILE_BYTE_LIMIT,
            aggregate_bytes: MATERIALIZED_CHECKOUT_BYTE_LIMIT,
        },
    )
    .await
}

/// Explicit read ceilings for one materialized-checkout digest.
#[derive(Clone, Copy)]
struct MaterializedReadLimits {
    /// Maximum bytes consumed from one regular file or symlink target.
    per_file_bytes: u64,
    /// Maximum bytes consumed across every tracked entry.
    aggregate_bytes: u64,
}

/// Hash tracked materializations while enforcing caller-selected testable ceilings.
async fn materialized_checkout_sha256_with_limits(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
    limits: MaterializedReadLimits,
) -> Result<String, EvaluationError> {
    require(
        limits.per_file_bytes > 0 && limits.aggregate_bytes > 0,
        "materialized checkout read limits must be greater than zero",
    )?;
    let files = git_output_bytes(git, repository, &["ls-files", "-z"], environment).await?;
    let mut hasher = Sha256::new();
    let mut aggregate_bytes = 0_u64;
    for relative_bytes in files.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let path_state = inspect_worktree_path(repository, relative_bytes)?;
        let relative = path_from_git_bytes(relative_bytes)?;
        hash_field(&mut hasher, relative_bytes);
        let path = repository.join(relative);
        match path_state {
            WorktreePathState::Symlink(observed_path) => {
                let target = fs::read_link(&observed_path)?;
                let target_bytes = native_path_bytes(&target);
                require(
                    u64::try_from(target_bytes.len()).unwrap_or(u64::MAX) <= limits.per_file_bytes,
                    "tracked materialized entry exceeds the per-file read limit",
                )?;
                account_materialized_bytes(
                    &mut aggregate_bytes,
                    u64::try_from(target_bytes.len()).unwrap_or(u64::MAX),
                    limits.aggregate_bytes,
                )?;
                hasher.update(*b"l");
                hash_field(&mut hasher, &target_bytes);
                require(
                    matches!(
                        inspect_worktree_path(repository, relative_bytes)?,
                        WorktreePathState::Symlink(ref current_path)
                            if native_path_bytes(&fs::read_link(current_path)?) == target_bytes
                    ),
                    "tracked symlink changed while its materialization was hashed",
                )?;
            }
            WorktreePathState::Regular(metadata) => {
                require(
                    metadata.len() <= limits.per_file_bytes,
                    "tracked materialized file exceeds the per-file read limit",
                )?;
                require(
                    aggregate_bytes
                        .checked_add(metadata.len())
                        .is_some_and(|bytes| bytes <= limits.aggregate_bytes),
                    "tracked checkout exceeds the aggregate read limit",
                )?;
                hasher.update(*b"f");
                hasher.update(metadata.len().to_le_bytes());
                let file = fs::File::open(&path)?;
                let remaining_aggregate = limits.aggregate_bytes - aggregate_bytes;
                let read_ceiling = limits.per_file_bytes.min(remaining_aggregate);
                let mut bounded = file.take(read_ceiling.saturating_add(1));
                let mut file_bytes = 0_u64;
                let mut buffer = [0_u8; 16 * 1024];
                loop {
                    let read_count = bounded.read(&mut buffer)?;
                    if read_count == 0 {
                        break;
                    }
                    let read_bytes = u64::try_from(read_count).unwrap_or(u64::MAX);
                    file_bytes = file_bytes.checked_add(read_bytes).ok_or_else(|| {
                        EvaluationError::Policy(
                            "materialized file byte accounting overflowed".into(),
                        )
                    })?;
                    require(
                        file_bytes <= limits.per_file_bytes,
                        "tracked materialized file exceeds the per-file read limit",
                    )?;
                    account_materialized_bytes(
                        &mut aggregate_bytes,
                        read_bytes,
                        limits.aggregate_bytes,
                    )?;
                    hasher.update(&buffer[..read_count]);
                }
                require(
                    file_bytes == metadata.len()
                        && matches!(
                            inspect_worktree_path(repository, relative_bytes)?,
                            WorktreePathState::Regular(ref current) if current.len() == metadata.len()
                        ),
                    "tracked file changed while its materialization was hashed",
                )?;
            }
            WorktreePathState::Missing => {
                return Err(EvaluationError::Policy(
                    "tracked materialized path is missing".into(),
                ));
            }
            WorktreePathState::Unsafe => {
                return Err(EvaluationError::Policy(
                    "tracked materialized path has a linked, reparse, or incompatible component"
                        .into(),
                ));
            }
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Add one materialized read to the aggregate byte budget.
fn account_materialized_bytes(
    aggregate_bytes: &mut u64,
    entry_bytes: u64,
    aggregate_limit: u64,
) -> Result<(), EvaluationError> {
    *aggregate_bytes = aggregate_bytes
        .checked_add(entry_bytes)
        .ok_or_else(|| EvaluationError::Policy("materialized byte accounting overflowed".into()))?;
    require(
        *aggregate_bytes <= aggregate_limit,
        "tracked checkout exceeds the aggregate read limit",
    )
}

/// Preserve the native symlink-target representation used by the host filesystem.
#[cfg(unix)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

/// Encode a Windows symlink target without requiring Unicode scalar conversion.
#[cfg(windows)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Hash one length-prefixed field into a canonical evidence digest.
fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Derive exact retained sample totals from runtime manifest values.
fn expected_sample_counts(
    corpus_count: usize,
    warmups_per_cell: usize,
    measured_repetitions: &[usize],
) -> Result<(usize, usize), EvaluationError> {
    require(
        corpus_count > 0
            && warmups_per_cell > 0
            && !measured_repetitions.is_empty()
            && measured_repetitions.iter().all(|count| *count > 0),
        "sample-count inputs must be non-vacuous",
    )?;
    let expected_warmups = corpus_count
        .checked_mul(measured_repetitions.len())
        .and_then(|cells| cells.checked_mul(warmups_per_cell))
        .ok_or_else(|| EvaluationError::Policy("warmup count overflowed".into()))?;
    let measurements_per_corpus =
        measured_repetitions
            .iter()
            .try_fold(0_usize, |total, repetitions| {
                total
                    .checked_add(*repetitions)
                    .ok_or_else(|| EvaluationError::Policy("measurement count overflowed".into()))
            })?;
    let expected_measurements = corpus_count
        .checked_mul(measurements_per_corpus)
        .ok_or_else(|| EvaluationError::Policy("measurement count overflowed".into()))?;
    Ok((expected_warmups, expected_measurements))
}

/// Stable output-directory identity shared by parent and validated child.
fn architecture_work_directory_name(
    corpus_id: &str,
    operation: ArchitectureOperationId,
    sample_kind: SampleKind,
    repetition: usize,
) -> String {
    format!(
        "{corpus_id}-{}-{}-{repetition}",
        operation.id(),
        sample_kind.id()
    )
}

/// Stable paired-order identity shared by parent and child.
fn architecture_stable_cell_identity(
    corpus_id: &str,
    operation: ArchitectureOperationId,
    sample_kind: SampleKind,
) -> String {
    format!("{corpus_id}:{}:{}", operation.id(), sample_kind.id())
}

/// Construct the exact serialized sample context expected from the child.
fn expected_architecture_sample_identity(
    global_seed_hex: &str,
    corpus_id: &str,
    operation: ArchitectureOperationId,
    sample_kind: SampleKind,
    repetition: usize,
) -> ArchitectureSampleIdentity {
    ArchitectureSampleIdentity {
        global_seed_reference: GLOBAL_SEED_REFERENCE.into(),
        global_seed_hex: global_seed_hex.into(),
        stable_cell_identity: architecture_stable_cell_identity(corpus_id, operation, sample_kind),
        repetition,
    }
}

/// Build the exact private command used to execute one architecture sample.
fn architecture_sample_argv(
    manifest: &Path,
    operation: ArchitectureOperationId,
    corpus_id: &str,
    sample_kind: SampleKind,
    repetition: usize,
    source_db: &Path,
    work_directory: &Path,
) -> Result<Vec<String>, EvaluationError> {
    Ok(vec![
        ARCHITECTURE_SAMPLE_COMMAND.into(),
        "--manifest".into(),
        path_text(manifest)?,
        "--operation".into(),
        operation.id().into(),
        "--corpus-id".into(),
        corpus_id.into(),
        "--sample-kind".into(),
        sample_kind.id().into(),
        "--repetition".into(),
        repetition.to_string(),
        "--source-db".into(),
        path_text(source_db)?,
        "--work-directory".into(),
        path_text(work_directory)?,
    ])
}

/// Interpret a supervised child without ever discarding its separate process evidence.
fn architecture_sample_outcome(
    process: &ProcessRun,
    operation: ArchitectureOperationId,
    expected_context: &ArchitectureSampleIdentity,
    result_schema: &ArchitectureResultSchema,
) -> ArchitectureSampleOutcome {
    match parse_architecture_sample_report(
        &process.stdout,
        operation,
        expected_context,
        result_schema,
    ) {
        Ok(report) => {
            let mut failures = Vec::new();
            if !process.success {
                failures.push(
                    "supervised architecture child failed its exit, timeout, or capture bound"
                        .to_owned(),
                );
            }
            if let Some(error) = report.error {
                failures.push(error);
            }
            let success = process.success && report.success && failures.is_empty();
            ArchitectureSampleOutcome {
                metrics: report.metrics,
                error: (!failures.is_empty()).then(|| truncate(&failures.join("; "), 4096)),
                success,
            }
        }
        Err(error) => ArchitectureSampleOutcome {
            metrics: None,
            error: Some(truncate(
                &format!("malformed architecture child output: {error}"),
                4096,
            )),
            success: false,
        },
    }
}

/// Parse one complete child document and validate its manifest-owned metric schema.
fn parse_architecture_sample_report(
    bytes: &[u8],
    expected_operation: ArchitectureOperationId,
    expected_context: &ArchitectureSampleIdentity,
    result_schema: &ArchitectureResultSchema,
) -> Result<ArchitectureSampleReport, EvaluationError> {
    let report_value: Value = serde_json::from_slice(bytes)?;
    require_exact_object_fields(
        &report_value,
        &result_schema.architecture_child_report,
        "architecture child report",
    )?;
    let report: ArchitectureSampleReport = serde_json::from_value(report_value)?;
    require(
        report.schema_version == ARCHITECTURE_SAMPLE_SCHEMA_VERSION
            && report.operation_id == expected_operation,
        "architecture child schema or operation differs from its request",
    )?;
    let metrics_eligible = match report.metrics.as_ref() {
        Some(metrics) => Some(validate_architecture_metrics(
            metrics,
            expected_operation,
            expected_context,
            result_schema,
        )?),
        None => None,
    };
    require(
        report.success == metrics_eligible.unwrap_or(false),
        "architecture child success differs from metric eligibility",
    )?;
    require(
        if report.success {
            report.error.is_none()
        } else {
            report.error.as_ref().is_some_and(|error| !error.is_empty())
        },
        "architecture child error state is inconsistent",
    )?;
    Ok(report)
}

/// Validate the exact top-level and per-cell fields owned by the compiled manifest.
fn validate_architecture_metrics(
    metrics: &Value,
    operation: ArchitectureOperationId,
    expected_context: &ArchitectureSampleIdentity,
    result_schema: &ArchitectureResultSchema,
) -> Result<bool, EvaluationError> {
    let expected_fields = match operation {
        ArchitectureOperationId::FtsDifferential => &result_schema.fts_result_metrics,
        ArchitectureOperationId::SqliteStrategy => &result_schema.sqlite_strategy_result_metrics,
    };
    require_exact_object_fields(metrics, expected_fields, "architecture result")?;
    require(
        metrics.get("result_kind").and_then(Value::as_str) == Some(operation.result_kind()),
        "architecture result kind differs from its operation",
    )?;
    let sample_context = metrics
        .get("sample_context")
        .ok_or_else(|| EvaluationError::Policy("architecture sample context is missing".into()))?;
    require_exact_object_fields(
        sample_context,
        &result_schema.architecture_sample_context,
        "architecture sample context",
    )?;
    let observed_context: ArchitectureSampleIdentity =
        serde_json::from_value(sample_context.clone())?;
    require(
        observed_context == *expected_context,
        "architecture sample context differs from its exact request",
    )?;
    if operation == ArchitectureOperationId::SqliteStrategy {
        let cells = metrics
            .get("cells")
            .and_then(Value::as_array)
            .ok_or_else(|| EvaluationError::Policy("SQLite strategy cells are missing".into()))?;
        require(!cells.is_empty(), "SQLite strategy cells are empty")?;
        for cell in cells {
            require_exact_object_fields(
                cell,
                &result_schema.sqlite_strategy_cell,
                "SQLite strategy cell",
            )?;
        }
    }
    metrics
        .get("eligible")
        .and_then(Value::as_bool)
        .ok_or_else(|| EvaluationError::Policy("architecture eligibility is not boolean".into()))
}

/// Validate supervised process and raw-stream evidence against manifest inventories.
fn validate_architecture_process_evidence(
    process: &Value,
    result_schema: &ArchitectureResultSchema,
) -> Result<(), EvaluationError> {
    require_exact_object_fields(
        process,
        &result_schema.architecture_process_evidence,
        "architecture process evidence",
    )?;
    for stream_name in ["stdout", "stderr"] {
        let stream = process.get(stream_name).ok_or_else(|| {
            EvaluationError::Policy(format!(
                "architecture process {stream_name} evidence is missing"
            ))
        })?;
        require_exact_object_fields(
            stream,
            &result_schema.raw_stream_evidence,
            "architecture raw stream evidence",
        )?;
    }
    Ok(())
}

/// Compare object fields against one manifest-owned inventory without duplicates.
fn require_exact_object_fields(
    value: &Value,
    expected_fields: &[String],
    owner: &str,
) -> Result<(), EvaluationError> {
    let object = value
        .as_object()
        .ok_or_else(|| EvaluationError::Policy(format!("{owner} is not an object")))?;
    let expected = expected_fields
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    require(
        expected.len() == expected_fields.len() && actual == expected,
        &format!("{owner} fields differ from the compiled manifest"),
    )
}

/// Run one architecture sample in a killable child of this release example.
async fn run_architecture_operation(
    context: &ExecutionContext<'_>,
    corpus: &CorpusRuntime,
    operation: &OperationSpec,
    sample_kind: SampleKind,
    repetition: usize,
    result_schema: &ArchitectureResultSchema,
    architecture_directory: &Path,
) -> Result<MeasurementRun, EvaluationError> {
    let architecture_operation = ArchitectureOperationId::try_from(operation.id)?;
    let expected_sample_context = expected_architecture_sample_identity(
        context.global_seed_hex,
        &corpus.spec.id,
        architecture_operation,
        sample_kind,
        repetition,
    );
    require(
        fs::metadata(&corpus.seed_db).is_ok_and(|metadata| metadata.is_file()),
        "architecture evaluation source seed is unavailable",
    )?;
    let work_directory = architecture_directory.join(architecture_work_directory_name(
        &corpus.spec.id,
        architecture_operation,
        sample_kind,
        repetition,
    ));
    let argv = architecture_sample_argv(
        context.manifest,
        architecture_operation,
        &corpus.spec.id,
        sample_kind,
        repetition,
        &corpus.seed_db,
        &work_directory,
    )?;
    let storage_before = storage_bytes(&corpus.seed_db)?;
    let started_unix_ms = unix_millis()?;
    let raw_stem = format!(
        "{}-{}-{}-{repetition}",
        corpus.spec.id,
        architecture_operation.id(),
        sample_kind.id()
    );
    let process = run_process(
        context.runner_executable,
        &argv,
        architecture_directory,
        Duration::from_secs(operation.timeout_seconds),
        None,
        context.environment,
        context.raw_directory,
        &raw_stem,
    )
    .await?;
    validate_architecture_process_evidence(&process.evidence, result_schema)?;
    let storage_after = storage_bytes(&corpus.seed_db)?;
    require(
        storage_before == storage_after,
        "architecture evaluation changed the read-only source seed",
    )?;
    let outcome = architecture_sample_outcome(
        &process,
        architecture_operation,
        &expected_sample_context,
        result_schema,
    );
    Ok(MeasurementRun {
        success: outcome.success,
        evidence: json!({
            "schema_version": 1,
            "artifact_kind": ARTIFACT_KIND,
            "corpus_id": corpus.spec.id,
            "operation_id": operation.id.id(),
            "sample_kind": sample_kind,
            "cache_state": operation.cache_state,
            "cache_state_evidence": cache_state_evidence(operation.id),
            "repetition": repetition,
            "registered_repetitions": operation.repetitions,
            "registered_timeout_seconds": operation.timeout_seconds,
            "started_unix_ms": started_unix_ms,
            "process": process.evidence,
            "restoration_process": null,
            "storage_before": storage_before,
            "storage_after": storage_after,
            "complete_process_tree_peak_rss_bytes": MetricAvailability::<u64>::Unavailable {
                reason: "the current process-tree supervisor does not expose peak RSS",
            },
            "package_bytes": context.package_bytes,
            "metrics": outcome.metrics,
            "error": outcome.error,
            "success": outcome.success,
            "claim_status": context.claim_status,
        }),
    })
}

/// Run one bounded sample and, when needed, an untimed restoration refresh.
async fn run_operation(
    context: &ExecutionContext<'_>,
    corpus: &CorpusRuntime,
    operation: &OperationSpec,
    sample_kind: SampleKind,
    repetition: usize,
) -> Result<MeasurementRun, EvaluationError> {
    if operation.id == OperationId::ColdFullScan {
        clear_database(&corpus.db)?;
    } else {
        restore_seed_database(corpus)?;
    }
    let before = storage_bytes(&corpus.db)?;
    let session = format!(
        "eval-{}-{}-{}-{}-{repetition}",
        context.run_id,
        corpus.spec.id,
        operation.id.id(),
        sample_kind.id(),
    );
    let mut argv = cli_prefix(&corpus.db, &session)?;
    let mut stdin = None;
    match operation.id {
        OperationId::ColdFullScan | OperationId::WarmFullScan | OperationId::NoChangeScan => {
            argv.extend(["scan".into(), path_text(&corpus.checkout)?]);
        }
        OperationId::OneFileRefresh => {
            argv.extend(watch_arguments(
                &corpus.checkout,
                operation.timeout_seconds,
            )?);
        }
        OperationId::LexicalSearch => {
            argv.extend([
                "search".into(),
                SEARCH_PATTERN.into(),
                "--limit".into(),
                "20".into(),
            ]);
        }
        OperationId::GraphLookup => {
            argv.extend([
                "symbols".into(),
                "relations".into(),
                "--limit".into(),
                "20".into(),
            ]);
        }
        OperationId::McpCallFlow => {
            argv.push("mcp".into());
            stdin = Some(mcp_input(corpus, &session)?);
        }
        _ => {
            return Err(EvaluationError::Policy(
                "non-baseline operation requested".into(),
            ));
        }
    }
    let mut restore = if operation.id == OperationId::OneFileRefresh {
        Some(FileRestore::mutate(&corpus.selected_file)?)
    } else {
        None
    };
    let started_unix_ms = unix_millis()?;
    let raw_stem = format!(
        "{}-{}-{}-{repetition}",
        corpus.spec.id,
        operation.id.id(),
        sample_kind.id(),
    );
    let process = run_process(
        context.executable,
        &argv,
        &corpus.checkout,
        Duration::from_secs(operation.timeout_seconds),
        stdin.as_deref(),
        context.environment,
        context.raw_directory,
        &raw_stem,
    )
    .await?;
    let after = storage_bytes(&corpus.db)?;
    let mut restoration = None;
    let mut restoration_ok = true;
    if let Some(restore) = restore.as_mut() {
        restore.restore()?;
        let mut restore_argv = cli_prefix(&corpus.db, &format!("{session}-restore"))?;
        restore_argv.extend(watch_arguments(
            &corpus.checkout,
            operation.timeout_seconds,
        )?);
        let restored = run_process(
            context.executable,
            &restore_argv,
            &corpus.checkout,
            Duration::from_secs(operation.timeout_seconds),
            None,
            context.environment,
            context.raw_directory,
            &format!("{raw_stem}-restore"),
        )
        .await?;
        let restoration_parse = restored
            .success
            .then(|| validate_watch_output(&restored.stdout))
            .transpose();
        restoration_ok = restored.success && restoration_parse.is_ok();
        restoration = Some(restored.evidence);
    }
    let parsed_metrics = if process.success {
        parse_operation_metrics(operation.id, &process.stdout, before, after, &session)
    } else {
        Err(EvaluationError::Policy(
            "measured process did not complete successfully".into(),
        ))
    };
    let metrics_error = parsed_metrics
        .as_ref()
        .err()
        .map(|error| truncate(&error.to_string(), 4096));
    let success = process.success && restoration_ok && parsed_metrics.is_ok();
    Ok(MeasurementRun {
        success,
        evidence: json!({
            "schema_version": 1,
            "artifact_kind": ARTIFACT_KIND,
            "corpus_id": corpus.spec.id,
            "operation_id": operation.id.id(),
            "sample_kind": sample_kind,
            "cache_state": operation.cache_state,
            "cache_state_evidence": cache_state_evidence(operation.id),
            "repetition": repetition,
            "registered_repetitions": operation.repetitions,
            "registered_timeout_seconds": operation.timeout_seconds,
            "started_unix_ms": started_unix_ms,
            "process": process.evidence,
            "restoration_process": restoration,
            "storage_before": before,
            "storage_after": after,
            "complete_process_tree_peak_rss_bytes": MetricAvailability::<u64>::Unavailable {
                reason: "processkit supervises the tree but does not expose portable complete-tree peak RSS",
            },
            "package_bytes": context.package_bytes,
            "metrics": parsed_metrics.ok(),
            "error": metrics_error,
            "success": success,
            "claim_status": context.claim_status,
        }),
    })
}

/// Build one current index outside registered warmups and measurements.
async fn prepare_seed_database(
    context: &ExecutionContext<'_>,
    corpus: &CorpusRuntime,
) -> Result<Value, EvaluationError> {
    clear_database(&corpus.db)?;
    let session = format!("eval-{}-{}-seed", context.run_id, corpus.spec.id);
    let mut argv = cli_prefix(&corpus.db, &session)?;
    argv.extend(["scan".into(), path_text(&corpus.checkout)?]);
    let process = run_process(
        context.executable,
        &argv,
        &corpus.checkout,
        Duration::from_mins(15),
        None,
        context.environment,
        context.raw_directory,
        &format!("{}-seed", corpus.spec.id),
    )
    .await?;
    require(process.success, "seed scan process failed")?;
    let scan = parse_scan_output(&process.stdout)?;
    require(
        fs::metadata(&corpus.db)?.is_file(),
        "seed database is missing",
    )?;
    copy_file_create_new(&corpus.db, &corpus.seed_db)?;
    let seed_sha256 = sha256_file(&corpus.seed_db)?;
    Ok(json!({
        "schema_version": 1,
        "artifact_kind": ARTIFACT_KIND,
        "corpus_id": corpus.spec.id,
        "status": "verified",
        "process": process.evidence,
        "scan": scan,
        "seed_database": path_text(&corpus.seed_db)?,
        "seed_database_sha256": seed_sha256,
        "seed_database_bytes": fs::metadata(&corpus.seed_db)?.len(),
    }))
}

/// Restore one frozen current-index database before a non-cold sample.
fn restore_seed_database(corpus: &CorpusRuntime) -> Result<(), EvaluationError> {
    require(
        fs::metadata(&corpus.seed_db).is_ok_and(|metadata| metadata.is_file()),
        "current-index seed database is unavailable",
    )?;
    clear_database(&corpus.db)?;
    copy_file_replace(&corpus.seed_db, &corpus.db)?;
    require(
        sha256_file(&corpus.db)? == sha256_file(&corpus.seed_db)?,
        "restored current-index database differs from its seed",
    )
}

/// Return the actual current process, connection, index, and host-cache state.
fn cache_state_evidence(operation: OperationId) -> CacheStateEvidence {
    let architecture_evaluation = matches!(
        operation,
        OperationId::FtsDifferential | OperationId::SqliteStrategy
    );
    CacheStateEvidence {
        process: if architecture_evaluation {
            PROCESS_STATE_SUPERVISED_CHILD
        } else {
            PROCESS_STATE_NEW
        },
        mcp_process: if operation == OperationId::McpCallFlow {
            MCP_PROCESS_STATE_NEW
        } else {
            MCP_PROCESS_NOT_APPLICABLE
        },
        sqlite_connection: if architecture_evaluation {
            SQLITE_CONNECTION_STATE_PER_SAMPLE
        } else {
            SQLITE_CONNECTION_STATE_NEW
        },
        index: if operation == OperationId::ColdFullScan {
            "absent"
        } else if architecture_evaluation {
            "read-only-current-seed-plus-output-owned-evaluation-databases"
        } else {
            "restored-current-seed"
        },
        os_file_cache: OS_FILE_CACHE_UNCONTROLLED,
        warm_process_claim_eligible: false,
        cold_cache_claim_eligible: false,
    }
}

/// Parse and validate the closed result schema for one operation.
fn parse_operation_metrics(
    operation: OperationId,
    stdout: &[u8],
    before: StorageBytes,
    after: StorageBytes,
    session: &str,
) -> Result<OperationMetrics, EvaluationError> {
    let retained_storage_delta_bytes = MetricAvailability::Observed {
        value: i128::from(after.total()) - i128::from(before.total()),
        method: "signed difference of retained database and sidecar file lengths",
    };
    let logical_written_bytes = MetricAvailability::Unavailable {
        reason: "the released runtime does not expose logical bytes written",
    };
    let physical_written_bytes = MetricAvailability::Unavailable {
        reason: "the evaluator does not observe host-level physical writes",
    };
    match operation {
        OperationId::ColdFullScan | OperationId::WarmFullScan | OperationId::NoChangeScan => {
            let scan = parse_scan_output(stdout)?;
            Ok(OperationMetrics::Index {
                retained_storage: after,
                retained_storage_delta_bytes,
                logical_written_bytes,
                physical_written_bytes,
                files: scan.overview.files,
                symbols: scan.symbols.symbols,
                relations: scan.symbols.relations,
            })
        }
        OperationId::OneFileRefresh => {
            let watch = validate_watch_output(stdout)?;
            Ok(OperationMetrics::Incremental {
                retained_storage: after,
                retained_storage_delta_bytes,
                logical_written_bytes,
                physical_written_bytes,
                changed_files: 1,
                symbols: watch.last_symbols.symbols,
                relations: watch.last_symbols.relations,
            })
        }
        OperationId::LexicalSearch => {
            let search: SearchOutput = serde_json::from_slice(stdout)?;
            require(
                search.returned > 0
                    && search.returned == search.results.len() as u64
                    && search.results.iter().all(|row| {
                        !row.path.trim().is_empty()
                            && row.line > 0
                            && row.text.to_ascii_lowercase().contains(SEARCH_PATTERN)
                    }),
                "lexical search output is empty, inconsistent, or malformed",
            )?;
            Ok(OperationMetrics::Query {
                returned_rows: search.returned,
                response_bytes: stdout.len(),
            })
        }
        OperationId::GraphLookup => {
            let relations: Vec<RelationOutput> = serde_json::from_slice(stdout)?;
            require(
                !relations.is_empty()
                    && relations.iter().all(|row| {
                        !row.path.trim().is_empty()
                            && !row.source_name.trim().is_empty()
                            && !row.target_name.trim().is_empty()
                            && !row.kind.trim().is_empty()
                            && row.line > 0
                    }),
                "graph lookup output is empty or malformed",
            )?;
            Ok(OperationMetrics::Query {
                returned_rows: relations.len() as u64,
                response_bytes: stdout.len(),
            })
        }
        OperationId::McpCallFlow => {
            let metrics = parse_mcp_metrics(stdout, session)?;
            Ok(OperationMetrics::AgentFlow {
                mcp_calls_observed: metrics.calls_observed,
                responses_observed: metrics.responses_observed,
                estimated_tokens_with_projectatlas: metrics.tokens,
                likely_file_reads_avoided: metrics.likely_file_reads_avoided,
                response_bytes: stdout.len(),
                errors: metrics.errors,
            })
        }
        _ => Err(EvaluationError::Policy(
            "non-baseline operation requested".into(),
        )),
    }
}

/// Deserialize a scan and reject vacuous success output.
fn parse_scan_output(stdout: &[u8]) -> Result<ScanOutput, EvaluationError> {
    let scan: ScanOutput = serde_json::from_slice(stdout)?;
    require(
        scan.overview.files > 0 && scan.symbols.symbols > 0 && scan.symbols.relations > 0,
        "scan output has zero files, symbols, or relations",
    )?;
    Ok(scan)
}

/// Deserialize a one-shot watcher report and validate its typed counts.
fn validate_watch_output(stdout: &[u8]) -> Result<WatchOutput, EvaluationError> {
    let watch: WatchOutput = serde_json::from_slice(stdout)?;
    require(
        watch.once && watch.cycles == 1 && watch.last_symbols.parsed > 0,
        "watch output is not one completed typed refresh",
    )?;
    Ok(watch)
}

/// Retain a typed failure row for a sample that could not produce valid evidence.
fn failed_sample_evidence(
    corpus: &CorpusRuntime,
    operation: &OperationSpec,
    sample_kind: SampleKind,
    repetition: usize,
    claim_status: &str,
    error: &EvaluationError,
) -> MeasurementRun {
    MeasurementRun {
        success: false,
        evidence: json!({
            "schema_version": 1,
            "artifact_kind": ARTIFACT_KIND,
            "corpus_id": corpus.spec.id,
            "operation_id": operation.id.id(),
            "sample_kind": sample_kind,
            "cache_state": operation.cache_state,
            "repetition": repetition,
            "success": false,
            "error": truncate(&error.to_string(), 4096),
            "claim_status": claim_status,
        }),
    }
}

/// Retain a typed seed-preparation failure without hiding later cell failures.
fn failed_seed_evidence(corpus: &CorpusRuntime, error: &EvaluationError) -> Value {
    json!({
        "schema_version": 1,
        "artifact_kind": ARTIFACT_KIND,
        "corpus_id": corpus.spec.id,
        "status": "failed",
        "error": truncate(&error.to_string(), 4096),
    })
}

/// Revalidate Git identity and actual materialized bytes after every sample.
async fn final_corpus_evidence(
    git: &Path,
    corpus: &CorpusRuntime,
    environment: &[EnvironmentEntry],
) -> Result<Value, EvaluationError> {
    let identity = checkout_identity(git, &corpus.checkout, environment).await?;
    let materialized_sha256 =
        materialized_checkout_sha256(git, &corpus.checkout, environment).await?;
    require(
        identity == corpus.initial_identity
            && materialized_sha256 == corpus.initial_materialized_sha256,
        "final corpus identity or materialized bytes differ from the initial copy",
    )?;
    Ok(json!({
        "id": corpus.spec.id,
        "status": "verified",
        "identity": identity,
        "materialized_sha256": materialized_sha256,
    }))
}

/// Build global CLI arguments shared by every operation.
fn cli_prefix(db: &Path, session: &str) -> Result<Vec<String>, EvaluationError> {
    Ok(vec![
        "--format".into(),
        "json".into(),
        "--db".into(),
        path_text(db)?,
        "--session".into(),
        session.into(),
    ])
}

/// Build one bounded watcher refresh invocation.
fn watch_arguments(root: &Path, timeout_seconds: u64) -> Result<Vec<String>, EvaluationError> {
    Ok(vec![
        "watch".into(),
        path_text(root)?,
        "--once".into(),
        "--timeout-seconds".into(),
        timeout_seconds.saturating_sub(5).max(1).to_string(),
    ])
}

/// Build the fixed atlas-first JSON-RPC stream.
fn mcp_input(corpus: &CorpusRuntime, session: &str) -> Result<Vec<u8>, EvaluationError> {
    let file = path_text(
        corpus
            .selected_file
            .strip_prefix(&corpus.checkout)
            .map_err(|error| EvaluationError::Policy(error.to_string()))?,
    )?;
    let folder = path_text(Path::new(&file).parent().unwrap_or_else(|| Path::new(".")))?;
    let calls = [
        (2, "atlas_overview", json!({})),
        (3, "atlas_folders", json!({"query": folder, "limit": 5})),
        (
            4,
            "atlas_files",
            json!({"query": "source", "folder": folder, "limit": 5}),
        ),
        (5, "atlas_file_summary", json!({"file": file, "limit": 10})),
        (
            6,
            "atlas_slice",
            json!({"file": file, "start_line": 1, "end_line": 10}),
        ),
        (
            7,
            "atlas_symbol_relations",
            json!({"file": file, "limit": 10}),
        ),
        (8, "atlas_token_report", json!({"session": session})),
    ];
    let mut messages = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"projectatlas-repository-evaluation","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ];
    messages.extend(calls.map(|(id, name, arguments)| {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
    }));
    let mut input = messages
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
        .into_bytes();
    input.push(b'\n');
    Ok(input)
}

/// Run one exact version probe through the same bounded evidence path.
async fn tool_version(
    executable: &Path,
    arguments: &[String],
    cwd: &Path,
    environment: &[EnvironmentEntry],
    raw_directory: &Path,
    raw_stem: &str,
) -> Result<(String, Value), EvaluationError> {
    let process = run_process(
        executable,
        arguments,
        cwd,
        Duration::from_secs(30),
        None,
        environment,
        raw_directory,
        raw_stem,
    )
    .await?;
    require(process.success, "version probe process failed")?;
    let version = std::str::from_utf8(&process.stdout)?.trim().to_owned();
    require(!version.is_empty(), "version probe returned empty stdout")?;
    Ok((version, process.evidence))
}

/// Execute one command through the shared process-tree supervisor.
async fn run_process(
    executable: &Path,
    argv: &[String],
    cwd: &Path,
    timeout: Duration,
    stdin: Option<&[u8]>,
    environment: &[EnvironmentEntry],
    raw_directory: &Path,
    raw_stem: &str,
) -> Result<ProcessRun, EvaluationError> {
    require(
        !raw_stem.is_empty()
            && raw_stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "raw process evidence stem is unsafe",
    )?;
    let mut command = Command::new(executable)
        .args(argv)
        .current_dir(cwd)
        .env_clear();
    for entry in environment {
        command = command.env(&entry.name, &entry.value);
    }
    if let Some(bytes) = stdin {
        command = command.stdin(Stdin::from_bytes(bytes));
    }
    let output = run_supervised(command, timeout, OUTPUT_LIMIT_BYTES).await?;
    let success = output.is_success();
    let stdout = output.stdout.retained.clone();
    let stdout_artifact = write_raw_create_new(
        &raw_directory.join(format!("{raw_stem}.stdout")),
        &output.stdout.retained,
    )?;
    let stderr_artifact = write_raw_create_new(
        &raw_directory.join(format!("{raw_stem}.stderr")),
        &output.stderr.retained,
    )?;
    require(
        stdout_artifact.sha256 == output.stdout.retained_sha256
            && stderr_artifact.sha256 == output.stderr.retained_sha256,
        "persisted raw stream digest differs from supervised capture",
    )?;
    let evidence = json!({
        "executable": path_text(executable)?,
        "argv": argv,
        "command_sha256": command_sha256(&path_text(executable)?, argv),
        "stdin_sha256": stdin.map(sha256_hex),
        "exit_code": output.exit_code,
        "timed_out": output.timed_out,
        "duration_ns": output.duration_ns,
        "output_truncated": output.output_truncated,
        "stdout": stdout_artifact,
        "stderr": stderr_artifact,
    });
    Ok(ProcessRun {
        evidence,
        stdout,
        success,
    })
}

/// Reconcile every response and parse session-isolated MCP metrics.
fn parse_mcp_metrics(bytes: &[u8], expected_session: &str) -> Result<McpMetrics, EvaluationError> {
    require(
        !expected_session.trim().is_empty(),
        "MCP metric session is empty",
    )?;
    let mut responses = BTreeMap::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: Value = serde_json::from_slice(line)?;
        require(
            value["jsonrpc"] == "2.0",
            "MCP response is not JSON-RPC 2.0",
        )?;
        let id = value["id"]
            .as_u64()
            .ok_or_else(|| EvaluationError::Policy("MCP response has no numeric ID".into()))?;
        require(
            (1..=MCP_RESPONSE_COUNT as u64).contains(&id) && responses.insert(id, value).is_none(),
            "MCP response ID is unknown or duplicated",
        )?;
    }
    require(
        responses.len() == MCP_RESPONSE_COUNT
            && (1..=MCP_RESPONSE_COUNT as u64).all(|id| responses.contains_key(&id)),
        "MCP response set is incomplete",
    )?;
    for (id, response) in &responses {
        require(
            response.get("error").is_none_or(Value::is_null),
            "MCP response contains a top-level JSON-RPC error",
        )?;
        let result = response
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| EvaluationError::Policy("MCP response result is missing".into()))?;
        if *id > 1 {
            let is_error = match result.get("isError") {
                Some(value) => value.as_bool().ok_or_else(|| {
                    EvaluationError::Policy("MCP result.isError is not Boolean".into())
                })?,
                None => false,
            };
            require(!is_error, "MCP tool result reported isError")?;
            require(
                result
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|content| !content.is_empty()),
                "MCP tool result content is missing or empty",
            )?;
        }
    }
    let token_response = responses
        .get(&(MCP_RESPONSE_COUNT as u64))
        .ok_or_else(|| EvaluationError::Policy("token response is missing".into()))?;
    let content = token_response["result"]["content"]
        .as_array()
        .ok_or_else(|| EvaluationError::Policy("token response content is missing".into()))?;
    let token_text = content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    require(!token_text.is_empty(), "token response has no text content")?;
    let tokens = toon_u64(&token_text, "estimated_with_projectatlas")
        .ok_or_else(|| EvaluationError::Policy("session token metric is missing".into()))?;
    let likely_file_reads_avoided =
        toon_u64(&token_text, "likely_file_reads_avoided").ok_or_else(|| {
            EvaluationError::Policy("session read-avoidance metric is missing".into())
        })?;
    Ok(McpMetrics {
        responses_observed: MCP_RESPONSE_COUNT as u64,
        calls_observed: MCP_TOOL_CALLS,
        tokens,
        likely_file_reads_avoided,
        errors: 0,
    })
}

/// Parse one unsigned scalar from a flat TOON line.
fn toon_u64(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once(':')?;
        (name == key).then(|| value.trim().parse().ok()).flatten()
    })
}

/// Capture main database and known `SQLite` sidecars.
fn storage_bytes(db: &Path) -> Result<StorageBytes, EvaluationError> {
    let wal = file_bytes(&path_with_suffix(db, "-wal")?)?;
    let shm = file_bytes(&path_with_suffix(db, "-shm")?)?;
    let journal = file_bytes(&path_with_suffix(db, "-journal")?)?;
    Ok(StorageBytes {
        database: file_bytes(db)?,
        wal,
        shm,
        journal,
        sidecars: wal.saturating_add(shm).saturating_add(journal),
    })
}

/// Return zero for an absent file and reject non-file paths.
fn file_bytes(path: &Path) -> Result<u64, EvaluationError> {
    match fs::metadata(path) {
        Ok(metadata) => {
            require(metadata.is_file(), "storage path is not a file")?;
            Ok(metadata.len())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

/// Remove only output-owned database files before a cold sample.
fn clear_database(db: &Path) -> Result<(), EvaluationError> {
    for path in [
        db.to_owned(),
        path_with_suffix(db, "-wal")?,
        path_with_suffix(db, "-shm")?,
        path_with_suffix(db, "-journal")?,
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Append a `SQLite` sidecar suffix to a Unicode database filename.
fn path_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, EvaluationError> {
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| EvaluationError::Policy("database filename is not Unicode".into()))?;
    Ok(path.with_file_name(format!("{name}{suffix}")))
}

/// Return UTF-8 stdout from one bounded local Git command.
async fn git_output(
    git: &Path,
    repository: &Path,
    arguments: &[&str],
    environment: &[EnvironmentEntry],
) -> Result<String, EvaluationError> {
    Ok(
        std::str::from_utf8(&git_output_bytes(git, repository, arguments, environment).await?)?
            .to_owned(),
    )
}

/// Return raw stdout from one bounded local Git command.
async fn git_output_bytes(
    git: &Path,
    repository: &Path,
    arguments: &[&str],
    environment: &[EnvironmentEntry],
) -> Result<Vec<u8>, EvaluationError> {
    git_output_bytes_with_stdin(git, repository, arguments, None, environment).await
}

/// Return raw stdout from one bounded local Git command with optional raw stdin.
async fn git_output_bytes_with_stdin(
    git: &Path,
    repository: &Path,
    arguments: &[&str],
    stdin: Option<&[u8]>,
    environment: &[EnvironmentEntry],
) -> Result<Vec<u8>, EvaluationError> {
    let work_tree = fs::canonicalize(repository)?;
    let git_directory = resolve_git_directory(&work_tree)?;
    let mut command_arguments = repository_bound_git_arguments(&git_directory, &work_tree)?;
    command_arguments.push(OsString::from("-c"));
    command_arguments.push(OsString::from("core.longpaths=true"));
    command_arguments.extend(arguments.iter().map(OsString::from));
    run_bounded_git(git, &work_tree, command_arguments, stdin, environment).await
}

/// Compute sanitized worktree state twice and reject concurrent drift.
async fn git_worktree_status(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
) -> Result<Vec<u8>, EvaluationError> {
    let first = sanitized_worktree_status_pass(git, repository, environment).await?;
    let second = sanitized_worktree_status_pass(git, repository, environment).await?;
    require(
        first == second,
        "sanitized Git worktree state changed between verification passes",
    )?;
    Ok(first.into_state())
}

/// Compute one HEAD/index/worktree pass with built-in conversion and no filter drivers.
async fn sanitized_worktree_status_pass(
    git: &Path,
    repository: &Path,
    environment: &[EnvironmentEntry],
) -> Result<SanitizedWorktreeEvidence, EvaluationError> {
    let work_tree = fs::canonicalize(repository)?;
    let source_git_directory = resolve_git_directory(&work_tree)?;
    let tracked_paths = git_output_bytes(git, &work_tree, &["ls-files", "-z"], environment).await?;
    reject_custom_filter_dependencies(git, &work_tree, &tracked_paths, environment).await?;
    let head = git_output_bytes(git, &work_tree, raw_head_tree_query(), environment).await?;
    let index = git_output_bytes(git, &work_tree, raw_index_query(), environment).await?;
    let index_flags = git_output_bytes(git, &work_tree, index_flags_query(), environment).await?;
    let plan = plan_sanitized_worktree_comparison(&work_tree, &head, &index, &index_flags)?;
    let workspace =
        SanitizedGitWorkspace::create(&source_git_directory, &work_tree, plan.object_format())?;
    let literal_input = workspace.materialize_literal_hash_inputs(plan.literal_hash_inputs())?;
    run_bounded_git(
        git,
        &work_tree,
        workspace.command_arguments(sanitized_index_import_query())?,
        Some(plan.index_input()),
        environment,
    )
    .await?;
    let hashes = run_bounded_git(
        git,
        &work_tree,
        workspace.command_arguments(sanitized_hash_query())?,
        Some(plan.hash_input()),
        environment,
    )
    .await?;
    let literal_hashes = run_bounded_git(
        git,
        workspace.literal_directory(),
        workspace.command_arguments(sanitized_literal_hash_query())?,
        Some(&literal_input),
        environment,
    )
    .await?;
    let untracked = run_bounded_git(
        git,
        &work_tree,
        workspace.command_arguments(sanitized_untracked_query())?,
        None,
        environment,
    )
    .await?;
    plan.finish(&hashes, &literal_hashes, &untracked)
        .map_err(Into::into)
}

/// Reject tracked paths whose staged attributes require a custom filter driver.
async fn reject_custom_filter_dependencies(
    git: &Path,
    repository: &Path,
    tracked_paths: &[u8],
    environment: &[EnvironmentEntry],
) -> Result<(), EvaluationError> {
    if tracked_paths.is_empty() {
        return Ok(());
    }
    let attributes = git_output_bytes_with_stdin(
        git,
        repository,
        &["check-attr", "--cached", "-z", "--stdin", "filter"],
        Some(tracked_paths),
        environment,
    )
    .await?;
    let fields = attributes.split(|byte| *byte == 0).collect::<Vec<_>>();
    require(
        fields.last().is_some_and(|field| field.is_empty()),
        "Git custom-filter attribute evidence is not NUL terminated",
    )?;
    let records = fields[..fields.len().saturating_sub(1)].chunks_exact(3);
    require(
        records.remainder().is_empty(),
        "Git custom-filter attribute evidence is not triplet framed",
    )?;
    let expected_paths = tracked_paths
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    let records = records.collect::<Vec<_>>();
    require(
        records.len() == expected_paths.len()
            && records
                .iter()
                .zip(expected_paths)
                .all(|(record, expected)| record[0] == expected && record[1] == b"filter"),
        "Git custom-filter attribute evidence does not match tracked paths",
    )?;
    require(
        records
            .iter()
            .all(|record| matches!(record[2], b"unspecified" | b"unset")),
        "repository depends on a custom Git filter and is evaluation-ineligible",
    )
}

/// Run local-only Git with the same environment and output bounds as product calls.
async fn git_checked(
    git: &Path,
    cwd: &Path,
    argv: &[String],
    environment: &[EnvironmentEntry],
) -> Result<Vec<u8>, EvaluationError> {
    let mut command_arguments = closed_git_arguments();
    command_arguments.push(OsString::from("-c"));
    command_arguments.push(OsString::from("core.longpaths=true"));
    command_arguments.extend(
        argv.iter()
            .map(|argument| OsString::from(argument.as_str())),
    );
    run_bounded_git(git, cwd, command_arguments, None, environment).await
}

/// Execute one fully assembled Git command with the evaluator's process bounds.
async fn run_bounded_git(
    git: &Path,
    cwd: &Path,
    command_arguments: Vec<OsString>,
    stdin: Option<&[u8]>,
    environment: &[EnvironmentEntry],
) -> Result<Vec<u8>, EvaluationError> {
    let executable_directory = git
        .parent()
        .ok_or_else(|| EvaluationError::Policy("Git executable has no parent directory".into()))?;
    let mut command = Command::new(git)
        .args(&command_arguments)
        .current_dir(cwd)
        .env_clear();
    for entry in environment {
        command = command.env(&entry.name, &entry.value);
    }
    command = command
        .env("PATH", executable_directory)
        .env("GIT_CONFIG_GLOBAL", git_null_device())
        .env("GIT_CONFIG_SYSTEM", git_null_device());
    for (name, value) in closed_git_environment() {
        command = command.env(name, value);
    }
    if let Some(bytes) = stdin {
        command = command.stdin(Stdin::from_bytes(bytes));
    }
    let output = run_supervised(
        command,
        Duration::from_secs(GIT_TIMEOUT_SECONDS),
        OUTPUT_LIMIT_BYTES,
    )
    .await?;
    require(
        !output.output_truncated,
        "bounded local Git command output was truncated",
    )?;
    if !output.is_success() {
        return Err(EvaluationError::Policy(format!(
            "bounded local Git command failed: argv={command_arguments:?}, exit={:?}, timed_out={}, truncated={}, stderr={}",
            output.exit_code,
            output.timed_out,
            output.output_truncated,
            String::from_utf8_lossy(&output.stderr.retained),
        )));
    }
    Ok(output.stdout.retained)
}

/// Build a credential-free child environment with loopback-denied proxy settings.
fn controlled_environment(control: &Path) -> Result<Vec<EnvironmentEntry>, EvaluationError> {
    let mut entries = Vec::new();
    for name in ["COMSPEC", "PATH", "PATHEXT", "SYSTEMROOT", "WINDIR"] {
        if let Some(value) = env::var_os(name) {
            entries.push(EnvironmentEntry {
                name: name.into(),
                value: value.into_string().map_err(|_value| {
                    EvaluationError::Policy(format!("environment variable `{name}` is not Unicode"))
                })?,
            });
        }
    }
    let control = path_text(control)?;
    entries.extend([
        EnvironmentEntry {
            name: "HOME".into(),
            value: control.clone(),
        },
        EnvironmentEntry {
            name: "TEMP".into(),
            value: control.clone(),
        },
        EnvironmentEntry {
            name: "TMP".into(),
            value: control,
        },
        EnvironmentEntry {
            name: "RUST_BACKTRACE".into(),
            value: "0".into(),
        },
        EnvironmentEntry {
            name: "GIT_CONFIG_NOSYSTEM".into(),
            value: "1".into(),
        },
        EnvironmentEntry {
            name: "GIT_TERMINAL_PROMPT".into(),
            value: "0".into(),
        },
        EnvironmentEntry {
            name: "LANG".into(),
            value: "C".into(),
        },
        EnvironmentEntry {
            name: "LC_ALL".into(),
            value: "C".into(),
        },
        EnvironmentEntry {
            name: "HTTP_PROXY".into(),
            value: "http://127.0.0.1:9".into(),
        },
        EnvironmentEntry {
            name: "HTTPS_PROXY".into(),
            value: "http://127.0.0.1:9".into(),
        },
        EnvironmentEntry {
            name: "ALL_PROXY".into(),
            value: "http://127.0.0.1:9".into(),
        },
        EnvironmentEntry {
            name: "NO_PROXY".into(),
            value: String::new(),
        },
    ]);
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    require(
        entries.windows(2).all(|pair| pair[0].name != pair[1].name),
        "controlled environment contains duplicate names",
    )?;
    Ok(entries)
}

/// Persist only environment names and value digests.
fn retained_environment(entries: &[EnvironmentEntry]) -> Value {
    json!({
        "env_clear": true,
        "names_and_value_sha256": entries.iter().map(|entry| {
            (entry.name.clone(), sha256_hex(entry.value.as_bytes()))
        }).collect::<BTreeMap<_, _>>(),
        "credential_variables_forwarded": false,
    })
}

/// Reject overlapping source and output roots.
fn require_disjoint(left: &Path, right: &Path) -> Result<(), EvaluationError> {
    require(
        left != right && !left.starts_with(right) && !right.starts_with(left),
        "corpus and output roots must be disjoint",
    )
}

/// Copy one immutable file into a caller-owned no-clobber destination.
fn copy_file_create_new(source: &Path, destination: &Path) -> Result<(), EvaluationError> {
    require(
        source != destination,
        "source and copied file paths are equal",
    )?;
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(destination)?;
    let copied = std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    require(
        copied == fs::metadata(source)?.len()
            && copied == fs::metadata(destination)?.len()
            && sha256_file(source)? == sha256_file(destination)?,
        "copied file length or digest differs from its source",
    )
}

/// Copy one executable into a no-clobber destination and retain runnable permissions.
fn copy_executable_create_new(source: &Path, destination: &Path) -> Result<(), EvaluationError> {
    copy_file_create_new(source, destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let source_mode = fs::metadata(source)?.permissions().mode() & 0o777;
        require(
            source_mode & 0o111 != 0,
            "release executable source has no executable permission",
        )?;
        let mut destination_permissions = fs::metadata(destination)?.permissions();
        destination_permissions.set_mode(source_mode);
        fs::set_permissions(destination, destination_permissions)?;
        fs::File::open(destination)?.sync_all()?;
    }
    Ok(())
}

/// Replace an output-owned file with an exact synced source copy.
fn copy_file_replace(source: &Path, destination: &Path) -> Result<(), EvaluationError> {
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)?;
    let copied = std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    require(
        copied == fs::metadata(source)?.len()
            && copied == fs::metadata(destination)?.len()
            && sha256_file(source)? == sha256_file(destination)?,
        "replaced file length or digest differs from its source",
    )
}

/// Write and read back one bounded no-clobber raw stream artifact.
fn write_raw_create_new(path: &Path, bytes: &[u8]) -> Result<RawStreamEvidence, EvaluationError> {
    require(
        bytes.len() <= OUTPUT_LIMIT_BYTES,
        "raw stream exceeds the configured capture ceiling",
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.seek(SeekFrom::Start(0))?;
    let mut persisted = Vec::new();
    file.take((OUTPUT_LIMIT_BYTES as u64).saturating_add(1))
        .read_to_end(&mut persisted)?;
    require(
        persisted == bytes,
        "raw stream readback differs from capture",
    )?;
    Ok(RawStreamEvidence {
        path: path_text(path)?,
        bytes: bytes.len(),
        sha256: sha256_hex(bytes),
    })
}

/// Write, sync, and read back one no-clobber JSON record.
fn write_json_create_new<T: Serialize>(path: &Path, value: &T) -> Result<(), EvaluationError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    require(
        bytes.len() as u64 <= RECORD_LIMIT_BYTES,
        "evidence record exceeds its byte limit",
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    file.seek(SeekFrom::Start(0))?;
    let mut persisted = Vec::new();
    file.take(RECORD_LIMIT_BYTES.saturating_add(1))
        .read_to_end(&mut persisted)?;
    require(
        persisted == bytes,
        "evidence readback differs from written bytes",
    )
}

/// Render a path without lossy conversion.
fn path_text(path: &Path) -> Result<String, EvaluationError> {
    let text = path
        .to_str()
        .ok_or_else(|| EvaluationError::Policy("path is not Unicode".into()))?;
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return Ok(rest.to_owned());
        }
    }
    Ok(text.to_owned())
}

/// Hash the exact executable and length-prefixed argument tuple.
fn command_sha256(executable: &str, arguments: &[String]) -> String {
    let mut hasher = Sha256::new();
    for field in std::iter::once(executable).chain(arguments.iter().map(String::as_str)) {
        hasher.update((field.len() as u64).to_le_bytes());
        hasher.update(field.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Hash one executable without loading it fully into memory.
fn sha256_file(path: &Path) -> Result<String, EvaluationError> {
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

/// Hash bytes as lowercase SHA-256.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Return current Unix time in milliseconds.
fn unix_millis() -> Result<u128, EvaluationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| EvaluationError::Policy(format!("clock predates Unix epoch: {error}")))
}

/// Return whether a string is an exact-length hexadecimal identifier.
fn is_hex_identifier(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Truncate an error by Unicode scalar count.
fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// Convert a failed invariant into a typed policy error.
fn require(condition: bool, message: &str) -> Result<(), EvaluationError> {
    if condition {
        Ok(())
    } else {
        Err(EvaluationError::Policy(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    /// The compiled manifest pins source, release artifacts, cache states, and counts.
    #[test]
    fn manifest_plan_is_closed() -> Result<(), EvaluationError> {
        let manifest = validate_manifest(MANIFEST_BYTES, Some(1))?;
        assert_eq!(manifest.corpora.len(), 3);
        assert_eq!(baseline_operations(&manifest).len(), 7);
        assert_eq!(registered_operations(&manifest).len(), 9);
        assert_eq!(manifest.experiment_design.warmups * 3 * 7, 63);
        assert_eq!(
            baseline_operations(&manifest)
                .iter()
                .map(|operation| operation.repetitions)
                .sum::<usize>()
                * 3,
            315
        );
        assert_eq!(manifest.experiment_design.warmups * 3 * 9, 81);
        assert_eq!(
            registered_operations(&manifest)
                .iter()
                .map(|operation| operation.repetitions)
                .sum::<usize>()
                * 3,
            405
        );
        assert_eq!(
            manifest.projectatlas.baseline_runtime_cargo_lock_sha256,
            "1e925a59e00fc8c0e82e925e64cf248e16fe08a37809b4923618342c382f073c"
        );
        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        let operations = changed["operations"]
            .as_array_mut()
            .ok_or_else(|| EvaluationError::Policy("operations fixture is not an array".into()))?;
        operations.pop();
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["projectatlas"]["cargo_lock_sha256"] = Value::String("0".repeat(64));
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["experiment_design"]["rng"]["algorithm"] =
            Value::String("unregistered-ordering".into());
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["experiment_design"]["rng"]["version"] = Value::String("0".into());
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["experiment_design"]["unregistered_field"] = json!(true);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["experiment_design"]["block_order"] = Value::String("host-width order".into());
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["experiment_design"]["minimum_valid_pairs"] = json!(9);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        let cold = changed["operations"]
            .as_array_mut()
            .and_then(|operations| {
                operations
                    .iter_mut()
                    .find(|operation| operation["id"] == "cold-full-scan")
            })
            .ok_or_else(|| EvaluationError::Policy("cold operation fixture is missing".into()))?;
        cold["repetitions"] = json!(29);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["decision_functions"]["superiority"]["cold_index_geometric_mean_ratio_upper"] =
            json!(0.95);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["decision_functions"]["superiority"]["agent_quality_point_estimate_difference_lower"] =
            json!(0.0);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["decision_functions"]["correctness"]["minimum_negative_examples_per_family"] =
            json!(0);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());

        let mut changed: Value = serde_json::from_slice(MANIFEST_BYTES)?;
        changed["projectatlas"]["baseline_release_artifacts"][0]["executable_sha256"] =
            Value::String("0".repeat(64));
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_ok());
        changed["projectatlas"]["baseline_release_artifacts"][0]["executable_bytes"] = json!(0);
        assert!(validate_manifest(&serde_json::to_vec(&changed)?, Some(1)).is_err());
        assert!(validate_manifest(MANIFEST_BYTES, Some(16)).is_err());
        Ok(())
    }

    /// Expected sample totals use checked arithmetic over runtime manifest dimensions.
    #[test]
    fn sample_counts_are_derived_exactly() -> Result<(), EvaluationError> {
        assert_eq!(expected_sample_counts(3, 2, &[4, 5])?, (12, 27));
        assert!(expected_sample_counts(0, 2, &[4, 5]).is_err());
        assert!(expected_sample_counts(3, 0, &[4, 5]).is_err());
        assert!(expected_sample_counts(3, 2, &[]).is_err());
        assert!(expected_sample_counts(usize::MAX, 2, &[1]).is_err());
        Ok(())
    }

    /// Refresh mutation is isolated and restored byte-for-byte.
    #[test]
    fn refresh_restores_only_the_copy() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let output = temp.path().join("output");
        fs::create_dir(&source)?;
        fs::create_dir(&output)?;
        let source = fs::canonicalize(source)?;
        let output = fs::canonicalize(output)?;
        require_disjoint(&source, &output)?;
        let source_file = source.join("lib.rs");
        let copy_file = output.join("lib.rs");
        fs::write(&source_file, b"pub fn value() -> u8 { 1 }\n")?;
        fs::copy(&source_file, &copy_file)?;
        let mut restore = FileRestore::mutate(&copy_file)?;
        assert_ne!(fs::read(&copy_file)?, fs::read(&source_file)?);
        restore.restore()?;
        assert_eq!(fs::read(&copy_file)?, fs::read(&source_file)?);
        assert!(require_disjoint(&source, &source.join("nested")).is_err());
        Ok(())
    }

    /// Malformed and vacuous exit-zero payloads cannot produce operation metrics.
    #[test]
    fn operation_output_validation_fails_closed() -> Result<(), EvaluationError> {
        let before = StorageBytes {
            database: 100,
            wal: 20,
            shm: 0,
            journal: 0,
            sidecars: 20,
        };
        let after = StorageBytes {
            database: 90,
            wal: 0,
            shm: 0,
            journal: 0,
            sidecars: 0,
        };
        assert_eq!(i128::from(after.total()) - i128::from(before.total()), -30);
        assert!(
            parse_operation_metrics(OperationId::ColdFullScan, b"{}", before, after, "session")
                .is_err()
        );
        assert!(
            parse_operation_metrics(
                OperationId::ColdFullScan,
                br#"{"overview":{"files":1},"symbols":{"parsed":1,"symbols":0,"relations":0}}"#,
                before,
                after,
                "session",
            )
            .is_err()
        );
        assert!(
            parse_operation_metrics(OperationId::GraphLookup, b"[]", before, after, "session")
                .is_err()
        );
        assert!(
            parse_operation_metrics(
                OperationId::GraphLookup,
                br#"[{"path":"src/lib.rs","source_name":"","target_name":"x","kind":"calls","line":1}]"#,
                before,
                after,
                "session",
            )
            .is_err()
        );
        let graph = br#"[{"path":"src/lib.rs","source_name":"run","target_name":"work","kind":"calls","line":1}]"#;
        assert!(
            parse_operation_metrics(OperationId::GraphLookup, graph, before, after, "session")
                .is_ok()
        );
        Ok(())
    }

    /// Child reports reject malformed JSON, schema drift, and inconsistent success state.
    #[test]
    fn architecture_child_output_validation_fails_closed() -> Result<(), EvaluationError> {
        let manifest = validate_manifest(MANIFEST_BYTES, Some(1))?;
        let expected_context = expected_architecture_sample_identity(
            &manifest.experiment_design.rng.seed_hex,
            "serde-json",
            ArchitectureOperationId::FtsDifferential,
            SampleKind::Measurement,
            0,
        );
        let metrics = architecture_metrics_fixture(
            &manifest.result_schema,
            ArchitectureOperationId::FtsDifferential,
            &expected_context,
            true,
        );
        let report = ArchitectureSampleReport {
            schema_version: ARCHITECTURE_SAMPLE_SCHEMA_VERSION,
            operation_id: ArchitectureOperationId::FtsDifferential,
            metrics: Some(metrics.clone()),
            error: None,
            success: true,
        };
        let bytes = serde_json::to_vec(&report)?;
        assert!(
            parse_architecture_sample_report(
                &bytes,
                ArchitectureOperationId::FtsDifferential,
                &expected_context,
                &manifest.result_schema,
            )
            .is_ok()
        );
        assert!(
            parse_architecture_sample_report(
                b"not-json",
                ArchitectureOperationId::FtsDifferential,
                &expected_context,
                &manifest.result_schema,
            )
            .is_err()
        );

        let mut missing_metric = metrics;
        missing_metric
            .as_object_mut()
            .ok_or_else(|| EvaluationError::Policy("metrics fixture is not an object".into()))?
            .remove("eligible");
        let missing_metric = ArchitectureSampleReport {
            metrics: Some(missing_metric),
            ..report
        };
        assert!(
            parse_architecture_sample_report(
                &serde_json::to_vec(&missing_metric)?,
                ArchitectureOperationId::FtsDifferential,
                &expected_context,
                &manifest.result_schema,
            )
            .is_err()
        );

        let mut unexpected_report = serde_json::to_value(&missing_metric)?;
        unexpected_report
            .as_object_mut()
            .ok_or_else(|| EvaluationError::Policy("report fixture is not an object".into()))?
            .insert("unexpected".into(), Value::Null);
        assert!(
            parse_architecture_sample_report(
                &serde_json::to_vec(&unexpected_report)?,
                ArchitectureOperationId::FtsDifferential,
                &expected_context,
                &manifest.result_schema,
            )
            .is_err()
        );
        let wrong_context = ArchitectureSampleIdentity {
            repetition: 1,
            ..expected_context.clone()
        };
        assert!(
            parse_architecture_sample_report(
                &bytes,
                ArchitectureOperationId::FtsDifferential,
                &wrong_context,
                &manifest.result_schema,
            )
            .is_err()
        );
        Ok(())
    }

    /// A supervised timeout remains failed even when stdout contains a valid success report.
    #[test]
    fn architecture_timeout_evidence_is_ineligible() -> Result<(), EvaluationError> {
        let manifest = validate_manifest(MANIFEST_BYTES, Some(1))?;
        let expected_context = expected_architecture_sample_identity(
            &manifest.experiment_design.rng.seed_hex,
            "serde-json",
            ArchitectureOperationId::FtsDifferential,
            SampleKind::Measurement,
            0,
        );
        let report = ArchitectureSampleReport {
            schema_version: ARCHITECTURE_SAMPLE_SCHEMA_VERSION,
            operation_id: ArchitectureOperationId::FtsDifferential,
            metrics: Some(architecture_metrics_fixture(
                &manifest.result_schema,
                ArchitectureOperationId::FtsDifferential,
                &expected_context,
                true,
            )),
            error: None,
            success: true,
        };
        let process = ProcessRun {
            evidence: json!({"timed_out": true}),
            stdout: serde_json::to_vec(&report)?,
            success: false,
        };
        let outcome = architecture_sample_outcome(
            &process,
            ArchitectureOperationId::FtsDifferential,
            &expected_context,
            &manifest.result_schema,
        );
        assert!(!outcome.success);
        assert!(outcome.metrics.is_some());
        assert!(outcome.error.is_some_and(|error| error.contains("timeout")));
        assert_eq!(process.evidence["timed_out"], true);
        Ok(())
    }

    /// Supervision and raw-stream records follow manifest-owned exact inventories.
    #[test]
    fn architecture_process_evidence_schema_is_closed() -> Result<(), EvaluationError> {
        let manifest = validate_manifest(MANIFEST_BYTES, Some(1))?;
        let raw_stream = Value::Object(
            manifest
                .result_schema
                .raw_stream_evidence
                .iter()
                .map(|field| (field.clone(), Value::Null))
                .collect(),
        );
        let mut process = manifest
            .result_schema
            .architecture_process_evidence
            .iter()
            .map(|field| (field.clone(), Value::Null))
            .collect::<serde_json::Map<_, _>>();
        process.insert("stdout".into(), raw_stream.clone());
        process.insert("stderr".into(), raw_stream);
        let mut process = Value::Object(process);
        validate_architecture_process_evidence(&process, &manifest.result_schema)?;
        process
            .as_object_mut()
            .ok_or_else(|| EvaluationError::Policy("process fixture is not an object".into()))?
            .remove("timed_out");
        assert!(validate_architecture_process_evidence(&process, &manifest.result_schema).is_err());
        Ok(())
    }

    /// MCP output requires exactly one successful response for every expected ID.
    #[test]
    fn mcp_response_reconciliation_is_exact() -> Result<(), EvaluationError> {
        let responses = valid_mcp_responses()?;
        let metrics = parse_mcp_metrics(&responses, "isolated-session")?;
        assert_eq!(metrics.responses_observed, 8);
        assert_eq!(metrics.calls_observed, 7);
        assert_eq!(metrics.tokens, 42);
        assert_eq!(metrics.likely_file_reads_avoided, 3);
        assert_eq!(metrics.errors, 0);

        let lines = responses
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let missing = lines[..lines.len().saturating_sub(1)].join(&b'\n');
        assert!(parse_mcp_metrics(&missing, "isolated-session").is_err());

        let mut duplicate = responses.clone();
        duplicate.extend_from_slice(lines.last().copied().unwrap_or_default());
        duplicate.push(b'\n');
        assert!(parse_mcp_metrics(&duplicate, "isolated-session").is_err());

        let top_level_error = replace_mcp_response(
            &responses,
            3,
            json!({"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"failed"}}),
        )?;
        assert!(parse_mcp_metrics(&top_level_error, "isolated-session").is_err());

        let tool_error = replace_mcp_response(
            &responses,
            4,
            json!({"jsonrpc":"2.0","id":4,"result":{"isError":true,"content":[{"type":"text","text":"failed"}]}}),
        )?;
        assert!(parse_mcp_metrics(&tool_error, "isolated-session").is_err());
        assert!(parse_mcp_metrics(b"not-json\n", "isolated-session").is_err());
        Ok(())
    }

    /// The fixed token-report call carries the exact per-sample session filter.
    #[test]
    fn mcp_token_report_is_session_isolated() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let selected_file = temp.path().join("lib.rs");
        fs::write(&selected_file, b"pub fn run() {}\n")?;
        let corpus = test_corpus(temp.path(), selected_file);
        let input = mcp_input(&corpus, "isolated-session")?;
        let token_call = input
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(serde_json::from_slice::<Value>)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|value| value["id"] == 8)
            .ok_or_else(|| EvaluationError::Policy("token call fixture is missing".into()))?;
        assert_eq!(
            token_call["params"]["arguments"]["session"],
            "isolated-session"
        );
        Ok(())
    }

    /// Unavailable host metrics and observed file-length deltas serialize distinctly.
    #[test]
    fn metric_availability_is_typed() -> Result<(), EvaluationError> {
        let unavailable = serde_json::to_value(MetricAvailability::<u64>::Unavailable {
            reason: "not observed",
        })?;
        let observed = serde_json::to_value(MetricAvailability::Observed {
            value: -30_i128,
            method: "signed retained length delta",
        })?;
        assert_eq!(unavailable["status"], "unavailable");
        assert_eq!(unavailable["reason"], "not observed");
        assert_eq!(observed["status"], "observed");
        assert_eq!(observed["value"], -30);
        Ok(())
    }

    /// No-clobber executable copies and raw evidence retain exact bytes and digests.
    #[test]
    fn immutable_copy_and_raw_evidence_are_exact() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source.bin");
        let copy = temp.path().join("copy.bin");
        let raw = temp.path().join("sample.stdout");
        fs::write(&source, b"release-bytes")?;
        copy_file_create_new(&source, &copy)?;
        assert_eq!(sha256_file(&source)?, sha256_file(&copy)?);
        assert!(copy_file_create_new(&source, &copy).is_err());
        let evidence = write_raw_create_new(&raw, b"raw-output")?;
        assert_eq!(evidence.bytes, 10);
        assert_eq!(evidence.sha256, sha256_hex(b"raw-output"));
        assert!(write_raw_create_new(&raw, b"replacement").is_err());
        Ok(())
    }

    /// Unix release copies retain runnable permissions and execute as real processes.
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn executable_copy_preserves_unix_mode_and_runs() -> Result<(), EvaluationError> {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source-runtime");
        let copy = temp.path().join("copied-runtime");
        fs::write(&source, b"#!/bin/sh\nprintf copied-runtime")?;
        let mut source_permissions = fs::metadata(&source)?.permissions();
        source_permissions.set_mode(0o750);
        fs::set_permissions(&source, source_permissions)?;

        copy_executable_create_new(&source, &copy)?;

        assert_eq!(fs::metadata(&copy)?.permissions().mode() & 0o777, 0o750);
        let output = run_supervised(Command::new(&copy), Duration::from_secs(5), 1024).await?;
        assert!(output.is_success());
        assert_eq!(output.stdout.retained, b"copied-runtime");
        Ok(())
    }

    /// The release verifier rejects real file digest and byte-length drift independently.
    #[test]
    fn release_executable_identity_mismatches_fail_closed() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let executable = temp.path().join("projectatlas-release.bin");
        fs::write(&executable, b"registered-release-bytes")?;
        let digest = sha256_file(&executable)?;
        let bytes = fs::metadata(&executable)?.len();
        let mut artifact = ReleaseArtifactSpec {
            target: current_target_triple().into(),
            executable_sha256: digest.clone(),
            executable_bytes: bytes,
            version: "projectatlas test".into(),
            build_profile: "release".into(),
            provenance: "unit-test fixture".into(),
        };

        assert_eq!(
            validate_release_executable(&executable, &artifact)?,
            (digest.clone(), bytes)
        );
        artifact.executable_sha256 = "0".repeat(64);
        assert!(validate_release_executable(&executable, &artifact).is_err());
        artifact.executable_sha256 = digest;
        artifact.executable_bytes = bytes.saturating_add(1);
        assert!(validate_release_executable(&executable, &artifact).is_err());
        Ok(())
    }

    /// Duplicate flags and unsafe run identifiers fail before filesystem access.
    #[test]
    fn invalid_arguments_fail_closed() {
        let valid = [
            "--manifest",
            "manifest.json",
            "--projectatlas",
            "projectatlas",
            "--source-root",
            "source",
            "--git",
            "git",
            "--corpora-root",
            "corpora",
            "--output-root",
            "output",
            "--run-id",
            "pilot",
            "--pilot-repetitions",
            "1",
        ];
        assert!(parse_arguments(valid.into_iter().map(OsString::from)).is_ok());
        let duplicate = [
            "--manifest",
            "one",
            "--manifest",
            "two",
            "--projectatlas",
            "projectatlas",
            "--source-root",
            "source",
            "--git",
            "git",
            "--corpora-root",
            "corpora",
            "--output-root",
            "output",
            "--run-id",
            "pilot",
        ];
        assert!(parse_arguments(duplicate.into_iter().map(OsString::from)).is_err());
        let unsafe_id = [
            "--manifest",
            "one",
            "--projectatlas",
            "projectatlas",
            "--source-root",
            "source",
            "--git",
            "git",
            "--corpora-root",
            "corpora",
            "--output-root",
            "output",
            "--run-id",
            "../escape",
        ];
        assert!(parse_arguments(unsafe_id.into_iter().map(OsString::from)).is_err());
    }

    /// The private child command accepts only its typed operation and exact flags.
    #[test]
    fn architecture_child_arguments_are_closed() -> Result<(), EvaluationError> {
        let valid = [
            ARCHITECTURE_SAMPLE_COMMAND,
            "--manifest",
            "manifest.json",
            "--operation",
            "fts-differential",
            "--corpus-id",
            "serde-json",
            "--sample-kind",
            "measurement",
            "--repetition",
            "0",
            "--source-db",
            "source.db",
            "--work-directory",
            "serde-json-fts-differential-measurement-0",
        ];
        let invocation = parse_invocation(valid.into_iter().map(OsString::from))?;
        let RunnerInvocation::ArchitectureSample(arguments) = invocation else {
            return Err(EvaluationError::Policy(
                "private command parsed as a normal campaign".into(),
            ));
        };
        assert_eq!(
            arguments.operation,
            ArchitectureOperationId::FtsDifferential
        );
        assert_eq!(arguments.sample_kind, SampleKind::Measurement);
        assert_eq!(arguments.repetition, 0);

        let mut invalid_operation = valid;
        invalid_operation[4] = "cold-full-scan";
        assert!(parse_invocation(invalid_operation.into_iter().map(OsString::from)).is_err());
        let mut invalid_repetition = valid;
        invalid_repetition[10] = "not-a-number";
        assert!(parse_invocation(invalid_repetition.into_iter().map(OsString::from)).is_err());
        let mut unknown_flag = valid;
        unknown_flag[13] = "--unknown";
        assert!(parse_invocation(unknown_flag.into_iter().map(OsString::from)).is_err());
        Ok(())
    }

    /// Child filesystem inputs must match one registered sample and unused output path.
    #[test]
    fn architecture_child_inputs_bind_manifest_and_work_identity() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let manifest_path = temp.path().join("manifest.json");
        let source_db = temp.path().join("source.db");
        let work_root = temp.path().join("work");
        fs::write(&manifest_path, MANIFEST_BYTES)?;
        fs::write(&source_db, b"sqlite-fixture")?;
        fs::create_dir(&work_root)?;
        let expected_name = architecture_work_directory_name(
            "serde-json",
            ArchitectureOperationId::FtsDifferential,
            SampleKind::Measurement,
            0,
        );
        let mut arguments = ArchitectureSampleArguments {
            manifest: manifest_path,
            operation: ArchitectureOperationId::FtsDifferential,
            corpus_id: "serde-json".into(),
            sample_kind: SampleKind::Measurement,
            repetition: 0,
            source_db: source_db.clone(),
            work_directory: work_root.join(&expected_name),
        };
        let validated = validate_architecture_sample(&arguments)?;
        assert_eq!(validated.source_db, fs::canonicalize(&source_db)?);
        assert_eq!(
            validated.work_directory,
            fs::canonicalize(&work_root)?.join(&expected_name)
        );

        arguments.work_directory = work_root.join("wrong-sample");
        assert!(validate_architecture_sample(&arguments).is_err());
        arguments.work_directory = work_root.join(&expected_name);
        fs::create_dir(&arguments.work_directory)?;
        assert!(validate_architecture_sample(&arguments).is_err());
        Ok(())
    }

    /// Git tree accounting preserves modes, file counts, and logical bytes.
    #[test]
    fn git_tree_accounting_is_exact() -> Result<(), EvaluationError> {
        let rows = b"100644 blob abc 10\tCargo.toml\0 100755 blob def 7\tscript.sh\0";
        let (files, bytes, modes) = parse_ls_tree(rows)?;
        assert_eq!(files, 2);
        assert_eq!(bytes, 17);
        assert_eq!(modes["100644"], 1);
        assert_eq!(modes["100755"], 1);
        Ok(())
    }

    /// Actual tracked bytes change under mutation and return after exact restoration.
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_detects_and_clears_mutation()
    -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        git_checked(&git, temp.path(), &["init".into()], &environment).await?;
        fs::write(temp.path().join("lib.rs"), b"pub fn value() -> u8 { 1 }\n")?;
        git_checked(
            &git,
            temp.path(),
            &["add".into(), "lib.rs".into()],
            &environment,
        )
        .await?;
        git_checked(
            &git,
            temp.path(),
            &[
                "-c".into(),
                "user.name=ProjectAtlas Test".into(),
                "-c".into(),
                "user.email=projectatlas@example.invalid".into(),
                "commit".into(),
                "-m".into(),
                "fixture".into(),
            ],
            &environment,
        )
        .await?;
        let initial = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        let mut restore = FileRestore::mutate(&temp.path().join("lib.rs"))?;
        let changed = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        assert_ne!(initial, changed);
        assert!(
            checkout_identity(&git, temp.path(), &environment)
                .await
                .is_err()
        );
        restore.restore()?;
        assert_eq!(
            initial,
            materialized_checkout_sha256(&git, temp.path(), &environment).await?
        );
        assert!(
            checkout_identity(&git, temp.path(), &environment)
                .await
                .is_ok()
        );
        Ok(())
    }

    /// Materialized hashing rejects files or aggregate content beyond explicit ceilings.
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_enforces_read_limits() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        git_checked(&git, temp.path(), &["init".into()], &environment).await?;
        fs::write(temp.path().join("first.bin"), b"1234")?;
        fs::write(temp.path().join("second.bin"), b"5678")?;
        git_checked(
            &git,
            temp.path(),
            &["add".into(), "first.bin".into(), "second.bin".into()],
            &environment,
        )
        .await?;

        let per_file = materialized_checkout_sha256_with_limits(
            &git,
            temp.path(),
            &environment,
            MaterializedReadLimits {
                per_file_bytes: 3,
                aggregate_bytes: 16,
            },
        )
        .await;
        assert!(matches!(
            per_file,
            Err(EvaluationError::Policy(ref message)) if message.contains("per-file")
        ));

        let aggregate = materialized_checkout_sha256_with_limits(
            &git,
            temp.path(),
            &environment,
            MaterializedReadLimits {
                per_file_bytes: 4,
                aggregate_bytes: 7,
            },
        )
        .await;
        assert!(matches!(
            aggregate,
            Err(EvaluationError::Policy(ref message)) if message.contains("aggregate")
        ));
        Ok(())
    }

    /// A tracked file cannot be read through a linked or reparse-point parent.
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_rejects_linked_parent() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let repository = temp.path().join("repository");
        let outside = temp.path().join("outside");
        let control = temp.path().join("control");
        fs::create_dir(&repository)?;
        fs::create_dir(&outside)?;
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        git_checked(&git, &repository, &["init".into()], &environment).await?;
        fs::create_dir(repository.join("nested"))?;
        fs::write(repository.join("nested/tracked.bin"), b"inside")?;
        fs::write(outside.join("tracked.bin"), b"outside")?;
        git_checked(
            &git,
            &repository,
            &["add".into(), "nested/tracked.bin".into()],
            &environment,
        )
        .await?;
        fs::remove_dir_all(repository.join("nested"))?;
        create_directory_link(&outside, &repository.join("nested"))?;

        let result = materialized_checkout_sha256(&git, &repository, &environment).await;
        assert!(matches!(
            result,
            Err(EvaluationError::Policy(ref message))
                if message.contains("linked") || message.contains("reparse")
        ));
        Ok(())
    }

    /// The repository root itself cannot be supplied through a symlink or junction.
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_rejects_linked_root() -> Result<(), EvaluationError> {
        let temp = tempfile::tempdir()?;
        let repository = temp.path().join("repository");
        let linked_root = temp.path().join("linked-repository");
        let control = temp.path().join("control");
        fs::create_dir(&repository)?;
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        git_checked(&git, &repository, &["init".into()], &environment).await?;
        fs::write(repository.join("tracked.bin"), b"inside")?;
        git_checked(
            &git,
            &repository,
            &["add".into(), "tracked.bin".into()],
            &environment,
        )
        .await?;
        create_directory_link(&repository, &linked_root)?;

        let result = materialized_checkout_sha256(&git, &linked_root, &environment).await;
        assert!(matches!(
            result,
            Err(EvaluationError::Policy(ref message))
                if message.contains("linked") || message.contains("reparse")
        ));
        Ok(())
    }

    /// Unix symlink targets are hashed as native bytes rather than forced through UTF-8.
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_preserves_non_utf8_symlink_target()
    -> Result<(), EvaluationError> {
        use std::os::unix::ffi::OsStringExt as _;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        git_checked(&git, temp.path(), &["init".into()], &environment).await?;
        let link = temp.path().join("tracked-link");
        symlink(OsString::from_vec(b"target-\xff".to_vec()), &link)?;
        git_checked(
            &git,
            temp.path(),
            &["add".into(), "tracked-link".into()],
            &environment,
        )
        .await?;
        let first = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        fs::remove_file(&link)?;
        symlink(OsString::from_vec(b"target-\xfe".to_vec()), &link)?;
        let second = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        require(
            first != second,
            "distinct native symlink-target bytes produced the same materialized digest",
        )?;
        Ok(())
    }

    /// Unix tracked filenames remain byte-preserving throughout materialized hashing.
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn materialized_checkout_digest_preserves_non_utf8_filename()
    -> Result<(), EvaluationError> {
        use std::os::unix::ffi::OsStringExt as _;

        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        run_git_fixture_command(
            &git,
            temp.path(),
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;
        let filename = OsString::from_vec(b"tracked-\xff.bin".to_vec());
        let path = temp.path().join(&filename);
        fs::write(&path, b"first")?;
        run_git_fixture_command(&git, temp.path(), &[OsString::from("add"), filename])?;
        let first = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        fs::write(path, b"second")?;
        let second = materialized_checkout_sha256(&git, temp.path(), &environment).await?;
        require(
            first != second,
            "distinct bytes under a non-UTF-8 filename produced the same materialized digest",
        )?;
        Ok(())
    }

    /// Successful Git processes cannot return evidence beyond the capture ceiling.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_reject_truncated_output() -> Result<(), EvaluationError> {
        let directory = tempfile::tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let control = root.join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        run_git_fixture_command(
            &git,
            &root,
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;
        fs::write(
            root.join("oversized.bin"),
            vec![b'x'; OUTPUT_LIMIT_BYTES + 1],
        )?;
        let oid = run_git_fixture_command(
            &git,
            &root,
            &[
                OsString::from("hash-object"),
                OsString::from("-w"),
                OsString::from("--"),
                OsString::from("oversized.bin"),
            ],
        )?;
        let oid = std::str::from_utf8(&oid)?.trim();
        let result = git_output_bytes(&git, &root, &["cat-file", "blob", oid], &environment).await;
        require(
            matches!(result, Err(EvaluationError::Policy(ref message)) if message.contains("truncated")),
            "oversized Git output did not fail closed",
        )
    }

    /// Corpus-local fsmonitor configuration cannot execute during evaluator Git reads.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_disable_repository_fsmonitor() -> Result<(), EvaluationError> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        let control = root.join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        run_git_fixture_command(
            &git,
            root,
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;
        fs::write(root.join("tracked.txt"), b"fixture\n")?;
        run_git_fixture_command(
            &git,
            root,
            &[OsString::from("add"), OsString::from("tracked.txt")],
        )?;
        run_git_fixture_command(
            &git,
            root,
            &[
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )?;

        let marker = root.join("fsmonitor-invoked");
        #[cfg(windows)]
        let hook = format!("cmd.exe /D /C echo invoked^>\"{}\"", marker.display());
        #[cfg(unix)]
        let hook = {
            use std::os::unix::fs::PermissionsExt as _;
            let script = root.join("fsmonitor-hook.sh");
            fs::write(
                &script,
                format!("#!/bin/sh\nprintf invoked > '{}'\n", marker.display()),
            )?;
            let mut permissions = fs::metadata(&script)?.permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&script, permissions)?;
            path_text(&script)?
        };
        run_git_fixture_command(
            &git,
            root,
            &[
                OsString::from("config"),
                OsString::from("core.fsmonitor"),
                OsString::from(hook),
            ],
        )?;
        run_git_fixture_command(
            &git,
            root,
            &[OsString::from("status"), OsString::from("--porcelain=v1")],
        )?;
        require(marker.is_file(), "fsmonitor fixture was not executable")?;
        fs::remove_file(&marker)?;

        let _status = git_worktree_status(&git, root, &environment).await?;
        require(
            !marker.exists(),
            "repository fsmonitor executed during evaluator Git read",
        )
    }

    /// Repository-local `core.worktree` cannot redirect evaluator reads outside the bound root.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_pin_the_canonical_worktree() -> Result<(), EvaluationError> {
        let directory = tempfile::tempdir()?;
        let repository = directory.path().join("intended repository");
        let outside = directory.path().join("outside worktree");
        fs::create_dir(&repository)?;
        fs::create_dir(&outside)?;
        let repository = fs::canonicalize(repository)?;
        let outside = fs::canonicalize(outside)?;
        let control = directory.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;

        run_git_fixture_command(
            &git,
            &repository,
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;
        fs::write(repository.join("tracked.txt"), b"intended\n")?;
        run_git_fixture_command(
            &git,
            &repository,
            &[
                OsString::from("add"),
                OsString::from("--"),
                OsString::from("tracked.txt"),
            ],
        )?;
        run_git_fixture_command(
            &git,
            &repository,
            &[
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )?;
        run_git_fixture_command(
            &git,
            &repository,
            &[
                OsString::from("config"),
                OsString::from("core.worktree"),
                outside.as_os_str().to_owned(),
            ],
        )?;
        fs::write(outside.join("outside-marker.txt"), b"outside\n")?;

        let unsanitized = run_git_fixture_command(
            &git,
            &repository,
            &[
                OsString::from("status"),
                OsString::from("--porcelain=v1"),
                OsString::from("--untracked-files=all"),
            ],
        )?;
        require(
            String::from_utf8_lossy(&unsanitized).contains("outside-marker.txt"),
            "hostile core.worktree fixture did not redirect unsanitized Git",
        )?;

        let status = git_worktree_status(&git, &repository, &environment).await?;
        require(
            status.is_empty(),
            "closed evaluator Git read escaped its canonical worktree",
        )
    }

    /// A repository-selected clean filter is never executed by evaluator identity reads.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_never_execute_clean_filters() -> Result<(), EvaluationError> {
        assert_executable_filter_is_never_run("clean").await
    }

    /// A repository-selected process filter is never executed by evaluator identity reads.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_never_execute_process_filters() -> Result<(), EvaluationError> {
        assert_executable_filter_is_never_run("process").await
    }

    /// Declared CRLF materialization remains clean without enabling filter drivers.
    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_git_reads_accept_declared_crlf_materialization()
    -> Result<(), EvaluationError> {
        let directory = tempfile::tempdir()?;
        let repository = directory.path().join("repository");
        let control = directory.path().join("control");
        fs::create_dir(&repository)?;
        fs::create_dir(&control)?;
        let repository = fs::canonicalize(repository)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        run_git_fixture_command(
            &git,
            &repository,
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;
        fs::write(repository.join(".gitattributes"), b"*.ps1 text eol=crlf\n")?;
        fs::write(repository.join("script.ps1"), b"Write-Output 'clean'\n")?;
        run_git_fixture_command(
            &git,
            &repository,
            &[OsString::from("add"), OsString::from("--all")],
        )?;
        run_git_fixture_command(
            &git,
            &repository,
            &[
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )?;
        fs::write(repository.join("script.ps1"), b"Write-Output 'clean'\r\n")?;

        require(
            git_worktree_status(&git, &repository, &environment)
                .await?
                .is_empty(),
            "declared CRLF materialization was not recognized as Git-clean",
        )
    }

    /// Current operations never claim a warmed process or controlled cold host cache.
    #[test]
    fn cache_state_labels_match_new_process_execution() -> Result<(), EvaluationError> {
        let manifest = validate_manifest(MANIFEST_BYTES, Some(1))?;
        for operation in baseline_operations(&manifest) {
            let evidence = cache_state_evidence(operation.id);
            assert_eq!(evidence.process, PROCESS_STATE_NEW);
            assert_eq!(evidence.sqlite_connection, SQLITE_CONNECTION_STATE_NEW);
            assert!(!evidence.warm_process_claim_eligible);
            assert!(!evidence.cold_cache_claim_eligible);
            assert_eq!(
                operation.cache_state,
                expected_baseline(operation.id)
                    .ok_or_else(|| EvaluationError::Policy("cache policy is missing".into()))?
                    .0
            );
        }
        for operation in registered_operations(&manifest)
            .into_iter()
            .filter(|operation| {
                matches!(
                    operation.id,
                    OperationId::FtsDifferential | OperationId::SqliteStrategy
                )
            })
        {
            let evidence = cache_state_evidence(operation.id);
            assert_eq!(evidence.process, PROCESS_STATE_SUPERVISED_CHILD);
            assert_eq!(
                operation.cache_state,
                CACHE_CURRENT_INDEX_SUPERVISED_ARCHITECTURE_CHILD
            );
            assert_eq!(
                evidence.sqlite_connection,
                SQLITE_CONNECTION_STATE_PER_SAMPLE
            );
            assert!(!evidence.warm_process_claim_eligible);
        }
        Ok(())
    }

    /// Build a field-complete architecture metric object for protocol tests.
    fn architecture_metrics_fixture(
        result_schema: &ArchitectureResultSchema,
        operation: ArchitectureOperationId,
        sample_context: &ArchitectureSampleIdentity,
        eligible: bool,
    ) -> Value {
        let fields = match operation {
            ArchitectureOperationId::FtsDifferential => &result_schema.fts_result_metrics,
            ArchitectureOperationId::SqliteStrategy => {
                &result_schema.sqlite_strategy_result_metrics
            }
        };
        let mut object = fields
            .iter()
            .map(|field| (field.clone(), Value::Null))
            .collect::<serde_json::Map<_, _>>();
        object.insert("result_kind".into(), operation.result_kind().into());
        object.insert("eligible".into(), eligible.into());
        object.insert(
            "sample_context".into(),
            serde_json::to_value(sample_context)
                .expect("sample-context fixture serialization must succeed"),
        );
        Value::Object(object)
    }

    /// Build a complete successful MCP response stream.
    fn valid_mcp_responses() -> Result<Vec<u8>, EvaluationError> {
        let mut rows = vec![json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"ProjectAtlas","version":"1"}}
        })];
        for id in 2..8 {
            rows.push(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"content":[{"type":"text","text":"ok"}]}
            }));
        }
        rows.push(json!({
            "jsonrpc": "2.0",
            "id": 8,
            "result": {"content":[{"type":"text","text":"token_savings:\n  estimated_with_projectatlas: 42\nread_avoidance:\n  likely_file_reads_avoided: 3\n"}]}
        }));
        let mut bytes = rows
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            .into_bytes();
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Replace one response by ID while preserving the complete stream.
    fn replace_mcp_response(
        bytes: &[u8],
        id: u64,
        replacement: Value,
    ) -> Result<Vec<u8>, EvaluationError> {
        let mut rows = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(serde_json::from_slice::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let row = rows
            .iter_mut()
            .find(|row| row["id"] == id)
            .ok_or_else(|| EvaluationError::Policy("MCP replacement ID is missing".into()))?;
        *row = replacement;
        let mut output = rows
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            .into_bytes();
        output.push(b'\n');
        Ok(output)
    }

    /// Build the minimum corpus needed by the MCP input serializer.
    fn test_corpus(checkout: &Path, selected_file: PathBuf) -> CorpusRuntime {
        CorpusRuntime {
            spec: CorpusSpec {
                id: "fixture".into(),
                stratum: "small".into(),
                commit: "0".repeat(40),
                tree: "0".repeat(40),
                clean_required: true,
                submodules_allowed: false,
                lfs_allowed: false,
                tracked_files: 1,
                tracked_logical_bytes: 1,
                git_modes: BTreeMap::from([("100644".into(), 1)]),
                materialization_state: "verified".into(),
            },
            evidence: Value::Null,
            initial_identity: CorpusIdentity {
                commit: "0".repeat(40),
                tree: "0".repeat(40),
                tracked_files: 1,
                tracked_logical_bytes: 1,
                git_modes: BTreeMap::from([("100644".into(), 1)]),
            },
            initial_materialized_sha256: "0".repeat(64),
            checkout: checkout.to_owned(),
            selected_file,
            db: checkout.join("fixture.db"),
            seed_db: checkout.join("fixture.seed.db"),
        }
    }

    /// Resolve the Git binary used by the current test host.
    fn test_git_executable() -> Result<PathBuf, EvaluationError> {
        let output = if cfg!(windows) {
            StdCommand::new("where.exe").arg("git.exe").output()?
        } else {
            StdCommand::new("sh")
                .args(["-c", "command -v git"])
                .output()?
        };
        require(output.status.success(), "Git executable is unavailable")?;
        let first = std::str::from_utf8(&output.stdout)?
            .lines()
            .next()
            .ok_or_else(|| EvaluationError::Policy("Git lookup returned no path".into()))?;
        Ok(PathBuf::from(first.trim()))
    }

    /// Run one Git process while constructing a deliberately hostile test repository.
    fn run_git_fixture_process(
        git: &Path,
        root: &Path,
        arguments: &[OsString],
    ) -> Result<std::process::Output, EvaluationError> {
        let executable_directory = git.parent().ok_or_else(|| {
            EvaluationError::Policy("Git fixture executable has no parent directory".into())
        })?;
        let mut command = StdCommand::new(git);
        command
            .arg("-C")
            .arg(root)
            .args(arguments)
            .env_clear()
            .env("PATH", executable_directory)
            .env("GIT_CONFIG_GLOBAL", git_null_device())
            .env("GIT_CONFIG_SYSTEM", git_null_device())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(windows)]
        for name in ["SYSTEMROOT", "WINDIR"] {
            if let Some(value) = env::var_os(name) {
                command.env(name, value);
            }
        }
        command.output().map_err(Into::into)
    }

    /// Require one fixture-construction Git command to succeed.
    fn run_git_fixture_command(
        git: &Path,
        root: &Path,
        arguments: &[OsString],
    ) -> Result<Vec<u8>, EvaluationError> {
        let output = run_git_fixture_process(git, root, arguments)?;
        require(
            output.status.success(),
            &format!(
                "Git fixture command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        Ok(output.stdout)
    }

    /// Create a directory symlink for an adversarial POSIX worktree fixture.
    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> Result<(), EvaluationError> {
        std::os::unix::fs::symlink(target, link).map_err(Into::into)
    }

    /// Create a junction without requiring Windows symbolic-link privileges.
    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> Result<(), EvaluationError> {
        let output = StdCommand::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()?;
        require(
            output.status.success(),
            &format!(
                "failed to create junction fixture: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    /// Build a Windows filter command that proves execution through one marker file.
    #[cfg(windows)]
    fn hostile_filter_command(
        _root: &Path,
        marker: &Path,
        _filter_kind: &str,
    ) -> Result<String, EvaluationError> {
        let marker = path_text(marker)?.replace('\\', "/");
        Ok(format!("cmd.exe /D /C echo invoked^>\"{marker}\""))
    }

    /// Build a POSIX filter command that proves execution through one marker file.
    #[cfg(unix)]
    fn hostile_filter_command(
        root: &Path,
        marker: &Path,
        filter_kind: &str,
    ) -> Result<String, EvaluationError> {
        use std::os::unix::fs::PermissionsExt as _;

        let script = root.join(format!("hostile-{filter_kind}-filter.sh"));
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf invoked > '{}'\n", marker.display()),
        )?;
        let mut permissions = fs::metadata(&script)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions)?;
        let script = path_text(&script)?.replace('\'', "'\"'\"'");
        Ok(format!("'{script}'"))
    }

    /// Prove ordinary Git reaches a hostile filter while evaluator reads never execute it.
    async fn assert_executable_filter_is_never_run(
        filter_kind: &str,
    ) -> Result<(), EvaluationError> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("repository with filters");
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        let control = directory.path().join("control");
        fs::create_dir(&control)?;
        let environment = controlled_environment(&control)?;
        let git = test_git_executable()?;
        run_git_fixture_command(
            &git,
            &root,
            &[OsString::from("init"), OsString::from("--quiet")],
        )?;

        let driver = format!("hostile-{filter_kind}");
        fs::write(
            root.join(".gitattributes"),
            format!("payload.txt filter={driver}\n"),
        )?;
        fs::write(root.join("payload.txt"), b"alpha\n")?;
        run_git_fixture_command(
            &git,
            &root,
            &[OsString::from("add"), OsString::from("--all")],
        )?;
        run_git_fixture_command(
            &git,
            &root,
            &[
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )?;

        let marker = root.join(format!("{filter_kind}-filter-invoked"));
        let command = hostile_filter_command(&root, &marker, filter_kind)?;
        for (key, value) in [
            (format!("filter.{driver}.{filter_kind}"), command),
            (format!("filter.{driver}.required"), "true".into()),
        ] {
            run_git_fixture_command(
                &git,
                &root,
                &[
                    OsString::from("config"),
                    OsString::from(key),
                    OsString::from(value),
                ],
            )?;
        }
        fs::write(root.join("payload.txt"), b"bravo\n")?;

        let _unsanitized = run_git_fixture_process(
            &git,
            &root,
            &[OsString::from("status"), OsString::from("--porcelain=v1")],
        )?;
        require(
            marker.is_file(),
            "hostile filter fixture was not executable through ordinary Git",
        )?;
        fs::remove_file(&marker)?;

        let status = git_worktree_status(&git, &root, &environment).await;
        require(
            matches!(
                status,
                Err(EvaluationError::Policy(ref message))
                    if message.contains("custom Git filter")
            ),
            "custom-filter-dependent repository remained evaluation-eligible",
        )?;
        require(
            !marker.exists(),
            "sanitized evaluator comparison executed a repository filter",
        )
    }
}
