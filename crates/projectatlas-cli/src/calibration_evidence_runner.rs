//! Test-only calibration runner with one supervised tree and one evidence journal.

use super::bounded_process_supervisor::{
    CapturedStream, SupervisedCommandOutput, SupervisionError, run_supervised,
};
#[cfg(test)]
use crate::git_process_policy::git_null_device;
use crate::git_process_policy::{RepositoryGitError, RepositoryGitProbe};
use processkit::Command;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use thiserror::Error;

/// Frozen evaluation manifest compiled into the dedicated runner.
const MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
/// Digest of the frozen evaluation manifest.
const MANIFEST_SHA256: &str = "35ed0bbb3560d0b68657309f4117fd61edc6471020a1a462364d71f5b4e018d8";
/// Calibration runner source compiled into the executable.
const RUNNER_BYTES: &[u8] = include_bytes!("calibration_evidence_runner.rs");
/// Repository-relative path of the dedicated runner source.
const RUNNER_SOURCE_PATH: &str = "crates/projectatlas-cli/src/calibration_evidence_runner.rs";
/// Stable example name built for official calibration runs.
const RUNNER_EXAMPLE_NAME: &str = "calibration-evidence-runner";
/// Complete execution-tree timeout.
const TREE_TIMEOUT_SECONDS: u64 = 5_400;
/// Deadline applied independently to every workload attempt.
const WORKLOAD_TIMEOUT_SECONDS: u64 = 120;
/// Retained byte ceiling for each execution-tree stream.
const STREAM_LIMIT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum JSON control or lifecycle record size.
const CONTROL_FILE_LIMIT: u64 = 1024 * 1024;
/// Maximum aggregate size.
const AGGREGATE_FILE_LIMIT: u64 = 16 * 1024 * 1024;
/// Maximum retained failure diagnostic.
const FAILURE_DIAGNOSTIC_BYTES: usize = 4 * 1024;
/// Warmup attempts for each calibration workload.
const WARMUPS: usize = 3;
/// Measured attempts for each calibration workload.
const REPETITIONS: usize = 15;
/// Deterministic BLAKE3 bytes processed by each attempt.
const BLAKE3_BUFFER_BYTES: usize = 64 * 1024 * 1024;
/// Deterministic BLAKE3 buffers processed serially.
const BLAKE3_BUFFER_COUNT: usize = 8;
/// Rows inserted by each `SQLite` attempt.
const SQLITE_ROW_COUNT: usize = 100_000;
/// Environment names allowed to cross the cleared process boundary.
const ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "CARGO_HOME",
    "COMSPEC",
    "DEVELOPER_DIR",
    "HOME",
    "INCLUDE",
    "LIB",
    "LIBPATH",
    "MACOSX_DEPLOYMENT_TARGET",
    "PATH",
    "PATHEXT",
    "Platform",
    "PROCESSOR_ARCHITECTURE",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "SDKROOT",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "UniversalCRTSdkDir",
    "UCRTVersion",
    "VCINSTALLDIR",
    "VCToolsInstallDir",
    "WindowsSdkBinPath",
    "WindowsSdkDir",
    "WindowsSDKVersion",
    "WINDIR",
];
/// Deterministic environment values forced for every child.
const FORCED_ENVIRONMENT: &[(&str, &str)] = &[("RUST_BACKTRACE", "0")];
/// Windows attributes bit identifying a reparse point.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Start record filename.
const START_FILE: &str = "start.json";
/// Supervised process metadata filename.
const PROCESS_FILE: &str = "process.json";
/// Bounded standard output filename.
const STDOUT_FILE: &str = "process.stdout";
/// Bounded standard error filename.
const STDERR_FILE: &str = "process.stderr";
/// Failure record filename.
const FAILURE_FILE: &str = "failure.json";
/// Completion record filename.
const COMPLETION_FILE: &str = "completion.json";
/// Closed runner subcommands.
const RUN_COMMAND: &str = "run";
/// Execute-only child subcommand.
const EXECUTE_COMMAND: &str = "execute";
/// Single-workload child subcommand.
const WORKLOAD_COMMAND: &str = "workload";
/// Closed runner option names.
const MANIFEST_OPTION: &str = "--manifest";
/// Before/after position option.
const POSITION_OPTION: &str = "--position";
/// Caller-selected run identifier option.
const RUN_ID_OPTION: &str = "--run-id";
/// Aggregate destination option.
const AGGREGATE_OPTION: &str = "--aggregate";
/// Raw-attempt destination option.
const RAW_ATTEMPTS_OPTION: &str = "--raw-attempts";
/// Workload kind option.
const KIND_OPTION: &str = "--kind";
/// Warmup/measured phase option.
const PHASE_OPTION: &str = "--phase";
/// Phase-local repetition option.
const REPETITION_OPTION: &str = "--repetition";
/// Single-use workload result option.
const RESULT_OPTION: &str = "--result";
/// Exact options accepted by the public run command.
const RUN_OPTIONS: &[&str] = &[MANIFEST_OPTION, POSITION_OPTION, RUN_ID_OPTION];
/// Exact options accepted by the internal execution command.
const EXECUTE_OPTIONS: &[&str] = &[
    AGGREGATE_OPTION,
    MANIFEST_OPTION,
    POSITION_OPTION,
    RAW_ATTEMPTS_OPTION,
    RUN_ID_OPTION,
];
/// Exact options accepted by the internal workload command.
const WORKLOAD_OPTIONS: &[&str] = &[KIND_OPTION, PHASE_OPTION, REPETITION_OPTION, RESULT_OPTION];

/// Errors from policy validation, evidence ownership, workload execution, or supervision.
#[derive(Debug, Error)]
pub(super) enum CalibrationError {
    /// Command-line arguments did not match a closed runner subcommand.
    #[error("invalid calibration arguments: {0}")]
    Arguments(String),
    /// The frozen evaluation policy did not match the executable contract.
    #[error("calibration policy rejected: {0}")]
    Policy(String),
    /// A runtime provenance or lifecycle binding drifted.
    #[error("calibration binding rejected: {0}")]
    Binding(String),
    /// One workload failed its deterministic contract.
    #[error("calibration workload failed: {0}")]
    Workload(String),
    /// Evidence retention failed after an earlier calibration error.
    #[error("{original}; failure evidence could not be retained: {marker}")]
    FailureMarker {
        /// Original calibration failure.
        original: Box<CalibrationError>,
        /// Secondary failure while retaining the marker.
        marker: Box<CalibrationError>,
    },
    /// Filesystem or child-process I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// `SQLite` setup or execution failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Native process-tree supervision failed.
    #[error(transparent)]
    Supervision(#[from] SupervisionError),
    /// A native `processkit` source probe failed.
    #[error(transparent)]
    Process(#[from] processkit::Error),
}

impl From<RepositoryGitError> for CalibrationError {
    fn from(error: RepositoryGitError) -> Self {
        match error {
            RepositoryGitError::Policy(message) => Self::Binding(message),
            RepositoryGitError::Io(source) => Self::Io(source),
            RepositoryGitError::Supervision(source) => Self::Supervision(source),
        }
    }
}

/// Before/after calibration position around one benchmark block.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RunPosition {
    /// Calibration captured before benchmark measurements.
    Before,
    /// Calibration captured after benchmark measurements.
    After,
}

impl RunPosition {
    /// Parse the closed position vocabulary.
    fn parse(value: &str) -> Result<Self, CalibrationError> {
        match value {
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            other => Err(CalibrationError::Arguments(format!(
                "unknown calibration position `{other}`"
            ))),
        }
    }

    /// Return the stable manifest key.
    const fn id(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

/// Warmup or measured attempt classification.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SamplePhase {
    /// Unmeasured cache and allocator warmup.
    Warmup,
    /// Attempt included in the retained median.
    Measured,
}

impl SamplePhase {
    /// Parse the closed phase vocabulary.
    fn parse(value: &str) -> Result<Self, CalibrationError> {
        match value {
            "warmup" => Ok(Self::Warmup),
            "measured" => Ok(Self::Measured),
            other => Err(CalibrationError::Arguments(format!(
                "unknown calibration phase `{other}`"
            ))),
        }
    }

    /// Return the stable command value.
    const fn id(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measured => "measured",
        }
    }
}

/// Closed calibration workload set.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WorkloadKind {
    /// Serial BLAKE3 throughput workload.
    Blake3,
    /// Prepared-transaction `SQLite` workload.
    Sqlite,
}

impl WorkloadKind {
    /// Parse one internal workload subcommand value.
    fn parse(value: &str) -> Result<Self, CalibrationError> {
        match value {
            "blake3" => Ok(Self::Blake3),
            "sqlite" => Ok(Self::Sqlite),
            other => Err(CalibrationError::Arguments(format!(
                "unknown calibration workload `{other}`"
            ))),
        }
    }

    /// Return the internal command value.
    const fn command_id(self) -> &'static str {
        match self {
            Self::Blake3 => "blake3",
            Self::Sqlite => "sqlite",
        }
    }

    /// Return the manifest workload identifier.
    const fn manifest_id(self) -> &'static str {
        match self {
            Self::Blake3 => "blake3-512mib",
            Self::Sqlite => "sqlite-100k-batch",
        }
    }
}

/// Typed ownership scope for retained provenance digests.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProvenanceScope {
    /// Clean committed repository state.
    Source,
    /// Exact Cargo lockfile bytes.
    CargoLock,
    /// Frozen evaluation manifest bytes.
    Manifest,
    /// Dedicated runner executable bytes.
    Executable,
    /// Exact executable and argument tuple.
    Command,
    /// Controlled environment name/presence/value digests.
    Environment,
    /// Complete supervised execution-tree record.
    Process,
    /// Raw JSON Lines attempt stream.
    RawAttempts,
}

/// Closed journal lifecycle stage used by failure evidence.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum JournalStage {
    /// Reserve all no-clobber destinations.
    Reservation,
    /// Bind source, executable, command, and environment.
    Provenance,
    /// Run the complete process tree.
    Execution,
    /// Validate source and raw attempts after execution.
    Verification,
    /// Publish the aggregate.
    Aggregate,
    /// Publish the eligibility marker.
    Completion,
}

/// Stable artifact kinds emitted by the single journal.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum ArtifactKind {
    /// First retained lifecycle record.
    #[serde(rename = "projectatlas.calibration.start")]
    Start,
    /// Process-tree supervision record.
    #[serde(rename = "projectatlas.calibration.process")]
    Process,
    /// Typed ineligible-run record.
    #[serde(rename = "projectatlas.calibration.failure")]
    Failure,
    /// Final raw calibration aggregate.
    #[serde(rename = "projectatlas.calibration.pilot")]
    Pilot,
    /// Marker written only after aggregate and raw-attempt readback.
    #[serde(rename = "projectatlas.calibration.completion")]
    Completion,
}

/// Raw pilot claim status.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ClaimStatus {
    /// Evidence exists but no benchmark claim has been evaluated.
    NotEvaluated,
}

/// Build profile accepted for claim-eligible calibration execution.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RunnerBuildProfile {
    /// Optimized Cargo release profile with debug assertions disabled.
    Release,
}

/// Executable role accepted by the calibration evidence contract.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RunnerExecutableRole {
    /// Dedicated example binary that owns calibration evidence capture.
    DedicatedCalibrationRunner,
}

/// Transient raw environment value that cannot be serialized.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TransientEnvironmentEntry {
    /// Environment variable name.
    name: String,
    /// Exact Unicode value retained only in process memory and environment.
    value: String,
}

/// Digest-only retained environment evidence.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct RetainedEnvironmentEntry {
    /// Environment variable name.
    name: String,
    /// Whether the value was present.
    present: bool,
    /// Digest over the exact Unicode value when present.
    value_sha256: Option<String>,
}

/// One typed provenance digest.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct ProvenanceDigest {
    /// Owner of the digested bytes.
    scope: ProvenanceScope,
    /// Lowercase SHA-256 digest.
    sha256: String,
}

/// Canonical executable identity used for every Git provenance query.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct GitExecutableBinding {
    /// Canonical absolute executable path.
    path: String,
    /// Digest over the exact executable bytes.
    sha256: String,
    /// Exact bounded `git --version` output.
    version: String,
}

/// Clean source and Cargo.lock binding captured around execution.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct SourceBinding {
    /// Closed Git executable identity used to capture repository state.
    git: GitExecutableBinding,
    /// Exact Git HEAD commit.
    head_commit: String,
    /// Whether Git reported any tracked or untracked change.
    dirty: bool,
    /// Digest over exact porcelain status bytes.
    worktree_state_sha256: String,
    /// Digest over exact Cargo.lock bytes.
    cargo_lock_sha256: String,
    /// Digest over the runner source bytes compiled into this executable.
    runner_source_sha256: String,
}

/// Declared reference host bound to directly observed runtime dimensions.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct ObservedEnvironmentBinding {
    /// Manifest-declared environment identifier used for before/after comparison.
    reference_environment_id: String,
    /// Runtime OS family reported by the Rust standard library.
    observed_os_family: String,
    /// Runtime target architecture reported by the Rust standard library.
    observed_architecture: String,
    /// Digest over the canonical controlled-environment evidence.
    controlled_environment_sha256: String,
    /// Digest over the exact dedicated runner executable bytes.
    executable_sha256: String,
}

/// Exact dedicated-runner invocation.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct InvocationEvidence {
    /// Canonical runner executable path.
    executable: String,
    /// Digest over exact runner executable bytes.
    executable_sha256: String,
    /// Build profile validated from compile-time assertions state.
    build_profile: RunnerBuildProfile,
    /// Dedicated executable role validated from the runtime filename.
    executable_role: RunnerExecutableRole,
    /// Exact internal execution arguments.
    arguments: Vec<String>,
    /// Digest over executable and arguments.
    command_sha256: String,
    /// Digest-only controlled environment.
    environment: Vec<RetainedEnvironmentEntry>,
}

/// First record retained before process execution.
#[derive(Debug, Serialize)]
struct StartEvidence {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Caller-selected run identifier.
    run_id: String,
    /// Before/after position.
    position: RunPosition,
    /// Wall-clock start time.
    started_unix_ms: u128,
    /// Clean source binding.
    source: SourceBinding,
    /// Exact runner invocation.
    invocation: InvocationEvidence,
    /// Directly observed host dimensions bound to the declared reference environment.
    observed_environment: ObservedEnvironmentBinding,
    /// Typed digest inventory.
    provenance: Vec<ProvenanceDigest>,
    /// Raw claim status.
    claim_status: ClaimStatus,
}

