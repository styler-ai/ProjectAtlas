//! Validate and project read-only agent-efficiency benchmark evidence.

use crate::{ServiceError, ServiceResult};
use projectatlas_core::telemetry::{
    AgentEfficiencyArtifactIdentity, AgentEfficiencyBaseline, AgentEfficiencyBaselineRow,
    AgentEfficiencyBreakEven, AgentEfficiencyCapability, AgentEfficiencyCapabilityContribution,
    AgentEfficiencyComparison, AgentEfficiencyEvidenceState, AgentEfficiencyMetricComparison,
    AgentEfficiencyMetricKind, AgentEfficiencyProviderMetric, AgentEfficiencyProviderMetricKind,
};
use projectatlas_core::{repo_path_to_native, validated_repo_file_key};
use projectatlas_db::CapturedProjectBinding;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

/// Maximum benchmark-result bytes admitted by one optional token request.
pub(super) const BENCHMARK_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Maximum retained schedule or run rows.
const BENCHMARK_MAX_RUNS: usize = 256;
/// Maximum aggregate workload/arm groups.
const BENCHMARK_MAX_GROUPS: usize = 64;
/// Maximum candidate/baseline comparison groups.
const BENCHMARK_MAX_COMPARISONS: usize = 64;
/// Maximum values retained by one distribution.
const BENCHMARK_MAX_DISTRIBUTION_VALUES: usize = 32;
/// Maximum MCP calls retained by one run.
const BENCHMARK_MAX_MCP_CALLS_PER_RUN: usize = 128;
/// Maximum trace-completed `ProjectAtlas` MCP calls retained by one artifact.
const BENCHMARK_MAX_TOTAL_MCP_CALLS: usize = 4_096;
/// Largest integer admitted through JSON without loss of exactness.
const BENCHMARK_MAX_EXACT_INTEGER: u64 = (1_u64 << 53) - 1;
/// Maximum supported wall-clock duration for one benchmark measurement.
const BENCHMARK_MAX_WALL_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
/// Maximum caller-visible validation reason bytes.
const BENCHMARK_REASON_MAX_BYTES: usize = 240;
/// Supported benchmark result schema.
const BENCHMARK_SCHEMA_VERSION: u32 = 1;
/// Minimum publication repeat count.
const BENCHMARK_MIN_REPEAT_COUNT: usize = 3;
/// Defensive repeat-count ceiling.
const BENCHMARK_MAX_REPEAT_COUNT: usize = 16;
/// Candidate arm identifier.
const CANDIDATE_ARM: &str = "v0.4";
/// Frozen `ProjectAtlas` arm identifier.
const FROZEN_ARM: &str = "v0.3.26";
/// Plain Codex arm identifier.
const PLAIN_ARM: &str = "plain";
/// Supported candidate semantic identity.
const CANDIDATE_VERSION: &str = "projectatlas 0.4.0";
/// Supported frozen semantic identity.
const FROZEN_VERSION: &str = "projectatlas 0.3.26";
/// Required provider-accounting separation note.
const PROVIDER_USAGE_NOTE: &str =
    "Provider counters are reported separately and are not attributed causally to navigation.";
/// Candidate setup wall-time distribution key.
const SETUP_WALL_SECONDS_METRIC: &str = "setup_wall_seconds";
/// Per-task runtime wall-time distribution key.
const RUNTIME_WALL_SECONDS_METRIC: &str = "wall_seconds";
/// Required benchmark workloads.
const WORKLOADS: [&str; 5] = [
    "small-clean",
    "small-dirty",
    "small-non-git",
    "medium",
    "huge-vscode",
];
/// Required benchmark arms.
const ARMS: [&str; 3] = [CANDIDATE_ARM, FROZEN_ARM, PLAIN_ARM];
/// Metrics projected into the public typed comparison.
const METRICS: [(AgentEfficiencyMetricKind, &str); 18] = [
    (AgentEfficiencyMetricKind::TotalToolCalls, "tool_calls"),
    (AgentEfficiencyMetricKind::ProjectAtlasCalls, "mcp_calls"),
    (
        AgentEfficiencyMetricKind::ProductiveFolders,
        "productive_folders",
    ),
    (
        AgentEfficiencyMetricKind::ProductiveFiles,
        "productive_files",
    ),
    (
        AgentEfficiencyMetricKind::ProductiveRelations,
        "productive_relations",
    ),
    (AgentEfficiencyMetricKind::WrongFolders, "wrong_folders"),
    (AgentEfficiencyMetricKind::WrongFiles, "wrong_files"),
    (AgentEfficiencyMetricKind::WrongRelations, "wrong_relations"),
    (AgentEfficiencyMetricKind::BroadReads, "broad_reads"),
    (AgentEfficiencyMetricKind::FullReads, "full_reads"),
    (AgentEfficiencyMetricKind::Backtracks, "backtracks"),
    (
        AgentEfficiencyMetricKind::GrossNavigationBytes,
        "gross_navigation_bytes",
    ),
    (
        AgentEfficiencyMetricKind::NetNavigationBytes,
        "net_navigation_bytes",
    ),
    (
        AgentEfficiencyMetricKind::GrossNavigationTokens,
        "gross_navigation_tokens",
    ),
    (
        AgentEfficiencyMetricKind::NetNavigationTokens,
        "net_navigation_tokens",
    ),
    (
        AgentEfficiencyMetricKind::SetupWallSeconds,
        SETUP_WALL_SECONDS_METRIC,
    ),
    (
        AgentEfficiencyMetricKind::RuntimeWallSeconds,
        RUNTIME_WALL_SECONDS_METRIC,
    ),
    (
        AgentEfficiencyMetricKind::PersistentBytes,
        "post_trial_persistent_bytes",
    ),
];
/// Provider counters retained only as descriptive context.
const PROVIDER_METRICS: [(AgentEfficiencyProviderMetricKind, &str); 5] = [
    (
        AgentEfficiencyProviderMetricKind::InputTokens,
        "input_tokens",
    ),
    (
        AgentEfficiencyProviderMetricKind::CachedInputTokens,
        "cached_input_tokens",
    ),
    (
        AgentEfficiencyProviderMetricKind::CacheWriteInputTokens,
        "cache_write_input_tokens",
    ),
    (
        AgentEfficiencyProviderMetricKind::OutputTokens,
        "output_tokens",
    ),
    (
        AgentEfficiencyProviderMetricKind::ReasoningOutputTokens,
        "reasoning_output_tokens",
    ),
];

/// Load an optional benchmark artifact under the captured project binding.
pub(crate) fn load_agent_efficiency_comparison(
    binding: &CapturedProjectBinding,
    benchmark_results: Option<&Path>,
) -> ServiceResult<AgentEfficiencyComparison> {
    let Some(benchmark_results) = benchmark_results else {
        return Ok(AgentEfficiencyComparison::default());
    };
    let bytes = match read_benchmark_bytes(binding, benchmark_results)? {
        BenchmarkRead::Bytes(bytes) => bytes,
        BenchmarkRead::Failed(reason) => return Ok(failed_comparison(reason)),
    };
    let artifact = match serde_json::from_slice::<BenchmarkArtifact>(&bytes) {
        Ok(artifact) => artifact,
        Err(error) => {
            return Ok(failed_comparison(format!(
                "benchmark artifact is malformed: {error}"
            )));
        }
    };
    match validate_and_project(&artifact, &bytes) {
        Ok(comparison) => Ok(comparison),
        Err(reason) => Ok(incompatible_comparison(reason)),
    }
}

/// Result of a boundary-checked benchmark read.
enum BenchmarkRead {
    /// Exact bounded artifact bytes.
    Bytes(Vec<u8>),
    /// Non-boundary filesystem failure represented in the typed report.
    Failed(String),
}

