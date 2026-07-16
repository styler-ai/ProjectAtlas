//! Concrete supervised-process evidence for repository-graph scale workloads.

use crate::bounded_process_supervisor::{SupervisionError, configure_supervised_command};
use processkit::{Command, Mechanism, ProcessGroup, RunningProcess};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use thiserror::Error;

/// Stable resident-memory metric identity.
const PROCESS_RESIDENT_METRIC: &str = "sampled-aggregate-process-group-resident-bytes";
/// Portable process sampling cannot expose private committed bytes.
const PRIVATE_COMMITTED_UNAVAILABLE: &str = "not-available-from-portable-sysinfo-process-sampling";
/// Aggregate RSS can count shared pages once per process.
const SHARED_PAGE_ACCOUNTING: &str =
    "sum-per-process-resident-bytes-shared-pages-may-be-double-counted";

/// Failures at the graph-scale process-measurement boundary.
#[derive(Debug, Error)]
pub(super) enum GraphScaleProcessError {
    /// Process evidence or configuration violated its closed contract.
    #[error("graph-scale process policy failed: {0}")]
    Policy(String),
    /// Shared command supervision policy rejected the command.
    #[error(transparent)]
    Supervision(#[from] SupervisionError),
    /// Native process-group creation, execution, or teardown failed.
    #[error(transparent)]
    Process(#[from] processkit::Error),
}

/// Typed process-gate outcome retained in evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProcessGateDecision {
    /// The measured value met its declared gate.
    Passed,
    /// The measured value missed its declared gate.
    Failed,
}

impl ProcessGateDecision {
    /// Return whether the decision passed.
    pub(super) const fn passed(self) -> bool {
        matches!(self, Self::Passed)
    }
}

impl From<bool> for ProcessGateDecision {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

/// Containment mechanism observed for one measured process group.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProcessContainmentMechanism {
    /// Windows Job Object containment.
    WindowsJobObject,
    /// Linux cgroup-v2 containment.
    LinuxCgroupV2,
    /// POSIX process-group containment.
    PosixProcessGroup,
    /// A future mechanism unknown to this evidence schema.
    Unknown,
}

impl ProcessContainmentMechanism {
    /// Whether membership snapshots cover every contained descendant.
    pub(super) const fn has_complete_membership(self) -> bool {
        matches!(self, Self::WindowsJobObject | Self::LinuxCgroupV2)
    }
}

impl From<Mechanism> for ProcessContainmentMechanism {
    fn from(value: Mechanism) -> Self {
        match value {
            Mechanism::JobObject => Self::WindowsJobObject,
            Mechanism::CgroupV2 => Self::LinuxCgroupV2,
            Mechanism::ProcessGroup => Self::PosixProcessGroup,
            _ => Self::Unknown,
        }
    }
}

/// Membership semantics paired with the observed containment mechanism.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProcessMembershipSemantics {
    /// Every Windows Job Object member is enumerable.
    CompleteJobObjectMembership,
    /// Every Linux cgroup-v2 member is enumerable.
    CompleteCgroupV2Membership,
    /// Only tracked group leaders and adopted children are enumerable.
    TrackedProcessGroupLeaders,
    /// Membership completeness is unknown.
    Unknown,
}

impl From<ProcessContainmentMechanism> for ProcessMembershipSemantics {
    fn from(value: ProcessContainmentMechanism) -> Self {
        match value {
            ProcessContainmentMechanism::WindowsJobObject => Self::CompleteJobObjectMembership,
            ProcessContainmentMechanism::LinuxCgroupV2 => Self::CompleteCgroupV2Membership,
            ProcessContainmentMechanism::PosixProcessGroup => Self::TrackedProcessGroupLeaders,
            ProcessContainmentMechanism::Unknown => Self::Unknown,
        }
    }
}

/// Bounded retained stream identity without embedding diagnostic bytes.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessStreamEvidence {
    /// Number of retained bytes after the configured ceiling.
    pub(super) retained_bytes: u64,
    /// SHA-256 over the retained bytes.
    pub(super) retained_sha256: String,
}

/// One raw aggregate resident-memory sample.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResidentMemorySample {
    /// Nanoseconds since sampling began.
    pub(super) timestamp_ns: u64,
    /// Sum of member resident bytes, with shared pages potentially double counted.
    pub(super) aggregate_resident_bytes: u64,
    /// Per-process observations in PID order.
    pub(super) processes: Vec<ResidentProcessSample>,
}