/// Digest-only metadata for one out-of-line stream.
#[derive(Debug, Serialize)]
struct StreamEvidence<'a> {
    /// Number of retained bytes.
    bytes: usize,
    /// Same-journal retained filename.
    file: &'a str,
    /// Digest over retained bytes.
    sha256: &'a str,
}

/// Complete process-tree record retained before success classification.
#[derive(Debug, Serialize)]
struct ProcessEvidence<'a> {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Provenance owner for this process tree.
    scope: ProvenanceScope,
    /// Leader exit code when available.
    exit_code: Option<i32>,
    /// Whether the deadline terminated the tree.
    timed_out: bool,
    /// Wall-clock lifetime.
    duration_ns: u64,
    /// Whether either stream crossed the configured ceiling.
    output_truncated: bool,
    /// Bounded standard output binding.
    stdout: StreamEvidence<'a>,
    /// Bounded standard error binding.
    stderr: StreamEvidence<'a>,
    /// Raw claim status.
    claim_status: ClaimStatus,
}

/// One raw workload attempt.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct CalibrationSample {
    /// Workload identifier.
    workload_id: String,
    /// Warmup or measured phase.
    phase: SamplePhase,
    /// Zero-based phase-local repetition.
    repetition: usize,
    /// Wall-clock attempt start.
    started_unix_ms: u128,
    /// Exact workload duration.
    duration_ns: u64,
    /// Digest over deterministic workload output.
    output_sha256: String,
}

/// Validated measured summary for one workload.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct WorkloadSummary {
    /// Workload identifier.
    workload_id: String,
    /// Number of measured attempts.
    measured_samples: usize,
    /// Median measured duration.
    median_ns: u64,
    /// Stable output digest shared by every attempt.
    output_sha256: String,
}

/// Aggregate written after source and raw attempts are revalidated.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct CalibrationArtifact {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Evaluation manifest identifier.
    manifest_id: String,
    /// Exact manifest digest.
    manifest_sha256: String,
    /// Caller-selected run identifier.
    run_id: String,
    /// Before/after position.
    position: RunPosition,
    /// Source binding before execution.
    source_before: SourceBinding,
    /// Source binding after execution.
    source_after: SourceBinding,
    /// Dedicated runner invocation.
    invocation: InvocationEvidence,
    /// Directly observed host dimensions comparable across before/after positions.
    observed_environment: ObservedEnvironmentBinding,
    /// All retained raw attempts.
    samples: Vec<CalibrationSample>,
    /// BLAKE3 measured summary.
    blake3: WorkloadSummary,
    /// `SQLite` measured summary.
    sqlite: WorkloadSummary,
    /// Raw claim status.
    claim_status: ClaimStatus,
}

/// Bounded failure retained without replacing prior evidence.
#[derive(Debug, Serialize)]
struct FailureEvidence {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Failure lifecycle stage.
    stage: JournalStage,
    /// Wall-clock failure time.
    failed_unix_ms: u128,
    /// Bounded diagnostic.
    error: String,
    /// Whether the original diagnostic exceeded the retained prefix.
    error_truncated: bool,
    /// Raw claim status.
    claim_status: ClaimStatus,
}

/// Final marker binding aggregate, raw attempts, and process evidence.
#[derive(Debug, Deserialize, Serialize)]
struct CompletionEvidence {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Wall-clock completion time.
    completed_unix_ms: u128,
    /// Aggregate digest.
    artifact_sha256: String,
    /// Raw JSON Lines digest.
    raw_attempts_sha256: String,
    /// Supervised process record digest.
    process_sha256: String,
    /// Number of raw attempts.
    sample_count: usize,
    /// Raw claim status.
    claim_status: ClaimStatus,
}

/// Digest and size returned after exact-byte readback.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileBinding {
    /// Digest over exact bytes.
    sha256: String,
    /// Exact persisted byte count.
    bytes: u64,
}

/// Manifest-owned aggregate and raw-attempt paths for one position.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputPaths {
    /// Aggregate JSON path.
    aggregate: PathBuf,
    /// Raw attempts JSON Lines path.
    raw_attempts: PathBuf,
}

/// Stable operating-system identity for one already-open file or directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    /// Device containing the inode.
    #[cfg(unix)]
    device: u64,
    /// Inode number on the device.
    #[cfg(unix)]
    inode: u64,
    /// Windows creation timestamp used with a retained handle to detect replacement.
    #[cfg(windows)]
    creation_time: u64,
}

/// Canonical repository root and its stable directory identity.
struct RepositoryBoundary {
    /// Canonical absolute repository root.
    root: PathBuf,
    /// Root identity captured before any evidence mutation.
    identity: FileIdentity,
}

/// One retained evidence handle that cannot be redirected by a path replacement.
struct BoundEvidenceFile {
    /// Open evidence handle.
    file: File,
    /// Identity captured from the open handle.
    identity: FileIdentity,
}

/// Validated subset of the frozen manifest executed by this runner.
struct CalibrationPolicy {
    /// Full manifest used for path and workload lookup.
    manifest: Value,
    /// Exact manifest digest.
    manifest_sha256: String,
    /// Complete tree timeout.
    tree_timeout: Duration,
    /// Per-stream retained byte ceiling.
    stream_limit: usize,
}

/// Validated release-only identity of the running executable.
struct EligibleRunner {
    /// Build profile accepted by the evidence contract.
    build_profile: RunnerBuildProfile,
    /// Dedicated executable role accepted by the evidence contract.
    executable_role: RunnerExecutableRole,
}

/// Single owner for every no-clobber calibration artifact.
struct EvidenceJournal {
    /// Final aggregate destination.
    aggregate_path: PathBuf,
    /// Append-only raw-attempt destination.
    raw_attempts_path: PathBuf,
    /// Retained append/read handle for raw attempts.
    raw_attempts_file: File,
    /// Stable identity of the retained raw-attempt file.
    raw_attempts_identity: FileIdentity,
    /// Directory containing lifecycle and process records.
    run_directory: PathBuf,
    /// Validated repository boundary shared by every managed path.
    repository: RepositoryBoundary,
    /// Stable owner-directory identity captured at reservation.
    parent_identity: FileIdentity,
    /// Stable lifecycle-directory identity captured at reservation.
    run_directory_identity: FileIdentity,
    /// Handles retained after publication so later reads cannot follow replacements.
    retained_files: BTreeMap<PathBuf, BoundEvidenceFile>,
    /// Current lifecycle stage for typed failure evidence.
    stage: JournalStage,
}

/// Closed runner command parsed from process arguments.
enum RunnerCommand {
    /// Own provenance, supervision, verification, and completion.
    Run {
        /// Frozen evaluation manifest path.
        manifest: PathBuf,
        /// Before/after benchmark position.
        position: RunPosition,
        /// Caller-selected evidence run identifier.
        run_id: String,
    },
    /// Execute all workload subprocesses inside the supervised tree.
    Execute {
        /// Frozen evaluation manifest path.
        manifest: PathBuf,
        /// Before/after benchmark position.
        position: RunPosition,
        /// Caller-selected evidence run identifier.
        run_id: String,
        /// Aggregate destination bound by the manifest.
        aggregate: PathBuf,
        /// Raw-attempt destination bound by the manifest.
        raw_attempts: PathBuf,
    },
    /// Execute one deterministic workload and publish one transient result.
    Workload {
        /// Closed workload kind.
        kind: WorkloadKind,
        /// Warmup or measured phase.
        phase: SamplePhase,
        /// Phase-local repetition index.
        repetition: usize,
        /// Single-use result transport path.
        result: PathBuf,
    },
}

/// Execute the dedicated runner's closed command surface.
pub(super) async fn run_from_arguments() -> Result<(), CalibrationError> {
    match parse_runner_command(env::args_os().skip(1))? {
        RunnerCommand::Run {
            manifest,
            position,
            run_id,
        } => run_calibration(&manifest, position, &run_id).await,
        RunnerCommand::Execute {
            manifest,
            position,
            run_id,
            aggregate,
            raw_attempts,
        } => {
            execute_workloads(
                &manifest,
                position,
                &run_id,
                OutputPaths {
                    aggregate,
                    raw_attempts,
                },
            )
            .await
        }
        RunnerCommand::Workload {
            kind,
            phase,
            repetition,
            result,
        } => run_workload_child(kind, phase, repetition, &result),
    }
}

/// Parse one exact subcommand with duplicate and positional-value rejection.
fn parse_runner_command(
    arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<RunnerCommand, CalibrationError> {
    let arguments = arguments
        .map(|value| {
            value.into_string().map_err(|_value| {
                CalibrationError::Arguments("runner arguments must be Unicode".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (command, tail) = arguments
        .split_first()
        .ok_or_else(|| CalibrationError::Arguments("runner subcommand is required".into()))?;
    let options = parse_options(tail)?;
    match command.as_str() {
        RUN_COMMAND => {
            validate_option_keys(&options, RUN_OPTIONS)?;
            Ok(RunnerCommand::Run {
                manifest: option_path(&options, MANIFEST_OPTION)?,
                position: RunPosition::parse(option(&options, POSITION_OPTION)?)?,
                run_id: validated_run_id(option(&options, RUN_ID_OPTION)?)?,
            })
        }
        EXECUTE_COMMAND => {
            validate_option_keys(&options, EXECUTE_OPTIONS)?;
            Ok(RunnerCommand::Execute {
                manifest: option_path(&options, MANIFEST_OPTION)?,
                position: RunPosition::parse(option(&options, POSITION_OPTION)?)?,
                run_id: validated_run_id(option(&options, RUN_ID_OPTION)?)?,
                aggregate: option_path(&options, AGGREGATE_OPTION)?,
                raw_attempts: option_path(&options, RAW_ATTEMPTS_OPTION)?,
            })
        }
        WORKLOAD_COMMAND => {
            validate_option_keys(&options, WORKLOAD_OPTIONS)?;
            Ok(RunnerCommand::Workload {
                kind: WorkloadKind::parse(option(&options, KIND_OPTION)?)?,
                phase: SamplePhase::parse(option(&options, PHASE_OPTION)?)?,
                repetition: option(&options, REPETITION_OPTION)?
                    .parse()
                    .map_err(|error| {
                        CalibrationError::Arguments(format!("invalid workload repetition: {error}"))
                    })?,
                result: option_path(&options, RESULT_OPTION)?,
            })
        }
        other => Err(CalibrationError::Arguments(format!(
            "unknown runner subcommand `{other}`"
        ))),
    }
}

/// Reject every option outside the exact subcommand contract.
fn validate_option_keys(
    options: &BTreeMap<String, String>,
    expected: &[&str],
) -> Result<(), CalibrationError> {
    if options.len() == expected.len() && expected.iter().all(|name| options.contains_key(*name)) {
        Ok(())
    } else {
        Err(CalibrationError::Arguments(format!(
            "runner options must be exactly {}",
            expected.join(", ")
        )))
    }
}

/// Parse flag/value pairs without accepting duplicates or positional values.
fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, CalibrationError> {
    if !arguments.len().is_multiple_of(2) {
        return Err(CalibrationError::Arguments(
            "every runner flag requires one value".into(),
        ));
    }
    let mut options = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !pair[0].starts_with("--") || options.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err(CalibrationError::Arguments(
                "runner flags must be unique named options".into(),
            ));
        }
    }
    Ok(options)
}

/// Return one required option and reject silent omission.
fn option<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, CalibrationError> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| CalibrationError::Arguments(format!("{name} is required")))
}

/// Return one required path option.
fn option_path(
    options: &BTreeMap<String, String>,
    name: &str,
) -> Result<PathBuf, CalibrationError> {
    option(options, name).map(PathBuf::from)
}

/// Validate a compact filesystem- and evidence-safe run identifier.
fn validated_run_id(value: &str) -> Result<String, CalibrationError> {
    require(
        !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "run id must be 1-80 ASCII letters, digits, dots, dashes, or underscores",
    )?;
    Ok(value.to_owned())
}

/// Validate the frozen manifest subset used by the consolidated runner.
fn calibration_policy(manifest_path: &Path) -> Result<CalibrationPolicy, CalibrationError> {
    let bytes = fs::read(manifest_path)?;
    require(
        bytes == MANIFEST_BYTES,
        "runtime manifest differs from compiled manifest bytes",
    )?;
    let digest = sha256_hex(&bytes);
    require(digest == MANIFEST_SHA256, "manifest digest drifted")?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    let runner = &manifest["reproduction"]["calibration_runner"];
    require(
        manifest["schema_version"] == 1
            && manifest["format"] == "projectatlas.evaluation-manifest"
            && runner["source_path"] == RUNNER_SOURCE_PATH
            && runner["example_name"] == RUNNER_EXAMPLE_NAME
            && runner["eligible_build_profile"] == "release"
            && runner["debug_assertions_must_be_disabled"] == true
            && runner["build_command_role"] == "reproduction-instruction-not-execution-proof"
            && runner["tree_timeout_seconds"] == TREE_TIMEOUT_SECONDS
            && runner["per_stream_capture_limit_bytes"] == STREAM_LIMIT_BYTES as u64
            && runner["warmups"] == WARMUPS
            && runner["repetitions"] == REPETITIONS
            && runner["process_supervision"] == "processkit-private-process-tree"
            && runner["environment_evidence"] == "name-presence-sha256-only"
            && runner["raw_environment_transport"] == "process-environment-only"
            && runner["allowed_environment_names"] == serde_json::to_value(ENVIRONMENT_ALLOWLIST)?
            && runner["forced_environment"] == serde_json::json!({"RUST_BACKTRACE": "0"})
            && runner["no_nonces_or_handoffs"] == true
            && manifest["reproduction"].get("external_launcher").is_none()
            && manifest["reproduction"].get("controller_handoff").is_none()
            && manifest["reproduction"]
                .get("calibration_controller_command")
                .is_none()
            && manifest["reproduction"]
                .get("calibration_inner_command")
                .is_none(),
        "consolidated calibration runner policy drifted",
    )?;
    let blake3_timeout = validate_workload_policy(&manifest, WorkloadKind::Blake3)?;
    let sqlite_timeout = validate_workload_policy(&manifest, WorkloadKind::Sqlite)?;
    require(
        blake3_timeout == Duration::from_secs(WORKLOAD_TIMEOUT_SECONDS)
            && sqlite_timeout == Duration::from_secs(WORKLOAD_TIMEOUT_SECONDS),
        "calibration workload deadlines drifted",
    )?;
    let before = output_paths(&manifest, RunPosition::Before)?;
    let after = output_paths(&manifest, RunPosition::After)?;
    require(
        before != after
            && before.aggregate != before.raw_attempts
            && after.aggregate != after.raw_attempts,
        "calibration position outputs overlap",
    )?;
    Ok(CalibrationPolicy {
        manifest,
        manifest_sha256: digest,
        tree_timeout: Duration::from_secs(TREE_TIMEOUT_SECONDS),
        stream_limit: STREAM_LIMIT_BYTES,
    })
}

/// Validate one workload row against the executable contract.
fn validate_workload_policy(
    manifest: &Value,
    kind: WorkloadKind,
) -> Result<Duration, CalibrationError> {
    let row = manifest["calibration"]["eligible_workloads"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["id"].as_str() == Some(kind.manifest_id()))
        })
        .ok_or_else(|| CalibrationError::Policy("calibration workload is missing".into()))?;
    let timeout_seconds = row["timeout_seconds"]
        .as_u64()
        .ok_or_else(|| CalibrationError::Policy("workload timeout is missing".into()))?;
    require(
        row["repetitions"] == REPETITIONS
            && timeout_seconds == WORKLOAD_TIMEOUT_SECONDS
            && timeout_seconds > 0,
        "calibration workload counts or timeout drifted",
    )?;
    Ok(Duration::from_secs(timeout_seconds))
}