/// Read one regular in-project artifact without following path indirection.
fn read_benchmark_bytes(
    binding: &CapturedProjectBinding,
    requested: &Path,
) -> ServiceResult<BenchmarkRead> {
    let key = validated_repo_file_key(requested)
        .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
    let root = Path::new(&binding.project_root);
    let relative = repo_path_to_native(&key);
    let path = root.join(&relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BenchmarkRead::Failed(
                    "benchmark artifact was not found".to_string(),
                ));
            }
            Err(error) => {
                return Ok(BenchmarkRead::Failed(format!(
                    "benchmark artifact metadata could not be read: {}",
                    error.kind()
                )));
            }
        };
        if metadata_is_indirect(&metadata) {
            return Err(ServiceError::InvalidInput(
                "benchmark artifact path contains a symlink or reparse point".to_string(),
            ));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|source| ServiceError::Io {
        path: path.clone(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(ServiceError::InvalidInput(
            "benchmark artifact path must name a regular file".to_string(),
        ));
    }
    if metadata.len() > BENCHMARK_MAX_BYTES as u64 {
        return Ok(BenchmarkRead::Failed(format!(
            "benchmark artifact exceeds the {BENCHMARK_MAX_BYTES}-byte limit"
        )));
    }
    let canonical_root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "selected project root could not be resolved: {}",
                error.kind()
            )));
        }
    };
    let canonical_path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact could not be resolved: {}",
                error.kind()
            )));
        }
    };
    if !canonical_path.starts_with(&canonical_root) {
        return Err(ServiceError::InvalidInput(
            "benchmark artifact resolves outside the selected project".to_string(),
        ));
    }
    let file = match open_benchmark_file(&canonical_path) {
        Ok(file) => file,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact could not be opened: {}",
                error.kind()
            )));
        }
    };
    let opened_canonical_path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact changed before the read: {}",
                error.kind()
            )));
        }
    };
    if opened_canonical_path != canonical_path
        || !opened_canonical_path.starts_with(&canonical_root)
    {
        return Ok(BenchmarkRead::Failed(
            "benchmark artifact changed before the read".to_string(),
        ));
    }
    let opened_metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact metadata could not be read from its open handle: {}",
                error.kind()
            )));
        }
    };
    if metadata_is_indirect(&opened_metadata)
        || !opened_metadata.file_type().is_file()
        || !benchmark_metadata_unchanged(&metadata, &opened_metadata)
    {
        return Ok(BenchmarkRead::Failed(
            "benchmark artifact changed before the read".to_string(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if let Err(error) = (&file)
        .take(BENCHMARK_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        return Ok(BenchmarkRead::Failed(format!(
            "benchmark artifact could not be read: {}",
            error.kind()
        )));
    }
    if bytes.len() > BENCHMARK_MAX_BYTES {
        return Ok(BenchmarkRead::Failed(format!(
            "benchmark artifact exceeds the {BENCHMARK_MAX_BYTES}-byte limit"
        )));
    }
    let final_metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact changed during the read: {}",
                error.kind()
            )));
        }
    };
    let read_metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact metadata changed during the read: {}",
                error.kind()
            )));
        }
    };
    let final_canonical_path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return Ok(BenchmarkRead::Failed(format!(
                "benchmark artifact changed during the read: {}",
                error.kind()
            )));
        }
    };
    if metadata_is_indirect(&final_metadata)
        || !final_metadata.file_type().is_file()
        || final_canonical_path != canonical_path
        || !final_canonical_path.starts_with(&canonical_root)
        || !benchmark_metadata_unchanged(&opened_metadata, &read_metadata)
        || !benchmark_metadata_unchanged(&opened_metadata, &final_metadata)
    {
        return Ok(BenchmarkRead::Failed(
            "benchmark artifact changed during the read".to_string(),
        ));
    }
    Ok(BenchmarkRead::Bytes(bytes))
}

/// Open one benchmark file while denying concurrent replacement where the platform supports it.
fn open_benchmark_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

/// Compare stable identity where available and modification metadata around one bounded read.
fn benchmark_metadata_unchanged(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if before.file_type().is_file() != after.file_type().is_file() || before.len() != after.len() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        before.creation_time() == after.creation_time()
            && before.last_write_time() == after.last_write_time()
            && before.file_attributes() == after.file_attributes()
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(any(unix, windows)))]
    {
        before.modified().ok() == after.modified().ok()
            && before.created().ok() == after.created().ok()
            && before.permissions().readonly() == after.permissions().readonly()
    }
}