/// One process identity and resident-memory observation.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResidentProcessSample {
    /// Operating-system process identifier.
    pub(super) pid: u32,
    /// Process start identity in seconds since boot as reported by sysinfo.
    pub(super) start_time_seconds_since_boot: u64,
    /// Resident bytes reported by sysinfo.
    pub(super) resident_bytes: u64,
}

/// Complete process-boundary evidence for one measured workload child.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent process-policy and lifecycle observations remain explicit evidence"
)]
pub(super) struct GraphScaleProcessEvidence {
    /// Exact metric name.
    pub(super) metric_name: String,
    /// Platform-specific membership semantics.
    pub(super) membership_semantics: ProcessMembershipSemantics,
    /// Configured process-group sampling interval.
    pub(super) sample_interval_ms: u64,
    /// Configured hard timeout.
    pub(super) timeout_ms: u64,
    /// Configured per-stream retained-output ceiling.
    pub(super) output_limit_bytes: u64,
    /// Whether the shared command policy requested parent-death protection.
    pub(super) parent_death_requested: bool,
    /// Root workload process identifier.
    pub(super) root_pid: u32,
    /// Process-group containment mechanism.
    pub(super) containment_mechanism: ProcessContainmentMechanism,
    /// Whether membership enumeration covers the complete descendant tree.
    pub(super) membership_complete: bool,
    /// Raw successful samples in execution order.
    pub(super) raw_samples: Vec<ResidentMemorySample>,
    /// Samples containing at least one process observation.
    pub(super) successful_sample_count: u64,
    /// Failed process-group membership reads.
    pub(super) membership_discovery_failures: u64,
    /// Members still reported active but unavailable during process refresh.
    pub(super) active_member_discovery_failures: u64,
    /// Maximum aggregate resident bytes across successful samples.
    pub(super) peak_aggregate_resident_bytes: u64,
    /// Portable sampling cannot provide private committed bytes.
    pub(super) private_committed_bytes: Option<u64>,
    /// Exact private/committed availability statement.
    pub(super) private_committed_status: String,
    /// Explicit shared-page aggregation policy.
    pub(super) shared_page_accounting_policy: String,
    /// Whether this evidence is eligible for a complete-tree claim.
    pub(super) complete_tree_claim_eligible: bool,
    /// Resident ceiling decision over the observed complete membership semantics.
    pub(super) resident_ceiling: ProcessGateDecision,
    /// Wall-clock process lifetime reported by `processkit`.
    pub(super) duration_ns: u64,
    /// Leader exit code when the operating system returned one.
    pub(super) exit_code: Option<i32>,
    /// Whether the hard timeout terminated the process tree.
    pub(super) timed_out: bool,
    /// Whether either stream exceeded the bounded retention ceiling.
    pub(super) output_truncated: bool,
    /// Retained standard-output identity.
    pub(super) stdout: ProcessStreamEvidence,
    /// Retained standard-error identity.
    pub(super) stderr: ProcessStreamEvidence,
    /// Sorted membership observed after reaping the leader and draining the sampler.
    pub(super) terminal_members_before_teardown: Vec<u32>,
    /// Sorted membership observed after explicit process-group shutdown.
    pub(super) post_teardown_members: Vec<u32>,
    /// Whether the sampler thread joined successfully.
    pub(super) sampler_drain_completed: bool,
    /// Whether explicit process-group shutdown completed successfully.
    pub(super) teardown_completed: bool,
    /// Whether the leader completed successfully within every process boundary.
    pub(super) successful_bounded_completion: bool,
}

/// Process result plus a non-retained human failure diagnostic.
pub(super) struct MeasuredProcessOutcome {
    /// Complete serializable process evidence.
    pub(super) evidence: GraphScaleProcessEvidence,
    /// Human-only diagnostic; never used as retained command or source identity.
    pub(super) stderr_diagnostic: String,
}

/// Run one same-executable child through the exact measured process boundary.
pub(super) async fn run_measured_process(
    executable: &Path,
    arguments: &[String],
    timeout: Duration,
    output_limit: usize,
    sample_interval: Duration,
    resident_ceiling_bytes: u64,
) -> Result<MeasuredProcessOutcome, GraphScaleProcessError> {
    run_measured_process_with_environment(
        executable,
        arguments,
        &[],
        timeout,
        output_limit,
        sample_interval,
        resident_ceiling_bytes,
    )
    .await
}