/// Resolve one manifest-owned position without accepting path traversal.
fn output_paths(manifest: &Value, position: RunPosition) -> Result<OutputPaths, CalibrationError> {
    let row = &manifest["reproduction"]["artifact_paths"]["calibration_positions"][position.id()];
    let aggregate = row["pilot"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| CalibrationError::Policy("calibration aggregate path is missing".into()))?;
    let raw_attempts = row["raw_attempts"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| CalibrationError::Policy("raw attempts path is missing".into()))?;
    require(
        is_safe_relative(&aggregate) && is_safe_relative(&raw_attempts),
        "calibration output path is not repository-relative",
    )?;
    Ok(OutputPaths {
        aggregate,
        raw_attempts,
    })
}

/// Return whether a path contains only normal relative components.
fn is_safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Validate the dedicated release example without claiming how Cargo was invoked.
fn validate_runner_execution(
    executable: &Path,
    debug_assertions_enabled: bool,
) -> Result<EligibleRunner, CalibrationError> {
    let executable_stem = executable
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| CalibrationError::Binding("runner executable name is not Unicode".into()))?;
    let examples_directory = executable
        .parent()
        .and_then(Path::file_name)
        .and_then(std::ffi::OsStr::to_str);
    let profile_directory = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(std::ffi::OsStr::to_str);
    require(
        !debug_assertions_enabled
            && executable_stem == RUNNER_EXAMPLE_NAME
            && examples_directory == Some("examples")
            && profile_directory == Some("release"),
        "calibration requires the dedicated release example with debug assertions disabled",
    )?;
    Ok(EligibleRunner {
        build_profile: RunnerBuildProfile::Release,
        executable_role: RunnerExecutableRole::DedicatedCalibrationRunner,
    })
}

/// Bind directly observed runtime dimensions to the manifest's reference host.
fn observed_environment_binding(
    manifest: &Value,
    retained_environment: &[RetainedEnvironmentEntry],
    executable_sha256: &str,
) -> Result<ObservedEnvironmentBinding, CalibrationError> {
    let reference_environment_id = manifest["calibration"]["reference_environment"]
        .as_str()
        .ok_or_else(|| CalibrationError::Policy("reference environment id is missing".into()))?;
    let binding = ObservedEnvironmentBinding {
        reference_environment_id: reference_environment_id.to_owned(),
        observed_os_family: std::env::consts::OS.to_owned(),
        observed_architecture: std::env::consts::ARCH.to_owned(),
        controlled_environment_sha256: sha256_hex(&serde_json::to_vec(retained_environment)?),
        executable_sha256: executable_sha256.to_owned(),
    };
    validate_observed_environment(manifest, &binding)?;
    Ok(binding)
}

/// Reject any declared or observed host-identity mismatch.
fn validate_observed_environment(
    manifest: &Value,
    binding: &ObservedEnvironmentBinding,
) -> Result<(), CalibrationError> {
    let reference = manifest["environments"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["id"].as_str() == Some(binding.reference_environment_id.as_str()))
        })
        .ok_or_else(|| CalibrationError::Policy("reference environment row is missing".into()))?;
    let host = &manifest["reproduction"]["reference_host_eligibility"];
    require(
        host["environment_id"].as_str() == Some(binding.reference_environment_id.as_str())
            && reference["os_family"].as_str() == Some(binding.observed_os_family.as_str())
            && reference["architecture"].as_str() == Some(binding.observed_architecture.as_str())
            && binding.controlled_environment_sha256.len() == 64
            && binding
                .controlled_environment_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && binding.executable_sha256.len() == 64
            && binding
                .executable_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "observed environment does not match the declared reference identity",
    )
}

/// Require two independently captured environment bindings to match exactly.
fn validate_environment_match(
    first: &ObservedEnvironmentBinding,
    second: &ObservedEnvironmentBinding,
) -> Result<(), CalibrationError> {
    require(
        first == second,
        "observed environment identity changed between evidence positions",
    )
}

/// Own one complete calibration lifecycle.
async fn run_calibration(
    manifest_path: &Path,
    position: RunPosition,
    run_id: &str,
) -> Result<(), CalibrationError> {
    let root = repository_root()?;
    require(
        fs::canonicalize(env::current_dir()?)? == root,
        "calibration runner must start at the repository root",
    )?;
    let manifest_path = fs::canonicalize(manifest_path)?;
    require(
        manifest_path == root.join("docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json"),
        "calibration manifest path is not the repository manifest",
    )?;
    let policy = calibration_policy(&manifest_path)?;
    let relative_paths = output_paths(&policy.manifest, position)?;
    let paths = OutputPaths {
        aggregate: root.join(relative_paths.aggregate),
        raw_attempts: root.join(relative_paths.raw_attempts),
    };
    let mut journal = EvidenceJournal::reserve(&root, paths)?;
    let result = run_calibration_owned(
        &root,
        &manifest_path,
        &policy,
        position,
        run_id,
        &mut journal,
    )
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => Err(journal.retain_failure(error)),
    }
}

/// Capture provenance, run one tree, verify outputs, and publish completion.
async fn run_calibration_owned(
    root: &Path,
    manifest_path: &Path,
    policy: &CalibrationPolicy,
    position: RunPosition,
    run_id: &str,
    journal: &mut EvidenceJournal,
) -> Result<(), CalibrationError> {
    journal.stage = JournalStage::Provenance;
    let source_before = source_binding(root).await?;
    require(!source_before.dirty, "calibration source is not clean")?;
    let expected_lock = policy.manifest["projectatlas"]["cargo_lock_sha256"]
        .as_str()
        .ok_or_else(|| CalibrationError::Policy("Cargo.lock digest is missing".into()))?;
    require(
        source_before.cargo_lock_sha256 == expected_lock,
        "Cargo.lock digest differs from the manifest",
    )?;
    let executable = fs::canonicalize(env::current_exe()?)?;
    let eligible_runner = validate_runner_execution(&executable, cfg!(debug_assertions))?;
    let executable_text = path_text(&executable)?;
    let executable_sha256 = sha256_file(&executable, u64::MAX)?;
    let arguments = execution_arguments(
        manifest_path,
        position,
        run_id,
        &journal.aggregate_path,
        &journal.raw_attempts_path,
    )?;
    let environment = controlled_environment()?;
    let environment_evidence = retained_environment(&environment);
    let invocation = InvocationEvidence {
        executable: executable_text.clone(),
        executable_sha256: executable_sha256.clone(),
        build_profile: eligible_runner.build_profile,
        executable_role: eligible_runner.executable_role,
        command_sha256: command_sha256(&executable_text, &arguments)?,
        arguments: arguments.clone(),
        environment: environment_evidence.clone(),
    };
    let observed_environment =
        observed_environment_binding(&policy.manifest, &environment_evidence, &executable_sha256)?;
    journal.record_start(&StartEvidence {
        schema_version: 1,
        artifact_kind: ArtifactKind::Start,
        run_id: run_id.to_owned(),
        position,
        started_unix_ms: unix_millis()?,
        source: source_before.clone(),
        invocation: invocation.clone(),
        observed_environment: observed_environment.clone(),
        provenance: vec![
            ProvenanceDigest {
                scope: ProvenanceScope::Source,
                sha256: source_before.worktree_state_sha256.clone(),
            },
            ProvenanceDigest {
                scope: ProvenanceScope::CargoLock,
                sha256: source_before.cargo_lock_sha256.clone(),
            },
            ProvenanceDigest {
                scope: ProvenanceScope::Manifest,
                sha256: policy.manifest_sha256.clone(),
            },
            ProvenanceDigest {
                scope: ProvenanceScope::Executable,
                sha256: invocation.executable_sha256.clone(),
            },
            ProvenanceDigest {
                scope: ProvenanceScope::Command,
                sha256: invocation.command_sha256.clone(),
            },
            ProvenanceDigest {
                scope: ProvenanceScope::Environment,
                sha256: observed_environment.controlled_environment_sha256.clone(),
            },
        ],
        claim_status: ClaimStatus::NotEvaluated,
    })?;

    journal.stage = JournalStage::Execution;
    let mut command = Command::new(&executable)
        .args(&arguments)
        .current_dir(root)
        .env_clear();
    for entry in &environment {
        command = command.env(&entry.name, &entry.value);
    }
    let output = run_supervised(command, policy.tree_timeout, policy.stream_limit).await?;
    let process_binding = journal.record_process(&output)?;
    require(
        output.is_success(),
        "calibration execution tree did not complete cleanly",
    )?;

    journal.stage = JournalStage::Verification;
    let source_after = source_binding(root).await?;
    require(
        source_after == source_before,
        "source or Cargo.lock changed during calibration",
    )?;
    require(
        sha256_file(&executable, u64::MAX)? == invocation.executable_sha256,
        "runner executable changed during calibration",
    )?;
    let retained_environment_after = retained_environment(&controlled_environment()?);
    let observed_environment_after = observed_environment_binding(
        &policy.manifest,
        &retained_environment_after,
        &invocation.executable_sha256,
    )?;
    validate_environment_match(&observed_environment, &observed_environment_after)?;
    let samples = journal.read_samples()?;
    require(
        samples.len() == (WARMUPS + REPETITIONS) * 2,
        "raw attempt count is incomplete",
    )?;
    let blake3 = summarize_workload(&samples, WorkloadKind::Blake3)?;
    let sqlite = summarize_workload(&samples, WorkloadKind::Sqlite)?;
    let artifact = CalibrationArtifact {
        schema_version: 1,
        artifact_kind: ArtifactKind::Pilot,
        manifest_id: policy.manifest["manifest_id"]
            .as_str()
            .ok_or_else(|| CalibrationError::Policy("manifest id is missing".into()))?
            .to_owned(),
        manifest_sha256: policy.manifest_sha256.clone(),
        run_id: run_id.to_owned(),
        position,
        source_before,
        source_after,
        invocation,
        observed_environment,
        samples,
        blake3,
        sqlite,
        claim_status: ClaimStatus::NotEvaluated,
    };

    journal.stage = JournalStage::Aggregate;
    let artifact_binding = journal.publish_aggregate(&artifact)?;
    let raw_attempts_binding = journal.bind_raw_attempts()?;
    journal.stage = JournalStage::Completion;
    journal.publish_completion(&CompletionEvidence {
        schema_version: 1,
        artifact_kind: ArtifactKind::Completion,
        completed_unix_ms: unix_millis()?,
        artifact_sha256: artifact_binding.sha256,
        raw_attempts_sha256: raw_attempts_binding.sha256,
        process_sha256: process_binding.sha256,
        sample_count: artifact.samples.len(),
        claim_status: ClaimStatus::NotEvaluated,
    })
}

/// Build the exact internal execution subcommand.
fn execution_arguments(
    manifest: &Path,
    position: RunPosition,
    run_id: &str,
    aggregate: &Path,
    raw_attempts: &Path,
) -> Result<Vec<String>, CalibrationError> {
    Ok(vec![
        EXECUTE_COMMAND.into(),
        MANIFEST_OPTION.into(),
        path_text(manifest)?,
        POSITION_OPTION.into(),
        position.id().into(),
        RUN_ID_OPTION.into(),
        run_id.into(),
        AGGREGATE_OPTION.into(),
        path_text(aggregate)?,
        RAW_ATTEMPTS_OPTION.into(),
        path_text(raw_attempts)?,
    ])
}

/// Execute all workload subprocesses as independently supervised descendants.
async fn execute_workloads(
    manifest_path: &Path,
    position: RunPosition,
    run_id: &str,
    paths: OutputPaths,
) -> Result<(), CalibrationError> {
    let policy = calibration_policy(manifest_path)?;
    let expected = output_paths(&policy.manifest, position)?;
    let root = repository_root()?;
    require(
        paths.aggregate == root.join(expected.aggregate)
            && paths.raw_attempts == root.join(expected.raw_attempts),
        "execution output paths differ from the manifest position",
    )?;
    let journal = EvidenceJournal::open(&root, paths)?;
    let start: StartEvidenceSnapshot = journal.read_json(START_FILE, CONTROL_FILE_LIMIT)?;
    let environment = environment_from_process(&start.invocation.environment)?;
    let executable = fs::canonicalize(env::current_exe()?)?;
    let eligible_runner = validate_runner_execution(&executable, cfg!(debug_assertions))?;
    let executable_sha256 = sha256_file(&executable, u64::MAX)?;
    let current_environment = observed_environment_binding(
        &policy.manifest,
        &retained_environment(&environment),
        &executable_sha256,
    )?;
    require(
        start.schema_version == 1
            && start.artifact_kind == ArtifactKind::Start
            && start.run_id == run_id
            && start.position == position
            && start.invocation.executable == path_text(&executable)?
            && start.invocation.executable_sha256 == executable_sha256
            && start.invocation.build_profile == eligible_runner.build_profile
            && start.invocation.executable_role == eligible_runner.executable_role
            && start.observed_environment == current_environment,
        "execution start binding differs from the current runner",
    )?;
    let mut sample_index = 0_usize;
    for kind in [WorkloadKind::Blake3, WorkloadKind::Sqlite] {
        let workload_timeout = validate_workload_policy(&policy.manifest, kind)?;
        for (phase, count) in [
            (SamplePhase::Warmup, WARMUPS),
            (SamplePhase::Measured, REPETITIONS),
        ] {
            for repetition in 0..count {
                let result_path = journal
                    .run_directory
                    .join(format!("attempt-{sample_index:02}.transport.json"));
                let arguments = vec![
                    WORKLOAD_COMMAND.to_owned(),
                    KIND_OPTION.to_owned(),
                    kind.command_id().to_owned(),
                    PHASE_OPTION.to_owned(),
                    phase.id().to_owned(),
                    REPETITION_OPTION.to_owned(),
                    repetition.to_string(),
                    RESULT_OPTION.to_owned(),
                    path_text(&result_path)?,
                ];
                let mut command = Command::new(&executable)
                    .args(&arguments)
                    .current_dir(&root)
                    .env_clear();
                for entry in &environment {
                    command = command.env(&entry.name, &entry.value);
                }
                run_workload_process(command, workload_timeout, policy.stream_limit).await?;
                let sample: CalibrationSample =
                    journal.read_path_json(&result_path, CONTROL_FILE_LIMIT)?;
                fs::remove_file(&result_path)?;
                require(
                    sample.workload_id == kind.manifest_id()
                        && sample.phase == phase
                        && sample.repetition == repetition,
                    "workload child result binding drifted",
                )?;
                journal.append_sample(&sample)?;
                sample_index += 1;
            }
        }
    }
    Ok(())
}