/// Return whether metadata represents a symlink or Windows reparse point.
fn metadata_is_indirect(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Validate the supported contract and project the bounded public report.
fn validate_and_project(
    artifact: &BenchmarkArtifact,
    source_bytes: &[u8],
) -> Result<AgentEfficiencyComparison, String> {
    validate_identity(artifact)?;
    let runs = validate_schedule_and_runs(artifact)?;
    validate_aggregate(artifact, &runs)?;
    let baselines = [
        (AgentEfficiencyBaseline::FrozenProjectAtlasV0326, FROZEN_ARM),
        (AgentEfficiencyBaseline::PlainCodex, PLAIN_ARM),
    ]
    .into_iter()
    .map(|(baseline, arm)| project_baseline(artifact, baseline, arm))
    .collect::<Result<Vec<_>, _>>()?;
    let capabilities = project_capabilities(&artifact.runs)?;
    let state = if baselines
        .iter()
        .all(|row| row.state == AgentEfficiencyEvidenceState::Compatible)
    {
        AgentEfficiencyEvidenceState::Compatible
    } else if baselines
        .iter()
        .any(|row| row.state == AgentEfficiencyEvidenceState::Compatible)
        || baselines
            .iter()
            .any(|row| row.state == AgentEfficiencyEvidenceState::Partial)
    {
        AgentEfficiencyEvidenceState::Partial
    } else {
        AgentEfficiencyEvidenceState::Failed
    };
    let reason = (state != AgentEfficiencyEvidenceState::Compatible)
        .then(|| "retained benchmark failures are excluded from matched denominators".to_string());
    let candidate = artifact
        .candidate_identities
        .get(CANDIDATE_ARM)
        .ok_or_else(|| "candidate identity is missing".to_string())?;
    let frozen = artifact
        .candidate_identities
        .get(FROZEN_ARM)
        .ok_or_else(|| "frozen identity is missing".to_string())?;
    Ok(AgentEfficiencyComparison {
        state,
        reason,
        artifact: Some(AgentEfficiencyArtifactIdentity {
            schema_version: artifact.schema_version,
            artifact_digest_kind: "blake3".to_string(),
            artifact_digest: blake3::hash(source_bytes).to_hex().to_string(),
            candidate_version: candidate
                .version
                .clone()
                .ok_or_else(|| "candidate version is missing".to_string())?,
            candidate_runtime_sha256: candidate
                .runtime_sha256
                .clone()
                .ok_or_else(|| "candidate runtime digest is missing".to_string())?,
            candidate_source_head: artifact.candidate_source_identity.checkout_head.clone(),
            frozen_version: frozen
                .version
                .clone()
                .ok_or_else(|| "frozen version is missing".to_string())?,
            frozen_runtime_sha256: frozen
                .runtime_sha256
                .clone()
                .ok_or_else(|| "frozen runtime digest is missing".to_string())?,
        }),
        baselines,
        capabilities,
        provider_counters_descriptive_only: true,
    })
}

/// Validate schema, candidate, and source identities.
fn validate_identity(artifact: &BenchmarkArtifact) -> Result<(), String> {
    require(
        artifact.schema_version == BENCHMARK_SCHEMA_VERSION,
        "unsupported benchmark schema version",
    )?;
    require(
        (BENCHMARK_MIN_REPEAT_COUNT..=BENCHMARK_MAX_REPEAT_COUNT).contains(&artifact.repeat_count),
        "benchmark repeat count is outside the supported bounds",
    )?;
    require(
        artifact.all_scheduled_runs_retained,
        "benchmark does not retain every scheduled run",
    )?;
    require(
        artifact.candidate_identities.len() == ARMS.len(),
        "benchmark candidate inventory is incompatible",
    )?;
    for arm in ARMS {
        require(
            artifact.candidate_identities.contains_key(arm),
            "benchmark candidate inventory is incomplete",
        )?;
    }
    validate_projectatlas_identity(
        artifact
            .candidate_identities
            .get(CANDIDATE_ARM)
            .ok_or_else(|| "candidate identity is missing".to_string())?,
        CANDIDATE_VERSION,
    )?;
    validate_projectatlas_identity(
        artifact
            .candidate_identities
            .get(FROZEN_ARM)
            .ok_or_else(|| "frozen identity is missing".to_string())?,
        FROZEN_VERSION,
    )?;
    let plain = artifact
        .candidate_identities
        .get(PLAIN_ARM)
        .ok_or_else(|| "plain identity is missing".to_string())?;
    require(
        !plain.projectatlas,
        "plain control unexpectedly enables ProjectAtlas",
    )?;
    require(
        is_lower_hex(&artifact.candidate_source_identity.checkout_head, 40),
        "candidate source identity is malformed",
    )?;
    Ok(())
}

/// Validate one `ProjectAtlas` runtime and packaged-skill identity.
fn validate_projectatlas_identity(
    identity: &BenchmarkCandidateIdentity,
    expected_version: &str,
) -> Result<(), String> {
    require(identity.projectatlas, "ProjectAtlas arm is disabled")?;
    require(
        identity.version.as_deref() == Some(expected_version),
        "ProjectAtlas semantic identity is incompatible",
    )?;
    require(
        identity
            .runtime_sha256
            .as_deref()
            .is_some_and(|digest| is_lower_hex(digest, 64))
            && identity
                .skill_sha256
                .as_deref()
                .is_some_and(|digest| is_lower_hex(digest, 64)),
        "ProjectAtlas runtime or skill digest is malformed",
    )?;
    require(
        identity
            .skill_bytes
            .is_some_and(|value| (1..=BENCHMARK_MAX_EXACT_INTEGER).contains(&value))
            && identity
                .tool_discovery_bytes
                .is_some_and(|value| (1..=BENCHMARK_MAX_EXACT_INTEGER).contains(&value)),
        "ProjectAtlas skill or tool inventory is empty or exceeds the supported bound",
    )
}

/// Validate exact schedule/run retention and return rows by run id.
fn validate_schedule_and_runs(
    artifact: &BenchmarkArtifact,
) -> Result<HashMap<&str, &BenchmarkRun>, String> {
    let expected_rows = WORKLOADS
        .len()
        .checked_mul(ARMS.len())
        .and_then(|count| count.checked_mul(artifact.repeat_count))
        .ok_or_else(|| "benchmark schedule size overflowed".to_string())?;
    require(
        expected_rows <= BENCHMARK_MAX_RUNS
            && artifact.schedule.len() == expected_rows
            && artifact.runs.len() == expected_rows,
        "benchmark schedule or run count is incompatible",
    )?;
    let mut scheduled = HashMap::with_capacity(expected_rows);
    let mut expected_cells = HashSet::with_capacity(expected_rows);
    for row in &artifact.schedule {
        validate_schedule_row(row, artifact.repeat_count)?;
        require(
            scheduled.insert(row.run_id.as_str(), row).is_none(),
            "benchmark schedule contains duplicate run ids",
        )?;
        require(
            expected_cells.insert((row.workload.as_str(), row.arm.as_str(), row.repeat)),
            "benchmark schedule contains duplicate workload-arm-repeat cells",
        )?;
    }
    for workload in WORKLOADS {
        for arm in ARMS {
            for repeat in 1..=artifact.repeat_count {
                require(
                    expected_cells.contains(&(workload, arm, repeat)),
                    "benchmark schedule is missing a required workload-arm-repeat cell",
                )?;
            }
        }
    }
    let mut runs = HashMap::with_capacity(expected_rows);
    for run in &artifact.runs {
        let schedule = scheduled
            .get(run.run_id.as_str())
            .ok_or_else(|| "benchmark run is not scheduled".to_string())?;
        require(
            run.repeat == schedule.repeat
                && run.workload == schedule.workload
                && run.arm == schedule.arm,
            "benchmark run does not match its scheduled identity",
        )?;
        require(!run.excluded, "benchmark contains an excluded run")?;
        require(
            matches!(run.execution_status.as_str(), "completed" | "failed"),
            "benchmark run has an unsupported execution status",
        )?;
        let mut projectatlas_calls = 0usize;
        let mut completed_projectatlas_calls = 0usize;
        if let Some(trace) = run.trace.as_ref() {
            require(
                trace.mcp_calls.len() <= BENCHMARK_MAX_MCP_CALLS_PER_RUN,
                "benchmark run exceeds the MCP-call bound",
            )?;
            for call in &trace.mcp_calls {
                require(
                    !call.server.is_empty()
                        && call.server.len() <= 128
                        && !call.tool.is_empty()
                        && call.tool.len() <= 128
                        && matches!(call.status.as_str(), "completed" | "failed" | "in_progress")
                        && call.emitted_bytes <= BENCHMARK_MAX_EXACT_INTEGER,
                    "benchmark MCP-call identity, status, or emitted-byte value is invalid",
                )?;
                if call.server == "projectatlas" {
                    projectatlas_calls += 1;
                    completed_projectatlas_calls += usize::from(call.status == "completed");
                }
            }
        }
        require(
            run.arm != PLAIN_ARM || projectatlas_calls == 0,
            "plain benchmark run contains a ProjectAtlas MCP call",
        )?;
        require(
            run.execution_status != "completed"
                || run.arm == PLAIN_ARM
                || completed_projectatlas_calls > 0,
            "completed ProjectAtlas benchmark run has no completed ProjectAtlas MCP call",
        )?;
        require(
            runs.insert(run.run_id.as_str(), run).is_none(),
            "benchmark contains duplicate retained run ids",
        )?;
    }
    require(
        runs.len() == scheduled.len(),
        "benchmark does not retain every scheduled run",
    )?;
    Ok(runs)
}

/// Validate one schedule row.
fn validate_schedule_row(row: &BenchmarkSchedule, repeat_count: usize) -> Result<(), String> {
    require(
        !row.run_id.is_empty() && row.run_id.len() <= 128,
        "benchmark run id is empty or too long",
    )?;
    require(
        (1..=repeat_count).contains(&row.repeat),
        "benchmark repeat number is outside the declared range",
    )?;
    require(
        WORKLOADS.contains(&row.workload.as_str()),
        "benchmark workload inventory is incompatible",
    )?;
    require(
        ARMS.contains(&row.arm.as_str()),
        "benchmark arm inventory is incompatible",
    )
}

/// Validate aggregate counts, groups, distributions, comparisons, and provider truth.
fn validate_aggregate(
    artifact: &BenchmarkArtifact,
    runs: &HashMap<&str, &BenchmarkRun>,
) -> Result<(), String> {
    let completed = artifact
        .runs
        .iter()
        .filter(|run| run.execution_status == "completed")
        .count();
    let failed = artifact
        .runs
        .iter()
        .filter(|run| run.execution_status == "failed")
        .count();
    require(
        artifact.aggregate.scheduled == artifact.schedule.len()
            && artifact.aggregate.completed == completed
            && artifact.aggregate.failed == failed
            && artifact.aggregate.excluded == 0
            && completed + failed == artifact.schedule.len(),
        "benchmark aggregate run totals do not reconcile",
    )?;
    require(
        artifact.aggregate.all_run_ids.len() == artifact.runs.len(),
        "benchmark aggregate run-id inventory is incomplete",
    )?;
    let aggregate_ids = artifact
        .aggregate
        .all_run_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    require(
        aggregate_ids.len() == runs.len() && aggregate_ids.iter().all(|id| runs.contains_key(id)),
        "benchmark aggregate run-id inventory does not match retained runs",
    )?;
    require(
        artifact.aggregate.groups.len() == WORKLOADS.len() * ARMS.len()
            && artifact.aggregate.groups.len() <= BENCHMARK_MAX_GROUPS,
        "benchmark aggregate group inventory is incompatible",
    )?;
    for workload in WORKLOADS {
        for arm in ARMS {
            let key = group_key(workload, arm);
            let group = artifact
                .aggregate
                .groups
                .get(&key)
                .ok_or_else(|| "benchmark aggregate group is missing".to_string())?;
            validate_group(group, workload, arm, artifact.repeat_count, runs)?;
        }
    }
    require(
        artifact.aggregate.comparisons.len() == WORKLOADS.len() * 2
            && artifact.aggregate.comparisons.len() <= BENCHMARK_MAX_COMPARISONS,
        "benchmark comparison inventory is incompatible",
    )?;
    for workload in WORKLOADS {
        for baseline in [FROZEN_ARM, PLAIN_ARM] {
            let comparison_key = comparison_key(workload, baseline);
            let comparison = artifact
                .aggregate
                .comparisons
                .get(&comparison_key)
                .ok_or_else(|| "benchmark comparison is missing".to_string())?;
            let candidate = artifact
                .aggregate
                .groups
                .get(&group_key(workload, CANDIDATE_ARM))
                .ok_or_else(|| "candidate aggregate group is missing".to_string())?;
            let baseline_group = artifact
                .aggregate
                .groups
                .get(&group_key(workload, baseline))
                .ok_or_else(|| "baseline aggregate group is missing".to_string())?;
            validate_comparison(comparison, candidate, baseline_group)?;
        }
    }
    require(
        artifact.aggregate.provider_usage_note == PROVIDER_USAGE_NOTE,
        "provider counters are not labeled descriptive-only",
    )
}

/// Validate one workload/arm aggregate group.
fn validate_group(
    group: &BenchmarkGroup,
    workload: &str,
    arm: &str,
    repeat_count: usize,
    runs: &HashMap<&str, &BenchmarkRun>,
) -> Result<(), String> {
    require(
        group.scheduled == repeat_count
            && group.completed + group.failed + group.excluded == repeat_count
            && group.excluded == 0,
        "benchmark group totals do not reconcile",
    )?;
    require(
        group.completed == 0 || group.completed == repeat_count,
        "partially completed workload groups are unsupported",
    )?;
    require(
        group.run_ids.len() == repeat_count,
        "benchmark group run-id count is incompatible",
    )?;
    let mut group_run_ids = HashSet::with_capacity(repeat_count);
    let mut completed = 0usize;
    let mut failed = 0usize;
    for run_id in &group.run_ids {
        require(
            group_run_ids.insert(run_id.as_str()),
            "benchmark group contains duplicate run ids",
        )?;
        let run = runs
            .get(run_id.as_str())
            .ok_or_else(|| "benchmark group contains an unknown run id".to_string())?;
        require(
            run.workload == workload && run.arm == arm,
            "benchmark group run belongs to another workload or arm",
        )?;
        match run.execution_status.as_str() {
            "completed" => completed += 1,
            "failed" => failed += 1,
            _ => return Err("benchmark run has an unsupported execution status".to_string()),
        }
    }
    require(
        completed == group.completed && failed == group.failed,
        "benchmark group status counts do not match retained runs",
    )?;
    if group.completed == 0 {
        require(
            group.distributions.is_empty() && group.provider_usage.is_empty(),
            "failed benchmark group exposes fabricated distributions",
        )?;
        return Ok(());
    }
    require(
        group.distributions.len() <= 64,
        "benchmark group contains too many distributions",
    )?;
    require(
        group.provider_usage.len() == PROVIDER_METRICS.len(),
        "benchmark group provider-counter inventory is incompatible",
    )?;
    for (key, distribution) in &group.distributions {
        validate_distribution(distribution, group.completed, key)?;
    }
    for (_, key) in METRICS {
        if !group.distributions.contains_key(key) {
            return Err(format!("benchmark group is missing required metric {key}"));
        }
    }
    for (key, distribution) in &group.provider_usage {
        validate_distribution(distribution, group.completed, key)?;
    }
    for (_, key) in PROVIDER_METRICS {
        if !group.provider_usage.contains_key(key) {
            return Err(format!("benchmark group is missing provider counter {key}"));
        }
    }
    Ok(())
}

/// Validate one bounded numeric distribution.
fn validate_distribution(
    distribution: &BenchmarkDistribution,
    expected_count: usize,
    metric: &str,
) -> Result<(), String> {
    require(
        distribution.count == expected_count
            && distribution.values.len() == expected_count
            && distribution.values.len() <= BENCHMARK_MAX_DISTRIBUTION_VALUES,
        "benchmark distribution count does not reconcile",
    )?;
    let wall_seconds = metric.ends_with("_seconds");
    let valid_number = |value: f64| {
        finite_nonnegative(value)
            && value
                <= if wall_seconds {
                    BENCHMARK_MAX_WALL_SECONDS
                } else {
                    BENCHMARK_MAX_EXACT_INTEGER as f64
                }
    };
    let valid_sample = |value: f64| valid_number(value) && (wall_seconds || value.fract() == 0.0);
    require(
        !metric.is_empty()
            && metric.len() <= 128
            && distribution.observed_tail == "maximum"
            && valid_number(distribution.median)
            && valid_sample(distribution.maximum)
            && distribution.values.iter().copied().all(valid_sample),
        "benchmark distribution contains invalid numeric, integer, bound, or tail values",
    )?;
    let mut values = distribution.values.clone();
    values.sort_by(f64::total_cmp);
    let median =
        median(&values).ok_or_else(|| "benchmark distribution has no median value".to_string())?;
    let maximum = values
        .last()
        .copied()
        .ok_or_else(|| "benchmark distribution has no maximum value".to_string())?;
    require(
        approximately_equal(distribution.median, median)
            && approximately_equal(distribution.maximum, maximum),
        "benchmark distribution summary does not match retained values",
    )
}

/// Validate one candidate/baseline comparison against its owning groups.
fn validate_comparison(
    comparison: &BenchmarkComparison,
    candidate: &BenchmarkGroup,
    baseline: &BenchmarkGroup,
) -> Result<(), String> {
    if candidate.completed == 0 || baseline.completed == 0 {
        return require(
            comparison.lower_is_better_percent_savings.is_empty()
                && comparison.provider_usage_descriptive_only.is_empty()
                && comparison.wall_time_break_even_tasks.is_none(),
            "unmatched comparison exposes fabricated values",
        );
    }
    require(
        (METRICS.len()..=64).contains(&comparison.lower_is_better_percent_savings.len()),
        "matched comparison metric inventory is incomplete or too large",
    )?;
    for (key, row) in &comparison.lower_is_better_percent_savings {
        require(
            !key.is_empty() && key.len() <= 128,
            "matched comparison metric identity is invalid",
        )?;
        validate_comparison_metric(
            row,
            candidate
                .distributions
                .get(key)
                .ok_or_else(|| format!("candidate comparison distribution is missing for {key}"))?,
            baseline
                .distributions
                .get(key)
                .ok_or_else(|| format!("baseline comparison distribution is missing for {key}"))?,
        )
        .map_err(|reason| format!("{key}: {reason}"))?;
    }
    for (_, key) in METRICS {
        if !comparison.lower_is_better_percent_savings.contains_key(key) {
            return Err(format!("matched comparison is missing metric {key}"));
        }
    }
    require(
        comparison.provider_usage_descriptive_only.len() == PROVIDER_METRICS.len(),
        "provider comparison inventory is incomplete",
    )?;
    for (_, key) in PROVIDER_METRICS {
        let row = comparison
            .provider_usage_descriptive_only
            .get(key)
            .ok_or_else(|| format!("provider comparison is missing counter {key}"))?;
        require(
            !row.causal_attribution,
            "provider comparison claims causal attribution",
        )?;
        validate_provider_metric(
            row,
            candidate
                .provider_usage
                .get(key)
                .ok_or_else(|| "candidate provider distribution is missing".to_string())?,
            baseline
                .provider_usage
                .get(key)
                .ok_or_else(|| "baseline provider distribution is missing".to_string())?,
        )
        .map_err(|reason| format!("{key}: {reason}"))?;
    }
    require(
        comparison.wall_time_break_even_tasks == wall_time_break_even(candidate, baseline)?,
        "wall-time break-even value does not match validated setup and runtime medians",
    )
}

/// Validate one comparison metric against aggregate medians and maxima.
fn validate_comparison_metric(
    row: &BenchmarkComparisonMetric,
    candidate: &BenchmarkDistribution,
    baseline: &BenchmarkDistribution,
) -> Result<(), String> {
    let expected_saving = percent_saving(candidate.median, baseline.median);
    require(
        finite_nonnegative(row.candidate_median)
            && finite_nonnegative(row.baseline_median)
            && finite_nonnegative(row.candidate_tail)
            && finite_nonnegative(row.baseline_tail)
            && approximately_equal(row.candidate_median, candidate.median)
            && approximately_equal(row.baseline_median, baseline.median)
            && approximately_equal(row.candidate_tail, candidate.maximum)
            && approximately_equal(row.baseline_tail, baseline.maximum)
            && option_approximately_equal(row.median_percent_saving, expected_saving)
            && row.tail_statistic == "observed maximum",
        "comparison metric does not match its validated distributions",
    )
}

/// Validate one descriptive provider counter against aggregate distributions.
fn validate_provider_metric(
    row: &BenchmarkProviderComparison,
    candidate: &BenchmarkDistribution,
    baseline: &BenchmarkDistribution,
) -> Result<(), String> {
    require(
        finite_nonnegative(row.candidate_median)
            && finite_nonnegative(row.baseline_median)
            && finite_nonnegative(row.candidate_tail)
            && finite_nonnegative(row.baseline_tail)
            && approximately_equal(row.candidate_median, candidate.median)
            && approximately_equal(row.baseline_median, baseline.median)
            && approximately_equal(row.candidate_tail, candidate.maximum)
            && approximately_equal(row.baseline_tail, baseline.maximum)
            && !row.causal_attribution,
        "provider counter does not match its validated distributions",
    )
}

/// Project one baseline across workload groups that completed in both arms.
fn project_baseline(
    artifact: &BenchmarkArtifact,
    baseline: AgentEfficiencyBaseline,
    baseline_arm: &str,
) -> Result<AgentEfficiencyBaselineRow, String> {
    let mut candidate_values = METRICS
        .iter()
        .map(|(_, key)| (*key, Vec::<f64>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut baseline_values = METRICS
        .iter()
        .map(|(_, key)| (*key, Vec::<f64>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut candidate_provider = PROVIDER_METRICS
        .iter()
        .map(|(_, key)| (*key, Vec::<f64>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut baseline_provider = PROVIDER_METRICS
        .iter()
        .map(|(_, key)| (*key, Vec::<f64>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut matched_trials = 0usize;
    let mut candidate_failed_trials = 0usize;
    let mut baseline_failed_trials = 0usize;
    let mut unmatched_trials = 0usize;
    let mut break_even = Vec::with_capacity(WORKLOADS.len());
    for workload in WORKLOADS {
        let candidate = artifact
            .aggregate
            .groups
            .get(&group_key(workload, CANDIDATE_ARM))
            .ok_or_else(|| "candidate aggregate group is missing".to_string())?;
        let baseline_group = artifact
            .aggregate
            .groups
            .get(&group_key(workload, baseline_arm))
            .ok_or_else(|| "baseline aggregate group is missing".to_string())?;
        candidate_failed_trials = candidate_failed_trials.saturating_add(candidate.failed);
        baseline_failed_trials = baseline_failed_trials.saturating_add(baseline_group.failed);
        if candidate.completed > 0 && baseline_group.completed > 0 {
            require(
                candidate.completed == baseline_group.completed,
                "matched benchmark groups have different completed-trial counts",
            )?;
            matched_trials = matched_trials.saturating_add(candidate.completed);
            append_group_values(&mut candidate_values, &candidate.distributions, &METRICS)?;
            append_group_values(
                &mut baseline_values,
                &baseline_group.distributions,
                &METRICS,
            )?;
            append_provider_values(
                &mut candidate_provider,
                &candidate.provider_usage,
                &PROVIDER_METRICS,
            )?;
            append_provider_values(
                &mut baseline_provider,
                &baseline_group.provider_usage,
                &PROVIDER_METRICS,
            )?;
            break_even.push(AgentEfficiencyBreakEven {
                workload: workload.to_string(),
                wall_time_tasks: wall_time_break_even(candidate, baseline_group)?,
            });
        } else {
            unmatched_trials = unmatched_trials
                .saturating_add(candidate.completed)
                .saturating_add(baseline_group.completed);
        }
    }
    let state = if matched_trials == 0 {
        AgentEfficiencyEvidenceState::Failed
    } else if candidate_failed_trials > 0 || baseline_failed_trials > 0 || unmatched_trials > 0 {
        AgentEfficiencyEvidenceState::Partial
    } else {
        AgentEfficiencyEvidenceState::Compatible
    };
    let metrics = if matched_trials == 0 {
        Vec::new()
    } else {
        METRICS
            .into_iter()
            .map(|(metric, key)| {
                metric_comparison(
                    metric,
                    candidate_values
                        .get(key)
                        .ok_or_else(|| "candidate metric values are missing".to_string())?,
                    baseline_values
                        .get(key)
                        .ok_or_else(|| "baseline metric values are missing".to_string())?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let provider_usage_descriptive_only = if matched_trials == 0 {
        Vec::new()
    } else {
        PROVIDER_METRICS
            .into_iter()
            .map(|(metric, key)| {
                provider_metric(
                    metric,
                    candidate_provider
                        .get(key)
                        .ok_or_else(|| "candidate provider values are missing".to_string())?,
                    baseline_provider
                        .get(key)
                        .ok_or_else(|| "baseline provider values are missing".to_string())?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(AgentEfficiencyBaselineRow {
        baseline,
        state,
        matched_trials,
        candidate_failed_trials,
        baseline_failed_trials,
        unmatched_trials,
        metrics,
        break_even,
        provider_usage_descriptive_only,
    })
}

/// Derive tasks required to repay incremental setup time from validated medians.
fn wall_time_break_even(
    candidate: &BenchmarkGroup,
    baseline: &BenchmarkGroup,
) -> Result<Option<u64>, String> {
    let median = |group: &BenchmarkGroup, metric: &str| {
        group
            .distributions
            .get(metric)
            .map(|distribution| distribution.median)
            .ok_or_else(|| format!("benchmark group is missing required metric {metric}"))
    };
    let warm_saving = median(baseline, RUNTIME_WALL_SECONDS_METRIC)?
        - median(candidate, RUNTIME_WALL_SECONDS_METRIC)?;
    if warm_saving <= 0.0 {
        return Ok(None);
    }
    let incremental_setup = median(candidate, SETUP_WALL_SECONDS_METRIC)?
        - median(baseline, SETUP_WALL_SECONDS_METRIC)?;
    if incremental_setup <= 0.0 {
        return Ok(Some(0));
    }
    let tasks = (incremental_setup / warm_saving).ceil();
    require(
        tasks.is_finite() && tasks <= f64::from(u32::MAX),
        "wall-time break-even value exceeds the supported bound",
    )?;
    Ok(Some(tasks as u64))
}

/// Append matched distribution values for public navigation metrics.
fn append_group_values(
    destination: &mut BTreeMap<&'static str, Vec<f64>>,
    source: &BTreeMap<String, BenchmarkDistribution>,
    metrics: &[(AgentEfficiencyMetricKind, &'static str)],
) -> Result<(), String> {
    for (_, key) in metrics {
        destination
            .get_mut(key)
            .ok_or_else(|| "metric destination is missing".to_string())?
            .extend_from_slice(
                &source
                    .get(*key)
                    .ok_or_else(|| "metric distribution is missing".to_string())?
                    .values,
            );
    }
    Ok(())
}

/// Append matched distribution values for descriptive provider counters.
fn append_provider_values(
    destination: &mut BTreeMap<&'static str, Vec<f64>>,
    source: &BTreeMap<String, BenchmarkDistribution>,
    metrics: &[(AgentEfficiencyProviderMetricKind, &'static str)],
) -> Result<(), String> {
    for (_, key) in metrics {
        destination
            .get_mut(key)
            .ok_or_else(|| "provider destination is missing".to_string())?
            .extend_from_slice(
                &source
                    .get(*key)
                    .ok_or_else(|| "provider distribution is missing".to_string())?
                    .values,
            );
    }
    Ok(())
}

/// Build one aggregate navigation metric from matched trial values.
fn metric_comparison(
    metric: AgentEfficiencyMetricKind,
    candidate: &[f64],
    baseline: &[f64],
) -> Result<AgentEfficiencyMetricComparison, String> {
    let (candidate_median, candidate_maximum) = summarize_values(candidate)?;
    let (baseline_median, baseline_maximum) = summarize_values(baseline)?;
    Ok(AgentEfficiencyMetricComparison {
        metric,
        candidate_median,
        baseline_median,
        candidate_maximum,
        baseline_maximum,
        median_percent_saving: percent_saving(candidate_median, baseline_median),
    })
}

/// Build one aggregate descriptive provider counter.
fn provider_metric(
    metric: AgentEfficiencyProviderMetricKind,
    candidate: &[f64],
    baseline: &[f64],
) -> Result<AgentEfficiencyProviderMetric, String> {
    let (candidate_median, candidate_maximum) = summarize_values(candidate)?;
    let (baseline_median, baseline_maximum) = summarize_values(baseline)?;
    Ok(AgentEfficiencyProviderMetric {
        metric,
        candidate_median,
        baseline_median,
        candidate_maximum,
        baseline_maximum,
        causal_attribution: false,
    })
}

/// Return median and observed maximum for non-empty validated values.
fn summarize_values(values: &[f64]) -> Result<(f64, f64), String> {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let median = median(&values).ok_or_else(|| "matched benchmark values are empty".to_string())?;
    let maximum = values
        .last()
        .copied()
        .ok_or_else(|| "matched benchmark values are empty".to_string())?;
    Ok((median, maximum))
}

/// Group trace-completed candidate MCP calls by navigation responsibility.
fn project_capabilities(
    runs: &[BenchmarkRun],
) -> Result<Vec<AgentEfficiencyCapabilityContribution>, String> {
    let mut counts = [0usize; 5];
    let mut bytes = [0u64; 5];
    let mut total_calls = 0usize;
    for run in runs
        .iter()
        .filter(|run| run.arm == CANDIDATE_ARM && !run.excluded)
    {
        let Some(trace) = run.trace.as_ref() else {
            continue;
        };
        for call in &trace.mcp_calls {
            require(
                call.server == "projectatlas",
                "candidate MCP call belongs to another server",
            )?;
            if call.status != "completed" {
                continue;
            }
            total_calls = total_calls
                .checked_add(1)
                .ok_or_else(|| "candidate MCP-call count overflowed".to_string())?;
            require(
                total_calls <= BENCHMARK_MAX_TOTAL_MCP_CALLS,
                "artifact exceeds the trace-completed MCP-call bound",
            )?;
            let index = capability_index(&call.tool);
            counts[index] = counts[index]
                .checked_add(1)
                .ok_or_else(|| "capability call count overflowed".to_string())?;
            bytes[index] = bytes[index]
                .checked_add(call.emitted_bytes)
                .ok_or_else(|| "capability emitted-byte count overflowed".to_string())?;
        }
    }
    let capabilities = [
        AgentEfficiencyCapability::Discovery,
        AgentEfficiencyCapability::SummaryAndSlice,
        AgentEfficiencyCapability::Search,
        AgentEfficiencyCapability::SymbolsAndRelations,
        AgentEfficiencyCapability::Other,
    ];
    Ok(capabilities
        .into_iter()
        .enumerate()
        .filter(|(index, _)| counts[*index] > 0)
        .map(
            |(index, capability)| AgentEfficiencyCapabilityContribution {
                capability,
                calls: counts[index],
                emitted_bytes: bytes[index],
            },
        )
        .collect())
}

/// Return the stable capability bucket for one trace-completed MCP tool.
fn capability_index(tool: &str) -> usize {
    match tool {
        "atlas_session_brief"
        | "atlas_overview"
        | "atlas_folders"
        | "atlas_files"
        | "atlas_next" => 0,
        "atlas_file_summary" | "atlas_outline" | "atlas_slice" => 1,
        "atlas_search" => 2,
        "atlas_symbols" | "atlas_symbol_relations" => 3,
        _ => 4,
    }
}

/// Return a bounded failed evidence state.
fn failed_comparison(reason: String) -> AgentEfficiencyComparison {
    AgentEfficiencyComparison {
        state: AgentEfficiencyEvidenceState::Failed,
        reason: Some(bounded_reason(reason)),
        artifact: None,
        baselines: Vec::new(),
        capabilities: Vec::new(),
        provider_counters_descriptive_only: true,
    }
}

/// Return a bounded incompatible evidence state.
fn incompatible_comparison(reason: String) -> AgentEfficiencyComparison {
    AgentEfficiencyComparison {
        state: AgentEfficiencyEvidenceState::Incompatible,
        reason: Some(bounded_reason(reason)),
        artifact: None,
        baselines: Vec::new(),
        capabilities: Vec::new(),
        provider_counters_descriptive_only: true,
    }
}

/// Bound a caller-visible reason without splitting UTF-8.
fn bounded_reason(reason: String) -> String {
    if reason.len() <= BENCHMARK_REASON_MAX_BYTES {
        return reason;
    }
    let mut end = BENCHMARK_REASON_MAX_BYTES;
    while !reason.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    reason[..end].to_string()
}

/// Return a workload/arm aggregate key.
fn group_key(workload: &str, arm: &str) -> String {
    format!("{workload}/{arm}")
}

/// Return a workload candidate/baseline comparison key.
fn comparison_key(workload: &str, baseline: &str) -> String {
    format!("{workload}/{CANDIDATE_ARM}-vs-{baseline}")
}

/// Return a lower-is-better median saving or `None` for a zero denominator.
fn percent_saving(candidate: f64, baseline: f64) -> Option<f64> {
    (baseline > 0.0).then(|| (baseline - candidate) / baseline * 100.0)
}

/// Return the median of sorted values.
fn median(sorted: &[f64]) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        Some(f64::midpoint(sorted[middle - 1], sorted[middle]))
    } else {
        Some(sorted[middle])
    }
}

/// Return whether one numeric value is finite and nonnegative.
fn finite_nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// Compare serialized floating summaries with a scale-aware tolerance.
fn approximately_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * 1.0e-6
}

/// Compare optional floating summaries.
fn option_approximately_equal(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => approximately_equal(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Return whether a value is exact lowercase hexadecimal of the requested length.
fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Convert a boolean invariant into a bounded validation error.
fn require(condition: bool, reason: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(reason.to_string())
    }
}

/// Private benchmark artifact projection; unrelated large fields are skipped by Serde.
#[derive(Deserialize)]
struct BenchmarkArtifact {
    /// Artifact schema version.
    schema_version: u32,
    /// Preregistered repeat count per workload and arm.
    repeat_count: usize,
    /// Complete preregistered run schedule.
    schedule: Vec<BenchmarkSchedule>,
    /// Runtime and skill identity for each benchmark arm.
    candidate_identities: BTreeMap<String, BenchmarkCandidateIdentity>,
    /// Descriptive source checkout identity recorded by the campaign.
    candidate_source_identity: BenchmarkSourceIdentity,
    /// Every retained benchmark run.
    runs: Vec<BenchmarkRun>,
    /// Published aggregate groups and comparisons.
    aggregate: BenchmarkAggregate,
    /// Whether every scheduled run remains present.
    all_scheduled_runs_retained: bool,
}

/// Candidate runtime and packaged-skill identity.
#[derive(Deserialize)]
struct BenchmarkCandidateIdentity {
    /// Whether this arm enables `ProjectAtlas`.
    projectatlas: bool,
    /// Runtime semantic version when `ProjectAtlas` is enabled.
    version: Option<String>,
    /// Runtime binary digest when `ProjectAtlas` is enabled.
    runtime_sha256: Option<String>,
    /// Packaged skill digest when `ProjectAtlas` is enabled.
    skill_sha256: Option<String>,
    /// Packaged skill bytes charged to the arm.
    skill_bytes: Option<u64>,
    /// Tool-discovery bytes charged to the arm.
    tool_discovery_bytes: Option<u64>,
}

/// Descriptive candidate source identity retained by the benchmark.
#[derive(Deserialize)]
struct BenchmarkSourceIdentity {
    /// Source checkout commit at campaign start.
    checkout_head: String,
}

/// One preregistered workload/arm/repeat cell.
#[derive(Deserialize)]
struct BenchmarkSchedule {
    /// Stable scheduled run identifier.
    run_id: String,
    /// One-based repeat number.
    repeat: usize,
    /// Preregistered workload name.
    #[serde(rename = "case")]
    workload: String,
    /// Candidate or baseline arm identifier.
    arm: String,
}

/// One retained benchmark run.
#[derive(Deserialize)]
struct BenchmarkRun {
    /// Stable retained run identifier.
    run_id: String,
    /// One-based repeat number.
    repeat: usize,
    /// Retained workload name.
    #[serde(rename = "case")]
    workload: String,
    /// Candidate or baseline arm identifier.
    arm: String,
    /// Whether the retained run was excluded.
    excluded: bool,
    /// Completed or failed execution state.
    execution_status: String,
    /// Optional detailed trace retained by the campaign.
    trace: Option<BenchmarkTrace>,
}

/// Required trace subset for candidate capability reconciliation.
#[derive(Deserialize)]
struct BenchmarkTrace {
    /// Bounded MCP calls retained for the run.
    #[serde(default)]
    mcp_calls: Vec<BenchmarkMcpCall>,
}

/// One retained MCP call.
#[derive(Deserialize)]
struct BenchmarkMcpCall {
    /// MCP server identifier.
    server: String,
    /// MCP tool identifier.
    tool: String,
    /// Retained call completion state.
    status: String,
    /// Bytes emitted by the call.
    emitted_bytes: u64,
}

/// Final aggregate report.
#[derive(Deserialize)]
struct BenchmarkAggregate {
    /// Complete retained run-id inventory.
    all_run_ids: Vec<String>,
    /// Scheduled run count.
    scheduled: usize,
    /// Completed run count.
    completed: usize,
    /// Failed run count.
    failed: usize,
    /// Excluded run count.
    excluded: usize,
    /// Workload-and-arm aggregate groups.
    groups: BTreeMap<String, BenchmarkGroup>,
    /// Workload-specific candidate/baseline comparisons.
    comparisons: BTreeMap<String, BenchmarkComparison>,
    /// Required non-causal provider-counter label.
    provider_usage_note: String,
}

/// One workload/arm aggregate.
#[derive(Deserialize)]
struct BenchmarkGroup {
    /// Run identifiers owned by this group.
    run_ids: Vec<String>,
    /// Scheduled group count.
    scheduled: usize,
    /// Completed group count.
    completed: usize,
    /// Failed group count.
    failed: usize,
    /// Excluded group count.
    excluded: usize,
    /// Navigation and runtime metric distributions.
    distributions: BTreeMap<String, BenchmarkDistribution>,
    /// Descriptive provider-counter distributions.
    provider_usage: BTreeMap<String, BenchmarkDistribution>,
}

/// Retained values plus median and observed maximum.
#[derive(Clone, Deserialize)]
struct BenchmarkDistribution {
    /// Retained value count.
    count: usize,
    /// Retained per-run values.
    values: Vec<f64>,
    /// Published median.
    median: f64,
    /// Published tail-statistic label.
    observed_tail: String,
    /// Published observed maximum.
    maximum: f64,
}

/// One workload-specific candidate/baseline comparison.
#[derive(Deserialize)]
struct BenchmarkComparison {
    /// Lower-is-better navigation and runtime comparisons.
    lower_is_better_percent_savings: BTreeMap<String, BenchmarkComparisonMetric>,
    /// Descriptive-only provider-counter comparisons.
    provider_usage_descriptive_only: BTreeMap<String, BenchmarkProviderComparison>,
    /// Tasks required to repay setup wall time.
    wall_time_break_even_tasks: Option<u64>,
}

/// One lower-is-better comparison row.
#[derive(Deserialize)]
struct BenchmarkComparisonMetric {
    /// Candidate median.
    candidate_median: f64,
    /// Baseline median.
    baseline_median: f64,
    /// Lower-is-better median percentage saving.
    median_percent_saving: Option<f64>,
    /// Published tail-statistic label.
    tail_statistic: String,
    /// Candidate observed tail.
    candidate_tail: f64,
    /// Baseline observed tail.
    baseline_tail: f64,
}

/// One provider comparison row that must remain non-causal.
#[derive(Deserialize)]
struct BenchmarkProviderComparison {
    /// Candidate provider-counter median.
    candidate_median: f64,
    /// Baseline provider-counter median.
    baseline_median: f64,
    /// Candidate provider-counter tail.
    candidate_tail: f64,
    /// Baseline provider-counter tail.
    baseline_tail: f64,
    /// Whether the artifact claims unsupported causal attribution.
    causal_attribution: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkArtifact, BenchmarkMcpCall, BenchmarkRun, BenchmarkTrace, CANDIDATE_ARM,
        PLAIN_ARM, project_capabilities, require, validate_and_project,
    };
    use projectatlas_core::telemetry::{
        AgentEfficiencyBaseline, AgentEfficiencyCapability, AgentEfficiencyEvidenceState,
    };
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    fn rejected_benchmark_reason(
        value: &serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = serde_json::to_vec(value)?;
        let artifact: BenchmarkArtifact = serde_json::from_slice(&bytes)?;
        validate_and_project(&artifact, &bytes)
            .err()
            .ok_or_else(|| io::Error::other("modified benchmark unexpectedly validated").into())
    }

    fn mcp_calls_for_arm<'a>(
        artifact: &'a mut serde_json::Value,
        arm: &str,
    ) -> Result<&'a mut Vec<serde_json::Value>, io::Error> {
        artifact
            .get_mut("runs")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|runs| {
                runs.iter_mut().find_map(|run| {
                    (run.get("arm").and_then(serde_json::Value::as_str) == Some(arm))
                        .then(|| {
                            run.get_mut("trace")
                                .and_then(|value| value.get_mut("mcp_calls"))
                                .and_then(serde_json::Value::as_array_mut)
                        })
                        .flatten()
                })
            })
            .ok_or_else(|| io::Error::other(format!("{arm} benchmark MCP calls are missing")))
    }

    #[test]
    fn published_benchmark_projects_partial_and_capability_truth()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/benchmarks/v0.4-agent-navigation-results.json");
        let bytes = fs::read(path)?;
        let artifact: BenchmarkArtifact = serde_json::from_slice(&bytes)?;
        let source_head = artifact.candidate_source_identity.checkout_head.clone();
        let comparison = validate_and_project(&artifact, &bytes)
            .map_err(|reason| io::Error::other(format!("benchmark validation failed: {reason}")))?;

        require(
            comparison
                .artifact
                .as_ref()
                .is_some_and(|identity| identity.candidate_source_head == source_head),
            "published benchmark source provenance was not retained",
        )
        .map_err(io::Error::other)?;
        require(
            comparison.state == AgentEfficiencyEvidenceState::Partial,
            "published benchmark was not partial",
        )
        .map_err(io::Error::other)?;
        require(
            comparison.baselines.len() == 2,
            "published benchmark baseline count changed",
        )
        .map_err(io::Error::other)?;
        let frozen = comparison
            .baselines
            .iter()
            .find(|row| row.baseline == AgentEfficiencyBaseline::FrozenProjectAtlasV0326)
            .ok_or_else(|| io::Error::other("frozen baseline row missing"))?;
        require(
            frozen.state == AgentEfficiencyEvidenceState::Partial
                && frozen.matched_trials == 12
                && frozen.baseline_failed_trials == 3
                && frozen.unmatched_trials == 3,
            "published frozen baseline truth changed",
        )
        .map_err(io::Error::other)?;
        let plain = comparison
            .baselines
            .iter()
            .find(|row| row.baseline == AgentEfficiencyBaseline::PlainCodex)
            .ok_or_else(|| io::Error::other("plain baseline row missing"))?;
        require(
            plain.state == AgentEfficiencyEvidenceState::Compatible
                && plain.matched_trials == 15
                && plain.baseline_failed_trials == 0,
            "published plain baseline truth changed",
        )
        .map_err(io::Error::other)?;

        let calls = comparison
            .capabilities
            .iter()
            .map(|row| row.calls)
            .sum::<usize>();
        require(
            calls == 176,
            "trace-completed capability calls did not reconcile",
        )
        .map_err(io::Error::other)?;
        for (capability, expected) in [
            (AgentEfficiencyCapability::Discovery, 16),
            (AgentEfficiencyCapability::SummaryAndSlice, 99),
            (AgentEfficiencyCapability::Search, 26),
            (AgentEfficiencyCapability::SymbolsAndRelations, 34),
            (AgentEfficiencyCapability::Other, 1),
        ] {
            require(
                comparison
                    .capabilities
                    .iter()
                    .find(|row| row.capability == capability)
                    .map(|row| row.calls)
                    == Some(expected),
                "published capability call count changed",
            )
            .map_err(io::Error::other)?;
        }
        Ok(())
    }

    #[test]
    fn benchmark_validation_enforces_each_arm_mcp_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/benchmarks/v0.4-agent-navigation-results.json");
        let published: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;

        let mut contaminated_plain = published.clone();
        let candidate_call = mcp_calls_for_arm(&mut contaminated_plain, CANDIDATE_ARM)?
            .first()
            .cloned()
            .ok_or_else(|| io::Error::other("candidate benchmark MCP call is missing"))?;
        mcp_calls_for_arm(&mut contaminated_plain, PLAIN_ARM)?.push(candidate_call);
        require(
            rejected_benchmark_reason(&contaminated_plain)?.contains("plain benchmark run"),
            "contaminated plain control did not fail MCP-contract validation",
        )
        .map_err(io::Error::other)?;

        let mut call_free_candidate = published;
        mcp_calls_for_arm(&mut call_free_candidate, CANDIDATE_ARM)?.clear();
        require(
            rejected_benchmark_reason(&call_free_candidate)?.contains("no completed ProjectAtlas"),
            "call-free completed ProjectAtlas run did not fail MCP-contract validation",
        )
        .map_err(io::Error::other)?;
        Ok(())
    }

    #[test]
    fn failed_candidate_completed_calls_are_projected() -> Result<(), Box<dyn std::error::Error>> {
        let capabilities = project_capabilities(&[BenchmarkRun {
            run_id: "failed-candidate".to_string(),
            repeat: 1,
            workload: "small-clean".to_string(),
            arm: CANDIDATE_ARM.to_string(),
            excluded: false,
            execution_status: "failed".to_string(),
            trace: Some(BenchmarkTrace {
                mcp_calls: vec![BenchmarkMcpCall {
                    server: "projectatlas".to_string(),
                    tool: "atlas_search".to_string(),
                    status: "completed".to_string(),
                    emitted_bytes: 42,
                }],
            }),
        }])
        .map_err(io::Error::other)?;
        require(
            capabilities.len() == 1
                && capabilities[0].capability == AgentEfficiencyCapability::Search
                && capabilities[0].calls == 1
                && capabilities[0].emitted_bytes == 42,
            "failed candidate trace-completed call was not projected",
        )
        .map_err(io::Error::other)?;
        Ok(())
    }

    #[test]
    fn fully_failed_baseline_remains_typed_without_fabricated_metrics()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/benchmarks/v0.4-agent-navigation-results.json");
        let mut artifact: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        for run in artifact
            .get_mut("runs")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark runs are missing"))?
        {
            if run.get("arm").and_then(serde_json::Value::as_str) == Some("v0.3.26") {
                run.as_object_mut()
                    .ok_or_else(|| io::Error::other("benchmark run is not an object"))?
                    .insert(
                        "execution_status".to_string(),
                        serde_json::Value::String("failed".to_string()),
                    );
            }
        }
        let aggregate = artifact
            .get_mut("aggregate")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("benchmark aggregate is missing"))?;
        aggregate.insert("completed".to_string(), serde_json::Value::from(30));
        aggregate.insert("failed".to_string(), serde_json::Value::from(15));
        for (key, group) in aggregate
            .get_mut("groups")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("benchmark groups are missing"))?
        {
            if key.ends_with("/v0.3.26") {
                let group = group
                    .as_object_mut()
                    .ok_or_else(|| io::Error::other("benchmark group is not an object"))?;
                group.insert("completed".to_string(), serde_json::Value::from(0));
                group.insert("failed".to_string(), serde_json::Value::from(3));
                group.insert(
                    "distributions".to_string(),
                    serde_json::Value::Object(serde_json::Map::new()),
                );
                group.insert(
                    "provider_usage".to_string(),
                    serde_json::Value::Object(serde_json::Map::new()),
                );
            }
        }
        for (key, comparison) in aggregate
            .get_mut("comparisons")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("benchmark comparisons are missing"))?
        {
            if key.ends_with("-vs-v0.3.26") {
                let comparison = comparison
                    .as_object_mut()
                    .ok_or_else(|| io::Error::other("benchmark comparison is not an object"))?;
                comparison.insert(
                    "lower_is_better_percent_savings".to_string(),
                    serde_json::Value::Object(serde_json::Map::new()),
                );
                comparison.insert(
                    "provider_usage_descriptive_only".to_string(),
                    serde_json::Value::Object(serde_json::Map::new()),
                );
                comparison.insert(
                    "wall_time_break_even_tasks".to_string(),
                    serde_json::Value::Null,
                );
            }
        }
        let bytes = serde_json::to_vec(&artifact)?;
        let artifact: BenchmarkArtifact = serde_json::from_slice(&bytes)?;
        let comparison = validate_and_project(&artifact, &bytes).map_err(io::Error::other)?;
        let frozen = comparison
            .baselines
            .iter()
            .find(|row| row.baseline == AgentEfficiencyBaseline::FrozenProjectAtlasV0326)
            .ok_or_else(|| io::Error::other("failed frozen baseline row is missing"))?;
        require(
            comparison.state == AgentEfficiencyEvidenceState::Partial
                && frozen.state == AgentEfficiencyEvidenceState::Failed
                && frozen.matched_trials == 0
                && frozen.baseline_failed_trials == 15
                && frozen.metrics.is_empty()
                && frozen.provider_usage_descriptive_only.is_empty(),
            "fully failed baseline was not preserved without fabricated metrics",
        )
        .map_err(io::Error::other)?;
        Ok(())
    }

    #[test]
    fn benchmark_validation_rejects_run_loss_invalid_numbers_and_provider_causality()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/benchmarks/v0.4-agent-navigation-results.json");
        let published: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;

        let mut missing_run = published.clone();
        missing_run
            .get_mut("runs")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark runs are missing"))?
            .pop();
        require(
            rejected_benchmark_reason(&missing_run)?.contains("schedule or run count"),
            "missing run did not fail retention validation",
        )
        .map_err(io::Error::other)?;

        let mut duplicate_group_run = published.clone();
        let group_run_ids = duplicate_group_run
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("groups"))
            .and_then(|value| value.get_mut("small-clean/v0.4"))
            .and_then(|value| value.get_mut("run_ids"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark group run ids are missing"))?;
        group_run_ids[1] = group_run_ids[0].clone();
        require(
            rejected_benchmark_reason(&duplicate_group_run)?.contains("duplicate run ids"),
            "duplicate group run id did not fail retention validation",
        )
        .map_err(io::Error::other)?;

        let mut excessive_mcp_calls = published.clone();
        let frozen_calls = excessive_mcp_calls
            .get_mut("runs")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|runs| {
                runs.iter_mut().find_map(|run| {
                    (run.get("arm").and_then(serde_json::Value::as_str) == Some("v0.3.26"))
                        .then(|| {
                            run.get_mut("trace")
                                .and_then(|value| value.get_mut("mcp_calls"))
                                .and_then(serde_json::Value::as_array_mut)
                        })
                        .flatten()
                        .filter(|calls| !calls.is_empty())
                })
            })
            .ok_or_else(|| io::Error::other("frozen benchmark MCP calls are missing"))?;
        let retained_call = frozen_calls
            .first()
            .cloned()
            .ok_or_else(|| io::Error::other("frozen benchmark MCP call is missing"))?;
        frozen_calls.resize(129, retained_call);
        require(
            rejected_benchmark_reason(&excessive_mcp_calls)?.contains("MCP-call bound"),
            "non-candidate MCP-call overflow did not fail collection validation",
        )
        .map_err(io::Error::other)?;

        let mut excessive_emitted_bytes = published.clone();
        let emitted_bytes = excessive_emitted_bytes
            .get_mut("runs")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|runs| {
                runs.iter_mut().find_map(|run| {
                    (run.get("arm").and_then(serde_json::Value::as_str) == Some("v0.4"))
                        .then(|| {
                            run.get_mut("trace")
                                .and_then(|value| value.get_mut("mcp_calls"))
                                .and_then(serde_json::Value::as_array_mut)
                                .and_then(|calls| calls.first_mut())
                                .and_then(|call| call.get_mut("emitted_bytes"))
                        })
                        .flatten()
                })
            })
            .ok_or_else(|| io::Error::other("candidate MCP emitted bytes are missing"))?;
        *emitted_bytes = serde_json::Value::from(9_007_199_254_740_992_u64);
        require(
            rejected_benchmark_reason(&excessive_emitted_bytes)?.contains("emitted-byte"),
            "excessive MCP emitted bytes did not fail numeric validation",
        )
        .map_err(io::Error::other)?;

        let mut invalid_numeric = published.clone();
        let invalid_values = invalid_numeric
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("groups"))
            .and_then(|value| value.get_mut("small-clean/v0.4"))
            .and_then(|value| value.get_mut("distributions"))
            .and_then(|value| value.get_mut("tool_calls"))
            .and_then(|value| value.get_mut("values"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark metric values are missing"))?;
        invalid_values[0] = serde_json::Value::from(-1);
        require(
            rejected_benchmark_reason(&invalid_numeric)?.contains("invalid numeric"),
            "negative metric did not fail numeric validation",
        )
        .map_err(io::Error::other)?;

        let mut fractional_count = published.clone();
        let fractional_values = fractional_count
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("groups"))
            .and_then(|value| value.get_mut("small-clean/v0.4"))
            .and_then(|value| value.get_mut("distributions"))
            .and_then(|value| value.get_mut("tool_calls"))
            .and_then(|value| value.get_mut("values"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark metric values are missing"))?;
        fractional_values[0] = serde_json::Value::from(1.5);
        require(
            rejected_benchmark_reason(&fractional_count)?.contains("integer"),
            "fractional count did not fail numeric validation",
        )
        .map_err(io::Error::other)?;

        let mut excessive_count = published.clone();
        let excessive_values = excessive_count
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("groups"))
            .and_then(|value| value.get_mut("small-clean/v0.4"))
            .and_then(|value| value.get_mut("distributions"))
            .and_then(|value| value.get_mut("tool_calls"))
            .and_then(|value| value.get_mut("values"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| io::Error::other("benchmark metric values are missing"))?;
        excessive_values[0] = serde_json::Value::from(9_007_199_254_740_992_u64);
        require(
            rejected_benchmark_reason(&excessive_count)?.contains("bound"),
            "excessive count did not fail numeric validation",
        )
        .map_err(io::Error::other)?;

        let mut false_break_even = published.clone();
        let break_even = false_break_even
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("comparisons"))
            .and_then(|value| value.get_mut("small-clean/v0.4-vs-plain"))
            .and_then(|value| value.get_mut("wall_time_break_even_tasks"))
            .ok_or_else(|| io::Error::other("benchmark break-even value is missing"))?;
        *break_even = serde_json::Value::from(123_456_u64);
        require(
            rejected_benchmark_reason(&false_break_even)?.contains("does not match"),
            "fabricated break-even value did not fail derived validation",
        )
        .map_err(io::Error::other)?;

        let mut excessive_comparison_metrics = published.clone();
        let comparison_metrics = excessive_comparison_metrics
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("comparisons"))
            .and_then(|value| value.get_mut("small-clean/v0.4-vs-plain"))
            .and_then(|value| value.get_mut("lower_is_better_percent_savings"))
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("benchmark comparison metrics are missing"))?;
        let comparison_template = comparison_metrics
            .values()
            .next()
            .cloned()
            .ok_or_else(|| io::Error::other("benchmark comparison metric is missing"))?;
        while comparison_metrics.len() <= 64 {
            comparison_metrics.insert(
                format!("extra_metric_{}", comparison_metrics.len()),
                comparison_template.clone(),
            );
        }
        require(
            rejected_benchmark_reason(&excessive_comparison_metrics)?.contains("too large"),
            "comparison metric overflow did not fail collection validation",
        )
        .map_err(io::Error::other)?;

        let mut excessive_identity_bytes = published.clone();
        let skill_bytes = excessive_identity_bytes
            .get_mut("candidate_identities")
            .and_then(|value| value.get_mut("v0.4"))
            .and_then(|value| value.get_mut("skill_bytes"))
            .ok_or_else(|| io::Error::other("candidate skill bytes are missing"))?;
        *skill_bytes = serde_json::Value::from(9_007_199_254_740_992_u64);
        require(
            rejected_benchmark_reason(&excessive_identity_bytes)?.contains("supported bound"),
            "excessive identity bytes did not fail numeric validation",
        )
        .map_err(io::Error::other)?;

        let mut malformed_source = published.clone();
        malformed_source["candidate_source_identity"]["checkout_head"] =
            serde_json::Value::String("not-a-commit".to_string());
        require(
            rejected_benchmark_reason(&malformed_source)?.contains("source identity"),
            "malformed source provenance did not fail validation",
        )
        .map_err(io::Error::other)?;

        let mut causal_provider = published;
        let causal_attribution = causal_provider
            .get_mut("aggregate")
            .and_then(|value| value.get_mut("comparisons"))
            .and_then(|value| value.get_mut("small-clean/v0.4-vs-plain"))
            .and_then(|value| value.get_mut("provider_usage_descriptive_only"))
            .and_then(|value| value.get_mut("input_tokens"))
            .and_then(|value| value.get_mut("causal_attribution"))
            .ok_or_else(|| io::Error::other("provider causality marker is missing"))?;
        *causal_attribution = serde_json::Value::Bool(true);
        require(
            rejected_benchmark_reason(&causal_provider)?.contains("causal attribution"),
            "causal provider counter did not fail validation",
        )
        .map_err(io::Error::other)?;
        Ok(())
    }
}