/// Test seam for the exact runner with explicit child-only environment markers.
pub(super) async fn run_measured_process_with_environment(
    executable: &Path,
    arguments: &[String],
    environment: &[(&str, &str)],
    timeout: Duration,
    output_limit: usize,
    sample_interval: Duration,
    resident_ceiling_bytes: u64,
) -> Result<MeasuredProcessOutcome, GraphScaleProcessError> {
    let timeout_ms = duration_millis(timeout, "process timeout")?;
    let sample_interval_ms = duration_millis(sample_interval, "process sample interval")?;
    require(output_limit > 0, "process output limit must be positive")?;
    require(timeout_ms > 0, "process timeout must be positive")?;
    require(
        sample_interval_ms > 0,
        "process sample interval must be positive",
    )?;

    let mut command =
        configure_supervised_command(Command::new(executable), timeout, output_limit)?;
    for argument in arguments {
        command = command.arg(argument);
    }
    for (name, value) in environment {
        command = command.env(name, value);
    }

    let group = Arc::new(ProcessGroup::new()?);
    let mechanism = group.mechanism();
    let running = group.start(&command).await?;
    let root_pid = running.pid().ok_or_else(|| {
        GraphScaleProcessError::Policy("workload child PID was unavailable".into())
    })?;
    let sampler = ProcessGroupSampler::start(Arc::clone(&group), root_pid, sample_interval);
    let supervised = SupervisedProcess {
        group,
        running: Some(running),
        sampler: Some(sampler),
        hard_timeout: timeout,
        finished: false,
    };
    let lifecycle = supervised.finish().await?;
    let result = lifecycle.result;
    let samples = lifecycle.samples;
    let stderr_diagnostic = result.stderr().to_owned();
    let output_truncated = result.truncated();
    let timed_out = result.timed_out() || lifecycle.hard_timeout_fired;
    let exit_code = if timed_out { None } else { result.code() };
    let resident_ceiling = (samples.peak_aggregate_resident_bytes <= resident_ceiling_bytes).into();
    let teardown_completed = lifecycle.post_teardown_members.is_empty();
    let successful_bounded_completion = exit_code == Some(0)
        && !timed_out
        && !output_truncated
        && lifecycle.sampler_drain_completed
        && teardown_completed
        && samples.successful_sample_count > 0
        && samples.peak_aggregate_resident_bytes > 0;
    let containment_mechanism = ProcessContainmentMechanism::from(mechanism);
    let evidence = GraphScaleProcessEvidence {
        metric_name: PROCESS_RESIDENT_METRIC.to_owned(),
        membership_semantics: containment_mechanism.into(),
        sample_interval_ms,
        timeout_ms,
        output_limit_bytes: usize_to_u64(output_limit),
        parent_death_requested: true,
        root_pid,
        containment_mechanism,
        membership_complete: containment_mechanism.has_complete_membership()
            && samples.membership_discovery_failures == 0,
        raw_samples: samples.raw_samples,
        successful_sample_count: samples.successful_sample_count,
        membership_discovery_failures: samples.membership_discovery_failures,
        active_member_discovery_failures: samples.active_member_discovery_failures,
        peak_aggregate_resident_bytes: samples.peak_aggregate_resident_bytes,
        private_committed_bytes: None,
        private_committed_status: PRIVATE_COMMITTED_UNAVAILABLE.to_owned(),
        shared_page_accounting_policy: SHARED_PAGE_ACCOUNTING.to_owned(),
        complete_tree_claim_eligible: containment_mechanism.has_complete_membership()
            && samples.membership_discovery_failures == 0
            && samples.active_member_discovery_failures == 0
            && samples.successful_sample_count > 0
            && teardown_completed,
        resident_ceiling,
        duration_ns: u64::try_from(result.duration().as_nanos()).unwrap_or(u64::MAX),
        exit_code,
        timed_out,
        output_truncated,
        stdout: stream_evidence(result.stdout()),
        stderr: stream_evidence(result.stderr().as_bytes()),
        terminal_members_before_teardown: lifecycle.terminal_members_before_teardown,
        post_teardown_members: lifecycle.post_teardown_members,
        sampler_drain_completed: lifecycle.sampler_drain_completed,
        teardown_completed,
        successful_bounded_completion,
    };
    Ok(MeasuredProcessOutcome {
        evidence,
        stderr_diagnostic,
    })
}