/// Supervise one workload attempt with its manifest deadline and bounded streams.
async fn run_workload_process(
    command: Command,
    timeout: Duration,
    output_limit: usize,
) -> Result<(), CalibrationError> {
    let output = run_supervised(command, timeout, output_limit).await?;
    if output.timed_out {
        return Err(CalibrationError::Workload(
            "workload child exceeded its manifest deadline".into(),
        ));
    }
    if output.output_truncated {
        return Err(CalibrationError::Workload(
            "workload child exceeded its retained output limit".into(),
        ));
    }
    if output.exit_code != Some(0) {
        return Err(CalibrationError::Workload(format!(
            "workload child exited unsuccessfully with {:?}",
            output.exit_code
        )));
    }
    Ok(())
}

/// Execute one internal workload and publish a single-use result transport.
fn run_workload_child(
    kind: WorkloadKind,
    phase: SamplePhase,
    repetition: usize,
    result_path: &Path,
) -> Result<(), CalibrationError> {
    let started_unix_ms = unix_millis()?;
    let started = Instant::now();
    let output_sha256 = match kind {
        WorkloadKind::Blake3 => blake3_workload(BLAKE3_BUFFER_COUNT, BLAKE3_BUFFER_BYTES)?,
        WorkloadKind::Sqlite => sqlite_workload(SQLITE_ROW_COUNT)?,
    };
    let sample = CalibrationSample {
        workload_id: kind.manifest_id().into(),
        phase,
        repetition,
        started_unix_ms,
        duration_ns: elapsed_ns(started),
        output_sha256,
    };
    write_json_create_new(result_path, &sample, CONTROL_FILE_LIMIT)?;
    Ok(())
}

/// Hash deterministic buffers serially without retaining the full 512 MiB working set.
fn blake3_workload(count: usize, bytes: usize) -> Result<String, CalibrationError> {
    require(
        count > 0 && bytes > 0,
        "BLAKE3 workload dimensions are empty",
    )?;
    let mut combined = Sha256::new();
    let mut buffer = vec![0_u8; bytes];
    for index in 0..count {
        for (offset, byte) in buffer.iter_mut().enumerate() {
            *byte = ((offset as u64 * 131 + index as u64 * 17) & 0xff) as u8;
        }
        combined.update(blake3::hash(&buffer).as_bytes());
    }
    Ok(format!("{:x}", combined.finalize()))
}

/// Execute one prepared `SQLite` transaction and digest its deterministic aggregate.
fn sqlite_workload(rows: usize) -> Result<String, CalibrationError> {
    require(rows > 0, "SQLite workload row count is empty")?;
    let file = NamedTempFile::new()?;
    let mut connection = Connection::open(file.path())?;
    connection.execute_batch(
        "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; \
         CREATE TABLE records(id INTEGER PRIMARY KEY, bucket INTEGER NOT NULL, payload TEXT NOT NULL); \
         CREATE INDEX records_bucket ON records(bucket);",
    )?;
    let transaction = connection.transaction()?;
    {
        let mut statement =
            transaction.prepare("INSERT INTO records(id, bucket, payload) VALUES (?1, ?2, ?3)")?;
        for id in 0..rows {
            statement.execute(params![
                id as i64,
                (id % 257) as i64,
                format!("row-{id:08}")
            ])?;
        }
    }
    transaction.commit()?;
    let aggregate: (i64, i64, i64) = connection.query_row(
        "SELECT COUNT(*), SUM(id), SUM(bucket) FROM records WHERE bucket BETWEEN 0 AND 256",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(sha256_hex(&serde_json::to_vec(&aggregate)?))
}

/// Validate and summarize one workload's measured attempts.
fn summarize_workload(
    samples: &[CalibrationSample],
    kind: WorkloadKind,
) -> Result<WorkloadSummary, CalibrationError> {
    let relevant = samples
        .iter()
        .filter(|sample| sample.workload_id == kind.manifest_id())
        .collect::<Vec<_>>();
    require(
        relevant.len() == WARMUPS + REPETITIONS
            && relevant
                .iter()
                .filter(|sample| sample.phase == SamplePhase::Warmup)
                .count()
                == WARMUPS,
        "workload attempt inventory is incomplete",
    )?;
    let output_sha256 = relevant
        .first()
        .map(|sample| sample.output_sha256.clone())
        .ok_or_else(|| CalibrationError::Workload("workload has no output digest".into()))?;
    require(
        relevant
            .iter()
            .all(|sample| sample.output_sha256 == output_sha256),
        "workload output digest changed across attempts",
    )?;
    let mut measured = relevant
        .iter()
        .filter(|sample| sample.phase == SamplePhase::Measured)
        .map(|sample| sample.duration_ns)
        .collect::<Vec<_>>();
    require(
        measured.len() == REPETITIONS && measured.iter().all(|duration| *duration > 0),
        "measured workload durations are incomplete",
    )?;
    measured.sort_unstable();
    Ok(WorkloadSummary {
        workload_id: kind.manifest_id().into(),
        measured_samples: measured.len(),
        median_ns: measured[measured.len() / 2],
        output_sha256,
    })
}

/// Run one repository-bound Git query and decode its line-oriented text strictly.
async fn git_text_output(
    git: &RepositoryGitProbe,
    arguments: &[&str],
) -> Result<String, CalibrationError> {
    let output = git.output_bytes(arguments).await?;
    Ok(std::str::from_utf8(&output)
        .map_err(|source| CalibrationError::Binding(source.to_string()))?
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}

/// Capture clean Git and Cargo.lock provenance with bounded native probes.
async fn source_binding(root: &Path) -> Result<SourceBinding, CalibrationError> {
    let git = RepositoryGitProbe::resolve(root)?;
    let version = git_text_output(&git, &["--version"]).await?;
    let head = git_text_output(&git, &["rev-parse", "HEAD"]).await?;
    let status = git.worktree_state().await?;
    let head_commit = head.trim().to_owned();
    require(
        (head_commit.len() == 40 || head_commit.len() == 64)
            && head_commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Git HEAD is not a full hexadecimal commit",
    )?;
    let runner_source_sha256 = sha256_file(&root.join(RUNNER_SOURCE_PATH), CONTROL_FILE_LIMIT * 4)?;
    require(
        runner_source_sha256 == sha256_hex(RUNNER_BYTES),
        "runner source differs from the bytes compiled into the executable",
    )?;
    Ok(SourceBinding {
        git: GitExecutableBinding {
            path: path_text(git.executable())?,
            sha256: git.executable_sha256().to_owned(),
            version,
        },
        head_commit,
        dirty: !status.is_empty(),
        worktree_state_sha256: sha256_hex(&status),
        cargo_lock_sha256: sha256_file(&root.join("Cargo.lock"), CONTROL_FILE_LIMIT * 4)?,
        runner_source_sha256,
    })
}

/// Capture the closed transient child environment.
fn controlled_environment() -> Result<Vec<TransientEnvironmentEntry>, CalibrationError> {
    let mut entries = Vec::new();
    for name in ENVIRONMENT_ALLOWLIST {
        if let Some(value) = env::var_os(name) {
            entries.push(TransientEnvironmentEntry {
                name: (*name).into(),
                value: value.into_string().map_err(|_value| {
                    CalibrationError::Binding(format!(
                        "allowlisted environment variable `{name}` is not Unicode"
                    ))
                })?,
            });
        }
    }
    entries.extend(
        FORCED_ENVIRONMENT
            .iter()
            .map(|(name, value)| TransientEnvironmentEntry {
                name: (*name).into(),
                value: (*value).into(),
            }),
    );
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    validate_transient_environment(&entries)?;
    Ok(entries)
}

/// Convert transient values into canonical name/presence/digest evidence.
fn retained_environment(entries: &[TransientEnvironmentEntry]) -> Vec<RetainedEnvironmentEntry> {
    let names = ENVIRONMENT_ALLOWLIST
        .iter()
        .map(|name| (*name).to_owned())
        .chain(
            FORCED_ENVIRONMENT
                .iter()
                .map(|(name, _value)| (*name).to_owned()),
        )
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .map(|name| {
            let value = entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.value.as_str());
            RetainedEnvironmentEntry {
                name,
                present: value.is_some(),
                value_sha256: value.map(|value| sha256_hex(value.as_bytes())),
            }
        })
        .collect()
}

/// Reconstruct transient values from an already-cleared child environment.
fn environment_from_process(
    evidence: &[RetainedEnvironmentEntry],
) -> Result<Vec<TransientEnvironmentEntry>, CalibrationError> {
    let mut entries = Vec::new();
    for retained in evidence {
        if let Some(value) = env::var_os(&retained.name) {
            entries.push(TransientEnvironmentEntry {
                name: retained.name.clone(),
                value: value.into_string().map_err(|_value| {
                    CalibrationError::Binding(format!(
                        "controlled environment variable `{}` is not Unicode",
                        retained.name
                    ))
                })?,
            });
        }
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    validate_transient_environment(&entries)?;
    require(
        retained_environment(&entries) == evidence,
        "controlled environment presence or digest changed",
    )?;
    Ok(entries)
}

/// Reject duplicate, empty, or undeclared transient entries.
fn validate_transient_environment(
    entries: &[TransientEnvironmentEntry],
) -> Result<(), CalibrationError> {
    let allowed = ENVIRONMENT_ALLOWLIST
        .iter()
        .copied()
        .chain(FORCED_ENVIRONMENT.iter().map(|(name, _value)| *name))
        .collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    require(
        entries.iter().all(|entry| {
            allowed.contains(entry.name.as_str())
                && !entry.value.is_empty()
                && names.insert(entry.name.as_str())
        }),
        "controlled environment contains a duplicate, empty, or undeclared entry",
    )?;
    for &(name, value) in FORCED_ENVIRONMENT {
        require(
            entries
                .iter()
                .any(|entry| entry.name == name && entry.value == value),
            "forced environment value drifted",
        )?;
    }
    Ok(())
}

/// Minimal deserializable start view used by the execution child.
#[derive(Deserialize)]
struct StartEvidenceSnapshot {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable artifact kind.
    artifact_kind: ArtifactKind,
    /// Caller-selected run identifier.
    run_id: String,
    /// Before/after position.
    position: RunPosition,
    /// Exact runner invocation.
    invocation: InvocationEvidence,
    /// Observed environment binding captured by the public runner.
    observed_environment: ObservedEnvironmentBinding,
}

/// Return whether metadata names a symbolic link or Windows reparse point.
fn is_link_or_reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(unix)]
    {
        metadata.file_type().is_symlink()
    }
}

/// Capture a stable Unix file identity.
#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt as _;
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// Capture the strongest replacement identity exposed by safe stable Windows metadata.
#[cfg(windows)]
fn file_identity(metadata: &Metadata) -> FileIdentity {
    use std::os::windows::fs::MetadataExt as _;
    FileIdentity {
        creation_time: metadata.creation_time(),
    }
}

impl RepositoryBoundary {
    /// Bind one canonical repository root before any evidence path is created.
    fn new(root: &Path) -> Result<Self, CalibrationError> {
        let canonical = fs::canonicalize(root)?;
        let metadata = fs::symlink_metadata(&canonical)?;
        require(
            canonical.is_absolute() && metadata.is_dir() && !is_link_or_reparse(&metadata),
            "evidence repository root is not a canonical plain directory",
        )?;
        Ok(Self {
            root: canonical,
            identity: file_identity(&metadata),
        })
    }

    /// Verify that the repository root still names the originally bound directory.
    fn verify(&self) -> Result<(), CalibrationError> {
        self.verify_directory(&self.root, self.identity)
    }

    /// Create missing plain directory components only after lexical containment is proven.
    fn prepare_directory(&self, directory: &Path) -> Result<FileIdentity, CalibrationError> {
        self.verify()?;
        let relative = directory.strip_prefix(&self.root).map_err(|error| {
            CalibrationError::Binding(format!(
                "evidence directory escapes the repository root: {error}"
            ))
        })?;
        require(
            relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
            "evidence directory contains a non-normal component",
        )?;
        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(CalibrationError::Binding(
                    "evidence directory component is not normal".into(),
                ));
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) => require(
                    metadata.is_dir() && !is_link_or_reparse(&metadata),
                    "evidence directory contains a symlink or reparse point",
                )?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current)?;
                    let metadata = fs::symlink_metadata(&current)?;
                    require(
                        metadata.is_dir() && !is_link_or_reparse(&metadata),
                        "created evidence directory is not a plain directory",
                    )?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let canonical = fs::canonicalize(directory)?;
        require(
            canonical == directory && canonical.starts_with(&self.root),
            "evidence directory canonical identity escapes its repository path",
        )?;
        let metadata = fs::symlink_metadata(directory)?;
        let identity = file_identity(&metadata);
        self.verify()?;
        Ok(identity)
    }

    /// Capture one existing contained plain directory identity without creating it.
    fn existing_directory(&self, directory: &Path) -> Result<FileIdentity, CalibrationError> {
        self.verify()?;
        require(
            directory.starts_with(&self.root),
            "evidence directory escapes the repository root",
        )?;
        let metadata = fs::symlink_metadata(directory)?;
        require(
            metadata.is_dir() && !is_link_or_reparse(&metadata),
            "evidence directory is a symlink, reparse point, or non-directory",
        )?;
        let canonical = fs::canonicalize(directory)?;
        require(
            canonical == directory && canonical.starts_with(&self.root),
            "evidence directory canonical identity escapes its repository path",
        )?;
        Ok(file_identity(&metadata))
    }

    /// Verify one contained directory against its original operating-system identity.
    fn verify_directory(
        &self,
        directory: &Path,
        expected: FileIdentity,
    ) -> Result<(), CalibrationError> {
        let metadata = fs::symlink_metadata(directory)?;
        require(
            directory.starts_with(&self.root)
                && metadata.is_dir()
                && !is_link_or_reparse(&metadata)
                && file_identity(&metadata) == expected,
            "evidence directory identity drifted",
        )
    }

    /// Verify that a managed file has the exact retained path and handle identity.
    fn verify_file(
        &self,
        path: &Path,
        handle: &File,
        expected: FileIdentity,
    ) -> Result<(), CalibrationError> {
        require(
            path.starts_with(&self.root),
            "evidence file escapes the repository root",
        )?;
        let path_metadata = fs::symlink_metadata(path)?;
        let handle_metadata = handle.metadata()?;
        require(
            path_metadata.is_file()
                && !is_link_or_reparse(&path_metadata)
                && file_identity(&path_metadata) == expected
                && file_identity(&handle_metadata) == expected,
            "evidence file identity drifted",
        )
    }
}