/// Recompute every process evidence invariant from raw observations.
pub(super) fn validate_process_evidence(
    evidence: &GraphScaleProcessEvidence,
    expected_sample_interval: Duration,
    expected_timeout: Duration,
    expected_output_limit: usize,
    resident_ceiling_bytes: u64,
    require_complete_tree: bool,
) -> Result<(), GraphScaleProcessError> {
    require(
        evidence.metric_name == PROCESS_RESIDENT_METRIC
            && evidence.root_pid > 0
            && evidence.duration_ns > 0
            && evidence.sample_interval_ms
                == duration_millis(expected_sample_interval, "expected process sample interval")?
            && evidence.timeout_ms
                == duration_millis(expected_timeout, "expected process timeout")?
            && evidence.output_limit_bytes == usize_to_u64(expected_output_limit)
            && evidence.parent_death_requested
            && evidence.successful_sample_count == usize_to_u64(evidence.raw_samples.len())
            && evidence.successful_sample_count > 0,
        "process identity, policy, or sample count drifted",
    )?;
    let complete_mechanism = evidence.containment_mechanism.has_complete_membership();
    require(
        evidence.membership_complete
            == (complete_mechanism && evidence.membership_discovery_failures == 0),
        "process membership completeness disagrees with its mechanism",
    )?;
    require(
        evidence.membership_semantics == evidence.containment_mechanism.into(),
        "process platform semantics disagree with its mechanism",
    )?;

    let mut process_identities = BTreeMap::<u32, u64>::new();
    let mut previous_timestamp = None;
    let mut observed_peak = 0_u64;
    let mut root_observed = false;
    for sample in &evidence.raw_samples {
        if let Some(previous) = previous_timestamp {
            require(
                sample.timestamp_ns > previous,
                "process sample timestamps are not strictly monotonic",
            )?;
        }
        previous_timestamp = Some(sample.timestamp_ns);
        require(
            !sample.processes.is_empty()
                && sample
                    .processes
                    .windows(2)
                    .all(|pair| pair[0].pid < pair[1].pid),
            "process sample identities are empty, duplicate, or unordered",
        )?;
        let aggregate = sample.processes.iter().fold(0_u64, |total, process| {
            total.saturating_add(process.resident_bytes)
        });
        require(
            aggregate == sample.aggregate_resident_bytes,
            "process aggregate resident bytes differ from the raw member sum",
        )?;
        for process in &sample.processes {
            require(
                process.start_time_seconds_since_boot > 0,
                "a sampled process has no usable start identity",
            )?;
            root_observed |= process.pid == evidence.root_pid;
            if let Some(previous_start) =
                process_identities.insert(process.pid, process.start_time_seconds_since_boot)
            {
                require(
                    previous_start == process.start_time_seconds_since_boot,
                    "a sampled PID changed process-start identity",
                )?;
            }
        }
        observed_peak = observed_peak.max(aggregate);
    }
    require(
        root_observed
            && observed_peak > 0
            && evidence.peak_aggregate_resident_bytes == observed_peak,
        "process root or peak resident observation drifted",
    )?;
    let expected_complete_tree = evidence.membership_complete
        && evidence.active_member_discovery_failures == 0
        && evidence.successful_sample_count > 0
        && evidence.teardown_completed;
    require(
        evidence.complete_tree_claim_eligible == expected_complete_tree,
        "complete-tree eligibility disagrees with raw discovery evidence",
    )?;
    if require_complete_tree {
        require(
            evidence.complete_tree_claim_eligible,
            "full evidence requires complete process-tree membership",
        )?;
    }
    require(
        evidence.private_committed_bytes.is_none()
            && evidence.private_committed_status == PRIVATE_COMMITTED_UNAVAILABLE
            && evidence.shared_page_accounting_policy == SHARED_PAGE_ACCOUNTING,
        "process memory scope or shared-page policy drifted",
    )?;
    require(
        evidence.sampler_drain_completed
            && evidence.teardown_completed == evidence.post_teardown_members.is_empty()
            && evidence.teardown_completed,
        "process sampler or explicit group teardown did not complete",
    )?;
    require(
        evidence.resident_ceiling
            == (evidence.peak_aggregate_resident_bytes <= resident_ceiling_bytes).into()
            && evidence.resident_ceiling.passed(),
        "process resident ceiling decision drifted or failed",
    )?;
    validate_stream(&evidence.stdout, evidence.output_limit_bytes, "stdout")?;
    validate_stream(&evidence.stderr, evidence.output_limit_bytes, "stderr")?;
    let expected_success = evidence.exit_code == Some(0)
        && !evidence.timed_out
        && !evidence.output_truncated
        && evidence.sampler_drain_completed
        && evidence.teardown_completed
        && evidence.successful_sample_count > 0
        && evidence.peak_aggregate_resident_bytes > 0;
    require(
        evidence.successful_bounded_completion == expected_success && expected_success,
        "process did not complete successfully within every bounded policy",
    )
}

/// Mutable result owned only by the sampling thread.
struct ResidentSamplingEvidence {
    /// Raw successful samples in execution order.
    raw_samples: Vec<ResidentMemorySample>,
    /// Samples containing at least one process observation.
    successful_sample_count: u64,
    /// Failed process-group membership reads.
    membership_discovery_failures: u64,
    /// Members still active but unavailable during process refresh.
    active_member_discovery_failures: u64,
    /// Maximum aggregate resident bytes across successful samples.
    peak_aggregate_resident_bytes: u64,
}

/// Process-tree sampler with an explicit join path.
struct ProcessGroupSampler {
    /// Stop signal set after the leader is reaped.
    stop: Arc<AtomicBool>,
    /// Sampling thread result.
    handle: Option<thread::JoinHandle<Result<ResidentSamplingEvidence, GraphScaleProcessError>>>,
}

impl ProcessGroupSampler {
    /// Start sampling every currently enumerable process-group member.
    fn start(group: Arc<ProcessGroup>, root_pid: u32, sample_interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            sample_process_group(&group, root_pid, sample_interval, &worker_stop)
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    /// Stop sampling after one final membership pass and join the sampler.
    fn finish(mut self) -> Result<ResidentSamplingEvidence, GraphScaleProcessError> {
        self.stop.store(true, Ordering::Release);
        let handle = self.handle.take().ok_or_else(|| {
            GraphScaleProcessError::Policy("process sampler handle was missing".into())
        })?;
        handle
            .join()
            .map_err(|_payload| GraphScaleProcessError::Policy("process sampler failed".into()))?
    }
}

/// Explicit lifecycle observations returned only after orderly teardown.
struct ProcessLifecycleOutcome {
    /// Reaped process result.
    result: processkit::ProcessResult<Vec<u8>>,
    /// Drained sampler observations.
    samples: ResidentSamplingEvidence,
    /// Membership after the leader was reaped and the sampler joined.
    terminal_members_before_teardown: Vec<u32>,
    /// Membership after explicit shutdown.
    post_teardown_members: Vec<u32>,
    /// Whether the sampler joined without a thread or sampling failure.
    sampler_drain_completed: bool,
    /// Whether the explicit whole-group deadline won the lifecycle race.
    hard_timeout_fired: bool,
}

/// RAII hard-kill fallback plus explicit child, sampler, and teardown finish path.
struct SupervisedProcess {
    /// Shared whole-tree containment owner.
    group: Arc<ProcessGroup>,
    /// Running same-executable child.
    running: Option<RunningProcess>,
    /// Concurrent process-group sampler.
    sampler: Option<ProcessGroupSampler>,
    /// Hard deadline that tears down the complete group, including descendants.
    hard_timeout: Duration,
    /// Whether explicit finish completed.
    finished: bool,
}

impl SupervisedProcess {
    /// Reap the child, drain the sampler, and retain pre/post-teardown membership.
    async fn finish(mut self) -> Result<ProcessLifecycleOutcome, GraphScaleProcessError> {
        let running = self.running.take().ok_or_else(|| {
            GraphScaleProcessError::Policy("supervised child handle was missing".into())
        })?;
        let mut output = Box::pin(running.output_bytes());
        let (result, hard_timeout_fired) = tokio::select! {
            biased;
            () = tokio::time::sleep(self.hard_timeout) => {
                self.group.kill_all()?;
                (output.await?, true)
            }
            result = &mut output => (result?, false),
        };
        let sampler = self
            .sampler
            .take()
            .ok_or_else(|| GraphScaleProcessError::Policy("process sampler was missing".into()))?;
        let samples = sampler.finish()?;
        let terminal_members_before_teardown = normalized_members(&self.group)?;
        self.group.shutdown_ref().await?;
        let post_teardown_members = normalized_members(&self.group)?;
        self.finished = true;
        Ok(ProcessLifecycleOutcome {
            result,
            samples,
            terminal_members_before_teardown,
            post_teardown_members,
            sampler_drain_completed: true,
            hard_timeout_fired,
        })
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        if !self.finished && self.group.kill_all().is_err() {
            // Drop is the non-fallible hard-kill fallback; explicit finish reports failures.
        }
        if let Some(mut sampler) = self.sampler.take() {
            sampler.stop.store(true, Ordering::Release);
            if let Some(handle) = sampler.handle.take() {
                let _join_result = handle.join();
            }
        }
    }
}

/// Sample resident memory for every currently enumerated group member.
fn sample_process_group(
    group: &ProcessGroup,
    root_pid: u32,
    sample_interval: Duration,
    stop: &AtomicBool,
) -> Result<ResidentSamplingEvidence, GraphScaleProcessError> {
    let started = Instant::now();
    let mut system = System::new();
    let refresh = ProcessRefreshKind::nothing().with_memory();
    let mut raw_samples = Vec::new();
    let mut successful_sample_count = 0_u64;
    let mut membership_discovery_failures = 0_u64;
    let mut active_member_discovery_failures = 0_u64;
    let mut peak_aggregate_resident_bytes = 0_u64;
    let mut sampled_once = false;
    loop {
        let stop_after_sample = sampled_once && stop.load(Ordering::Acquire);
        let mut members = match group.members() {
            Ok(members) => members,
            Err(_error) => {
                membership_discovery_failures = membership_discovery_failures.saturating_add(1);
                vec![root_pid]
            }
        };
        members.sort_unstable();
        members.dedup();
        let pids = members
            .iter()
            .map(|pid| sysinfo::Pid::from_u32(*pid))
            .collect::<Vec<_>>();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, refresh);
        let mut processes = Vec::with_capacity(pids.len());
        let mut aggregate = 0_u64;
        let mut missing_members = Vec::new();
        for (raw_pid, pid) in members.into_iter().zip(pids) {
            if let Some(process) = system.process(pid) {
                let resident_bytes = process.memory();
                aggregate = aggregate.checked_add(resident_bytes).ok_or_else(|| {
                    GraphScaleProcessError::Policy(
                        "aggregate resident-memory sample overflowed".into(),
                    )
                })?;
                processes.push(ResidentProcessSample {
                    pid: raw_pid,
                    start_time_seconds_since_boot: process.start_time(),
                    resident_bytes,
                });
            } else {
                missing_members.push(raw_pid);
            }
        }
        if !missing_members.is_empty() {
            match group.members() {
                Ok(mut refreshed_members) => {
                    refreshed_members.sort_unstable();
                    for pid in missing_members {
                        if refreshed_members.binary_search(&pid).is_ok() {
                            active_member_discovery_failures =
                                active_member_discovery_failures.saturating_add(1);
                        }
                    }
                }
                Err(_error) => {
                    membership_discovery_failures = membership_discovery_failures.saturating_add(1);
                    active_member_discovery_failures = active_member_discovery_failures
                        .saturating_add(usize_to_u64(missing_members.len()));
                }
            }
        }
        if !processes.is_empty() {
            successful_sample_count = successful_sample_count.saturating_add(1);
            peak_aggregate_resident_bytes = peak_aggregate_resident_bytes.max(aggregate);
            raw_samples.push(ResidentMemorySample {
                timestamp_ns: elapsed_ns(started),
                aggregate_resident_bytes: aggregate,
                processes,
            });
        }
        sampled_once = true;
        if stop_after_sample {
            break;
        }
        thread::sleep(sample_interval);
    }
    Ok(ResidentSamplingEvidence {
        raw_samples,
        successful_sample_count,
        membership_discovery_failures,
        active_member_discovery_failures,
        peak_aggregate_resident_bytes,
    })
}