/// Require that a managed destination has no file, directory, or dangling link entry.
fn require_path_absent(path: &Path) -> Result<(), CalibrationError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(CalibrationError::Binding(format!(
            "calibration evidence destination already exists: {}",
            path.display()
        ))),
        Err(error) => Err(error.into()),
    }
}

/// Read one already-open regular file without consulting its pathname again.
fn read_bound_file(file: &File, limit: u64) -> Result<Vec<u8>, CalibrationError> {
    let metadata = file.metadata()?;
    require(
        metadata.is_file() && metadata.len() <= limit,
        "evidence handle is not a bounded regular file",
    )?;
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 <= limit,
        "evidence read exceeded its limit",
    )?;
    Ok(bytes)
}

impl EvidenceJournal {
    /// Reserve every owned destination without replacing prior evidence.
    fn reserve(repository_root: &Path, paths: OutputPaths) -> Result<Self, CalibrationError> {
        let repository = RepositoryBoundary::new(repository_root)?;
        let run_directory = paths.aggregate.with_extension("run");
        let parent = paths
            .aggregate
            .parent()
            .ok_or_else(|| CalibrationError::Binding("aggregate path has no parent".into()))?;
        require(
            paths.raw_attempts.parent() == Some(parent),
            "aggregate and raw attempts must share one directory",
        )?;
        let parent_identity = repository.prepare_directory(parent)?;
        require_path_absent(&paths.aggregate)?;
        require_path_absent(&paths.raw_attempts)?;
        require_path_absent(&run_directory)?;
        fs::create_dir(&run_directory)?;
        let run_directory_identity = repository.existing_directory(&run_directory)?;
        repository.verify_directory(parent, parent_identity)?;
        let raw_attempts_file = OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(true)
            .open(&paths.raw_attempts)?;
        raw_attempts_file.sync_all()?;
        let raw_attempts_identity = file_identity(&raw_attempts_file.metadata()?);
        repository.verify_file(
            &paths.raw_attempts,
            &raw_attempts_file,
            raw_attempts_identity,
        )?;
        let journal = Self {
            aggregate_path: paths.aggregate,
            raw_attempts_path: paths.raw_attempts,
            raw_attempts_file,
            raw_attempts_identity,
            run_directory,
            repository,
            parent_identity,
            run_directory_identity,
            retained_files: BTreeMap::new(),
            stage: JournalStage::Reservation,
        };
        journal.verify()?;
        Ok(journal)
    }

    /// Open the already-reserved journal from inside the supervised tree.
    fn open(repository_root: &Path, paths: OutputPaths) -> Result<Self, CalibrationError> {
        let repository = RepositoryBoundary::new(repository_root)?;
        let run_directory = paths.aggregate.with_extension("run");
        let parent = paths
            .aggregate
            .parent()
            .ok_or_else(|| CalibrationError::Binding("aggregate path has no parent".into()))?;
        require_path_absent(&paths.aggregate)?;
        let parent_identity = repository.existing_directory(parent)?;
        let run_directory_identity = repository.existing_directory(&run_directory)?;
        let raw_metadata = fs::symlink_metadata(&paths.raw_attempts)?;
        require(
            raw_metadata.is_file() && !is_link_or_reparse(&raw_metadata),
            "reserved raw-attempt path is not a plain file",
        )?;
        let raw_attempts_file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&paths.raw_attempts)?;
        let raw_attempts_identity = file_identity(&raw_attempts_file.metadata()?);
        require(
            file_identity(&raw_metadata) == raw_attempts_identity,
            "reserved raw-attempt file changed while opening",
        )?;
        let journal = Self {
            aggregate_path: paths.aggregate,
            raw_attempts_path: paths.raw_attempts,
            raw_attempts_file,
            raw_attempts_identity,
            run_directory,
            repository,
            parent_identity,
            run_directory_identity,
            retained_files: BTreeMap::new(),
            stage: JournalStage::Execution,
        };
        journal.verify()?;
        Ok(journal)
    }

    /// Revalidate root, directory, pathname, and retained-handle ownership.
    fn verify(&self) -> Result<(), CalibrationError> {
        self.repository.verify()?;
        let parent = self
            .aggregate_path
            .parent()
            .ok_or_else(|| CalibrationError::Binding("aggregate path has no parent".into()))?;
        self.repository
            .verify_directory(parent, self.parent_identity)?;
        self.repository
            .verify_directory(&self.run_directory, self.run_directory_identity)?;
        self.repository.verify_file(
            &self.raw_attempts_path,
            &self.raw_attempts_file,
            self.raw_attempts_identity,
        )?;
        for (path, retained) in &self.retained_files {
            self.repository
                .verify_file(path, &retained.file, retained.identity)?;
        }
        Ok(())
    }

    /// Retain the first lifecycle record.
    fn record_start(&mut self, evidence: &StartEvidence) -> Result<FileBinding, CalibrationError> {
        self.write_json(START_FILE, evidence, CONTROL_FILE_LIMIT)
    }

    /// Retain bounded streams out of line, then publish process metadata.
    fn record_process(
        &mut self,
        output: &SupervisedCommandOutput,
    ) -> Result<FileBinding, CalibrationError> {
        self.write_bytes(
            STDOUT_FILE,
            &output.stdout.retained,
            STREAM_LIMIT_BYTES as u64,
        )?;
        self.write_bytes(
            STDERR_FILE,
            &output.stderr.retained,
            STREAM_LIMIT_BYTES as u64,
        )?;
        self.write_json(
            PROCESS_FILE,
            &ProcessEvidence {
                schema_version: 1,
                artifact_kind: ArtifactKind::Process,
                scope: ProvenanceScope::Process,
                exit_code: output.exit_code,
                timed_out: output.timed_out,
                duration_ns: output.duration_ns,
                output_truncated: output.output_truncated,
                stdout: stream_evidence(&output.stdout, STDOUT_FILE),
                stderr: stream_evidence(&output.stderr, STDERR_FILE),
                claim_status: ClaimStatus::NotEvaluated,
            },
            CONTROL_FILE_LIMIT,
        )
    }

    /// Append one raw attempt and sync it before the next child starts.
    fn append_sample(&self, sample: &CalibrationSample) -> Result<(), CalibrationError> {
        self.verify()?;
        let mut file = &self.raw_attempts_file;
        serde_json::to_writer(&mut file, sample)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        self.verify()
    }

    /// Read and validate every raw JSON Lines attempt.
    fn read_samples(&self) -> Result<Vec<CalibrationSample>, CalibrationError> {
        let bytes = self.read_path(&self.raw_attempts_path, AGGREGATE_FILE_LIMIT)?;
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).map_err(Into::into))
            .collect()
    }

    /// Publish the aggregate with exact-byte readback.
    fn publish_aggregate(
        &mut self,
        artifact: &CalibrationArtifact,
    ) -> Result<FileBinding, CalibrationError> {
        let bytes = serde_json::to_vec_pretty(artifact)?;
        let aggregate_path = self.aggregate_path.clone();
        self.write_path(&aggregate_path, &bytes, AGGREGATE_FILE_LIMIT)
    }

    /// Bind the exact raw-attempt stream after execution.
    fn bind_raw_attempts(&self) -> Result<FileBinding, CalibrationError> {
        let bytes = self.read_path(&self.raw_attempts_path, AGGREGATE_FILE_LIMIT)?;
        Ok(FileBinding {
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        })
    }

    /// Publish completion only after aggregate and raw-attempt readback.
    fn publish_completion(
        &mut self,
        evidence: &CompletionEvidence,
    ) -> Result<(), CalibrationError> {
        self.write_json(COMPLETION_FILE, evidence, CONTROL_FILE_LIMIT)?;
        Ok(())
    }

    /// Preserve a typed failure marker and return the original failure.
    fn retain_failure(&mut self, error: CalibrationError) -> CalibrationError {
        let failed_unix_ms = match unix_millis() {
            Ok(timestamp) => timestamp,
            Err(marker) => {
                return CalibrationError::FailureMarker {
                    original: Box::new(error),
                    marker: Box::new(marker),
                };
            }
        };
        let mut diagnostic = error.to_string();
        let error_truncated = diagnostic.len() > FAILURE_DIAGNOSTIC_BYTES;
        if error_truncated {
            let mut end = FAILURE_DIAGNOSTIC_BYTES;
            while !diagnostic.is_char_boundary(end) {
                end -= 1;
            }
            diagnostic.truncate(end);
        }
        let marker = self.write_json(
            FAILURE_FILE,
            &FailureEvidence {
                schema_version: 1,
                artifact_kind: ArtifactKind::Failure,
                stage: self.stage,
                failed_unix_ms,
                error: diagnostic,
                error_truncated,
                claim_status: ClaimStatus::NotEvaluated,
            },
            CONTROL_FILE_LIMIT,
        );
        match marker {
            Ok(_binding) => error,
            Err(marker) => CalibrationError::FailureMarker {
                original: Box::new(error),
                marker: Box::new(marker),
            },
        }
    }

    /// Write one JSON record inside the owned run directory.
    fn write_json<T: Serialize>(
        &mut self,
        filename: &str,
        value: &T,
        limit: u64,
    ) -> Result<FileBinding, CalibrationError> {
        self.write_bytes(filename, &serde_json::to_vec_pretty(value)?, limit)
    }

    /// Write one no-clobber byte record inside the owned run directory.
    fn write_bytes(
        &mut self,
        filename: &str,
        bytes: &[u8],
        limit: u64,
    ) -> Result<FileBinding, CalibrationError> {
        let path = self.run_directory.join(filename);
        self.write_path(&path, bytes, limit)
    }

    /// Write, sync, and read back one no-clobber path.
    fn write_path(
        &mut self,
        path: &Path,
        bytes: &[u8],
        limit: u64,
    ) -> Result<FileBinding, CalibrationError> {
        self.verify()?;
        require(
            bytes.len() as u64 <= limit,
            "evidence record exceeds its byte limit",
        )?;
        self.verify_managed_parent(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let identity = file_identity(&file.metadata()?);
        self.verify_managed_parent(path)?;
        self.repository.verify_file(path, &file, identity)?;
        let persisted = read_bound_file(&file, limit)?;
        require(
            persisted == bytes,
            "evidence readback differs from written bytes",
        )?;
        require(
            self.retained_files
                .insert(path.to_path_buf(), BoundEvidenceFile { file, identity })
                .is_none(),
            "evidence path was retained more than once",
        )?;
        self.verify()?;
        Ok(FileBinding {
            sha256: sha256_hex(&persisted),
            bytes: persisted.len() as u64,
        })
    }

    /// Read one bounded journal path.
    fn read_path(&self, path: &Path, limit: u64) -> Result<Vec<u8>, CalibrationError> {
        self.verify()?;
        let bytes = if path == self.raw_attempts_path {
            read_bound_file(&self.raw_attempts_file, limit)?
        } else if let Some(retained) = self.retained_files.get(path) {
            read_bound_file(&retained.file, limit)?
        } else {
            self.verify_managed_parent(path)?;
            let path_metadata = fs::symlink_metadata(path)?;
            require(
                path_metadata.is_file()
                    && !is_link_or_reparse(&path_metadata)
                    && path_metadata.len() <= limit,
                "evidence path is not a bounded plain file",
            )?;
            let file = File::open(path)?;
            let identity = file_identity(&file.metadata()?);
            require(
                file_identity(&path_metadata) == identity,
                "evidence path changed while opening",
            )?;
            self.verify_managed_parent(path)?;
            self.repository.verify_file(path, &file, identity)?;
            read_bound_file(&file, limit)?
        };
        self.verify()?;
        Ok(bytes)
    }

    /// Verify that a managed file is owned by the retained parent or lifecycle directory.
    fn verify_managed_parent(&self, path: &Path) -> Result<(), CalibrationError> {
        let parent = path
            .parent()
            .ok_or_else(|| CalibrationError::Binding("evidence path has no parent".into()))?;
        if parent == self.run_directory {
            self.repository
                .verify_directory(parent, self.run_directory_identity)
        } else if self.aggregate_path.parent() == Some(parent) {
            self.repository
                .verify_directory(parent, self.parent_identity)
        } else {
            Err(CalibrationError::Binding(
                "evidence file is outside the journal-owned directories".into(),
            ))
        }
    }

    /// Read one typed record by journal filename.
    fn read_json<T: for<'de> Deserialize<'de>>(
        &self,
        filename: &str,
        limit: u64,
    ) -> Result<T, CalibrationError> {
        self.read_path_json(&self.run_directory.join(filename), limit)
    }

    /// Read one typed bounded record by exact path.
    fn read_path_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &Path,
        limit: u64,
    ) -> Result<T, CalibrationError> {
        serde_json::from_slice(&self.read_path(path, limit)?).map_err(Into::into)
    }
}

/// Build digest-only stream metadata.
fn stream_evidence<'a>(stream: &'a CapturedStream, filename: &'a str) -> StreamEvidence<'a> {
    StreamEvidence {
        bytes: stream.retained_bytes,
        file: filename,
        sha256: &stream.retained_sha256,
    }
}

/// Write one transient child result with no-clobber semantics.
fn write_json_create_new<T: Serialize>(
    path: &Path,
    value: &T,
    limit: u64,
) -> Result<(), CalibrationError> {
    let bytes = serde_json::to_vec(value)?;
    require(
        bytes.len() as u64 <= limit,
        "child result exceeds its byte limit",
    )?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Return the canonical repository root.
fn repository_root() -> Result<PathBuf, CalibrationError> {
    fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).map_err(Into::into)
}

/// Render a path without lossy conversion.
fn path_text(path: &Path) -> Result<String, CalibrationError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CalibrationError::Binding("path is not valid Unicode".into()))
}

/// Hash an exact executable and argument tuple.
fn command_sha256(program: &str, arguments: &[String]) -> Result<String, CalibrationError> {
    Ok(sha256_hex(&serde_json::to_vec(&(program, arguments))?))
}

/// Hash one bounded regular file.
fn sha256_file(path: &Path, limit: u64) -> Result<String, CalibrationError> {
    let metadata = fs::metadata(path)?;
    require(
        metadata.is_file() && metadata.len() <= limit,
        "hashed path is not a bounded regular file",
    )?;
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    let mut read = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read = read.saturating_add(count as u64);
        require(read <= limit, "hashed file grew beyond its byte limit")?;
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Return a lowercase SHA-256 digest.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Return Unix epoch milliseconds.
fn unix_millis() -> Result<u128, CalibrationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| CalibrationError::Binding(error.to_string()))
}

/// Convert elapsed time into saturated nanoseconds.
fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Convert one failed predicate into a typed binding error.
fn require(condition: bool, message: &str) -> Result<(), CalibrationError> {
    if condition {
        Ok(())
    } else {
        Err(CalibrationError::Binding(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::process::{Command as StdCommand, Stdio};
    use std::thread;
    use tempfile::tempdir;

    /// Frozen digest for three deterministic 4 KiB BLAKE3 buffers.
    const SMALL_BLAKE3_DIGEST: &str =
        "d808feb1fcb477157c1975e2b687680fcb81447ab354df3085c17974690a40c7";
    /// Select the real-child redaction probe.
    const REDACTION_PROBE_ENV: &str = "PROJECTATLAS_REDACTION_PROBE";
    /// Single-use digest-only child result path.
    const REDACTION_RESULT_ENV: &str = "PROJECTATLAS_REDACTION_RESULT";
    /// Exact test-harness child entry.
    const REDACTION_PROBE_TEST: &str =
        "calibration_evidence_runner::tests::redaction_subprocess_probe";
    /// Select the hanging workload supervision probe role.
    const WORKLOAD_PROBE_ENV: &str = "PROJECTATLAS_WORKLOAD_PROBE";
    /// Destination written only if a descendant escapes workload teardown.
    const WORKLOAD_MARKER_ENV: &str = "PROJECTATLAS_WORKLOAD_MARKER";
    /// Destination proving that the hanging leader spawned its descendant.
    const WORKLOAD_READY_ENV: &str = "PROJECTATLAS_WORKLOAD_READY";
    /// Exact test-harness workload supervision probe.
    const WORKLOAD_PROBE_TEST: &str =
        "calibration_evidence_runner::tests::workload_supervision_probe";
    /// Select the real-child poisoned Git-environment probe.
    const GIT_ENVIRONMENT_PROBE_ENV: &str = "PROJECTATLAS_GIT_ENVIRONMENT_PROBE";
    /// Single-use poisoned Git-environment probe result.
    const GIT_ENVIRONMENT_RESULT_ENV: &str = "PROJECTATLAS_GIT_ENVIRONMENT_RESULT";
    /// Exact test-harness Git environment probe.
    const GIT_ENVIRONMENT_PROBE_TEST: &str =
        "calibration_evidence_runner::tests::git_environment_subprocess_probe";

    /// Child result for a real source-binding attempt under a poisoned environment.
    #[derive(Debug, Deserialize, Serialize)]
    struct GitEnvironmentProbeResult {
        /// Successful source binding when every poisoned input was excluded.
        binding: Option<SourceBinding>,
        /// Typed failure text when resolution correctly failed closed.
        error: Option<String>,
    }

    /// Create a real directory symlink for POSIX containment tests.
    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> Result<(), CalibrationError> {
        std::os::unix::fs::symlink(target, link).map_err(Into::into)
    }

    /// Create a real junction without requiring Windows symbolic-link privileges.
    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> Result<(), CalibrationError> {
        let command = env::var_os("COMSPEC").unwrap_or_else(|| OsString::from("cmd.exe"));
        let output = StdCommand::new(command)
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()?;
        require(
            output.status.success(),
            "Windows junction fixture could not be created",
        )
    }

    /// Remove only the POSIX link entry created by the containment fixture.
    #[cfg(unix)]
    fn remove_directory_link(link: &Path) -> Result<(), CalibrationError> {
        fs::remove_file(link).map_err(Into::into)
    }

    /// Remove only the Windows junction entry created by the containment fixture.
    #[cfg(windows)]
    fn remove_directory_link(link: &Path) -> Result<(), CalibrationError> {
        fs::remove_dir(link).map_err(Into::into)
    }

    /// Run one bounded absolute Git process while constructing an isolated test repository.
    async fn run_git_fixture_process(
        executable: &Path,
        arguments: &[OsString],
    ) -> Result<SupervisedCommandOutput, CalibrationError> {
        let executable_directory = executable.parent().ok_or_else(|| {
            CalibrationError::Binding("Git fixture executable has no parent".into())
        })?;
        let command = Command::new(executable)
            .args(arguments)
            .env_clear()
            .env("PATH", executable_directory)
            .env("GIT_CONFIG_GLOBAL", git_null_device())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(windows)]
        let command = {
            let mut command = command;
            for name in ["SYSTEMROOT", "WINDIR"] {
                if let Some(value) = env::var_os(name) {
                    command = command.env(name, value);
                }
            }
            command
        };
        run_supervised(command, Duration::from_secs(15), 32 * 1024)
            .await
            .map_err(Into::into)
    }

    /// Require one bounded fixture-construction Git command to succeed.
    async fn run_git_fixture_command(
        executable: &Path,
        arguments: &[OsString],
    ) -> Result<Vec<u8>, CalibrationError> {
        let output = run_git_fixture_process(executable, arguments).await?;
        require(output.is_success(), "Git fixture command failed")?;
        Ok(output.stdout.retained)
    }

    /// Build a Windows filter command that proves execution through one marker file.
    #[cfg(windows)]
    fn hostile_filter_command(
        _root: &Path,
        marker: &Path,
        _filter_kind: &str,
    ) -> Result<String, CalibrationError> {
        let marker = marker
            .to_str()
            .ok_or_else(|| CalibrationError::Binding("marker path is not Unicode".into()))?
            .replace('\\', "/");
        Ok(format!("cmd.exe /D /C echo invoked^>\"{marker}\""))
    }

    /// Build a POSIX filter command that proves execution through one marker file.
    #[cfg(unix)]
    fn hostile_filter_command(
        root: &Path,
        marker: &Path,
        filter_kind: &str,
    ) -> Result<String, CalibrationError> {
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

    /// Prove ordinary Git reaches a hostile filter while provenance rejects it pre-execution.
    async fn assert_executable_filter_is_never_run(
        filter_kind: &str,
    ) -> Result<(), CalibrationError> {
        let trusted_git = RepositoryGitProbe::resolve(&repository_root()?)?
            .executable()
            .to_owned();
        let directory = tempdir()?;
        let root = directory.path().join("repository with filters");
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        let root_argument = root.as_os_str().to_owned();
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument.clone(),
                OsString::from("init"),
                OsString::from("--quiet"),
            ],
        )
        .await?;

        let driver = format!("hostile-{filter_kind}");
        fs::write(
            root.join(".gitattributes"),
            format!("payload.txt filter={driver}\n"),
        )?;
        fs::write(root.join("payload.txt"), b"alpha\n")?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument.clone(),
                OsString::from("add"),
                OsString::from("--all"),
            ],
        )
        .await?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument.clone(),
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )
        .await?;

        let marker = root.join(format!("{filter_kind}-filter-invoked"));
        let command = hostile_filter_command(&root, &marker, filter_kind)?;
        for (key, value) in [
            (format!("filter.{driver}.{filter_kind}"), command),
            (format!("filter.{driver}.required"), "true".into()),
        ] {
            run_git_fixture_command(
                &trusted_git,
                &[
                    OsString::from("-C"),
                    root_argument.clone(),
                    OsString::from("config"),
                    OsString::from(key),
                    OsString::from(value),
                ],
            )
            .await?;
        }
        fs::write(root.join("payload.txt"), b"bravo\n")?;

        let _unsanitized = run_git_fixture_process(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument,
                OsString::from("status"),
                OsString::from("--porcelain=v1"),
            ],
        )
        .await?;
        require(
            marker.is_file(),
            "hostile filter fixture was not executable through ordinary Git",
        )?;
        fs::remove_file(&marker)?;

        let git = RepositoryGitProbe::resolve(&root)?;
        let result = git.worktree_state().await;
        require(
            result.is_ok_and(|status| !status.is_empty()),
            "calibration provenance did not report raw filter-transformed bytes as dirty",
        )?;
        require(
            !marker.exists(),
            "sanitized calibration comparison executed a repository filter",
        )
    }

    /// A repository-selected clean filter is never executed by calibration provenance.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_never_executes_clean_filters() -> Result<(), CalibrationError> {
        assert_executable_filter_is_never_run("clean").await
    }

    /// A repository-selected process filter is never executed by calibration provenance.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_never_executes_process_filters() -> Result<(), CalibrationError> {
        assert_executable_filter_is_never_run("process").await
    }

    /// Declared and canonical CRLF materialization remain clean without filter drivers.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_accepts_declared_and_canonical_crlf_materialization()
    -> Result<(), CalibrationError> {
        let trusted_git = RepositoryGitProbe::resolve(&repository_root()?)?
            .executable()
            .to_owned();
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let root_argument = root.as_os_str().to_owned();
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument.clone(),
                OsString::from("init"),
                OsString::from("--quiet"),
            ],
        )
        .await?;
        fs::write(root.join(".gitattributes"), b"*.ps1 text eol=crlf\n")?;
        fs::write(root.join("script.ps1"), b"Write-Output 'clean'\n")?;
        fs::write(root.join("script.txt"), b"first\nsecond\n")?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument.clone(),
                OsString::from("add"),
                OsString::from("--all"),
            ],
        )
        .await?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                root_argument,
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )
        .await?;
        fs::write(root.join("script.ps1"), b"Write-Output 'clean'\r\n")?;
        fs::write(root.join("script.txt"), b"first\r\nsecond\r\n")?;

        let git = RepositoryGitProbe::resolve(&root)?;
        require(
            git.worktree_state().await?.is_empty(),
            "declared or canonical CRLF materialization was not recognized as Git-clean",
        )
    }

    /// The runner surface is closed and rejects duplicate flags.
    #[test]
    fn runner_arguments_are_closed() -> Result<(), CalibrationError> {
        require(
            matches!(
                parse_runner_command(
                    [
                        "run",
                        "--manifest",
                        "manifest.json",
                        "--position",
                        "before",
                        "--run-id",
                        "pilot-1",
                    ]
                    .into_iter()
                    .map(std::ffi::OsString::from),
                )?,
                RunnerCommand::Run { .. }
            ) && parse_runner_command(
                [
                    "run",
                    "--manifest",
                    "one",
                    "--manifest",
                    "two",
                    "--position",
                    "before",
                    "--run-id",
                    "pilot",
                ]
                .into_iter()
                .map(std::ffi::OsString::from),
            )
            .is_err()
                && parse_runner_command(
                    [
                        "run",
                        "--manifest",
                        "manifest.json",
                        "--position",
                        "before",
                        "--run-id",
                        "pilot",
                        "--unexpected",
                        "value",
                    ]
                    .into_iter()
                    .map(std::ffi::OsString::from),
                )
                .is_err(),
            "runner accepted an open or duplicate argument shape",
        )
    }

    /// Claim-eligible execution requires the dedicated release example.
    #[test]
    fn runner_execution_rejects_debug_or_wrong_executable() -> Result<(), CalibrationError> {
        let filename = format!("{RUNNER_EXAMPLE_NAME}{}", std::env::consts::EXE_SUFFIX);
        let release = PathBuf::from("target")
            .join("release")
            .join("examples")
            .join(filename);
        let debug = PathBuf::from("target").join("debug").join("examples").join(
            release.file_name().ok_or_else(|| {
                CalibrationError::Binding("release fixture has no filename".into())
            })?,
        );
        let wrong = release.with_file_name(format!("projectatlas{}", std::env::consts::EXE_SUFFIX));
        require(
            validate_runner_execution(&release, false).is_ok()
                && validate_runner_execution(&release, true).is_err()
                && validate_runner_execution(&debug, false).is_err()
                && validate_runner_execution(&wrong, false).is_err(),
            "runner eligibility accepted debug, wrong-profile, or wrong-role execution",
        )
    }

    /// Every manifest-owned output is ignored by the checked-in repository policy.
    #[tokio::test(flavor = "current_thread")]
    async fn manifest_outputs_are_ignored_by_checked_in_git_policy() -> Result<(), CalibrationError>
    {
        let root = repository_root()?;
        let manifest_path = root.join("docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
        let policy = calibration_policy(&manifest_path)?;
        let git = RepositoryGitProbe::resolve(&root)?;
        for position in [RunPosition::Before, RunPosition::After] {
            let paths = output_paths(&policy.manifest, position)?;
            for relative_path in [paths.aggregate, paths.raw_attempts] {
                let relative_path = path_text(&relative_path)?;
                let arguments = [
                    "--no-literal-pathspecs",
                    "check-ignore",
                    "--no-index",
                    "--verbose",
                    "--",
                    relative_path.as_str(),
                ];
                let output = git_text_output(&git, &arguments).await?;
                let not_ignored = format!("manifest output {relative_path} is not ignored by Git");
                require(!output.is_empty(), &not_ignored)?;
                let locally_ignored = format!(
                    "manifest output {relative_path} is ignored only by local Git metadata"
                );
                require(
                    output.lines().all(|line| line.starts_with(".gitignore:")),
                    &locally_ignored,
                )?;
            }
        }
        Ok(())
    }

    /// Declared and before/after observed environment mismatches fail closed.
    #[test]
    fn observed_environment_mismatches_are_rejected() -> Result<(), CalibrationError> {
        let manifest = serde_json::json!({
            "calibration": {"reference_environment": "reference"},
            "environments": [{
                "id": "reference",
                "os_family": std::env::consts::OS,
                "architecture": std::env::consts::ARCH
            }],
            "reproduction": {
                "reference_host_eligibility": {"environment_id": "reference"}
            }
        });
        let expected = ObservedEnvironmentBinding {
            reference_environment_id: "reference".into(),
            observed_os_family: std::env::consts::OS.into(),
            observed_architecture: std::env::consts::ARCH.into(),
            controlled_environment_sha256: "a".repeat(64),
            executable_sha256: "b".repeat(64),
        };
        validate_observed_environment(&manifest, &expected)?;

        let mut mismatch = expected.clone();
        mismatch.observed_os_family = "mismatched-os".into();
        require(
            validate_observed_environment(&manifest, &mismatch).is_err()
                && validate_environment_match(&expected, &mismatch).is_err(),
            "OS mismatch remained eligible",
        )?;
        mismatch = expected.clone();
        mismatch.observed_architecture = "mismatched-architecture".into();
        require(
            validate_observed_environment(&manifest, &mismatch).is_err()
                && validate_environment_match(&expected, &mismatch).is_err(),
            "architecture mismatch remained eligible",
        )?;
        mismatch = expected.clone();
        mismatch.controlled_environment_sha256 = "c".repeat(64);
        require(
            validate_environment_match(&expected, &mismatch).is_err(),
            "controlled environment mismatch remained eligible",
        )?;
        mismatch = expected.clone();
        mismatch.executable_sha256 = "d".repeat(64);
        require(
            validate_environment_match(&expected, &mismatch).is_err(),
            "executable mismatch remained eligible",
        )
    }

    /// Transient plaintext cannot enter serialized environment evidence.
    #[test]
    fn retained_environment_is_digest_only() -> Result<(), CalibrationError> {
        let canary = "projectatlas-environment-plaintext-canary";
        let entries = vec![
            TransientEnvironmentEntry {
                name: "DEVELOPER_DIR".into(),
                value: canary.into(),
            },
            TransientEnvironmentEntry {
                name: "RUST_BACKTRACE".into(),
                value: "0".into(),
            },
        ];
        let bytes = serde_json::to_vec(&retained_environment(&entries))?;
        let text = String::from_utf8(bytes)
            .map_err(|error| CalibrationError::Binding(error.to_string()))?;
        require(
            !text.contains(canary)
                && text.contains("DEVELOPER_DIR")
                && text.contains(&sha256_hex(canary.as_bytes())),
            "retained environment exposed plaintext or lost its digest",
        )
    }

    /// Produce digest-only evidence from a canary received through a real process environment.
    #[test]
    fn redaction_subprocess_probe() -> Result<(), CalibrationError> {
        if env::var(REDACTION_PROBE_ENV).as_deref() != Ok("child") {
            return Ok(());
        }
        let result_path = env::var(REDACTION_RESULT_ENV)
            .map(PathBuf::from)
            .map_err(|error| CalibrationError::Binding(error.to_string()))?;
        let evidence = retained_environment(&controlled_environment()?);
        write_json_create_new(&result_path, &evidence, CONTROL_FILE_LIMIT)
    }

    /// Capture a real source-binding result from a child with caller-controlled Git variables.
    #[tokio::test(flavor = "current_thread")]
    async fn git_environment_subprocess_probe() -> Result<(), CalibrationError> {
        if env::var(GIT_ENVIRONMENT_PROBE_ENV).as_deref() != Ok("child") {
            return Ok(());
        }
        let result_path = env::var(GIT_ENVIRONMENT_RESULT_ENV)
            .map(PathBuf::from)
            .map_err(|error| CalibrationError::Binding(error.to_string()))?;
        let result = match source_binding(&repository_root()?).await {
            Ok(binding) => GitEnvironmentProbeResult {
                binding: Some(binding),
                error: None,
            },
            Err(error) => GitEnvironmentProbeResult {
                binding: None,
                error: Some(error.to_string()),
            },
        };
        write_json_create_new(&result_path, &result, CONTROL_FILE_LIMIT)
    }

    /// Exercise a hanging workload leader and descendant in real subprocesses.
    #[test]
    fn workload_supervision_probe() -> Result<(), CalibrationError> {
        match env::var(WORKLOAD_PROBE_ENV).as_deref() {
            Ok("leader") => {
                let executable = env::current_exe()?;
                let marker = env::var(WORKLOAD_MARKER_ENV)
                    .map_err(|error| CalibrationError::Binding(error.to_string()))?;
                StdCommand::new(executable)
                    .args(["--exact", WORKLOAD_PROBE_TEST, "--nocapture"])
                    .env(WORKLOAD_PROBE_ENV, "descendant")
                    .env(WORKLOAD_MARKER_ENV, marker)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                fs::write(
                    env::var(WORKLOAD_READY_ENV)
                        .map_err(|error| CalibrationError::Binding(error.to_string()))?,
                    b"ready",
                )?;
                thread::sleep(Duration::from_secs(5));
            }
            Ok("descendant") => {
                thread::sleep(Duration::from_secs(1));
                fs::write(
                    env::var(WORKLOAD_MARKER_ENV)
                        .map_err(|error| CalibrationError::Binding(error.to_string()))?,
                    b"escaped",
                )?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Every workload deadline is enforced by `processkit` and tears down descendants.
    #[tokio::test(flavor = "current_thread")]
    async fn workload_timeout_is_bounded_and_tears_down_descendants() -> Result<(), CalibrationError>
    {
        let directory = tempdir()?;
        let ready = directory.path().join("ready");
        let marker = directory.path().join("escaped");
        let result = run_workload_process(
            Command::new(env::current_exe()?)
                .args(["--exact", WORKLOAD_PROBE_TEST, "--nocapture"])
                .env_clear()
                .env(WORKLOAD_PROBE_ENV, "leader")
                .env(WORKLOAD_READY_ENV, &ready)
                .env(WORKLOAD_MARKER_ENV, &marker),
            Duration::from_millis(500),
            8 * 1024,
        )
        .await;
        require(
            matches!(result, Err(CalibrationError::Workload(ref message)) if message.contains("deadline")),
            "hanging workload did not fail through its manifest deadline",
        )?;
        require(
            ready.is_file(),
            "workload leader never started its descendant",
        )?;
        thread::sleep(Duration::from_millis(1_100));
        require(
            !marker.exists(),
            "workload descendant survived its attempt deadline",
        )
    }

    /// Git provenance ignores inherited repository, index, config, helper, and pager overrides.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_uses_a_closed_environment() -> Result<(), CalibrationError> {
        let root = repository_root()?;
        let expected = source_binding(&root).await?;
        let directory = tempdir()?;
        let result_path = directory.path().join("git-environment.json");
        let poisoned_config = directory.path().join("poisoned.gitconfig");
        fs::write(
            &poisoned_config,
            b"[core]\n\trepositoryformatversion = 999\n\tfsmonitor = definitely-not-a-helper\n",
        )?;
        let output = run_supervised(
            Command::new(env::current_exe()?)
                .args(["--exact", GIT_ENVIRONMENT_PROBE_TEST, "--nocapture"])
                .env(GIT_ENVIRONMENT_PROBE_ENV, "child")
                .env(GIT_ENVIRONMENT_RESULT_ENV, &result_path)
                .env("GIT_DIR", directory.path().join("missing-git-dir"))
                .env("GIT_WORK_TREE", directory.path())
                .env("GIT_INDEX_FILE", directory.path().join("foreign-index"))
                .env("GIT_CONFIG_GLOBAL", &poisoned_config)
                .env("GIT_CONFIG_COUNT", "2")
                .env("GIT_CONFIG_KEY_0", "core.repositoryformatversion")
                .env("GIT_CONFIG_VALUE_0", "999")
                .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
                .env("GIT_CONFIG_VALUE_1", "definitely-not-a-helper")
                .env("GIT_EXEC_PATH", directory.path())
                .env("GIT_OBJECT_DIRECTORY", directory.path())
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", directory.path())
                .env("GIT_EXTERNAL_DIFF", "definitely-not-a-helper")
                .env("GIT_PAGER", "definitely-not-a-helper"),
            Duration::from_mins(2),
            32 * 1024,
        )
        .await?;
        require(
            output.is_success(),
            "poisoned-environment Git probe child failed",
        )?;
        let result: GitEnvironmentProbeResult = serde_json::from_slice(&fs::read(&result_path)?)?;
        let binding = result.binding.ok_or_else(|| {
            CalibrationError::Binding(format!(
                "closed Git probe rejected the real repository: {}",
                result.error.as_deref().unwrap_or("missing child error")
            ))
        })?;
        require(
            binding.git == expected.git
                && binding.head_commit == expected.head_commit
                && binding.cargo_lock_sha256 == expected.cargo_lock_sha256
                && binding.runner_source_sha256 == expected.runner_source_sha256,
            "poisoned Git variables changed the captured source binding",
        )
    }

    /// A PATH-prepended executable cannot silently replace the resolved Git identity.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_rejects_a_path_shim() -> Result<(), CalibrationError> {
        let directory = tempdir()?;
        let shim_directory = directory.path().join("shim");
        fs::create_dir(&shim_directory)?;
        let shim = shim_directory.join(format!("git{}", env::consts::EXE_SUFFIX));
        fs::copy(env::current_exe()?, &shim)?;
        let inherited_path = env::var_os("PATH")
            .ok_or_else(|| CalibrationError::Binding("test PATH is missing".into()))?;
        let mut entries = vec![shim_directory];
        entries.extend(env::split_paths(&inherited_path));
        let poisoned_path = env::join_paths(entries)
            .map_err(|error| CalibrationError::Binding(error.to_string()))?;
        let result_path = directory.path().join("git-path-shim.json");
        let output = run_supervised(
            Command::new(env::current_exe()?)
                .args(["--exact", GIT_ENVIRONMENT_PROBE_TEST, "--nocapture"])
                .env(GIT_ENVIRONMENT_PROBE_ENV, "child")
                .env(GIT_ENVIRONMENT_RESULT_ENV, &result_path)
                .env("PATH", poisoned_path),
            Duration::from_mins(2),
            32 * 1024,
        )
        .await?;
        require(output.is_success(), "PATH-shim Git probe child failed")?;
        let result: GitEnvironmentProbeResult = serde_json::from_slice(&fs::read(&result_path)?)?;
        require(
            result.binding.is_none()
                && result.error.as_deref().is_some_and(|error| {
                    error.contains("Git executable identity is ambiguous across PATH")
                }),
            "Git PATH shim did not fail closed as an ambiguous executable identity",
        )
    }

    /// Repository-local fsmonitor configuration cannot execute during provenance capture.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_disables_repository_fsmonitor() -> Result<(), CalibrationError> {
        let trusted_git = RepositoryGitProbe::resolve(&repository_root()?)?
            .executable()
            .to_owned();
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let mut init_arguments = vec![OsString::from("-C")];
        init_arguments.push(root.as_os_str().to_owned());
        init_arguments.extend([OsString::from("init"), OsString::from("--quiet")]);
        run_git_fixture_command(&trusted_git, &init_arguments).await?;
        fs::write(root.join("tracked.txt"), b"fixture\n")?;
        let mut add_arguments = vec![OsString::from("-C")];
        add_arguments.push(root.as_os_str().to_owned());
        add_arguments.extend([OsString::from("add"), OsString::from("tracked.txt")]);
        run_git_fixture_command(&trusted_git, &add_arguments).await?;
        let mut commit_arguments = vec![OsString::from("-C")];
        commit_arguments.push(root.as_os_str().to_owned());
        commit_arguments.extend([
            OsString::from("-c"),
            OsString::from("user.name=ProjectAtlas Test"),
            OsString::from("-c"),
            OsString::from("user.email=projectatlas@example.invalid"),
            OsString::from("commit"),
            OsString::from("--quiet"),
            OsString::from("-m"),
            OsString::from("fixture"),
        ]);
        run_git_fixture_command(&trusted_git, &commit_arguments).await?;

        let marker = root.join("fsmonitor-invoked");
        #[cfg(windows)]
        let hook = format!(
            "cmd.exe /D /C echo invoked^>\"{}\"",
            marker
                .to_str()
                .ok_or_else(|| CalibrationError::Binding("marker path is not Unicode".into()))?
                .replace('\\', "/")
        );
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
        let mut config_arguments = vec![OsString::from("-C")];
        config_arguments.push(root.as_os_str().to_owned());
        config_arguments.extend([
            OsString::from("config"),
            OsString::from("core.fsmonitor"),
            OsString::from(hook),
        ]);
        run_git_fixture_command(&trusted_git, &config_arguments).await?;

        let mut status_arguments = vec![OsString::from("-C")];
        status_arguments.push(root.as_os_str().to_owned());
        status_arguments.extend([OsString::from("status"), OsString::from("--porcelain=v1")]);
        run_git_fixture_command(&trusted_git, &status_arguments).await?;
        require(marker.is_file(), "fsmonitor fixture was not executable")?;
        fs::remove_file(&marker)?;

        let git = RepositoryGitProbe::resolve(&root)?;
        let _status = git.worktree_state().await?;
        require(
            !marker.exists(),
            "repository fsmonitor executed during closed Git provenance",
        )
    }

    /// Repository-local `core.worktree` cannot redirect provenance reads outside the bound root.
    #[tokio::test(flavor = "current_thread")]
    async fn git_provenance_pins_the_canonical_worktree() -> Result<(), CalibrationError> {
        let trusted_git = RepositoryGitProbe::resolve(&repository_root()?)?
            .executable()
            .to_owned();
        let directory = tempdir()?;
        let repository = directory.path().join("intended repository");
        let outside = directory.path().join("outside worktree");
        fs::create_dir(&repository)?;
        fs::create_dir(&outside)?;
        let repository = fs::canonicalize(repository)?;
        let outside = fs::canonicalize(outside)?;

        let repository_argument = repository.as_os_str().to_owned();
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                repository_argument.clone(),
                OsString::from("init"),
                OsString::from("--quiet"),
            ],
        )
        .await?;
        fs::write(repository.join("tracked.txt"), b"intended\n")?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                repository_argument.clone(),
                OsString::from("add"),
                OsString::from("--"),
                OsString::from("tracked.txt"),
            ],
        )
        .await?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                repository_argument.clone(),
                OsString::from("-c"),
                OsString::from("user.name=ProjectAtlas Test"),
                OsString::from("-c"),
                OsString::from("user.email=projectatlas@example.invalid"),
                OsString::from("commit"),
                OsString::from("--quiet"),
                OsString::from("-m"),
                OsString::from("fixture"),
            ],
        )
        .await?;
        run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                repository_argument.clone(),
                OsString::from("config"),
                OsString::from("core.worktree"),
                outside.as_os_str().to_owned(),
            ],
        )
        .await?;
        fs::write(outside.join("outside-marker.txt"), b"outside\n")?;

        let unsanitized = run_git_fixture_command(
            &trusted_git,
            &[
                OsString::from("-C"),
                repository_argument,
                OsString::from("status"),
                OsString::from("--porcelain=v1"),
                OsString::from("--untracked-files=all"),
            ],
        )
        .await?;
        require(
            String::from_utf8_lossy(&unsanitized).contains("outside-marker.txt"),
            "hostile core.worktree fixture did not redirect unsanitized Git",
        )?;

        let git = RepositoryGitProbe::resolve(&repository)?;
        let status = git.worktree_state().await?;
        require(
            status.is_empty(),
            "closed Git provenance escaped its canonical worktree",
        )
    }

    /// Plaintext crosses the child boundary but never enters its retained evidence.
    #[test]
    fn child_environment_evidence_is_redacted() -> Result<(), CalibrationError> {
        let directory = tempdir()?;
        let result_path = directory.path().join("environment.json");
        let canary = "projectatlas-real-child-environment-canary";
        let output = StdCommand::new(env::current_exe()?)
            .args(["--exact", REDACTION_PROBE_TEST, "--nocapture"])
            .env_clear()
            .env(REDACTION_PROBE_ENV, "child")
            .env(REDACTION_RESULT_ENV, &result_path)
            .env("CARGO_HOME", canary)
            .env("RUST_BACKTRACE", "0")
            .output()?;
        require(output.status.success(), "redaction child failed")?;
        let bytes = fs::read(&result_path)?;
        let evidence: Vec<RetainedEnvironmentEntry> = serde_json::from_slice(&bytes)?;
        let serialized = String::from_utf8(bytes)
            .map_err(|error| CalibrationError::Binding(error.to_string()))?;
        require(
            !serialized.contains(canary)
                && !String::from_utf8_lossy(&output.stdout).contains(canary)
                && !String::from_utf8_lossy(&output.stderr).contains(canary)
                && evidence.iter().any(|entry| {
                    entry.name == "CARGO_HOME"
                        && entry.present
                        && entry.value_sha256.as_deref() == Some(&sha256_hex(canary.as_bytes()))
                }),
            "real child environment evidence exposed plaintext or lost its digest",
        )
    }

    /// One journal rejects replacement and binds completion to retained files.
    #[test]
    fn evidence_journal_is_no_clobber() -> Result<(), CalibrationError> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let paths = OutputPaths {
            aggregate: root.join("pilot.json"),
            raw_attempts: root.join("attempts.jsonl"),
        };
        let mut journal = EvidenceJournal::reserve(&root, paths.clone())?;
        require(
            EvidenceJournal::reserve(&root, paths).is_err(),
            "journal reservation replaced existing evidence",
        )?;
        journal.stage = JournalStage::Execution;
        let error = CalibrationError::Workload("injected failure".into());
        let retained = journal.retain_failure(error);
        require(
            retained.to_string().contains("injected failure")
                && journal.run_directory.join(FAILURE_FILE).is_file(),
            "journal lost the original failure or marker",
        )
    }

    /// Out-of-root and linked parents fail before any external evidence write.
    #[test]
    fn evidence_journal_rejects_external_and_linked_parents() -> Result<(), CalibrationError> {
        let repository_directory = tempdir()?;
        let outside_directory = tempdir()?;
        let root = fs::canonicalize(repository_directory.path())?;
        let outside = fs::canonicalize(outside_directory.path())?;
        let outside_paths = OutputPaths {
            aggregate: outside.join("outside-pilot.json"),
            raw_attempts: outside.join("outside-attempts.jsonl"),
        };
        require(
            EvidenceJournal::reserve(&root, outside_paths).is_err()
                && fs::read_dir(&outside)?.next().is_none(),
            "out-of-root journal reservation wrote external evidence",
        )?;

        let linked_parent = root.join("linked-evidence");
        create_directory_link(&outside, &linked_parent)?;
        let linked_paths = OutputPaths {
            aggregate: linked_parent.join("linked-pilot.json"),
            raw_attempts: linked_parent.join("linked-attempts.jsonl"),
        };
        let rejected = EvidenceJournal::reserve(&root, linked_paths).is_err();
        let outside_is_empty = fs::read_dir(&outside)?.next().is_none();
        remove_directory_link(&linked_parent)?;
        require(
            rejected && outside_is_empty,
            "linked journal parent redirected evidence outside the repository",
        )
    }

    /// Replacing the raw-attempt pathname cannot redirect an append to another file.
    #[test]
    fn evidence_journal_rejects_raw_path_substitution() -> Result<(), CalibrationError> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let paths = OutputPaths {
            aggregate: root.join("pilot.json"),
            raw_attempts: root.join("attempts.jsonl"),
        };
        let journal = EvidenceJournal::reserve(&root, paths.clone())?;
        let original = root.join("original-attempts.jsonl");
        match fs::rename(&paths.raw_attempts, &original) {
            Ok(()) => {
                fs::write(&paths.raw_attempts, b"replacement")?;
                let sample = CalibrationSample {
                    workload_id: WorkloadKind::Blake3.manifest_id().into(),
                    phase: SamplePhase::Warmup,
                    repetition: 0,
                    started_unix_ms: 1,
                    duration_ns: 1,
                    output_sha256: "a".repeat(64),
                };
                let append = journal.append_sample(&sample);
                let original_bytes = fs::read(&original)?;
                require(
                    fs::read(&paths.raw_attempts)? == b"replacement"
                        && ((append.is_err() && original_bytes.is_empty())
                            || (append.is_ok() && !original_bytes.is_empty())),
                    "raw-attempt substitution redirected an append or lost the bound handle",
                )
            }
            Err(error) => {
                #[cfg(windows)]
                {
                    require(
                        matches!(
                            error.kind(),
                            std::io::ErrorKind::PermissionDenied
                                | std::io::ErrorKind::Other
                                | std::io::ErrorKind::ResourceBusy
                        ) && paths.raw_attempts.is_file(),
                        "Windows did not retain the raw-attempt handle against replacement",
                    )
                }
                #[cfg(unix)]
                {
                    Err(error.into())
                }
            }
        }
    }

    /// Small BLAKE3 workload remains deterministic without a 512 MiB allocation.
    #[test]
    fn small_blake3_workload_is_deterministic() -> Result<(), CalibrationError> {
        require(
            blake3_workload(3, 4 * 1024)? == SMALL_BLAKE3_DIGEST,
            "small BLAKE3 calibration digest drifted",
        )
    }

    /// Small `SQLite` workload performs real prepared transactional I/O.
    #[test]
    fn small_sqlite_workload_is_deterministic() -> Result<(), CalibrationError> {
        let first = sqlite_workload(1_024)?;
        require(
            first == sqlite_workload(1_024)? && first.len() == 64,
            "small SQLite calibration output drifted",
        )
    }

    /// Workload summaries reject inconsistent outputs and retain the measured median.
    #[test]
    fn workload_summary_is_fail_closed() -> Result<(), CalibrationError> {
        let mut samples = Vec::new();
        for (phase, count) in [
            (SamplePhase::Warmup, WARMUPS),
            (SamplePhase::Measured, REPETITIONS),
        ] {
            for repetition in 0..count {
                samples.push(CalibrationSample {
                    workload_id: WorkloadKind::Blake3.manifest_id().into(),
                    phase,
                    repetition,
                    started_unix_ms: 1,
                    duration_ns: repetition as u64 + 1,
                    output_sha256: "a".repeat(64),
                });
            }
        }
        let summary = summarize_workload(&samples, WorkloadKind::Blake3)?;
        samples[0].output_sha256 = "b".repeat(64);
        require(
            summary.measured_samples == REPETITIONS
                && summary.median_ns == 8
                && summarize_workload(&samples, WorkloadKind::Blake3).is_err(),
            "workload summary accepted inconsistent output or changed its median",
        )
    }

    /// A bounded real journal lifecycle retains success and failure evidence.
    #[tokio::test(flavor = "current_thread")]
    async fn evidence_journal_completes_real_bounded_lifecycles() -> Result<(), CalibrationError> {
        let directory = tempdir()?;
        let root = fs::canonicalize(directory.path())?;
        let paths = OutputPaths {
            aggregate: root.join("pilot.json"),
            raw_attempts: root.join("attempts.jsonl"),
        };
        let mut journal = EvidenceJournal::reserve(&root, paths.clone())?;
        let executable = env::current_exe()?;
        let executable_text = path_text(&executable)?;
        let executable_sha256 = sha256_file(&executable, u64::MAX)?;
        let environment = retained_environment(&[TransientEnvironmentEntry {
            name: "RUST_BACKTRACE".into(),
            value: "0".into(),
        }]);
        let manifest = serde_json::json!({
            "calibration": {"reference_environment": "test-host"},
            "environments": [{
                "id": "test-host",
                "os_family": std::env::consts::OS,
                "architecture": std::env::consts::ARCH
            }],
            "reproduction": {
                "reference_host_eligibility": {"environment_id": "test-host"}
            }
        });
        let observed_environment =
            observed_environment_binding(&manifest, &environment, &executable_sha256)?;
        let invocation_arguments = vec![
            "--exact".into(),
            REDACTION_PROBE_TEST.into(),
            "--nocapture".into(),
        ];
        let invocation = InvocationEvidence {
            executable: executable_text.clone(),
            executable_sha256: executable_sha256.clone(),
            build_profile: RunnerBuildProfile::Release,
            executable_role: RunnerExecutableRole::DedicatedCalibrationRunner,
            arguments: invocation_arguments.clone(),
            command_sha256: command_sha256(&executable_text, &invocation_arguments)?,
            environment,
        };
        let source = SourceBinding {
            git: GitExecutableBinding {
                path: "test-git".into(),
                sha256: "0".repeat(64),
                version: "git version test".into(),
            },
            head_commit: "1".repeat(40),
            dirty: false,
            worktree_state_sha256: "2".repeat(64),
            cargo_lock_sha256: "3".repeat(64),
            runner_source_sha256: "4".repeat(64),
        };
        let start_binding = journal.record_start(&StartEvidence {
            schema_version: 1,
            artifact_kind: ArtifactKind::Start,
            run_id: "bounded-lifecycle".into(),
            position: RunPosition::Before,
            started_unix_ms: unix_millis()?,
            source: source.clone(),
            invocation: invocation.clone(),
            observed_environment: observed_environment.clone(),
            provenance: vec![ProvenanceDigest {
                scope: ProvenanceScope::Environment,
                sha256: observed_environment.controlled_environment_sha256.clone(),
            }],
            claim_status: ClaimStatus::NotEvaluated,
        })?;

        journal.stage = JournalStage::Execution;
        let output = run_supervised(
            Command::new(&executable)
                .args(["--exact", REDACTION_PROBE_TEST, "--nocapture"])
                .env_clear(),
            Duration::from_secs(5),
            8 * 1024,
        )
        .await?;
        require(output.is_success(), "bounded lifecycle child failed")?;
        let process_binding = journal.record_process(&output)?;

        let mut samples = Vec::new();
        for kind in [WorkloadKind::Blake3, WorkloadKind::Sqlite] {
            for (phase, repetition) in [
                (SamplePhase::Warmup, 0_usize),
                (SamplePhase::Measured, 0_usize),
            ] {
                let sample = CalibrationSample {
                    workload_id: kind.manifest_id().into(),
                    phase,
                    repetition,
                    started_unix_ms: unix_millis()?,
                    duration_ns: if kind == WorkloadKind::Blake3 { 11 } else { 17 },
                    output_sha256: if kind == WorkloadKind::Blake3 {
                        "a".repeat(64)
                    } else {
                        "b".repeat(64)
                    },
                };
                journal.append_sample(&sample)?;
                samples.push(sample);
            }
        }
        let artifact = CalibrationArtifact {
            schema_version: 1,
            artifact_kind: ArtifactKind::Pilot,
            manifest_id: "bounded-test-manifest".into(),
            manifest_sha256: "5".repeat(64),
            run_id: "bounded-lifecycle".into(),
            position: RunPosition::Before,
            source_before: source.clone(),
            source_after: source,
            invocation,
            observed_environment: observed_environment.clone(),
            samples: samples.clone(),
            blake3: WorkloadSummary {
                workload_id: WorkloadKind::Blake3.manifest_id().into(),
                measured_samples: 1,
                median_ns: 11,
                output_sha256: "a".repeat(64),
            },
            sqlite: WorkloadSummary {
                workload_id: WorkloadKind::Sqlite.manifest_id().into(),
                measured_samples: 1,
                median_ns: 17,
                output_sha256: "b".repeat(64),
            },
            claim_status: ClaimStatus::NotEvaluated,
        };
        journal.stage = JournalStage::Aggregate;
        let artifact_binding = journal.publish_aggregate(&artifact)?;
        let raw_binding = journal.bind_raw_attempts()?;
        journal.stage = JournalStage::Completion;
        journal.publish_completion(&CompletionEvidence {
            schema_version: 1,
            artifact_kind: ArtifactKind::Completion,
            completed_unix_ms: unix_millis()?,
            artifact_sha256: artifact_binding.sha256,
            raw_attempts_sha256: raw_binding.sha256,
            process_sha256: process_binding.sha256,
            sample_count: samples.len(),
            claim_status: ClaimStatus::NotEvaluated,
        })?;

        let retained_artifact: CalibrationArtifact =
            serde_json::from_slice(&fs::read(&paths.aggregate)?)?;
        let retained_process: Value = journal.read_json(PROCESS_FILE, CONTROL_FILE_LIMIT)?;
        let completion: CompletionEvidence =
            journal.read_json(COMPLETION_FILE, CONTROL_FILE_LIMIT)?;
        require(
            start_binding.bytes > 0
                && retained_artifact.observed_environment == observed_environment
                && journal.read_samples()? == samples
                && completion.sample_count == samples.len()
                && retained_process.get("tree_terminated").is_none()
                && !journal.run_directory.join(FAILURE_FILE).exists(),
            "bounded success lifecycle lost or overstated evidence",
        )?;

        let failure_paths = OutputPaths {
            aggregate: root.join("failed-pilot.json"),
            raw_attempts: root.join("failed-attempts.jsonl"),
        };
        let mut failure_journal = EvidenceJournal::reserve(&root, failure_paths)?;
        failure_journal.stage = JournalStage::Execution;
        let retained = failure_journal.retain_failure(CalibrationError::Workload(
            "bounded injected failure".into(),
        ));
        require(
            retained.to_string().contains("bounded injected failure")
                && failure_journal.run_directory.join(FAILURE_FILE).is_file()
                && !failure_journal.run_directory.join(COMPLETION_FILE).exists(),
            "bounded failure lifecycle lost its ineligible marker",
        )
    }
}