/// Return sorted, duplicate-free process-group membership.
fn normalized_members(group: &ProcessGroup) -> Result<Vec<u32>, GraphScaleProcessError> {
    let mut members = group.members()?;
    members.sort_unstable();
    members.dedup();
    Ok(members)
}

/// Retain one bounded stream's byte count and digest.
fn stream_evidence(bytes: &[u8]) -> ProcessStreamEvidence {
    ProcessStreamEvidence {
        retained_bytes: usize_to_u64(bytes.len()),
        retained_sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

/// Validate one bounded stream identity.
fn validate_stream(
    evidence: &ProcessStreamEvidence,
    output_limit: u64,
    label: &str,
) -> Result<(), GraphScaleProcessError> {
    require(
        evidence.retained_bytes <= output_limit
            && evidence.retained_sha256.len() == 64
            && evidence
                .retained_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        format!("bounded {label} evidence is invalid"),
    )
}

/// Convert a duration to its retained millisecond identity.
fn duration_millis(duration: Duration, label: &str) -> Result<u64, GraphScaleProcessError> {
    u64::try_from(duration.as_millis()).map_err(|_error| {
        GraphScaleProcessError::Policy(format!("{label} does not fit the evidence schema"))
    })
}

/// Convert a monotonic duration to saturated nanoseconds.
fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Convert a platform-sized count to the retained schema width.
fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Return a typed policy error when an invariant is false.
fn require(condition: bool, message: impl Into<String>) -> Result<(), GraphScaleProcessError> {
    if condition {
        Ok(())
    } else {
        Err(GraphScaleProcessError::Policy(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::{self, Write};
    use std::process::Command as StandardCommand;

    const PROBE_ROOT_ENV: &str = "PROJECTATLAS_GRAPH_SCALE_PROCESS_PROBE_ROOT";
    const PROBE_CHILD_ENV: &str = "PROJECTATLAS_GRAPH_SCALE_PROCESS_PROBE_CHILD";
    const PROBE_HANG_ENV: &str = "PROJECTATLAS_GRAPH_SCALE_PROCESS_PROBE_HANG";
    const PROBE_MARKER_ENV: &str = "PROJECTATLAS_GRAPH_SCALE_PROCESS_PROBE_MARKER";
    const PROBE_OUTPUT_ENV: &str = "PROJECTATLAS_GRAPH_SCALE_PROCESS_PROBE_OUTPUT";
    const PROBE_TEST_NAME: &str =
        "graph_scale_process::tests::task_arri_ut_arri_4_23_process_probe";

    /// Exact-harness process fixture that creates a measurable descendant.
    #[test]
    fn task_arri_ut_arri_4_23_process_probe() -> Result<(), Box<dyn std::error::Error>> {
        if env::var_os(PROBE_OUTPUT_ENV).is_some() {
            io::stdout().write_all(&vec![b'o'; 8 * 1024])?;
            io::stderr().write_all(&vec![b'e'; 8 * 1024])?;
            return Ok(());
        }
        if env::var_os(PROBE_CHILD_ENV).is_some() {
            thread::sleep(Duration::from_millis(350));
            if let Some(marker) = env::var_os(PROBE_MARKER_ENV) {
                fs::write(marker, b"descendant-finished")?;
            }
            return Ok(());
        }
        if env::var_os(PROBE_ROOT_ENV).is_some() {
            let mut child = StandardCommand::new(env::current_exe()?)
                .arg("--exact")
                .arg(PROBE_TEST_NAME)
                .arg("--nocapture")
                .env(PROBE_CHILD_ENV, "1")
                .spawn()?;
            if env::var_os(PROBE_HANG_ENV).is_some() {
                thread::sleep(Duration::from_secs(3));
                return Ok(());
            }
            let status = child.wait()?;
            if !status.success() {
                return Err("graph-scale descendant probe failed".into());
            }
        }
        Ok(())
    }

    /// Real process smoke for sampling, descendant membership, timeout, and teardown.
    #[tokio::test(flavor = "current_thread")]
    async fn task_arri_ut_arri_4_23_supervisor_process_smoke()
    -> Result<(), Box<dyn std::error::Error>> {
        let executable = env::current_exe()?;
        let arguments = vec![
            "--exact".to_owned(),
            PROBE_TEST_NAME.to_owned(),
            "--nocapture".to_owned(),
        ];
        let timeout = Duration::from_secs(5);
        let sample_interval = Duration::from_millis(20);
        let output_limit = 1024 * 1024;
        let outcome = run_measured_process_with_environment(
            &executable,
            &arguments,
            &[(PROBE_ROOT_ENV, "1")],
            timeout,
            output_limit,
            sample_interval,
            u64::MAX,
        )
        .await?;
        validate_process_evidence(
            &outcome.evidence,
            sample_interval,
            timeout,
            output_limit,
            u64::MAX,
            outcome
                .evidence
                .containment_mechanism
                .has_complete_membership(),
        )?;
        if outcome
            .evidence
            .containment_mechanism
            .has_complete_membership()
        {
            require(
                outcome
                    .evidence
                    .raw_samples
                    .iter()
                    .any(|sample| sample.processes.len() >= 2),
                "complete-membership smoke never observed the descendant",
            )?;
        } else {
            require(
                !outcome.evidence.complete_tree_claim_eligible,
                "partial-membership mechanism claimed a complete tree",
            )?;
        }

        let directory = tempfile::tempdir()?;
        let marker = directory.path().join("descendant-finished");
        let marker_text = marker
            .to_str()
            .ok_or("timeout marker path is not Unicode")?;
        let timeout_duration = Duration::from_millis(100);
        let timed_out = run_measured_process_with_environment(
            &executable,
            &arguments,
            &[
                (PROBE_ROOT_ENV, "1"),
                (PROBE_HANG_ENV, "1"),
                (PROBE_MARKER_ENV, marker_text),
            ],
            timeout_duration,
            output_limit,
            sample_interval,
            u64::MAX,
        )
        .await?;
        thread::sleep(Duration::from_millis(400));
        require(
            timed_out.evidence.timed_out,
            "hard timeout was not recorded",
        )?;
        require(
            timed_out.evidence.exit_code.is_none(),
            format!(
                "hard timeout unexpectedly retained exit code {:?}",
                timed_out.evidence.exit_code
            ),
        )?;
        require(
            !timed_out.evidence.successful_bounded_completion,
            "hard timeout serialized as successful bounded completion",
        )?;
        require(
            timed_out.evidence.sampler_drain_completed,
            "hard timeout sampler did not drain",
        )?;
        require(
            timed_out.evidence.teardown_completed,
            format!(
                "hard timeout teardown retained members {:?}",
                timed_out.evidence.post_teardown_members
            ),
        )?;
        require(
            validate_process_evidence(
                &timed_out.evidence,
                sample_interval,
                timeout_duration,
                output_limit,
                u64::MAX,
                false,
            )
            .is_err(),
            "hard timeout evidence passed success validation",
        )?;
        require(
            !marker.exists(),
            format!(
                "hard-timeout descendant survived containment teardown: mechanism={:?}, duration_ns={}, samples={:?}, terminal_members={:?}, post_teardown_members={:?}",
                timed_out.evidence.containment_mechanism,
                timed_out.evidence.duration_ns,
                timed_out
                    .evidence
                    .raw_samples
                    .iter()
                    .map(|sample| sample
                        .processes
                        .iter()
                        .map(|process| process.pid)
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                timed_out.evidence.terminal_members_before_teardown,
                timed_out.evidence.post_teardown_members
            ),
        )?;
        Ok(())
    }

    /// Output overflow remains bounded and cannot serialize as successful evidence.
    #[tokio::test(flavor = "current_thread")]
    async fn task_arri_ut_arri_4_23_process_output_is_bounded()
    -> Result<(), Box<dyn std::error::Error>> {
        let executable = env::current_exe()?;
        let arguments = vec![
            "--exact".to_owned(),
            PROBE_TEST_NAME.to_owned(),
            "--nocapture".to_owned(),
        ];
        let timeout = Duration::from_secs(5);
        let sample_interval = Duration::from_millis(20);
        let output_limit = 128;
        let outcome = run_measured_process_with_environment(
            &executable,
            &arguments,
            &[(PROBE_OUTPUT_ENV, "1")],
            timeout,
            output_limit,
            sample_interval,
            u64::MAX,
        )
        .await?;
        require(
            outcome.evidence.output_truncated
                && outcome.evidence.stdout.retained_bytes <= usize_to_u64(output_limit)
                && outcome.evidence.stderr.retained_bytes <= usize_to_u64(output_limit)
                && !outcome.evidence.successful_bounded_completion
                && validate_process_evidence(
                    &outcome.evidence,
                    sample_interval,
                    timeout,
                    output_limit,
                    u64::MAX,
                    false,
                )
                .is_err(),
            "bounded output overflow was accepted as successful evidence",
        )?;
        Ok(())
    }
}
