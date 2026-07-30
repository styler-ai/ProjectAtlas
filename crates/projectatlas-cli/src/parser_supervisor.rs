//! Bounded process supervision for the separately shipped optional parser pack.

use std::fs::{self, File, Metadata};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::io::Seek;
use std::io::{self, Read, Write};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use crate::parser_linux_authority::{
    ACCEPTED_FD_ARGUMENT, ARTIFACT_FD_ARGUMENT, GRAMMAR_FD_ARGUMENT, POLICY_FD_ARGUMENT,
    SERVE_ARGUMENT,
};
use projectatlas_core::IndexCancellation;
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES, OptionalParserPackArtifactManifest,
    OptionalParserPackManifest, OptionalParserPackManifestError, PackPlatform,
    ParserPackMemoryProbe, ParserPackPayloadRole,
};
#[cfg(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
))]
use projectatlas_core::optional_parser_pack::{ParserPackMemoryControl, ParserPackVerifiedControl};
#[cfg(windows)]
use projectatlas_core::optional_parser_protocol::PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE;
use projectatlas_core::optional_parser_protocol::{
    PARSER_FRAME_HEADER_BYTES, PARSER_MAX_NODE_COUNT, PARSER_MAX_OUTPUT_BYTES,
    PARSER_MAX_SOURCE_BYTES, PARSER_MAX_STDERR_BYTES, PARSER_MAX_TREE_DEPTH,
    PARSER_SESSION_ENTROPY_BYTES, PARSER_WINDOWS_BROKER_ADMISSION_RECORD, ParserArtifactIdentity,
    ParserCompletionEvidence, ParserContainmentKind, ParserControl, ParserFailureCode, ParserFrame,
    ParserFrameHeader, ParserFrameKind, ParserLanguageIdentity, ParserProgress,
    ParserProgressDisposition, ParserProtocolError, ParserRequest, ParserRequestIdentity,
    ParserRequestLimits, ParserSessionIdentity, ParserSessionOpen, ParserSourceIdentity,
    decode_parser_completion_for_request, decode_parser_failure_for_request,
    decode_parser_progress_for_request, decode_parser_ready_for_launch, encode_parser_control,
};
use projectatlas_core::optional_parser_protocol::{
    PARSER_WORKER_JOB_MEMORY_BYTES, PARSER_WORKER_PROCESS_MEMORY_BYTES,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Exact logical capability manifest packaged beside the worker.
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
/// Exact immutable artifact manifest packaged beside the worker.
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
/// Only accepted Windows broker operation.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const BROKER_SERVE_ARGUMENT: &str = "serve-worker";
/// Poll interval for cancellation and bounded child state.
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Parent-only random record that orders each stdout frame against stderr.
const PARSER_DIAGNOSTIC_FENCE_BYTES: usize = 32;
/// Grace period for a healthy worker to close after its input pipe closes.
const SUPERVISOR_GRACEFUL_CLOSE: Duration = Duration::from_millis(500);
/// Hard cleanup ceiling after a child session becomes terminal.
const SUPERVISOR_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
/// Absolute deadline shared by both fixtures for one artifact grammar admission.
const ARTIFACT_ADMISSION_TIMEOUT: Duration = Duration::from_secs(15);
/// Aggregate ceiling for one complete lifecycle/release artifact admission.
const ARTIFACT_ADMISSION_AGGREGATE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Maximum interval without meaningful worker progress during artifact admission.
const ARTIFACT_ADMISSION_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(5);
/// Source bytes that force post-admission parser allocation through the Windows job limit.
const WINDOWS_MEMORY_PROBE_SOURCE_BYTES: usize = 1024 * 1024;
/// Declared maximum interval between sampled Linux resident-memory observations.
pub const PARSER_LINUX_RSS_OBSERVATION_INTERVAL: Duration = Duration::from_millis(20);

/// Closed memory ceilings owned by one supervisor instance.
#[derive(Clone, Copy)]
struct ParserMemoryLimits {
    /// Maximum resident or committed bytes for the contained worker.
    process_bytes: u64,
    /// Maximum aggregate bytes for the contained process tree.
    process_tree_bytes: u64,
}

impl ParserMemoryLimits {
    /// Production parser-worker ceilings.
    const PRODUCTION: Self = Self {
        process_bytes: PARSER_WORKER_PROCESS_MEMORY_BYTES,
        process_tree_bytes: PARSER_WORKER_JOB_MEMORY_BYTES,
    };

    /// Validate a release-probe limit before it reaches an OS adapter.
    fn checked(self) -> Result<Self, ParserSupervisorError> {
        if self.process_bytes == 0
            || self.process_bytes > PARSER_WORKER_PROCESS_MEMORY_BYTES
            || self.process_tree_bytes < self.process_bytes
            || self.process_tree_bytes > PARSER_WORKER_JOB_MEMORY_BYTES
        {
            return Err(ParserSupervisorError::InvalidMemoryLimits {
                process_bytes: self.process_bytes,
                process_tree_bytes: self.process_tree_bytes,
            });
        }
        Ok(self)
    }
}
/// Bounded chunks used while reading artifact files.
const ARTIFACT_READ_CHUNK_BYTES: usize = 64 * 1024;
/// Request phase used while reading parser-pack launch authority.
const ARTIFACT_IO_PHASE: &str = "artifact authority";
/// Request phase covering process creation and synchronous supervisor setup.
const PROCESS_LAUNCH_PHASE: &str = "process launch";
/// Only one potentially blocked artifact reader may exist per process.
/// ponytail: use a killable helper process if stuck kernel reads become an observed problem.
static ARTIFACT_IO_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Process-wide lease that caps potentially blocked child creation at one.
static PROCESS_SPAWN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Sticky fail-closed ownership for cleanup that completed after its caller returned.
static PROCESS_SPAWN_CLEANUP_FAILURE: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);
/// One-shot deterministic handoff used only by debug-build Linux race tests.
#[cfg(all(debug_assertions, target_os = "linux", target_arch = "x86_64"))]
static LINUX_LAUNCH_TEST_HOOK: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);
/// One-shot debug-test delay at the real currentness boundary.
#[cfg(debug_assertions)]
static CURRENTNESS_TEST_HOOK: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>> =
    std::sync::Mutex::new(None);
/// One-shot debug-test delay immediately before the cumulative process-launch bound check.
#[cfg(debug_assertions)]
static PRE_SPAWN_TEST_HOOK: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>> =
    std::sync::Mutex::new(None);
/// One-shot unit-test delay after an owner-retained rendezvous and before final bounds.
#[cfg(test)]
static PROCESS_SPAWN_AFTER_RENDEZVOUS_TEST_HOOK: std::sync::Mutex<
    Option<Box<dyn FnOnce() + Send>>,
> = std::sync::Mutex::new(None);
/// One-shot unit-test delay after the final bounds decision and before owner notification.
#[cfg(test)]
static PROCESS_SPAWN_AFTER_FINAL_CHECK_TEST_HOOK: std::sync::Mutex<
    Option<Box<dyn FnOnce() + Send>>,
> = std::sync::Mutex::new(None);
/// One-shot unit-test delay before owner-side unadmitted-child cleanup.
#[cfg(test)]
static PROCESS_SPAWN_BEFORE_CLEANUP_TEST_HOOK: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>> =
    std::sync::Mutex::new(None);
/// Maximum bytes read from one kernel-owned Linux accounting record.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const LINUX_MEMORY_RECORD_MAX_BYTES: u64 = 64 * 1024;
/// Canonical unified-cgroup mount used only when the current user already owns a delegation.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CGROUP_V2_ROOT: &str = "/sys/fs/cgroup";
/// Maximum unified-cgroup ancestors inspected while locating an existing delegation.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
const MAX_CGROUP_ANCESTORS: usize = 32;
/// Process-local collision guard for delegated child-cgroup names.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static CGROUP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Closed Linux resident-memory accounting mode attached to one worker session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParserMemoryAccountingKind {
    /// Kernel-enforced delegated cgroup-v2 memory accounting.
    LinuxCgroupV2,
    /// Bounded supervisor sampling of the single-worker `VmRSS` record.
    LinuxProcStatus,
}

/// Failure while validating, running, or cleaning up the optional parser supervisor.
#[derive(Debug, Error)]
pub enum ParserSupervisorError {
    /// The current host has no accepted optional-pack containment adapter.
    #[error("optional parser containment is unsupported on {os}/{architecture}")]
    UnsupportedContainment {
        /// Host operating-system identity.
        os: &'static str,
        /// Host architecture identity.
        architecture: &'static str,
    },
    /// A required pack path could not be canonicalized or inspected.
    #[error("could not inspect optional parser pack path {path:?}")]
    PackPath {
        /// Path being inspected.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A required pack path violated the immutable artifact boundary.
    #[error("optional parser pack path {path:?} is invalid: {reason}")]
    InvalidPackPath {
        /// Rejected path.
        path: PathBuf,
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// One bounded artifact file could not be read.
    #[error("could not read optional parser artifact file {path:?}")]
    ArtifactRead {
        /// Artifact file being read.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Linux could not construct one immutable launch payload.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("could not construct sealed Linux parser launch authority for {role}")]
    LinuxLaunchAuthority {
        /// Stable payload responsibility.
        role: &'static str,
        /// Operating-system failure.
        #[source]
        source: io::Error,
    },
    /// One artifact file exceeded its declared byte ceiling.
    #[error("optional parser artifact file {path:?} has {actual} bytes; maximum is {maximum}")]
    ArtifactFileTooLarge {
        /// Oversized file.
        path: PathBuf,
        /// Observed bytes.
        actual: u64,
        /// Inclusive maximum.
        maximum: u64,
    },
    /// The strict artifact manifest could not be decoded.
    #[error("optional parser artifact manifest is invalid")]
    ArtifactManifestJson {
        /// Strict JSON decoding failure.
        #[source]
        source: serde_json::Error,
    },
    /// A logical or artifact manifest invariant failed.
    #[error("optional parser pack manifest validation failed")]
    ManifestValidation {
        /// Typed manifest failure.
        #[source]
        source: OptionalParserPackManifestError,
    },
    /// An artifact payload did not match its immutable manifest row.
    #[error("optional parser artifact payload {path:?} is invalid: {reason}")]
    PayloadMismatch {
        /// Rejected payload path.
        path: PathBuf,
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// A requested grammar is absent from the accepted capability manifest.
    #[error("optional parser grammar {language_id:?} is not accepted by the verified artifact")]
    GrammarNotAccepted {
        /// Rejected language identity.
        language_id: String,
    },
    /// One accepted fixture produced the opposite root-error state.
    #[error(
        "optional parser grammar {language_id:?} fixture {case_name:?} error state was {actual}; expected {expected}"
    )]
    FixtureExpectationMismatch {
        /// Accepted grammar identity under admission.
        language_id: String,
        /// Manifest-owned fixture case name.
        case_name: String,
        /// Identity-validated worker result.
        actual: bool,
        /// Manifest-declared positive or negative expectation.
        expected: bool,
    },
    /// An internal release probe requested invalid worker or process-tree ceilings.
    #[error(
        "optional parser memory limits are invalid: process {process_bytes} bytes; process tree {process_tree_bytes} bytes"
    )]
    InvalidMemoryLimits {
        /// Requested per-worker ceiling.
        process_bytes: u64,
        /// Requested aggregate process-tree ceiling.
        process_tree_bytes: u64,
    },
    /// The exact worker completed under a deliberately reduced release-probe ceiling.
    #[error(
        "optional parser memory-boundary probe did not breach its {process_bytes}-byte worker ceiling"
    )]
    MemoryProbeDidNotBreach {
        /// Deliberately reduced ceiling that should be below exact-worker residency.
        process_bytes: u64,
    },
    /// A parser protocol invariant failed.
    #[error("optional parser protocol validation failed")]
    Protocol {
        /// Typed protocol failure.
        #[source]
        source: ParserProtocolError,
    },
    /// Operating-system entropy was unavailable for a fresh worker session.
    #[error("operating-system entropy was unavailable for the optional parser session")]
    EntropyUnavailable,
    /// The exact worker or containment broker could not be started.
    #[error("could not launch optional parser program {program:?}")]
    Spawn {
        /// Exact verified executable.
        program: PathBuf,
        /// Process creation failure.
        #[source]
        source: io::Error,
    },
    /// A required child protocol pipe was not created.
    #[error("optional parser child did not expose its {stream} protocol pipe")]
    MissingPipe {
        /// Missing standard-stream identity.
        stream: &'static str,
    },
    /// The Windows broker did not emit the exact admission record.
    #[error("optional parser Windows containment admission did not validate")]
    InvalidAdmission,
    /// A bounded I/O thread failed.
    #[error("optional parser {phase} I/O failed: {message}")]
    IoThread {
        /// Stable I/O phase.
        phase: &'static str,
        /// Bounded failure detail.
        message: String,
    },
    /// The caller requested cooperative cancellation.
    #[error("optional parser operation was cancelled during {phase}")]
    Cancelled {
        /// Stable operation phase.
        phase: &'static str,
    },
    /// The caller-owned absolute deadline elapsed.
    #[error("optional parser absolute deadline elapsed during {phase}")]
    DeadlineExceeded {
        /// Stable operation phase.
        phase: &'static str,
    },
    /// No meaningful progress occurred within the caller limit.
    #[error("optional parser made no progress during {phase}")]
    NoProgress {
        /// Stable operation phase.
        phase: &'static str,
    },
    /// The direct child exited before completing its protocol operation.
    #[error("optional parser child exited during {phase} with code {code:?}")]
    ChildExited {
        /// Stable operation phase.
        phase: &'static str,
        /// Portable process exit code when available.
        code: Option<i32>,
    },
    /// Linux resident memory reached its configured ceiling and the worker group was terminated.
    #[error(
        "optional parser resident-memory ceiling reached during {phase}: observed {observed_bytes} bytes with {accounting:?}; maximum {maximum_bytes} bytes; observation interval {observation_interval_millis} ms"
    )]
    ResidentMemoryLimitExceeded {
        /// Stable operation phase.
        phase: &'static str,
        /// Active Linux accounting path.
        accounting: ParserMemoryAccountingKind,
        /// Last observed resident or cgroup memory bytes.
        observed_bytes: u64,
        /// Inclusive configured ceiling.
        maximum_bytes: u64,
        /// Declared maximum sampling interval.
        observation_interval_millis: u64,
    },
    /// Linux resident-memory accounting became unreadable while the worker was live.
    #[error(
        "optional parser resident-memory observation failed during {phase} with {accounting:?}: {message}"
    )]
    ResidentMemoryObservationFailed {
        /// Stable operation phase.
        phase: &'static str,
        /// Accounting path that failed closed.
        accounting: ParserMemoryAccountingKind,
        /// Bounded failure detail.
        message: String,
    },
    /// The Windows broker observed an exact Job process/job memory-limit completion message.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[error("optional parser Windows Job memory ceiling was reached during {phase}")]
    WindowsJobMemoryLimitExceeded {
        /// Stable operation phase in which the broker terminated.
        phase: &'static str,
    },
    /// The worker returned an identity-validated closed failure.
    #[error("optional parser worker returned {code:?}")]
    WorkerFailure {
        /// Closed worker failure code.
        code: ParserFailureCode,
    },
    /// The session-local request identity space was exhausted.
    #[error("optional parser request identity space was exhausted")]
    RequestIdentityExhausted,
    /// Child-tree termination, pipe draining, reaping, or thread joining failed.
    #[error("optional parser cleanup failed: {message}")]
    Cleanup {
        /// Bounded cleanup detail.
        message: String,
    },
    /// An operation failed and its mandatory cleanup also failed.
    #[error("optional parser operation failed: {operation}; cleanup also failed: {cleanup}")]
    OperationAndCleanup {
        /// Original typed operation failure.
        operation: Box<Self>,
        /// Typed cleanup failure.
        cleanup: Box<Self>,
    },
}

/// Install one debug-build hook after sealed authority is ready and before spawn.
#[cfg(all(debug_assertions, target_os = "linux", target_arch = "x86_64"))]
#[doc(hidden)]
pub fn install_linux_launch_test_hook(
    hook: impl FnOnce() + Send + 'static,
) -> Result<(), ParserSupervisorError> {
    let mut slot =
        LINUX_LAUNCH_TEST_HOOK
            .lock()
            .map_err(|_poisoned| ParserSupervisorError::IoThread {
                phase: "Linux launch test hook",
                message: "test hook lock is poisoned".to_owned(),
            })?;
    if slot.is_some() {
        return Err(ParserSupervisorError::IoThread {
            phase: "Linux launch test hook",
            message: "another test hook is already installed".to_owned(),
        });
    }
    *slot = Some(Box::new(hook));
    Ok(())
}

/// Invoke and remove the one installed debug-build Linux launch hook.
#[cfg(all(debug_assertions, target_os = "linux", target_arch = "x86_64"))]
fn invoke_linux_launch_test_hook() -> Result<(), ParserSupervisorError> {
    let hook = LINUX_LAUNCH_TEST_HOOK
        .lock()
        .map_err(|_poisoned| ParserSupervisorError::IoThread {
            phase: "Linux launch test hook",
            message: "test hook lock is poisoned".to_owned(),
        })?
        .take();
    if let Some(hook) = hook {
        hook();
    }
    Ok(())
}

/// Install one debug-build hook at the first launch-input currentness observation.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn install_currentness_test_hook(
    hook: impl FnOnce() + Send + 'static,
) -> Result<(), ParserSupervisorError> {
    let mut slot =
        CURRENTNESS_TEST_HOOK
            .lock()
            .map_err(|_poisoned| ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "currentness test hook lock is poisoned".to_owned(),
            })?;
    if slot.is_some() {
        return Err(ParserSupervisorError::IoThread {
            phase: ARTIFACT_IO_PHASE,
            message: "another currentness test hook is already installed".to_owned(),
        });
    }
    *slot = Some(Box::new(hook));
    Ok(())
}

/// Invoke and remove the installed currentness test hook.
#[cfg(debug_assertions)]
fn invoke_currentness_test_hook() -> Result<(), ParserSupervisorError> {
    let hook = CURRENTNESS_TEST_HOOK
        .lock()
        .map_err(|_poisoned| ParserSupervisorError::IoThread {
            phase: ARTIFACT_IO_PHASE,
            message: "currentness test hook lock is poisoned".to_owned(),
        })?
        .take();
    if let Some(hook) = hook {
        hook();
    }
    Ok(())
}

/// Install one debug-build delay before the final pre-spawn bound check.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn install_pre_spawn_test_hook(
    hook: impl FnOnce() + Send + 'static,
) -> Result<(), ParserSupervisorError> {
    let mut slot =
        PRE_SPAWN_TEST_HOOK
            .lock()
            .map_err(|_poisoned| ParserSupervisorError::IoThread {
                phase: PROCESS_LAUNCH_PHASE,
                message: "pre-spawn test hook lock is poisoned".to_owned(),
            })?;
    if slot.is_some() {
        return Err(ParserSupervisorError::IoThread {
            phase: PROCESS_LAUNCH_PHASE,
            message: "another pre-spawn test hook is already installed".to_owned(),
        });
    }
    *slot = Some(Box::new(hook));
    Ok(())
}

/// Invoke and remove the installed pre-spawn test hook.
#[cfg(debug_assertions)]
fn invoke_pre_spawn_test_hook() -> Result<(), ParserSupervisorError> {
    let hook = PRE_SPAWN_TEST_HOOK
        .lock()
        .map_err(|_poisoned| ParserSupervisorError::IoThread {
            phase: PROCESS_LAUNCH_PHASE,
            message: "pre-spawn test hook lock is poisoned".to_owned(),
        })?
        .take();
    if let Some(hook) = hook {
        hook();
    }
    Ok(())
}

impl ParserSupervisorError {
    /// Return whether the caller stopped an otherwise live protocol operation.
    const fn is_caller_stop(&self) -> bool {
        matches!(
            self,
            Self::Cancelled { .. } | Self::DeadlineExceeded { .. } | Self::NoProgress { .. }
        )
    }

    /// Return whether mandatory process, pipe, reap, or thread cleanup failed.
    ///
    /// [`Self::OperationAndCleanup`] is itself a cleanup failure even when its
    /// operation or cleanup branch contains another nested combined failure.
    #[must_use]
    pub const fn has_mandatory_cleanup_failure(&self) -> bool {
        matches!(
            self,
            Self::Cleanup { .. } | Self::OperationAndCleanup { .. }
        )
    }
}

impl From<ParserProtocolError> for ParserSupervisorError {
    fn from(source: ParserProtocolError) -> Self {
        Self::Protocol { source }
    }
}

impl From<OptionalParserPackManifestError> for ParserSupervisorError {
    fn from(source: OptionalParserPackManifestError) -> Self {
        Self::ManifestValidation { source }
    }
}

/// Constant-size filesystem identity used to detect mutation without rehashing a hot path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FileChangeEpoch {
    /// Observed file length.
    bytes: u64,
    /// Modification timestamp when the host filesystem exposes one.
    modified: Option<SystemTime>,
    /// Filesystem device identity.
    #[cfg(unix)]
    device: u64,
    /// Filesystem inode identity.
    #[cfg(unix)]
    inode: u64,
    /// Last metadata-change time, which cannot be restored through ordinary mtime APIs.
    #[cfg(unix)]
    changed_seconds: i64,
    /// Nanosecond component of the last metadata-change time.
    #[cfg(unix)]
    changed_nanoseconds: i64,
    /// Windows file attributes captured while an owned handle denies writes and replacement.
    #[cfg(windows)]
    attributes: u32,
    /// Windows creation time captured while an owned handle denies writes and replacement.
    #[cfg(windows)]
    created: u64,
}

impl FileChangeEpoch {
    /// Capture the platform metadata that changes when an observed file is replaced or mutated.
    fn from_metadata(metadata: &Metadata) -> Self {
        if !metadata.is_file() {
            return Self::default();
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Self {
                bytes: metadata.len(),
                modified: metadata.modified().ok(),
                device: metadata.dev(),
                inode: metadata.ino(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            Self {
                bytes: metadata.len(),
                modified: metadata.modified().ok(),
                attributes: metadata.file_attributes(),
                created: metadata.creation_time(),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self {
                bytes: metadata.len(),
                modified: metadata.modified().ok(),
            }
        }
    }
}

/// Open one observed file while denying Windows write and replacement sharing.
fn open_observed_file(path: &Path) -> Result<File, ParserSupervisorError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    options
        .open(path)
        .map_err(|source| ParserSupervisorError::ArtifactRead {
            path: path.to_path_buf(),
            source,
        })
}

/// One file observed before digest verification and kept write-locked on Windows.
#[derive(Debug)]
struct FileObservation {
    /// Canonical file path.
    path: PathBuf,
    /// Constant-size identity captured before digest verification.
    epoch: FileChangeEpoch,
    /// Owned handle that denies Windows write and replacement sharing.
    #[cfg(windows)]
    write_guard: Option<File>,
}

/// Owned constant-size file identity safe to move behind bounded filesystem I/O.
#[derive(Debug)]
struct FileCurrentnessProbe {
    /// Canonical path whose current identity must still match.
    path: PathBuf,
    /// Identity captured before digest verification.
    epoch: FileChangeEpoch,
    /// Whether Windows still owns the deny-write/delete handle.
    #[cfg(windows)]
    guarded: bool,
    /// Deterministic metadata-boundary blocker for cancellation tests.
    #[cfg(test)]
    blocker: Option<std::sync::Arc<MetadataProbeBlocker>>,
}

/// Deterministically pauses the test-only path-observation boundary.
#[cfg(test)]
#[derive(Debug)]
struct MetadataProbeBlocker {
    /// Signals that the filesystem worker reached the metadata boundary.
    entered: SyncSender<()>,
    /// Releases the worker so the real metadata lookup can continue.
    release: std::sync::Mutex<Receiver<()>>,
}

#[cfg(test)]
impl MetadataProbeBlocker {
    /// Pause immediately before the real metadata lookup.
    fn wait(&self) -> Result<(), ParserSupervisorError> {
        self.entered
            .send(())
            .map_err(|_closed| ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "metadata-probe entry receiver closed".to_owned(),
            })?;
        self.release
            .lock()
            .map_err(|_poisoned| ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "metadata-probe release lock was poisoned".to_owned(),
            })?
            .recv()
            .map_err(|_closed| ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "metadata-probe release sender closed".to_owned(),
            })
    }
}

impl FileCurrentnessProbe {
    /// Observe the path and compare it with the verified change epoch.
    fn is_current(&self) -> Result<bool, ParserSupervisorError> {
        #[cfg(debug_assertions)]
        invoke_currentness_test_hook()?;
        #[cfg(test)]
        if let Some(blocker) = &self.blocker {
            blocker.wait()?;
        }
        #[cfg(windows)]
        if !self.guarded {
            return Ok(false);
        }
        let metadata =
            fs::metadata(&self.path).map_err(|source| ParserSupervisorError::ArtifactRead {
                path: self.path.clone(),
                source,
            })?;
        Ok(metadata.is_file() && FileChangeEpoch::from_metadata(&metadata) == self.epoch)
    }
}

impl FileObservation {
    /// Capture one regular file before its digest is read and verified.
    fn capture(path: PathBuf) -> Result<Self, ParserSupervisorError> {
        let write_guard = open_observed_file(&path)?;
        let metadata =
            write_guard
                .metadata()
                .map_err(|source| ParserSupervisorError::ArtifactRead {
                    path: path.clone(),
                    source,
                })?;
        if !metadata.is_file() {
            return Err(ParserSupervisorError::PayloadMismatch {
                path,
                reason: "payload is not a regular file",
            });
        }
        Ok(Self {
            path,
            epoch: FileChangeEpoch::from_metadata(&metadata),
            #[cfg(windows)]
            write_guard: Some(write_guard),
        })
    }

    /// Return whether the guarded path still resolves to the captured file identity.
    #[cfg(test)]
    fn is_current(&self) -> Result<bool, ParserSupervisorError> {
        self.currentness_probe().is_current()
    }

    /// Copy only the bounded path and metadata needed by the filesystem worker.
    fn currentness_probe(&self) -> FileCurrentnessProbe {
        FileCurrentnessProbe {
            path: self.path.clone(),
            epoch: self.epoch,
            #[cfg(windows)]
            guarded: self.write_guard.is_some(),
            #[cfg(test)]
            blocker: None,
        }
    }

    /// Build deliberately unavailable file authority for process-free tests.
    #[cfg(test)]
    fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            epoch: FileChangeEpoch::default(),
            #[cfg(windows)]
            write_guard: None,
        }
    }
}

/// Metadata used to detect mutation around and after payload digest verification.
#[derive(Debug)]
struct PayloadObservation {
    /// Guarded canonical payload file.
    file: FileObservation,
    /// Manifest-owned payload responsibility.
    role: ParserPackPayloadRole,
    /// Exact manifest-owned byte count.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    bytes: u64,
    /// Exact manifest-owned SHA-256 digest.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    sha256: String,
}

impl PayloadObservation {
    /// Return whether this payload can affect one grammar-affined worker launch.
    fn contributes_to_launch(&self, language_id: &str) -> bool {
        let shared_launch_input = matches!(
            &self.role,
            ParserPackPayloadRole::Worker
                | ParserPackPayloadRole::ContainmentBroker
                | ParserPackPayloadRole::AcceptedManifest
        );
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let shared_launch_input =
            shared_launch_input || matches!(&self.role, ParserPackPayloadRole::NativeImportPolicy);
        shared_launch_input
            || matches!(
                &self.role,
                ParserPackPayloadRole::GrammarLibrary {
                    language_id: payload_language
                } if payload_language == language_id
            )
    }

    /// Return whether one payload retains the identity captured before digest verification.
    #[cfg(test)]
    fn is_current(&self) -> Result<bool, ParserSupervisorError> {
        self.file.is_current()
    }

    /// Retain the immutable manifest row without retaining the source handle.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn linux_spec(&self) -> VerifiedLinuxPayloadSpec {
        VerifiedLinuxPayloadSpec {
            path: self.file.path.clone(),
            epoch: self.file.epoch,
            bytes: self.bytes,
            sha256: self.sha256.clone(),
        }
    }
}

/// Request-owned stop bounds shared by every pre-READY artifact phase.
struct ArtifactIoControl<'a> {
    /// Immutable absolute request deadline.
    absolute_deadline: Instant,
    /// Fixed pre-READY progress epoch; artifact work does not extend the bound.
    last_progress: Instant,
    /// Maximum pre-READY interval without validated parser progress.
    no_progress_timeout: Duration,
    /// Request-owned cooperative cancellation signal.
    cancellation: &'a IndexCancellation,
}

impl ArtifactIoControl<'_> {
    /// Reject cancellation or an expired request bound before more reload work.
    fn poll(&self) -> Result<(), ParserSupervisorError> {
        poll_stop(
            ARTIFACT_IO_PHASE,
            self.absolute_deadline,
            self.last_progress,
            self.no_progress_timeout,
            self.cancellation,
        )
    }
}

/// Process-wide lease that caps potentially blocked artifact readers at one.
struct ArtifactIoLease;

impl ArtifactIoLease {
    /// Acquire the only artifact-reader slot.
    fn acquire() -> Result<Self, ParserSupervisorError> {
        ARTIFACT_IO_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_inactive| Self)
            .map_err(|_active| ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "another parser-pack artifact reader is still active".to_owned(),
            })
    }
}

impl Drop for ArtifactIoLease {
    fn drop(&mut self) {
        ARTIFACT_IO_ACTIVE.store(false, Ordering::Release);
    }
}

/// One metadata-probe request owned by the process-wide filesystem worker.
struct ArtifactCurrentnessRequest {
    /// Exact constant-size path observations for this parse request.
    probe: ArtifactCurrentnessProbe,
    /// Immutable absolute request deadline.
    absolute_deadline: Instant,
    /// Caller-owned pre-READY progress epoch.
    last_progress: Instant,
    /// Maximum metadata-probe duration.
    no_progress_timeout: Duration,
    /// Request-owned cooperative cancellation signal.
    cancellation: IndexCancellation,
    /// One-shot response channel; a canceled caller may close it before completion.
    response: SyncSender<Result<bool, ParserSupervisorError>>,
    /// Process-wide admission retained even when a filesystem call remains blocked.
    lease: ArtifactIoLease,
}

/// Start the single lazy process-wide metadata worker.
fn artifact_currentness_sender()
-> Result<&'static SyncSender<ArtifactCurrentnessRequest>, ParserSupervisorError> {
    static WORKER: OnceLock<Result<SyncSender<ArtifactCurrentnessRequest>, String>> =
        OnceLock::new();

    match WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<ArtifactCurrentnessRequest>(1);
        thread::Builder::new()
            .name("projectatlas-artifact-currentness".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let ArtifactCurrentnessRequest {
                        probe,
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation,
                        response,
                        lease,
                    } = request;
                    let control = ArtifactIoControl {
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation: &cancellation,
                    };
                    let result = probe.is_current(Some(&control));
                    drop(lease);
                    let _send_result = response.try_send(result);
                }
            })
            .map(|worker| {
                drop(worker);
                sender
            })
            .map_err(|source| bounded_message(source.to_string()))
    }) {
        Ok(sender) => Ok(sender),
        Err(message) => Err(ParserSupervisorError::IoThread {
            phase: ARTIFACT_IO_PHASE,
            message: message.clone(),
        }),
    }
}

/// Run one hot-path metadata probe without exposing blocking filesystem calls to the caller.
fn run_bounded_artifact_currentness(
    probe: ArtifactCurrentnessProbe,
    control: &ArtifactIoControl<'_>,
) -> Result<bool, ParserSupervisorError> {
    control.poll()?;
    let sender = artifact_currentness_sender()?;
    let lease = ArtifactIoLease::acquire()?;
    let (response, receiver) = mpsc::sync_channel(1);
    let request = ArtifactCurrentnessRequest {
        probe,
        absolute_deadline: control.absolute_deadline,
        last_progress: control.last_progress,
        no_progress_timeout: control.no_progress_timeout,
        cancellation: control.cancellation.clone(),
        response,
        lease,
    };
    match sender.try_send(request) {
        Ok(()) => {}
        Err(TrySendError::Full(_request)) => {
            return Err(ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "parser-pack currentness worker is still active".to_owned(),
            });
        }
        Err(TrySendError::Disconnected(_request)) => {
            return Err(ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                message: "parser-pack currentness worker disconnected".to_owned(),
            });
        }
    }

    loop {
        control.poll()?;
        match receiver.recv_timeout(next_poll_wait(
            control.absolute_deadline,
            control.last_progress,
            control.no_progress_timeout,
        )) {
            Ok(result) => {
                control.poll()?;
                return result;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ParserSupervisorError::IoThread {
                    phase: ARTIFACT_IO_PHASE,
                    message: "parser-pack currentness response disconnected".to_owned(),
                });
            }
        }
    }
}

/// Run potentially blocking artifact I/O behind a request-bounded worker.
fn run_bounded_artifact_io<T>(
    operation: impl FnOnce() -> Result<T, ParserSupervisorError> + Send + 'static,
    control: &ArtifactIoControl<'_>,
) -> Result<T, ParserSupervisorError>
where
    T: Send + 'static,
{
    control.poll()?;
    let lease = ArtifactIoLease::acquire()?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("projectatlas-artifact-authority".to_owned())
        .spawn(move || {
            let result = operation();
            drop(lease);
            let _send_result = sender.send(result);
        })
        .map_err(|source| ParserSupervisorError::IoThread {
            phase: ARTIFACT_IO_PHASE,
            message: bounded_message(source.to_string()),
        })?;
    drop(worker);

    loop {
        control.poll()?;
        match receiver.recv_timeout(next_poll_wait(
            control.absolute_deadline,
            control.last_progress,
            control.no_progress_timeout,
        )) {
            Ok(result) => {
                control.poll()?;
                return result;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ParserSupervisorError::IoThread {
                    phase: ARTIFACT_IO_PHASE,
                    message: "parser-pack artifact reader disconnected".to_owned(),
                });
            }
        }
    }
}

/// Process-wide lease retained until a blocked spawn returns and any late child is reaped.
struct ProcessSpawnLease;

impl ProcessSpawnLease {
    /// Acquire the only potentially blocked process-creation slot.
    fn acquire() -> Result<Self, ParserSupervisorError> {
        PROCESS_SPAWN_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_inactive| Self)
            .map_err(|_active| ParserSupervisorError::IoThread {
                phase: PROCESS_LAUNCH_PHASE,
                message: "another optional-parser process creation is still active".to_owned(),
            })
    }
}

impl Drop for ProcessSpawnLease {
    fn drop(&mut self) {
        PROCESS_SPAWN_ACTIVE.store(false, Ordering::Release);
    }
}

/// Child that must be reaped unless the caller explicitly accepts ownership.
struct UnadmittedChild {
    /// Direct worker or broker child.
    child: Option<Child>,
    /// Process-creation slot retained until admission or mandatory cleanup.
    _lease: ProcessSpawnLease,
}

impl UnadmittedChild {
    /// Retain cleanup ownership across a bounded caller handoff.
    const fn new(child: Child, lease: ProcessSpawnLease) -> Self {
        Self {
            child: Some(child),
            _lease: lease,
        }
    }

    /// Transfer the child to the normal resident-session owner.
    fn admit(mut self) -> Result<Child, ParserSupervisorError> {
        self.child
            .take()
            .ok_or_else(|| ParserSupervisorError::IoThread {
                phase: PROCESS_LAUNCH_PHASE,
                message: "process-spawn worker returned no child".to_owned(),
            })
    }
}

impl Drop for UnadmittedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        #[cfg(test)]
        if let Ok(mut slot) = PROCESS_SPAWN_BEFORE_CLEANUP_TEST_HOOK.lock()
            && let Some(hook) = slot.take()
        {
            hook();
        }
        if let Err(error) = cleanup_partial_launch(&mut child, Vec::new(), None, None, None) {
            record_process_spawn_cleanup_failure(&error);
        }
    }
}

/// Preserve the first late cleanup failure for every later launch attempt.
fn record_process_spawn_cleanup_failure(error: &ParserSupervisorError) {
    if let Ok(mut slot) = PROCESS_SPAWN_CLEANUP_FAILURE.lock()
        && slot.is_none()
    {
        *slot = Some(bounded_message(format!(
            "late optional-parser process cleanup failed: {error}"
        )));
    }
}

/// Reject new launches after a late cleanup failure has made process ownership uncertain.
fn require_process_spawn_cleanup_health() -> Result<(), ParserSupervisorError> {
    let slot = PROCESS_SPAWN_CLEANUP_FAILURE.lock().map_err(|_poisoned| {
        ParserSupervisorError::IoThread {
            phase: PROCESS_LAUNCH_PHASE,
            message: "process-spawn cleanup state is poisoned".to_owned(),
        }
    })?;
    if let Some(message) = slot.as_ref() {
        return Err(ParserSupervisorError::Cleanup {
            message: message.clone(),
        });
    }
    Ok(())
}

/// Run one potentially blocking `Command::spawn` without retaining the bounded caller.
fn run_bounded_process_spawn(
    command: Command,
    absolute_deadline: Instant,
    last_progress: Instant,
    no_progress_timeout: Duration,
    cancellation: &IndexCancellation,
) -> Result<Child, ParserSupervisorError> {
    run_bounded_process_spawn_with(
        command,
        absolute_deadline,
        last_progress,
        no_progress_timeout,
        cancellation,
        |mut command| command.spawn(),
    )
}

/// Execute the concrete spawn operation behind one owner-side admission handshake.
fn run_bounded_process_spawn_with(
    command: Command,
    absolute_deadline: Instant,
    last_progress: Instant,
    no_progress_timeout: Duration,
    cancellation: &IndexCancellation,
    spawn: impl FnOnce(Command) -> io::Result<Child> + Send + 'static,
) -> Result<Child, ParserSupervisorError> {
    poll_stop(
        PROCESS_LAUNCH_PHASE,
        absolute_deadline,
        last_progress,
        no_progress_timeout,
        cancellation,
    )?;
    require_process_spawn_cleanup_health()?;
    let lease = ProcessSpawnLease::acquire()?;
    let program = PathBuf::from(command.get_program());
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (rendezvous_sender, rendezvous_receiver) = mpsc::sync_channel(0);
    let (handoff_commit_sender, handoff_commit_receiver) = mpsc::sync_channel(1);
    let (child_sender, child_receiver) = mpsc::sync_channel(0);
    let worker = thread::Builder::new()
        .name("projectatlas-process-spawn".to_owned())
        .spawn(move || {
            let child = match spawn(command) {
                Ok(child) => UnadmittedChild::new(child, lease),
                Err(source) => {
                    let _undelivered =
                        ready_sender.send(Err(ParserSupervisorError::Spawn { program, source }));
                    return;
                }
            };
            if ready_sender.send(Ok(())).is_err() {
                return;
            }
            if rendezvous_sender.send(()).is_err() {
                return;
            }
            if handoff_commit_receiver.recv().is_err() {
                return;
            }
            if let Err(undelivered) = child_sender.send(child) {
                drop(undelivered);
            }
        })
        .map_err(|source| ParserSupervisorError::IoThread {
            phase: PROCESS_LAUNCH_PHASE,
            message: bounded_message(source.to_string()),
        })?;
    drop(worker);

    loop {
        poll_stop(
            PROCESS_LAUNCH_PHASE,
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        )?;
        match ready_receiver.recv_timeout(next_poll_wait(
            absolute_deadline,
            last_progress,
            no_progress_timeout,
        )) {
            Ok(ready) => {
                ready?;
                poll_stop(
                    PROCESS_LAUNCH_PHASE,
                    absolute_deadline,
                    last_progress,
                    no_progress_timeout,
                    cancellation,
                )?;
                loop {
                    poll_stop(
                        PROCESS_LAUNCH_PHASE,
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation,
                    )?;
                    match rendezvous_receiver.recv_timeout(next_poll_wait(
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                    )) {
                        Ok(()) => {
                            #[cfg(test)]
                            if let Ok(mut slot) = PROCESS_SPAWN_AFTER_RENDEZVOUS_TEST_HOOK.lock()
                                && let Some(hook) = slot.take()
                            {
                                hook();
                            }
                            poll_stop(
                                PROCESS_LAUNCH_PHASE,
                                absolute_deadline,
                                last_progress,
                                no_progress_timeout,
                                cancellation,
                            )?;
                            // The successful final check commits ownership. This bounded
                            // acknowledgement only notifies the owner; later stops belong
                            // to the normal resident-session owner.
                            #[cfg(test)]
                            if let Ok(mut slot) = PROCESS_SPAWN_AFTER_FINAL_CHECK_TEST_HOOK.lock()
                                && let Some(hook) = slot.take()
                            {
                                hook();
                            }
                            handoff_commit_sender.send(()).map_err(|_closed| {
                                ParserSupervisorError::IoThread {
                                    phase: PROCESS_LAUNCH_PHASE,
                                    message:
                                        "process-spawn owner disconnected before handoff commit"
                                            .to_owned(),
                                }
                            })?;
                            return child_receiver
                                .recv()
                                .map_err(|_closed| ParserSupervisorError::IoThread {
                                    phase: PROCESS_LAUNCH_PHASE,
                                    message:
                                        "process-spawn owner disconnected during committed handoff"
                                            .to_owned(),
                                })?
                                .admit();
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            return Err(ParserSupervisorError::IoThread {
                                phase: PROCESS_LAUNCH_PHASE,
                                message: "process-spawn owner disconnected during rendezvous"
                                    .to_owned(),
                            });
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ParserSupervisorError::IoThread {
                    phase: PROCESS_LAUNCH_PHASE,
                    message: "process-spawn worker disconnected".to_owned(),
                });
            }
        }
    }
}

/// Bytes and digest produced together by one bounded artifact-file pass.
struct BoundedArtifactRead {
    /// Exact bounded file bytes.
    bytes: Vec<u8>,
    /// Lowercase SHA-256 computed during the bounded read.
    sha256: String,
}

/// Manifest-owned identity needed to re-read one launch payload exactly.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Clone, Debug)]
struct VerifiedLinuxPayloadSpec {
    /// Canonical path already constrained to the parser-pack root.
    path: PathBuf,
    /// File identity captured before the artifact digest was accepted.
    epoch: FileChangeEpoch,
    /// Exact declared byte count.
    bytes: u64,
    /// Exact lowercase SHA-256 digest.
    sha256: String,
}

/// Read-only, fully sealed Linux launch payload.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Debug)]
struct SealedLinuxPayload {
    /// Read-only descriptor for the sealed memfd inode.
    file: File,
}

/// Create a modern memfd and retry only the unsupported-flag legacy-kernel case.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn create_memfd_with_legacy_fallback<T>(
    flags: nix::sys::memfd::MFdFlags,
    mode_flag: nix::libc::c_uint,
    mut create: impl FnMut(nix::sys::memfd::MFdFlags) -> Result<T, nix::errno::Errno>,
) -> Result<T, nix::errno::Errno> {
    let requested_flags = flags | nix::sys::memfd::MFdFlags::from_bits_retain(mode_flag);
    match create(requested_flags) {
        Err(nix::errno::Errno::EINVAL) => create(flags),
        result => result,
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl SealedLinuxPayload {
    /// Copy already verified bytes into one immutable anonymous file.
    fn from_verified_bytes(
        role: &'static str,
        name: &str,
        bytes: &[u8],
        executable: bool,
        control: &ArtifactIoControl<'_>,
    ) -> Result<Self, ParserSupervisorError> {
        use nix::sys::memfd::memfd_create;

        Self::from_verified_bytes_with_create(role, name, bytes, executable, control, |flags| {
            memfd_create(name, flags)
        })
    }

    /// Copy verified bytes through an injected memfd creator for fallback-path proof.
    fn from_verified_bytes_with_create(
        role: &'static str,
        name: &str,
        bytes: &[u8],
        executable: bool,
        control: &ArtifactIoControl<'_>,
        create: impl FnMut(nix::sys::memfd::MFdFlags) -> Result<std::os::fd::OwnedFd, nix::errno::Errno>,
    ) -> Result<Self, ParserSupervisorError> {
        use nix::fcntl::{FcntlArg, SealFlag, fcntl};
        use nix::libc;
        use nix::sys::memfd::MFdFlags;
        use nix::sys::stat::{Mode, fchmod};

        let authority_error =
            |source: nix::errno::Errno| ParserSupervisorError::LinuxLaunchAuthority {
                role,
                source: io::Error::from_raw_os_error(source as i32),
            };
        let flags = MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING;
        let mode_flag = if executable {
            libc::MFD_EXEC
        } else {
            libc::MFD_NOEXEC_SEAL
        };
        let descriptor =
            create_memfd_with_legacy_fallback(flags, mode_flag, create).map_err(authority_error)?;
        let mut file = File::from(descriptor);
        let mode = if executable {
            Mode::S_IRUSR | Mode::S_IXUSR
        } else {
            Mode::S_IRUSR
        };
        fchmod(&file, mode).map_err(authority_error)?;
        for chunk in bytes.chunks(ARTIFACT_READ_CHUNK_BYTES) {
            control.poll()?;
            file.write_all(chunk)
                .map_err(|source| ParserSupervisorError::LinuxLaunchAuthority { role, source })?;
        }
        file.rewind()
            .map_err(|source| ParserSupervisorError::LinuxLaunchAuthority { role, source })?;
        let required = SealFlag::F_SEAL_WRITE
            | SealFlag::F_SEAL_GROW
            | SealFlag::F_SEAL_SHRINK
            | SealFlag::F_SEAL_SEAL;
        fcntl(&file, FcntlArg::F_ADD_SEALS(required)).map_err(authority_error)?;
        let observed = fcntl(&file, FcntlArg::F_GET_SEALS).map_err(authority_error)?;
        if observed & required.bits() != required.bits() {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: PathBuf::from(name),
                reason: "Linux launch authority does not carry the complete seal set",
            });
        }

        let read_only_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        let read_only = File::open(&read_only_path)
            .map_err(|source| ParserSupervisorError::LinuxLaunchAuthority { role, source })?;
        drop(file);
        Ok(Self { file: read_only })
    }

    /// Return the process-local descriptor identity retained through spawn.
    fn raw_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
}

/// Exact immutable authority consumed by one Linux resident launch.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Debug)]
struct LinuxResidentLaunchAuthority {
    /// Executable parser worker.
    worker: SealedLinuxPayload,
    /// Exact artifact-manifest bytes.
    artifact_manifest: SealedLinuxPayload,
    /// Exact accepted-capability manifest bytes.
    accepted_manifest: SealedLinuxPayload,
    /// Exact native-import policy bytes.
    native_import_policy: SealedLinuxPayload,
    /// One grammar selected for this resident.
    grammar: SealedLinuxPayload,
}

/// Complete private launch authority derived from one exact immutable artifact.
#[derive(Debug)]
struct VerifiedParserPackLaunch {
    /// Canonical artifact root.
    pack_root: PathBuf,
    /// Accepted target bound by the artifact manifest.
    platform: PackPlatform,
    /// Exact containment broker launched on Windows.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    containment_broker: Option<PathBuf>,
    /// Sorted accepted language identities.
    accepted_grammars: Vec<String>,
    /// Exact artifact-manifest byte identity independently observed by Rust.
    artifact: ParserArtifactIdentity,
    /// Exact already verified artifact-manifest bytes retained for Linux handoff.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    artifact_manifest_bytes: Vec<u8>,
    /// Exact already verified accepted-capability bytes retained for Linux handoff.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    accepted_manifest_bytes: Vec<u8>,
    /// Exact already verified native-import policy bytes retained for Linux handoff.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    native_import_policy_bytes: Vec<u8>,
    /// Guarded artifact manifest captured before verification and rechecked afterward.
    artifact_manifest: FileObservation,
    /// Cheap metadata observations for every already hashed payload.
    payloads: Vec<PayloadObservation>,
    /// Test-only pause at the real currentness metadata boundary.
    #[cfg(test)]
    currentness_blocker: Option<std::sync::Arc<MetadataProbeBlocker>>,
}

/// One complete owned change-epoch probe for a grammar-affined parse request.
struct ArtifactCurrentnessProbe {
    /// Artifact manifest and every payload that can affect the requested launch.
    files: Vec<FileCurrentnessProbe>,
}

/// Number of path identities that can affect one grammar-affined launch:
/// artifact manifest, worker, platform authority (broker or native policy),
/// accepted manifest, and selected grammar.
const MAX_CURRENTNESS_PROBE_FILES: usize = 5;

impl ArtifactCurrentnessProbe {
    /// Require every path to retain its verified constant-size identity.
    fn is_current(
        &self,
        control: Option<&ArtifactIoControl<'_>>,
    ) -> Result<bool, ParserSupervisorError> {
        for file in &self.files {
            if let Some(control) = control {
                control.poll()?;
            }
            if !file.is_current()? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl VerifiedParserPackLaunch {
    /// Validate and canonicalize one exact artifact before process creation.
    fn load(pack_root: &Path) -> Result<Self, ParserSupervisorError> {
        Self::load_inner(pack_root, None)
    }

    /// Reload a changed artifact while honoring the active parse request bounds.
    fn load_controlled(
        pack_root: &Path,
        language_id: &str,
        last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<Self, ParserSupervisorError> {
        let control = ArtifactIoControl {
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        };
        let pack_root = pack_root.to_path_buf();
        let language_id = language_id.to_owned();
        let worker_cancellation = cancellation.clone();
        run_bounded_artifact_io(
            move || {
                let worker_control = ArtifactIoControl {
                    absolute_deadline,
                    last_progress,
                    no_progress_timeout,
                    cancellation: &worker_cancellation,
                };
                let refreshed = Self::load_inner(&pack_root, Some(&worker_control))?;
                if !refreshed
                    .currentness_probe(&language_id)
                    .is_current(Some(&worker_control))?
                {
                    return Err(ParserSupervisorError::PayloadMismatch {
                        path: pack_root,
                        reason: "artifact changed during digest revalidation",
                    });
                }
                Ok(refreshed)
            },
            &control,
        )
    }

    /// Validate one artifact with optional worker-side request bounds.
    fn load_inner(
        pack_root: &Path,
        control: Option<&ArtifactIoControl<'_>>,
    ) -> Result<Self, ParserSupervisorError> {
        if let Some(control) = control {
            control.poll()?;
        }
        let platform =
            host_pack_platform().ok_or(ParserSupervisorError::UnsupportedContainment {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
            })?;
        let pack_root = canonical_directory(pack_root)?;
        let accepted_path = canonical_direct_file(&pack_root, ACCEPTED_MANIFEST_FILE_NAME)?;
        let artifact_path = canonical_direct_file(&pack_root, ARTIFACT_MANIFEST_FILE_NAME)?;
        let accepted_manifest_file = FileObservation::capture(accepted_path.clone())?;
        let artifact_manifest_file = FileObservation::capture(artifact_path.clone())?;
        let accepted_read = read_bounded_file(
            &accepted_path,
            accepted_manifest_file.epoch,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
            control,
        )?;
        let artifact_read = read_bounded_file(
            &artifact_path,
            artifact_manifest_file.epoch,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
            control,
        )?;
        let mut accepted_manifest = Some(accepted_manifest_file);
        let logical = OptionalParserPackManifest::from_json(&accepted_read.bytes)?;
        let artifact_manifest: OptionalParserPackArtifactManifest =
            serde_json::from_slice(&artifact_read.bytes)
                .map_err(|source| ParserSupervisorError::ArtifactManifestJson { source })?;
        artifact_manifest.validate(&logical)?;
        if artifact_manifest.platform != platform {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: artifact_path,
                reason: "artifact target does not match the current host",
            });
        }

        let mut worker = None;
        let mut containment_broker = None;
        let mut accepted_payload_sha256 = None;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let mut native_import_policy_bytes = None;
        let mut payloads = Vec::with_capacity(artifact_manifest.files.len());
        for payload in &artifact_manifest.files {
            let path = canonical_payload_file(&pack_root, payload.path.as_str())?;
            let file = if matches!(payload.role, ParserPackPayloadRole::AcceptedManifest) {
                if path != accepted_path {
                    return Err(ParserSupervisorError::PayloadMismatch {
                        path,
                        reason: "accepted capability manifest is not at its defined artifact path",
                    });
                }
                accepted_manifest
                    .take()
                    .ok_or_else(|| ParserSupervisorError::PayloadMismatch {
                        path: path.clone(),
                        reason: "artifact contains more than one accepted capability manifest",
                    })?
            } else {
                FileObservation::capture(path.clone())?
            };
            let payload_read = read_bounded_file(&path, file.epoch, payload.bytes, control)?;
            if u64::try_from(payload_read.bytes.len()).ok() != Some(payload.bytes) {
                return Err(ParserSupervisorError::PayloadMismatch {
                    path,
                    reason: "payload byte count differs from the artifact manifest",
                });
            }
            if payload_read.sha256 != payload.sha256.as_str() {
                return Err(ParserSupervisorError::PayloadMismatch {
                    path,
                    reason: "payload SHA-256 differs from the artifact manifest",
                });
            }
            payloads.push(PayloadObservation {
                file,
                role: payload.role.clone(),
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                bytes: payload.bytes,
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                sha256: payload.sha256.as_str().to_owned(),
            });
            match &payload.role {
                ParserPackPayloadRole::Worker => worker = Some(path),
                ParserPackPayloadRole::ContainmentBroker => containment_broker = Some(path),
                ParserPackPayloadRole::AcceptedManifest => {
                    accepted_payload_sha256 = Some(payload.sha256.as_str().to_owned());
                }
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                ParserPackPayloadRole::NativeImportPolicy => {
                    native_import_policy_bytes = Some(payload_read.bytes.clone());
                }
                ParserPackPayloadRole::FixtureCorpus
                | ParserPackPayloadRole::ProjectLicense
                | ParserPackPayloadRole::NativeAuditReport
                | ParserPackPayloadRole::GrammarLibrary { .. } => {}
                #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
                ParserPackPayloadRole::NativeImportPolicy => {}
            }
        }

        let worker = worker.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
            path: pack_root.join(platform.worker_file_name()),
            reason: "artifact does not contain its exact worker payload",
        })?;
        #[cfg(unix)]
        require_executable(&worker)?;
        let expected_worker = canonical_direct_file(&pack_root, platform.worker_file_name())?;
        if worker != expected_worker {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: worker,
                reason: "worker is not at its platform-defined artifact path",
            });
        }
        let containment_broker = match platform.containment_broker_file_name() {
            Some(file_name) => {
                let broker =
                    containment_broker.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
                        path: pack_root.join(file_name),
                        reason: "artifact does not contain its required containment broker",
                    })?;
                #[cfg(unix)]
                require_executable(&broker)?;
                let expected = canonical_direct_file(&pack_root, file_name)?;
                if broker != expected {
                    return Err(ParserSupervisorError::PayloadMismatch {
                        path: broker,
                        reason: "containment broker is not at its platform-defined artifact path",
                    });
                }
                Some(expected)
            }
            None if containment_broker.is_none() => None,
            None => {
                return Err(ParserSupervisorError::PayloadMismatch {
                    path: pack_root,
                    reason: "artifact contains an unsupported containment broker",
                });
            }
        };
        let accepted_manifest_sha256 =
            accepted_payload_sha256.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
                path: accepted_path.clone(),
                reason: "artifact does not contain its accepted capability manifest",
            })?;
        if accepted_read.sha256 != accepted_manifest_sha256 {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: accepted_path,
                reason: "accepted capability manifest does not match its artifact payload row",
            });
        }
        #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
        let _ = containment_broker;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let native_import_policy_bytes =
            native_import_policy_bytes.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
                path: pack_root.clone(),
                reason: "Linux artifact does not contain its native-import policy",
            })?;

        Ok(Self {
            pack_root,
            platform,
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            containment_broker,
            accepted_grammars: logical
                .grammars()
                .iter()
                .map(|grammar| grammar.language_id.clone())
                .collect(),
            artifact: ParserArtifactIdentity::for_bytes(&artifact_read.bytes),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            artifact_manifest_bytes: artifact_read.bytes,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            accepted_manifest_bytes: accepted_read.bytes,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            native_import_policy_bytes,
            artifact_manifest: artifact_manifest_file,
            payloads,
            #[cfg(test)]
            currentness_blocker: None,
        })
    }

    /// Copy the constant-size identities needed for one bounded currentness probe.
    fn currentness_probe(&self, language_id: &str) -> ArtifactCurrentnessProbe {
        let mut files = Vec::with_capacity(MAX_CURRENTNESS_PROBE_FILES);
        files.push(self.artifact_manifest.currentness_probe());
        files.extend(
            self.payloads
                .iter()
                .filter(|payload| payload.contributes_to_launch(language_id))
                .map(|payload| payload.file.currentness_probe()),
        );
        #[cfg(test)]
        if let (Some(file), Some(blocker)) = (files.first_mut(), &self.currentness_blocker) {
            file.blocker = Some(std::sync::Arc::clone(blocker));
        }
        ArtifactCurrentnessProbe { files }
    }

    /// Validate one requested grammar against the exact accepted manifest.
    fn require_grammar(
        &self,
        language_id: &str,
    ) -> Result<ParserLanguageIdentity, ParserSupervisorError> {
        let language = ParserLanguageIdentity::new(language_id)?;
        if self
            .accepted_grammars
            .binary_search_by(|candidate| candidate.as_str().cmp(language.as_str()))
            .is_err()
        {
            return Err(ParserSupervisorError::GrammarNotAccepted {
                language_id: language_id.to_owned(),
            });
        }
        Ok(language)
    }

    /// Build immutable authority for one Linux grammar-affined resident.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn prepare_resident_launch_controlled(
        &self,
        language_id: &str,
        last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<LinuxResidentLaunchAuthority, ParserSupervisorError> {
        let mut workers = self
            .payloads
            .iter()
            .filter(|payload| matches!(payload.role, ParserPackPayloadRole::Worker));
        let worker = workers.next().map(PayloadObservation::linux_spec);
        if worker.is_none() || workers.next().is_some() {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: self.pack_root.clone(),
                reason: "artifact must bind exactly one Linux worker payload",
            });
        }
        let mut grammars = self.payloads.iter().filter(|payload| {
            matches!(
                &payload.role,
                ParserPackPayloadRole::GrammarLibrary {
                    language_id: payload_language
                } if payload_language == language_id
            )
        });
        let grammar = grammars.next().map(PayloadObservation::linux_spec);
        if grammar.is_none() || grammars.next().is_some() {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: self.pack_root.clone(),
                reason: "artifact must bind exactly one selected grammar payload",
            });
        }

        let worker = worker.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
            path: self.pack_root.clone(),
            reason: "artifact has no Linux worker payload",
        })?;
        let grammar = grammar.ok_or_else(|| ParserSupervisorError::PayloadMismatch {
            path: self.pack_root.clone(),
            reason: "artifact has no selected grammar payload",
        })?;
        let artifact_manifest = self.artifact_manifest_bytes.clone();
        let accepted_manifest = self.accepted_manifest_bytes.clone();
        let native_import_policy = self.native_import_policy_bytes.clone();
        let worker_cancellation = cancellation.clone();
        let control = ArtifactIoControl {
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        };
        run_bounded_artifact_io(
            move || {
                let worker_control = ArtifactIoControl {
                    absolute_deadline,
                    last_progress,
                    no_progress_timeout,
                    cancellation: &worker_cancellation,
                };
                let worker_bytes = read_verified_linux_payload(&worker, &worker_control)?;
                let grammar_bytes = read_verified_linux_payload(&grammar, &worker_control)?;
                Ok(LinuxResidentLaunchAuthority {
                    worker: SealedLinuxPayload::from_verified_bytes(
                        "worker",
                        "projectatlas-parser-worker",
                        &worker_bytes,
                        true,
                        &worker_control,
                    )?,
                    artifact_manifest: SealedLinuxPayload::from_verified_bytes(
                        "artifact manifest",
                        "projectatlas-artifact-manifest",
                        &artifact_manifest,
                        false,
                        &worker_control,
                    )?,
                    accepted_manifest: SealedLinuxPayload::from_verified_bytes(
                        "accepted capability manifest",
                        "projectatlas-accepted-manifest",
                        &accepted_manifest,
                        false,
                        &worker_control,
                    )?,
                    native_import_policy: SealedLinuxPayload::from_verified_bytes(
                        "native-import policy",
                        "projectatlas-native-policy",
                        &native_import_policy,
                        false,
                        &worker_control,
                    )?,
                    grammar: SealedLinuxPayload::from_verified_bytes(
                        "selected grammar",
                        "projectatlas-selected-grammar",
                        &grammar_bytes,
                        true,
                        &worker_control,
                    )?,
                })
            },
            &control,
        )
    }
}

/// Re-read one manifest-owned Linux payload and require its exact bytes and digest.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_verified_linux_payload(
    spec: &VerifiedLinuxPayloadSpec,
    control: &ArtifactIoControl<'_>,
) -> Result<Vec<u8>, ParserSupervisorError> {
    let read = read_bounded_file(&spec.path, spec.epoch, spec.bytes, Some(control))?;
    if u64::try_from(read.bytes.len()).ok() != Some(spec.bytes) {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: spec.path.clone(),
            reason: "launch payload byte count differs from the artifact manifest",
        });
    }
    if read.sha256 != spec.sha256 {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: spec.path.clone(),
            reason: "launch payload SHA-256 differs from the artifact manifest",
        });
    }
    Ok(read.bytes)
}

/// Return the accepted target for the current host or refuse before reading source.
fn host_pack_platform() -> Option<PackPlatform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(PackPlatform::LinuxX86_64),
        ("windows", "x86_64") => Some(PackPlatform::WindowsX86_64),
        _ => None,
    }
}

/// Canonicalize one required artifact directory.
fn canonical_directory(path: &Path) -> Result<PathBuf, ParserSupervisorError> {
    let canonical = fs::canonicalize(path).map_err(|source| ParserSupervisorError::PackPath {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file_metadata(&canonical)?;
    if !canonical.is_absolute() || !metadata.is_dir() {
        return Err(ParserSupervisorError::InvalidPackPath {
            path: canonical,
            reason: "expected an absolute regular directory",
        });
    }
    Ok(canonical)
}

/// Canonicalize one exact file at the artifact root.
fn canonical_direct_file(
    pack_root: &Path,
    file_name: &str,
) -> Result<PathBuf, ParserSupervisorError> {
    let path = canonical_payload_file(pack_root, file_name)?;
    if path.parent() != Some(pack_root) {
        return Err(ParserSupervisorError::InvalidPackPath {
            path,
            reason: "expected a direct artifact-root file",
        });
    }
    Ok(path)
}

/// Canonicalize one manifest-approved payload without following mutable indirection.
fn canonical_payload_file(
    pack_root: &Path,
    relative: &str,
) -> Result<PathBuf, ParserSupervisorError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ParserSupervisorError::InvalidPackPath {
            path: relative_path.to_path_buf(),
            reason: "expected a normalized artifact-relative path",
        });
    }
    let requested = pack_root.join(relative_path);
    let mut component_path = pack_root.to_path_buf();
    for component in relative_path.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(ParserSupervisorError::InvalidPackPath {
                path: requested,
                reason: "expected only normal relative components",
            });
        };
        component_path.push(component);
        let metadata = fs::symlink_metadata(&component_path).map_err(|source| {
            ParserSupervisorError::PackPath {
                path: component_path.clone(),
                source,
            }
        })?;
        if is_link_or_reparse_point(&metadata) {
            return Err(ParserSupervisorError::InvalidPackPath {
                path: component_path,
                reason: "symbolic links and reparse points are not accepted in immutable packs",
            });
        }
    }
    let canonical =
        fs::canonicalize(&requested).map_err(|source| ParserSupervisorError::PackPath {
            path: requested.clone(),
            source,
        })?;
    let metadata = file_metadata(&canonical)?;
    if !canonical.starts_with(pack_root) || !metadata.is_file() {
        return Err(ParserSupervisorError::InvalidPackPath {
            path: canonical,
            reason: "payload must be a regular file inside the canonical pack root",
        });
    }
    Ok(canonical)
}

/// Return whether metadata represents mutable path indirection.
#[cfg(windows)]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Return whether metadata represents a symbolic link.
#[cfg(not(windows))]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Read regular-file metadata with a typed path error.
fn file_metadata(path: &Path) -> Result<Metadata, ParserSupervisorError> {
    fs::metadata(path).map_err(|source| ParserSupervisorError::PackPath {
        path: path.to_path_buf(),
        source,
    })
}

/// Read and hash one exact regular file without permitting growth beyond its bound.
fn read_bounded_file(
    path: &Path,
    expected_epoch: FileChangeEpoch,
    maximum: u64,
    control: Option<&ArtifactIoControl<'_>>,
) -> Result<BoundedArtifactRead, ParserSupervisorError> {
    let mut file = File::open(path).map_err(|source| ParserSupervisorError::ArtifactRead {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file
        .metadata()
        .map_err(|source| ParserSupervisorError::ArtifactRead {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(ParserSupervisorError::InvalidPackPath {
            path: path.to_path_buf(),
            reason: "expected a regular artifact file",
        });
    }
    if FileChangeEpoch::from_metadata(&metadata) != expected_epoch {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: path.to_path_buf(),
            reason: "artifact read handle does not match the captured file identity",
        });
    }
    if metadata.len() > maximum {
        return Err(ParserSupervisorError::ArtifactFileTooLarge {
            path: path.to_path_buf(),
            actual: metadata.len(),
            maximum,
        });
    }
    let capacity = usize::try_from(metadata.len()).unwrap_or(ARTIFACT_READ_CHUNK_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    let mut sha256 = Sha256::new();
    read_bounded_chunks(&mut file, path, maximum, &mut bytes, &mut sha256, control)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(ParserSupervisorError::ArtifactFileTooLarge {
            path: path.to_path_buf(),
            actual: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(BoundedArtifactRead {
        bytes,
        sha256: encode_sha256(sha256.finalize()),
    })
}

/// Read bounded chunks while polling active request stop conditions.
fn read_bounded_chunks(
    reader: &mut impl Read,
    path: &Path,
    maximum: u64,
    bytes: &mut Vec<u8>,
    sha256: &mut Sha256,
    control: Option<&ArtifactIoControl<'_>>,
) -> Result<(), ParserSupervisorError> {
    let mut chunk = vec![0_u8; ARTIFACT_READ_CHUNK_BYTES].into_boxed_slice();
    loop {
        if let Some(control) = control {
            control.poll()?;
        }
        let remaining = maximum
            .saturating_add(1)
            .saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if remaining == 0 {
            break;
        }
        let limit = usize::try_from(remaining)
            .unwrap_or(ARTIFACT_READ_CHUNK_BYTES)
            .min(ARTIFACT_READ_CHUNK_BYTES);
        let read = match reader.read(&mut chunk[..limit]) {
            Ok(read) => read,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(ParserSupervisorError::ArtifactRead {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        if read == 0 {
            break;
        }
        sha256.update(&chunk[..read]);
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

/// Encode one SHA-256 digest as lowercase hexadecimal.
fn encode_sha256(digest: impl AsRef<[u8]>) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = digest.as_ref();
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        encoded.push(char::from(LOWER_HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(LOWER_HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

/// Require an executable payload on hosts that expose Unix mode bits.
#[cfg(unix)]
fn require_executable(path: &Path) -> Result<(), ParserSupervisorError> {
    use std::os::unix::fs::PermissionsExt;

    if file_metadata(path)?.permissions().mode() & 0o111 == 0 {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: path.to_path_buf(),
            reason: "executable payload has no execute permission",
        });
    }
    Ok(())
}

/// Failure produced inside one owned standard-stream thread.
#[derive(Debug, Error)]
enum ParserIoThreadError {
    /// A stream read or write failed.
    #[error("{operation}: {source}")]
    Stream {
        /// Stable stream operation.
        operation: &'static str,
        /// Standard I/O failure.
        #[source]
        source: io::Error,
    },
    /// A fixed frame header violated the closed protocol.
    #[error("frame header: {source}")]
    FrameHeader {
        /// Typed header failure.
        #[source]
        source: ParserProtocolError,
    },
    /// The Windows broker admission record differed from the fixed contract.
    #[error("Windows admission record mismatch")]
    AdmissionMismatch,
    /// A worker or broker wrote bytes outside the framed protocol.
    #[error("unexpected diagnostic bytes: {diagnostic}")]
    UnexpectedDiagnostic {
        /// Bounded lossy rendering of the first observed bytes.
        diagnostic: String,
    },
}

/// One bounded stdout-reader event.
#[derive(Debug)]
enum FrameReaderEvent {
    /// One complete frame whose header was validated before allocation.
    Frame(Vec<u8>),
    /// Clean end of stream between frames.
    EndOfStream,
    /// Terminal bounded reader failure.
    Failure(ParserIoThreadError),
}

/// One bounded stderr/admission-reader event.
#[derive(Debug)]
enum DiagnosticReaderEvent {
    /// Platform admission completed and protocol input may begin.
    AdmissionAccepted,
    /// The parent-authored fence after one complete stdout frame was observed.
    FenceObserved,
    /// Terminal bounded reader failure.
    Failure(ParserIoThreadError),
}

/// One random parent-only record used to order independent standard pipes.
#[derive(Clone, Copy)]
struct DiagnosticFence([u8; PARSER_DIAGNOSTIC_FENCE_BYTES]);

/// One exact write owned by the fixed worker-input thread.
struct WriterCommand {
    /// Complete bytes for one indivisible protocol send.
    bytes: Vec<u8>,
    /// One-shot write and flush result.
    acknowledgement: SyncSender<Result<(), ParserIoThreadError>>,
}

/// Read one frame with fixed-header validation before payload allocation.
fn read_one_frame(input: &mut impl Read) -> Result<Option<Vec<u8>>, ParserIoThreadError> {
    let mut header_bytes = [0_u8; PARSER_FRAME_HEADER_BYTES];
    let mut header_read = 0_usize;
    while header_read < header_bytes.len() {
        match input.read(&mut header_bytes[header_read..]) {
            Ok(0) if header_read == 0 => return Ok(None),
            Ok(0) => {
                return Err(ParserIoThreadError::Stream {
                    operation: "read partial frame header",
                    source: io::Error::new(io::ErrorKind::UnexpectedEof, "partial frame header"),
                });
            }
            Ok(count) => header_read = header_read.saturating_add(count),
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => {
                return Err(ParserIoThreadError::Stream {
                    operation: "read frame header",
                    source,
                });
            }
        }
    }
    let header = ParserFrameHeader::decode(&header_bytes)
        .map_err(|source| ParserIoThreadError::FrameHeader { source })?;
    let payload_len = header.payload_len() as usize;
    let frame_len = PARSER_FRAME_HEADER_BYTES.saturating_add(payload_len);
    let mut frame = Vec::with_capacity(frame_len);
    frame.extend_from_slice(&header_bytes);
    frame.resize(frame_len, 0);
    input
        .read_exact(&mut frame[PARSER_FRAME_HEADER_BYTES..])
        .map_err(|source| ParserIoThreadError::Stream {
            operation: "read frame payload",
            source,
        })?;
    Ok(Some(frame))
}

/// Own worker stdout and fence every complete or failed frame through the diagnostic pipe.
fn frame_reader_loop(
    mut stdout: ChildStdout,
    mut diagnostic_fence_writer: impl Write,
    diagnostic_fence: DiagnosticFence,
    events: &SyncSender<FrameReaderEvent>,
) {
    loop {
        let event = match read_one_frame(&mut stdout) {
            Ok(Some(frame)) => FrameReaderEvent::Frame(frame),
            Ok(None) => FrameReaderEvent::EndOfStream,
            Err(error) => FrameReaderEvent::Failure(error),
        };
        let event = if matches!(event, FrameReaderEvent::EndOfStream) {
            event
        } else {
            match diagnostic_fence_writer
                .write_all(&diagnostic_fence.0)
                .and_then(|()| diagnostic_fence_writer.flush())
            {
                Ok(()) => event,
                Err(source) => FrameReaderEvent::Failure(ParserIoThreadError::Stream {
                    operation: "write diagnostic fence",
                    source,
                }),
            }
        };
        let terminal = !matches!(event, FrameReaderEvent::Frame(_));
        if events.send(event).is_err() || terminal {
            return;
        }
    }
}

/// Own worker or broker stderr, validating Windows admission before diagnostics.
fn diagnostic_reader_loop(
    mut stderr: impl Read,
    expect_windows_admission: bool,
    diagnostic_fence: DiagnosticFence,
    events: &SyncSender<DiagnosticReaderEvent>,
) -> Result<Vec<u8>, ParserIoThreadError> {
    if expect_windows_admission {
        let mut observed = [0_u8; PARSER_WINDOWS_BROKER_ADMISSION_RECORD.len()];
        if let Err(source) = stderr.read_exact(&mut observed) {
            let message = source.to_string();
            return if events
                .send(DiagnosticReaderEvent::Failure(
                    ParserIoThreadError::Stream {
                        operation: "read Windows admission record",
                        source,
                    },
                ))
                .is_ok()
            {
                Ok(Vec::new())
            } else {
                Err(ParserIoThreadError::Stream {
                    operation: "read Windows admission record",
                    source: io::Error::other(message),
                })
            };
        }
        if observed != PARSER_WINDOWS_BROKER_ADMISSION_RECORD {
            let error = ParserIoThreadError::AdmissionMismatch;
            return if events.send(DiagnosticReaderEvent::Failure(error)).is_ok() {
                Ok(Vec::new())
            } else {
                Err(ParserIoThreadError::AdmissionMismatch)
            };
        }
    }
    if events
        .send(DiagnosticReaderEvent::AdmissionAccepted)
        .is_err()
    {
        return Ok(Vec::new());
    }

    loop {
        let mut observed = [0_u8; PARSER_DIAGNOSTIC_FENCE_BYTES];
        let mut observed_len = 0_usize;
        while observed_len < observed.len() {
            match stderr.read(&mut observed[observed_len..]) {
                Ok(0) if observed_len == 0 => return Ok(Vec::new()),
                Ok(0) => break,
                Ok(count) => observed_len = observed_len.saturating_add(count),
                Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
                Err(source) => {
                    let message = source.to_string();
                    return if events
                        .send(DiagnosticReaderEvent::Failure(
                            ParserIoThreadError::Stream {
                                operation: "read diagnostic stream",
                                source,
                            },
                        ))
                        .is_ok()
                    {
                        Ok(observed[..observed_len].to_vec())
                    } else {
                        Err(ParserIoThreadError::Stream {
                            operation: "read diagnostic stream",
                            source: io::Error::other(message),
                        })
                    };
                }
            }
        }
        if observed_len == observed.len() && observed == diagnostic_fence.0 {
            if events.send(DiagnosticReaderEvent::FenceObserved).is_err() {
                return Ok(Vec::new());
            }
            continue;
        }
        let diagnostics = observed[..observed_len].to_vec();
        let diagnostic = bounded_diagnostic(&diagnostics);
        return if events
            .send(DiagnosticReaderEvent::Failure(
                ParserIoThreadError::UnexpectedDiagnostic {
                    diagnostic: diagnostic.clone(),
                },
            ))
            .is_ok()
        {
            Ok(diagnostics)
        } else {
            Err(ParserIoThreadError::UnexpectedDiagnostic { diagnostic })
        };
    }
}

/// Own worker stdin and acknowledge each bounded write after flushing.
fn writer_loop(mut stdin: impl Write, commands: &Receiver<WriterCommand>) {
    while let Ok(command) = commands.recv() {
        let result = stdin
            .write_all(&command.bytes)
            .and_then(|()| stdin.flush())
            .map_err(|source| ParserIoThreadError::Stream {
                operation: "write protocol frame",
                source,
            });
        let failed = result.is_err();
        if command.acknowledgement.send(result).is_err() || failed {
            return;
        }
    }
}

/// Owned stdout-reader thread and its capacity-one event channel.
struct FrameReader {
    /// Capacity-one framed output channel.
    events: Receiver<FrameReaderEvent>,
    /// Owned reader thread.
    handle: Option<JoinHandle<()>>,
}

/// Owned stderr/admission-reader thread and its capacity-one event channel.
struct DiagnosticReader {
    /// Capacity-one admission and failure channel.
    events: Receiver<DiagnosticReaderEvent>,
    /// Owned diagnostic reader thread.
    handle: Option<JoinHandle<Result<Vec<u8>, ParserIoThreadError>>>,
}

/// One observed Linux resident-memory breach.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
struct LinuxMemoryBreach {
    /// Active accounting path.
    accounting: ParserMemoryAccountingKind,
    /// Last observed resident or cgroup memory bytes.
    observed_bytes: u64,
}

/// Bounded resolution of a transient Linux process-exit/accounting transition.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
enum LinuxMemoryObservation {
    /// Resident memory became readable again while the worker remained live.
    Memory(Option<LinuxMemoryBreach>),
    /// The direct child became waitable before memory accounting recovered.
    ChildExited {
        /// Platform exit code, when the process reported one.
        code: Option<i32>,
    },
}

/// One observed direct-child exit stripped to the public diagnostic contract.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
struct LinuxChildExit {
    /// Platform exit code, when the process reported one.
    code: Option<i32>,
}

/// Result of one Linux process-group signal attempt.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
enum LinuxProcessGroupTermination {
    /// `SIGKILL` was delivered to the process group.
    Signalled,
    /// The kernel reported that the process group was absent.
    Absent,
}

/// Optional delegated cgroup-v2 state retained until the worker has been reaped.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxCgroupMemory {
    /// Product-owned child cgroup inside an already delegated parent.
    directory: PathBuf,
    /// `memory.events:max` counter before the worker was attached.
    initial_max_events: u64,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl LinuxCgroupMemory {
    /// Create, configure, and attach only inside an existing writable delegation.
    fn try_attach(process_id: u32, maximum_bytes: u64) -> io::Result<Option<Self>> {
        for parent in delegated_cgroup_parents()? {
            let sequence = CGROUP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(
                "projectatlas-parser-{}-{sequence}",
                std::process::id()
            ));
            if fs::create_dir(&directory).is_err() {
                continue;
            }
            let mut candidate = Self {
                directory,
                initial_max_events: 0,
            };
            if !prepare_delegated_memory_parent(&parent) {
                candidate.cleanup()?;
                continue;
            }
            if candidate.configure(maximum_bytes).is_err() {
                candidate.cleanup()?;
                continue;
            }
            let Ok(initial_max_events) = read_cgroup_max_events(&candidate.directory) else {
                candidate.cleanup()?;
                continue;
            };
            candidate.initial_max_events = initial_max_events;
            if fs::write(
                candidate.directory.join("cgroup.procs"),
                process_id.to_string(),
            )
            .is_err()
            {
                candidate.cleanup()?;
                continue;
            }
            return Ok(Some(candidate));
        }
        Ok(None)
    }

    /// Install and read back the hard kernel memory ceiling.
    fn configure(&self, maximum_bytes: u64) -> io::Result<()> {
        let maximum = maximum_bytes.to_string();
        fs::write(self.directory.join("memory.max"), &maximum)?;
        if read_bounded_linux_text(
            &self.directory.join("memory.max"),
            LINUX_MEMORY_RECORD_MAX_BYTES,
        )?
        .trim()
            != maximum
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delegated cgroup memory.max did not retain the configured ceiling",
            ));
        }
        let oom_group = self.directory.join("memory.oom.group");
        if oom_group.is_file() {
            fs::write(oom_group, "1")?;
        }
        Ok(())
    }

    /// Observe current kernel-accounted memory and allocation-limit events.
    fn observe(&self, maximum_bytes: u64) -> io::Result<Option<LinuxMemoryBreach>> {
        let observed_bytes = read_cgroup_current(&self.directory)?;
        let maximum_events = read_cgroup_max_events(&self.directory)?;
        if observed_bytes >= maximum_bytes || maximum_events > self.initial_max_events {
            Ok(Some(LinuxMemoryBreach {
                accounting: ParserMemoryAccountingKind::LinuxCgroupV2,
                observed_bytes,
            }))
        } else {
            Ok(None)
        }
    }

    /// Remove the now-empty delegated child cgroup after the worker was reaped.
    fn cleanup(&mut self) -> io::Result<()> {
        if !self.directory.exists() {
            return Ok(());
        }
        fs::remove_dir(&self.directory)
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Drop for LinuxCgroupMemory {
    fn drop(&mut self) {
        drop(self.cleanup());
    }
}

/// Supervisor-owned Linux memory observer with a sampled-RSS fallback.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxMemoryObserver {
    /// Optional kernel-hard cgroup accounting retained for cleanup.
    cgroup: Option<LinuxCgroupMemory>,
    /// Whether cgroup observation remains readable for this session.
    observe_cgroup: bool,
    /// Inclusive resident-memory ceiling.
    maximum_bytes: u64,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl LinuxMemoryObserver {
    /// Attach opportunistic cgroup accounting and always retain sampled-RSS fallback.
    fn attach(process_id: u32, maximum_bytes: u64) -> io::Result<Self> {
        let cgroup = LinuxCgroupMemory::try_attach(process_id, maximum_bytes)?;
        let observe_cgroup = cgroup.is_some();
        Ok(Self {
            cgroup,
            observe_cgroup,
            maximum_bytes,
        })
    }

    /// Construct the ordinary sampled-RSS path after a failed optional-cgroup cleanup.
    fn sampled_rss(maximum_bytes: u64) -> Self {
        Self {
            cgroup: None,
            observe_cgroup: false,
            maximum_bytes,
        }
    }

    /// Observe one current cgroup or sampled-RSS value.
    fn observe(&mut self, process_id: u32) -> io::Result<Option<LinuxMemoryBreach>> {
        if self.observe_cgroup
            && let Some(cgroup) = self.cgroup.as_ref()
        {
            match cgroup.observe(self.maximum_bytes) {
                Ok(observation) => return Ok(observation),
                Err(_source) => self.observe_cgroup = false,
            }
        }
        let observed_bytes = read_process_rss(process_id)?;
        if observed_bytes >= self.maximum_bytes {
            Ok(Some(LinuxMemoryBreach {
                accounting: ParserMemoryAccountingKind::LinuxProcStatus,
                observed_bytes,
            }))
        } else {
            Ok(None)
        }
    }

    /// Remove delegated cgroup state after the direct child has been reaped.
    fn cleanup(&mut self) -> io::Result<()> {
        match self.cgroup.as_mut() {
            Some(cgroup) => cgroup.cleanup(),
            None => Ok(()),
        }
    }

    /// Return the accounting path used by the next observation.
    fn accounting_kind(&self) -> ParserMemoryAccountingKind {
        if self.observe_cgroup {
            ParserMemoryAccountingKind::LinuxCgroupV2
        } else {
            ParserMemoryAccountingKind::LinuxProcStatus
        }
    }
}

/// One terminal event emitted by the continuous Linux memory monitor.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
enum LinuxMemoryMonitorEvent {
    /// The configured resident-memory ceiling was reached.
    Limit {
        /// Accounting mode that observed the breach.
        breach: LinuxMemoryBreach,
        /// Process-group termination failure, when the first kill attempt failed.
        termination_error: Option<String>,
    },
    /// Both cgroup accounting and its sampled-RSS fallback became unreadable.
    ObservationFailed {
        /// Accounting mode that became unreadable.
        accounting: ParserMemoryAccountingKind,
        /// Bounded observation failure.
        message: String,
    },
}

/// Exactly one owned continuous Linux resident-memory monitor.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxMemoryMonitor {
    /// Capacity-one stop signal.
    stop: SyncSender<()>,
    /// Capacity-one terminal event channel.
    events: Receiver<LinuxMemoryMonitorEvent>,
    /// Owned monitor thread joined before the child can be reaped.
    handle: Option<JoinHandle<()>>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl LinuxMemoryMonitor {
    /// Start continuous sampling for one process group.
    fn start(process_id: u32, observer: Arc<Mutex<LinuxMemoryObserver>>) -> io::Result<Self> {
        let (stop, stop_receiver) = mpsc::sync_channel(1);
        let (event_sender, events) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("parser-supervisor-memory".to_owned())
            .spawn(move || {
                linux_memory_monitor_loop(process_id, &observer, &stop_receiver, &event_sender);
            })?;
        Ok(Self {
            stop,
            events,
            handle: Some(handle),
        })
    }

    /// Stop and join exactly once, returning any terminal observation.
    fn stop(&mut self) -> Result<Option<LinuxMemoryMonitorEvent>, ParserSupervisorError> {
        let _stop_signal_result = self.stop.try_send(());
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_panic| ParserSupervisorError::Cleanup {
                    message: "Linux resident-memory monitor panicked".to_owned(),
                })?;
        }
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }
}

/// Sample continuously while the resident worker is alive, including idle periods.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn linux_memory_monitor_loop(
    process_id: u32,
    observer: &Arc<Mutex<LinuxMemoryObserver>>,
    stop: &Receiver<()>,
    events: &SyncSender<LinuxMemoryMonitorEvent>,
) {
    let mut next_observation = Instant::now();
    loop {
        let observation = match observer.lock() {
            Ok(mut observer) => {
                let observation = observer.observe(process_id);
                let accounting = observer.accounting_kind();
                observation.map_err(|source| (accounting, source))
            }
            Err(_poisoned) => Err((
                ParserMemoryAccountingKind::LinuxProcStatus,
                io::Error::other("Linux memory observer lock was poisoned"),
            )),
        };
        let event = match observation {
            Ok(None) => None,
            Ok(Some(breach)) => Some(LinuxMemoryMonitorEvent::Limit {
                breach,
                termination_error: linux_monitor_termination_error(process_id),
            }),
            Err((accounting, source)) => Some(LinuxMemoryMonitorEvent::ObservationFailed {
                accounting,
                message: bounded_message(source.to_string()),
            }),
        };
        if let Some(event) = event {
            drop(events.try_send(event));
            return;
        }
        next_observation = next_observation
            .checked_add(PARSER_LINUX_RSS_OBSERVATION_INTERVAL)
            .unwrap_or_else(Instant::now);
        let wait = next_observation.saturating_duration_since(Instant::now());
        match stop.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Convert a group-signal miss into an event so the child owner performs direct fallback.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn linux_monitor_termination_error(process_id: u32) -> Option<String> {
    match terminate_linux_process_group(process_id) {
        Ok(LinuxProcessGroupTermination::Signalled) => None,
        Ok(LinuxProcessGroupTermination::Absent) => {
            Some("worker process group was absent".to_owned())
        }
        Err(source) => Some(source),
    }
}

/// Return bounded current-to-root cgroup-v2 candidates for delegation probing.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn delegated_cgroup_parents() -> io::Result<Vec<PathBuf>> {
    let membership = read_bounded_linux_text(
        Path::new("/proc/self/cgroup"),
        LINUX_MEMORY_RECORD_MAX_BYTES,
    )?;
    let Some(relative_path) = parse_unified_cgroup_path(&membership)? else {
        return Ok(Vec::new());
    };
    let root = Path::new(CGROUP_V2_ROOT);
    let mut candidate = root.join(relative_path);
    let mut candidates = Vec::new();
    loop {
        if !candidate.starts_with(root) || candidates.len() >= MAX_CGROUP_ANCESTORS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unified cgroup membership exceeds its ancestor bound",
            ));
        }
        candidates.push(candidate.clone());
        if candidate == root {
            break;
        }
        if !candidate.pop() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unified cgroup membership escaped its mount",
            ));
        }
    }
    Ok(candidates)
}

/// Parse exactly one safe unified-cgroup membership path.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
fn parse_unified_cgroup_path(membership: &str) -> io::Result<Option<PathBuf>> {
    let mut unified = membership
        .lines()
        .filter_map(|line| line.strip_prefix("0::"));
    let Some(relative) = unified.next() else {
        return Ok(None);
    };
    if unified.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "multiple unified cgroup memberships were reported",
        ));
    }
    let relative = relative.trim_start_matches('/');
    let relative_path = Path::new(relative);
    let component_count = relative_path.components().count();
    if component_count > MAX_CGROUP_ANCESTORS.saturating_sub(1)
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unified cgroup membership path is unsafe or too deep",
        ));
    }
    Ok(Some(relative_path.to_path_buf()))
}

/// Require one candidate parent to expose a usable delegated memory controller.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn prepare_delegated_memory_parent(parent: &Path) -> bool {
    let controllers = read_bounded_linux_text(
        &parent.join("cgroup.controllers"),
        LINUX_MEMORY_RECORD_MAX_BYTES,
    );
    let Ok(controllers) = controllers else {
        return false;
    };
    if !has_cgroup_token(&controllers, "memory") {
        return false;
    }
    let subtree_path = parent.join("cgroup.subtree_control");
    let Ok(mut subtree_control) =
        read_bounded_linux_text(&subtree_path, LINUX_MEMORY_RECORD_MAX_BYTES)
    else {
        return false;
    };
    if !has_cgroup_token(&subtree_control, "memory") {
        if fs::write(&subtree_path, "+memory").is_err() {
            return false;
        }
        let Ok(observed) = read_bounded_linux_text(&subtree_path, LINUX_MEMORY_RECORD_MAX_BYTES)
        else {
            return false;
        };
        subtree_control = observed;
        if !has_cgroup_token(&subtree_control, "memory") {
            return false;
        }
    }
    true
}

/// Return whether one whitespace-delimited cgroup controller set contains an exact token.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
fn has_cgroup_token(values: &str, expected: &str) -> bool {
    values.split_whitespace().any(|value| {
        value == expected
            || value
                .strip_prefix('+')
                .is_some_and(|value| value == expected)
    })
}

/// Read one kernel-generated Linux accounting record within a fixed byte ceiling.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_bounded_linux_text(path: &Path, maximum: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Linux memory accounting record exceeded its byte ceiling",
        ));
    }
    String::from_utf8(bytes).map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))
}

/// Read the worker's resident memory from one bounded procfs status record.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_process_rss(process_id: u32) -> io::Result<u64> {
    let status = read_bounded_linux_text(
        &PathBuf::from(format!("/proc/{process_id}/status")),
        LINUX_MEMORY_RECORD_MAX_BYTES,
    )?;
    parse_process_rss(&status)
}

/// Resolve the short Linux interval between releasing a process address space and becoming
/// waitable without treating unreadable accounting as successful containment.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
fn resolve_linux_memory_exit_transition(
    initial_error: io::Error,
    timeout: Duration,
    mut observe_memory: impl FnMut() -> io::Result<Option<LinuxMemoryBreach>>,
    mut observe_exit: impl FnMut() -> io::Result<Option<LinuxChildExit>>,
) -> io::Result<LinuxMemoryObservation> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    let mut memory_error = initial_error;
    loop {
        match observe_exit() {
            Ok(Some(exit)) => {
                return Ok(LinuxMemoryObservation::ChildExited { code: exit.code });
            }
            Ok(None) => {}
            Err(source) => {
                return Err(io::Error::other(format!(
                    "memory observation failed: {memory_error}; child-state observation also failed: {source}"
                )));
            }
        }
        match observe_memory() {
            Ok(observation) => return Ok(LinuxMemoryObservation::Memory(observation)),
            Err(source) => memory_error = source,
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(memory_error);
        }
        thread::sleep(Duration::from_millis(1).min(deadline.saturating_duration_since(now)));
    }
}

/// Parse one exact `VmRSS` value expressed by Linux in kibibytes.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
fn parse_process_rss(status: &str) -> io::Result<u64> {
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VmRSS is absent"))?;
    let mut fields = value.split_whitespace();
    let kibibytes = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VmRSS value is absent"))?
        .parse::<u64>()
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
    if fields.next() != Some("kB") || fields.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "VmRSS does not use the exact Linux kB unit",
        ));
    }
    kibibytes.checked_mul(1024).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "VmRSS byte conversion overflowed",
        )
    })
}

/// Read one cgroup-v2 `memory.current` byte count.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_cgroup_current(directory: &Path) -> io::Result<u64> {
    let current = read_bounded_linux_text(
        &directory.join("memory.current"),
        LINUX_MEMORY_RECORD_MAX_BYTES,
    )?;
    current
        .trim()
        .parse::<u64>()
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))
}

/// Read the cgroup-v2 count of allocations rejected by `memory.max`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_cgroup_max_events(directory: &Path) -> io::Result<u64> {
    let events = read_bounded_linux_text(
        &directory.join("memory.events"),
        LINUX_MEMORY_RECORD_MAX_BYTES,
    )?;
    parse_cgroup_event(&events, "max")
}

/// Parse one exact cgroup-v2 event counter.
#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), test))]
fn parse_cgroup_event(events: &str, name: &str) -> io::Result<u64> {
    let value = events
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some(name)).then(|| (fields.next(), fields.next()))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cgroup event is absent"))?;
    if value.1.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cgroup event row has extra fields",
        ));
    }
    value
        .0
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cgroup event value is absent"))?
        .parse::<u64>()
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))
}

/// One admitted grammar-affined child session.
struct ResidentParserSession {
    /// Direct worker on Linux or direct containment broker on Windows.
    child: Child,
    /// Grammar identity accepted for this process lifetime.
    grammar: ParserLanguageIdentity,
    /// Fresh process-session identity echoed by every response.
    session: ParserSessionIdentity,
    /// Exact independently observed artifact identity.
    artifact: ParserArtifactIdentity,
    /// Next non-zero request identity.
    next_request_id: u64,
    /// Whether a resource observer already requested terminal process-tree cleanup.
    termination_requested: bool,
    /// Exact worker and process-tree ceilings used by this session.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    memory_limits: ParserMemoryLimits,
    /// Capacity-one input queue.
    writer: Option<SyncSender<WriterCommand>>,
    /// Owned fixed writer thread.
    writer_handle: Option<JoinHandle<()>>,
    /// Owned fixed-header stdout reader.
    frame_reader: FrameReader,
    /// Owned bounded diagnostic/admission reader.
    diagnostic_reader: DiagnosticReader,
    /// Parent-authored diagnostic fences already observed ahead of their frame event.
    pending_diagnostic_fences: usize,
    /// Bounded Linux resident-memory accounting retained through cleanup.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    memory_observer: Arc<Mutex<LinuxMemoryObserver>>,
    /// Continuous Linux resident-memory monitor retained through cleanup.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    memory_monitor: Option<LinuxMemoryMonitor>,
}

impl ResidentParserSession {
    /// Launch, admit, open, and validate one exact worker session.
    fn launch(
        launch: &VerifiedParserPackLaunch,
        grammar: ParserLanguageIdentity,
        memory_limits: ParserMemoryLimits,
        last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<Self, ParserSupervisorError> {
        let memory_limits = memory_limits.checked()?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let command = {
            let authority = launch.prepare_resident_launch_controlled(
                grammar.as_str(),
                last_progress,
                absolute_deadline,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(debug_assertions)]
            invoke_linux_launch_test_hook()?;
            platform_command(launch, authority, memory_limits)?
        };
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let command = platform_command(launch, memory_limits)?;
        Self::launch_command(
            launch,
            grammar,
            memory_limits,
            last_progress,
            absolute_deadline,
            no_progress_timeout,
            cancellation,
            command,
        )
    }

    /// Launch one already closed command through the production process owner.
    fn launch_command(
        launch: &VerifiedParserPackLaunch,
        grammar: ParserLanguageIdentity,
        memory_limits: ParserMemoryLimits,
        last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
        mut command: Command,
    ) -> Result<Self, ParserSupervisorError> {
        let _ = memory_limits;
        poll_stop(
            PROCESS_LAUNCH_PHASE,
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        )?;
        let session = fresh_session_identity()?;
        let containment = containment_for_platform(launch.platform);
        let diagnostic_fence = fresh_diagnostic_fence()?;
        let (diagnostic_pipe, child_diagnostic_writer) =
            io::pipe().map_err(|source| ParserSupervisorError::IoThread {
                phase: "diagnostic pipe startup",
                message: source.to_string(),
            })?;
        let diagnostic_fence_writer = child_diagnostic_writer.try_clone().map_err(|source| {
            ParserSupervisorError::IoThread {
                phase: "diagnostic pipe startup",
                message: source.to_string(),
            }
        })?;
        command.stderr(Stdio::from(child_diagnostic_writer));
        #[cfg(debug_assertions)]
        invoke_pre_spawn_test_hook()?;
        let mut child = run_bounded_process_spawn(
            command,
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        )?;
        let stdin = child
            .stdin
            .take()
            .ok_or(ParserSupervisorError::MissingPipe { stream: "stdin" });
        let stdout = child
            .stdout
            .take()
            .ok_or(ParserSupervisorError::MissingPipe { stream: "stdout" });
        let (stdin, stdout) = match (stdin, stdout) {
            (Ok(stdin), Ok(stdout)) => (stdin, stdout),
            (stdin, stdout) => {
                let operation = stdin
                    .err()
                    .or_else(|| stdout.err())
                    .unwrap_or(ParserSupervisorError::MissingPipe { stream: "unknown" });
                return Err(attach_cleanup(
                    operation,
                    cleanup_partial_launch(&mut child, Vec::new(), None, None, None),
                ));
            }
        };

        let (writer_sender, writer_receiver) = mpsc::sync_channel(1);
        let writer_handle = thread::Builder::new()
            .name("parser-supervisor-writer".to_owned())
            .spawn(move || writer_loop(stdin, &writer_receiver))
            .map_err(|source| ParserSupervisorError::IoThread {
                phase: "writer startup",
                message: source.to_string(),
            });
        let writer_handle = match writer_handle {
            Ok(handle) => handle,
            Err(error) => {
                return Err(attach_cleanup(
                    error,
                    cleanup_partial_launch(&mut child, Vec::new(), None, None, None),
                ));
            }
        };
        let (frame_sender, frame_events) = mpsc::sync_channel(1);
        let frame_handle = thread::Builder::new()
            .name("parser-supervisor-stdout".to_owned())
            .spawn(move || {
                frame_reader_loop(
                    stdout,
                    diagnostic_fence_writer,
                    diagnostic_fence,
                    &frame_sender,
                );
            })
            .map_err(|source| ParserSupervisorError::IoThread {
                phase: "stdout reader startup",
                message: source.to_string(),
            });
        let frame_handle = match frame_handle {
            Ok(handle) => handle,
            Err(error) => {
                drop(writer_sender);
                return Err(attach_cleanup(
                    error,
                    cleanup_partial_launch(&mut child, vec![writer_handle], None, None, None),
                ));
            }
        };
        let (diagnostic_sender, diagnostic_events) = mpsc::sync_channel(1);
        let expect_windows_admission = launch.platform == PackPlatform::WindowsX86_64;
        let diagnostic_handle = thread::Builder::new()
            .name("parser-supervisor-stderr".to_owned())
            .spawn(move || {
                diagnostic_reader_loop(
                    diagnostic_pipe,
                    expect_windows_admission,
                    diagnostic_fence,
                    &diagnostic_sender,
                )
            })
            .map_err(|source| ParserSupervisorError::IoThread {
                phase: "diagnostic reader startup",
                message: source.to_string(),
            });
        let diagnostic_handle = match diagnostic_handle {
            Ok(handle) => handle,
            Err(error) => {
                drop(writer_sender);
                return Err(attach_cleanup(
                    error,
                    cleanup_partial_launch(
                        &mut child,
                        vec![writer_handle, frame_handle],
                        None,
                        Some(frame_events),
                        None,
                    ),
                ));
            }
        };
        if let Err(operation) = poll_stop(
            PROCESS_LAUNCH_PHASE,
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        ) {
            drop(writer_sender);
            return Err(attach_cleanup(
                operation,
                cleanup_partial_launch(
                    &mut child,
                    vec![writer_handle, frame_handle],
                    Some(diagnostic_handle),
                    Some(frame_events),
                    Some(diagnostic_events),
                ),
            ));
        }

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let (memory_observer, memory_attachment_error) =
            match LinuxMemoryObserver::attach(child.id(), memory_limits.process_bytes) {
                Ok(observer) => (observer, None),
                Err(source) => (
                    LinuxMemoryObserver::sampled_rss(memory_limits.process_bytes),
                    Some(source),
                ),
            };
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let memory_observer = Arc::new(Mutex::new(memory_observer));
        let mut resident = Self {
            child,
            grammar,
            session: session.clone(),
            artifact: launch.artifact.clone(),
            next_request_id: 1,
            termination_requested: false,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            memory_limits,
            writer: Some(writer_sender),
            writer_handle: Some(writer_handle),
            frame_reader: FrameReader {
                events: frame_events,
                handle: Some(frame_handle),
            },
            diagnostic_reader: DiagnosticReader {
                events: diagnostic_events,
                handle: Some(diagnostic_handle),
            },
            pending_diagnostic_fences: 0,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            memory_observer,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            memory_monitor: None,
        };
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(source) = memory_attachment_error {
            let operation = ParserSupervisorError::ResidentMemoryObservationFailed {
                phase: "delegated cgroup attachment cleanup",
                accounting: ParserMemoryAccountingKind::LinuxCgroupV2,
                message: bounded_message(source.to_string()),
            };
            return match resident.shutdown() {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        match LinuxMemoryMonitor::start(resident.child.id(), Arc::clone(&resident.memory_observer))
        {
            Ok(monitor) => resident.memory_monitor = Some(monitor),
            Err(source) => {
                let operation = ParserSupervisorError::IoThread {
                    phase: "resident-memory monitor startup",
                    message: source.to_string(),
                };
                return match resident.shutdown() {
                    Ok(()) => Err(operation),
                    Err(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                        operation: Box::new(operation),
                        cleanup: Box::new(cleanup),
                    }),
                };
            }
        }
        let opening: Result<(), ParserSupervisorError> = (|| {
            resident.wait_for_admission(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            let session_open = encode_parser_control(&ParserControl::SessionOpen(
                ParserSessionOpen::new(session.clone()),
            ))?;
            resident.send_bytes(
                session_open,
                "SessionOpen write",
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            let ready_bytes = resident.wait_for_frame(
                "READY",
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            let ready_frame = ParserFrame::decode_exact(&ready_bytes)?;
            decode_parser_ready_for_launch(ready_frame, &session, &launch.artifact, containment)?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            resident.enforce_memory_bound("READY", true)?;
            Ok(())
        })();
        if let Err(operation) = opening {
            if operation.is_caller_stop() {
                resident.termination_requested = true;
            }
            return match resident.shutdown() {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(resident)
    }

    /// Send one request/source pair and validate all response identities.
    fn parse(
        &mut self,
        source: &[u8],
        source_identity: ParserSourceIdentity,
        limits: ParserRequestLimits,
        mut last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<ParserCompletionEvidence, ParserSupervisorError> {
        let request_id = ParserRequestIdentity::new(self.next_request_id)?;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(ParserSupervisorError::RequestIdentityExhausted)?;
        let request = ParserRequest::new(
            self.session.clone(),
            request_id,
            self.artifact.clone(),
            self.grammar.clone(),
            source_identity,
            limits,
        );
        let mut request_bytes = encode_parser_control(&ParserControl::Request(request.clone()))?;
        let source_len = u32::try_from(source.len()).map_err(|_source| {
            ParserProtocolError::FramePayloadTooLarge {
                kind: ParserFrameKind::RawSource,
                actual: u32::MAX,
                maximum: ParserFrameKind::RawSource.maximum_payload_bytes(),
            }
        })?;
        let source_header = ParserFrameHeader::new(ParserFrameKind::RawSource, source_len)?;
        request_bytes.reserve(PARSER_FRAME_HEADER_BYTES.saturating_add(source.len()));
        request_bytes.extend_from_slice(&source_header.encode());
        request_bytes.extend_from_slice(source);

        self.send_bytes(
            request_bytes,
            "request write",
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        )?;
        let mut previous_progress: Option<ParserProgress> = None;
        loop {
            let response_bytes = self.wait_for_frame(
                "request response",
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            let frame = ParserFrame::decode_exact(&response_bytes)?;
            match frame.kind() {
                ParserFrameKind::Progress => {
                    let (progress, disposition) = decode_parser_progress_for_request(
                        frame,
                        &request,
                        previous_progress.as_ref(),
                    )?;
                    if disposition == ParserProgressDisposition::Advanced {
                        last_progress = Instant::now();
                    }
                    previous_progress = Some(progress);
                }
                ParserFrameKind::Completion => {
                    let completion = decode_parser_completion_for_request(frame, &request)?;
                    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                    self.enforce_memory_bound("request completion", true)?;
                    return Ok(completion.evidence().clone());
                }
                ParserFrameKind::Failure => {
                    let failure = decode_parser_failure_for_request(frame, &request)?;
                    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                    match self.enforce_memory_bound("request failure", true) {
                        Ok(()) | Err(ParserSupervisorError::ChildExited { code: Some(0), .. }) => {}
                        Err(error) => return Err(error),
                    }
                    return Err(ParserSupervisorError::WorkerFailure {
                        code: failure.code(),
                    });
                }
                kind => {
                    return Err(ParserProtocolError::UnexpectedFrameKind { kind }.into());
                }
            }
        }
    }

    /// Wait until the platform adapter authorizes protocol input.
    fn wait_for_admission(
        &mut self,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        loop {
            poll_stop(
                "containment admission",
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound("containment admission", false)?;
            match self.diagnostic_reader.events.recv_timeout(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            )) {
                Ok(DiagnosticReaderEvent::AdmissionAccepted) => return Ok(()),
                Ok(DiagnosticReaderEvent::FenceObserved) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase: "containment admission",
                        message: "diagnostic fence arrived before admission".to_owned(),
                    });
                }
                Ok(DiagnosticReaderEvent::Failure(ParserIoThreadError::AdmissionMismatch)) => {
                    return Err(ParserSupervisorError::InvalidAdmission);
                }
                Ok(DiagnosticReaderEvent::Failure(error)) => {
                    return Err(io_thread_error("containment admission", &error));
                }
                Err(RecvTimeoutError::Timeout) => self.require_child_running("admission")?,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase: "containment admission",
                        message: "diagnostic reader closed before admission".to_owned(),
                    });
                }
            }
        }
    }

    /// Submit one bounded write without blocking the caller on the pipe itself.
    fn send_bytes(
        &mut self,
        bytes: Vec<u8>,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        let (acknowledgement, result) = mpsc::sync_channel(1);
        let mut command = WriterCommand {
            bytes,
            acknowledgement,
        };
        loop {
            poll_stop(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound(phase, false)?;
            let Some(writer) = self.writer.as_ref() else {
                return Err(ParserSupervisorError::IoThread {
                    phase,
                    message: "writer was already closed".to_owned(),
                });
            };
            match writer.try_send(command) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => {
                    command = returned;
                    self.require_child_running(phase)?;
                    thread::sleep(next_poll_wait(
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                    ));
                }
                Err(TrySendError::Disconnected(_returned)) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase,
                        message: "writer thread closed".to_owned(),
                    });
                }
            }
        }
        loop {
            poll_stop(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound(phase, false)?;
            match result.recv_timeout(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            )) {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) => return Err(io_thread_error(phase, &error)),
                Err(RecvTimeoutError::Timeout) => self.require_child_running(phase)?,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase,
                        message: "writer acknowledgement closed".to_owned(),
                    });
                }
            }
        }
    }

    /// Wait for one framed response while polling every terminal condition.
    fn wait_for_frame(
        &mut self,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<Vec<u8>, ParserSupervisorError> {
        loop {
            poll_stop(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            if let Some(event) = try_frame_event(&self.frame_reader.events)? {
                self.synchronize_frame_event(
                    &event,
                    phase,
                    absolute_deadline,
                    last_progress,
                    no_progress_timeout,
                    cancellation,
                )?;
                return self.finish_frame_event(event, phase);
            }
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound(phase, false)?;
            self.check_diagnostic_reader(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            match self.frame_reader.events.recv_timeout(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            )) {
                Ok(event) => {
                    self.synchronize_frame_event(
                        &event,
                        phase,
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation,
                    )?;
                    return self.finish_frame_event(event, phase);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase,
                        message: "stdout reader closed".to_owned(),
                    });
                }
            }
        }
    }

    /// Order one stdout event against every earlier diagnostic-pipe write.
    fn synchronize_frame_event(
        &mut self,
        event: &FrameReaderEvent,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        if matches!(event, FrameReaderEvent::EndOfStream) {
            self.wait_for_diagnostic_termination(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )
        } else {
            self.wait_for_diagnostic_fence(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )
        }
    }

    /// Drain the diagnostic boundary before accepting clean stdout termination.
    fn wait_for_diagnostic_termination(
        &mut self,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        loop {
            poll_stop(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound(phase, false)?;
            self.check_diagnostic_reader(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            if thread_finished(self.diagnostic_reader.handle.as_ref()) {
                self.check_diagnostic_reader(
                    phase,
                    absolute_deadline,
                    last_progress,
                    no_progress_timeout,
                    cancellation,
                )?;
                return Ok(());
            }
            thread::sleep(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            ));
        }
    }

    /// Require the parent-authored stderr fence for one complete stdout frame.
    fn wait_for_diagnostic_fence(
        &mut self,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        if self.pending_diagnostic_fences > 0 {
            self.pending_diagnostic_fences = self.pending_diagnostic_fences.saturating_sub(1);
            return Ok(());
        }
        loop {
            poll_stop(
                phase,
                absolute_deadline,
                last_progress,
                no_progress_timeout,
                cancellation,
            )?;
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            match self.enforce_memory_bound(phase, false) {
                Ok(()) | Err(ParserSupervisorError::ChildExited { code: Some(0), .. }) => {}
                Err(error) => return Err(error),
            }
            match self.diagnostic_reader.events.recv_timeout(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            )) {
                Ok(DiagnosticReaderEvent::FenceObserved) => return Ok(()),
                Ok(DiagnosticReaderEvent::Failure(error)) => {
                    self.termination_requested = true;
                    return Err(diagnostic_failure_after_exit_observation(
                        &mut self.child,
                        phase,
                        &error,
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation,
                    ));
                }
                Ok(DiagnosticReaderEvent::AdmissionAccepted) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ParserSupervisorError::IoThread {
                        phase,
                        message: "diagnostic reader closed before frame fence".to_owned(),
                    });
                }
            }
        }
    }

    /// Convert one frame event and mark an OS-proved Windows memory exit as expected termination.
    fn finish_frame_event(
        &mut self,
        event: FrameReaderEvent,
        phase: &'static str,
    ) -> Result<Vec<u8>, ParserSupervisorError> {
        let result = frame_event_result(event, &mut self.child, phase);
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        if matches!(
            &result,
            Err(ParserSupervisorError::WindowsJobMemoryLimitExceeded { .. })
        ) {
            self.termination_requested = true;
        }
        result
    }

    /// Surface diagnostic bytes and retain frame fences observed ahead of stdout.
    fn check_diagnostic_reader(
        &mut self,
        phase: &'static str,
        absolute_deadline: Instant,
        last_progress: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        loop {
            match self.diagnostic_reader.events.try_recv() {
                Ok(DiagnosticReaderEvent::Failure(error)) => {
                    self.termination_requested = true;
                    return Err(diagnostic_failure_after_exit_observation(
                        &mut self.child,
                        phase,
                        &error,
                        absolute_deadline,
                        last_progress,
                        no_progress_timeout,
                        cancellation,
                    ));
                }
                Ok(DiagnosticReaderEvent::FenceObserved) => {
                    self.pending_diagnostic_fences = self
                        .pending_diagnostic_fences
                        .checked_add(1)
                        .ok_or_else(|| ParserSupervisorError::IoThread {
                            phase,
                            message: "diagnostic fence count overflowed".to_owned(),
                        })?;
                }
                Ok(DiagnosticReaderEvent::AdmissionAccepted) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    /// Enforce the Linux resident-memory ceiling and terminate the worker group on failure.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn enforce_memory_bound(
        &mut self,
        phase: &'static str,
        force: bool,
    ) -> Result<(), ParserSupervisorError> {
        let monitor_event = self
            .memory_monitor
            .as_ref()
            .and_then(|monitor| monitor.events.try_recv().ok());
        if let Some(event) = monitor_event {
            return Err(self.memory_monitor_error(phase, event));
        }
        if !force {
            return Ok(());
        }
        let process_id = self.child.id();
        let observation = match self.memory_observer.lock() {
            Ok(mut observer) => observer.observe(process_id),
            Err(_poisoned) => Err(io::Error::other("Linux memory observer lock was poisoned")),
        };
        let observation = match observation {
            Ok(observation) => observation,
            Err(source) => {
                let observer = Arc::clone(&self.memory_observer);
                let child = &mut self.child;
                let transition = resolve_linux_memory_exit_transition(
                    source,
                    SUPERVISOR_POLL_INTERVAL,
                    || match observer.try_lock() {
                        Ok(mut observer) => observer.observe(process_id),
                        Err(std::sync::TryLockError::WouldBlock) => Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Linux memory observer is busy",
                        )),
                        Err(std::sync::TryLockError::Poisoned(_poisoned)) => {
                            Err(io::Error::other("Linux memory observer lock was poisoned"))
                        }
                    },
                    || {
                        child.try_wait().map(|status| {
                            status.map(|status| LinuxChildExit {
                                code: status.code(),
                            })
                        })
                    },
                );
                match transition {
                    Ok(LinuxMemoryObservation::Memory(observation)) => observation,
                    Ok(LinuxMemoryObservation::ChildExited { code }) => {
                        return Err(ParserSupervisorError::ChildExited { phase, code });
                    }
                    Err(source) => {
                        self.termination_requested = true;
                        let operation = ParserSupervisorError::ResidentMemoryObservationFailed {
                            phase,
                            accounting: ParserMemoryAccountingKind::LinuxProcStatus,
                            message: bounded_message(source.to_string()),
                        };
                        return Err(attach_cleanup(
                            operation,
                            kill_direct_child(&mut self.child),
                        ));
                    }
                }
            }
        };
        let Some(breach) = observation else {
            return Ok(());
        };
        self.termination_requested = true;
        let observation_interval_millis =
            u64::try_from(PARSER_LINUX_RSS_OBSERVATION_INTERVAL.as_millis()).unwrap_or(u64::MAX);
        let operation = ParserSupervisorError::ResidentMemoryLimitExceeded {
            phase,
            accounting: breach.accounting,
            observed_bytes: breach.observed_bytes,
            maximum_bytes: self.memory_limits.process_bytes,
            observation_interval_millis,
        };
        Err(attach_cleanup(
            operation,
            kill_direct_child(&mut self.child),
        ))
    }

    /// Convert one monitor-owned terminal event into the public typed failure.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn memory_monitor_error(
        &mut self,
        phase: &'static str,
        event: LinuxMemoryMonitorEvent,
    ) -> ParserSupervisorError {
        let (operation, termination_error) = match event {
            LinuxMemoryMonitorEvent::Limit {
                breach,
                termination_error,
            } => {
                self.termination_requested = true;
                (
                    ParserSupervisorError::ResidentMemoryLimitExceeded {
                        phase,
                        accounting: breach.accounting,
                        observed_bytes: breach.observed_bytes,
                        maximum_bytes: self.memory_limits.process_bytes,
                        observation_interval_millis: u64::try_from(
                            PARSER_LINUX_RSS_OBSERVATION_INTERVAL.as_millis(),
                        )
                        .unwrap_or(u64::MAX),
                    },
                    termination_error,
                )
            }
            LinuxMemoryMonitorEvent::ObservationFailed {
                accounting,
                message,
            } => {
                let transition = resolve_linux_memory_exit_transition(
                    io::Error::other(message.clone()),
                    SUPERVISOR_POLL_INTERVAL,
                    || Err(io::Error::other(message.clone())),
                    || {
                        self.child.try_wait().map(|status| {
                            status.map(|status| LinuxChildExit {
                                code: status.code(),
                            })
                        })
                    },
                );
                if let Ok(LinuxMemoryObservation::ChildExited { code }) = transition {
                    return ParserSupervisorError::ChildExited { phase, code };
                }
                self.termination_requested = true;
                return attach_cleanup(
                    ParserSupervisorError::ResidentMemoryObservationFailed {
                        phase,
                        accounting,
                        message,
                    },
                    kill_direct_child(&mut self.child),
                );
            }
        };
        let Some(termination_error) = termination_error else {
            return operation;
        };
        let retry = kill_direct_child(&mut self.child);
        let initial = ParserSupervisorError::Cleanup {
            message: format!(
                "continuous memory monitor could not terminate the worker process group: {termination_error}"
            ),
        };
        let cleanup = match retry {
            Ok(()) => initial,
            Err(retry) => ParserSupervisorError::OperationAndCleanup {
                operation: Box::new(initial),
                cleanup: Box::new(retry),
            },
        };
        ParserSupervisorError::OperationAndCleanup {
            operation: Box::new(operation),
            cleanup: Box::new(cleanup),
        }
    }

    /// Retain the first memory failure while shutdown continues mandatory cleanup.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn observe_shutdown_memory(&mut self, failure: &mut Option<ParserSupervisorError>) {
        if failure.is_some() {
            return;
        }
        match self.enforce_memory_bound("shutdown", true) {
            Ok(()) | Err(ParserSupervisorError::ChildExited { .. }) => {}
            Err(error) => *failure = Some(error),
        }
    }

    /// Require that the direct child has not exited.
    fn require_child_running(&mut self, phase: &'static str) -> Result<(), ParserSupervisorError> {
        match self
            .child
            .try_wait()
            .map_err(|source| ParserSupervisorError::IoThread {
                phase,
                message: source.to_string(),
            })? {
            None => Ok(()),
            Some(status) => Err(ParserSupervisorError::ChildExited {
                phase,
                code: status.code(),
            }),
        }
    }

    /// Close input, terminate if needed, reap, drain, and join exactly once.
    fn shutdown(mut self) -> Result<(), ParserSupervisorError> {
        let mut failures = Vec::new();
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let mut memory_failure = None;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(mut monitor) = self.memory_monitor.take() {
            match monitor.stop() {
                Ok(Some(event)) => {
                    memory_failure = Some(self.memory_monitor_error("idle resident", event));
                }
                Ok(None) => {}
                Err(error) => failures.push(error.to_string()),
            }
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        self.observe_shutdown_memory(&mut memory_failure);
        self.writer.take();
        let graceful_deadline = Instant::now()
            .checked_add(SUPERVISOR_GRACEFUL_CLOSE)
            .unwrap_or_else(Instant::now);
        while self
            .writer_handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
            && Instant::now() < graceful_deadline
        {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.observe_shutdown_memory(&mut memory_failure);
            drain_reader_events(&self.frame_reader.events, &self.diagnostic_reader.events);
            thread::sleep(SUPERVISOR_POLL_INTERVAL);
        }
        let mut forced = self.termination_requested;
        let mut status = match self.child.try_wait() {
            Ok(status) => status,
            Err(source) => {
                failures.push(source.to_string());
                None
            }
        };
        while status.is_none() && Instant::now() < graceful_deadline {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.observe_shutdown_memory(&mut memory_failure);
            drain_reader_events(&self.frame_reader.events, &self.diagnostic_reader.events);
            thread::sleep(SUPERVISOR_POLL_INTERVAL);
            match self.child.try_wait() {
                Ok(observed) => status = observed,
                Err(source) => {
                    failures.push(source.to_string());
                    break;
                }
            }
        }
        if status.is_none() {
            forced = true;
            if let Err(error) = kill_direct_child(&mut self.child) {
                failures.push(error.to_string());
            }
        }
        let cleanup_deadline = Instant::now()
            .checked_add(SUPERVISOR_CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        while status.is_none() && Instant::now() < cleanup_deadline {
            drain_reader_events(&self.frame_reader.events, &self.diagnostic_reader.events);
            thread::sleep(SUPERVISOR_POLL_INTERVAL);
            match self.child.try_wait() {
                Ok(observed) => status = observed,
                Err(source) => {
                    failures.push(source.to_string());
                    break;
                }
            }
        }
        if status.is_none() {
            failures.push("direct child was not reaped within the cleanup deadline".to_owned());
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if status.is_some() {
            let cleanup = match self.memory_observer.lock() {
                Ok(mut observer) => observer.cleanup(),
                Err(_poisoned) => Err(io::Error::other(
                    "Linux memory observer lock was poisoned during cleanup",
                )),
            };
            if let Err(source) = cleanup {
                failures.push(format!(
                    "delegated Linux memory-accounting cleanup failed: {source}"
                ));
            }
        }

        while (!thread_finished(self.writer_handle.as_ref())
            || !thread_finished(self.frame_reader.handle.as_ref())
            || !thread_finished(self.diagnostic_reader.handle.as_ref()))
            && Instant::now() < cleanup_deadline
        {
            drain_reader_events(&self.frame_reader.events, &self.diagnostic_reader.events);
            thread::sleep(SUPERVISOR_POLL_INTERVAL);
        }
        if !thread_finished(self.writer_handle.as_ref())
            || !thread_finished(self.frame_reader.handle.as_ref())
            || !thread_finished(self.diagnostic_reader.handle.as_ref())
        {
            failures
                .push("protocol I/O threads did not drain within the cleanup deadline".to_owned());
        }
        if thread_finished(self.writer_handle.as_ref())
            && let Err(error) = join_unit_thread(self.writer_handle.take(), "writer")
        {
            failures.push(error.to_string());
        }
        if thread_finished(self.frame_reader.handle.as_ref())
            && let Err(error) = join_unit_thread(self.frame_reader.handle.take(), "stdout reader")
        {
            failures.push(error.to_string());
        }
        let diagnostics = if thread_finished(self.diagnostic_reader.handle.as_ref()) {
            match join_diagnostic_thread(self.diagnostic_reader.handle.take()) {
                Ok(diagnostics) => diagnostics,
                Err(error) => {
                    failures.push(error.to_string());
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        forced |= self.termination_requested;
        if !forced && status.as_ref().is_some_and(|status| !status.success()) {
            failures.push(format!(
                "direct child reported failed cleanup; diagnostic={}",
                bounded_diagnostic(&diagnostics)
            ));
        }
        if !forced && status.as_ref().is_some_and(ExitStatus::success) && !diagnostics.is_empty() {
            failures.push(format!(
                "direct child emitted unexpected diagnostic bytes: {}",
                bounded_diagnostic(&diagnostics)
            ));
        }
        let cleanup = (!failures.is_empty()).then(|| ParserSupervisorError::Cleanup {
            message: bounded_message(failures.join("; ")),
        });
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if let Some(operation) = memory_failure {
            return match cleanup {
                Some(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
                None => Err(operation),
            };
        }
        match cleanup {
            None => Ok(()),
            Some(cleanup) => Err(cleanup),
        }
    }
}

/// Inherit only the sealed Linux authority descriptors needed after `exec`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[expect(
    unsafe_code,
    reason = "Command has no safe descriptor-sanitizing hook; this pre-exec closure performs only async-signal-safe Linux syscalls"
)]
fn inherit_linux_authority_on_exec(command: &mut Command, authority: LinuxResidentLaunchAuthority) {
    use nix::libc;
    use std::os::unix::process::CommandExt;

    let inherited = [
        authority.artifact_manifest.raw_fd(),
        authority.accepted_manifest.raw_fd(),
        authority.native_import_policy.raw_fd(),
        authority.grammar.raw_fd(),
    ];
    // SAFETY: `pre_exec` runs after fork. The closure retains every source
    // descriptor and performs only allocation-free `close_range` and `fcntl`
    // syscalls. Parent descriptors remain CLOEXEC, so concurrent spawns cannot
    // inherit them. Rust's spawn-error pipe remains CLOEXEC until successful exec.
    unsafe {
        command.pre_exec(move || {
            let _authority_guard = &authority;
            let result = libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC | libc::CLOSE_RANGE_UNSHARE,
            );
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            for descriptor in inherited {
                if libc::fcntl(descriptor, libc::F_SETFD, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

/// Build the one accepted platform command with closed arguments and environment.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn platform_command(
    launch: &VerifiedParserPackLaunch,
    authority: LinuxResidentLaunchAuthority,
    _memory_limits: ParserMemoryLimits,
) -> Result<Command, ParserSupervisorError> {
    use std::os::unix::process::CommandExt;

    if launch.platform != PackPlatform::LinuxX86_64 {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: launch.pack_root.clone(),
            reason: "Linux supervisor received another platform artifact",
        });
    }
    let worker_fd = authority.worker.raw_fd();
    let artifact_fd = authority.artifact_manifest.raw_fd();
    let accepted_fd = authority.accepted_manifest.raw_fd();
    let policy_fd = authority.native_import_policy.raw_fd();
    let grammar_fd = authority.grammar.raw_fd();
    let mut command = Command::new(format!("/proc/self/fd/{worker_fd}"));
    command
        .arg(SERVE_ARGUMENT)
        .arg(ARTIFACT_FD_ARGUMENT)
        .arg(artifact_fd.to_string())
        .arg(ACCEPTED_FD_ARGUMENT)
        .arg(accepted_fd.to_string())
        .arg(POLICY_FD_ARGUMENT)
        .arg(policy_fd.to_string())
        .arg(GRAMMAR_FD_ARGUMENT)
        .arg(grammar_fd.to_string())
        .current_dir(&launch.pack_root)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    inherit_linux_authority_on_exec(&mut command, authority);
    Ok(command)
}

/// Build the one accepted Windows broker command with closed arguments and environment.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn platform_command(
    launch: &VerifiedParserPackLaunch,
    memory_limits: ParserMemoryLimits,
) -> Result<Command, ParserSupervisorError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if launch.platform != PackPlatform::WindowsX86_64 {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: launch.pack_root.clone(),
            reason: "Windows supervisor received another platform artifact",
        });
    }
    let broker = launch.containment_broker.as_ref().ok_or_else(|| {
        ParserSupervisorError::PayloadMismatch {
            path: launch.pack_root.clone(),
            reason: "Windows artifact has no containment broker",
        }
    })?;
    let mut command = Command::new(broker);
    command
        .arg(BROKER_SERVE_ARGUMENT)
        .arg("--parent-pid")
        .arg(std::process::id().to_string())
        .arg("--process-memory-bytes")
        .arg(memory_limits.process_bytes.to_string())
        .arg("--job-memory-bytes")
        .arg(memory_limits.process_tree_bytes.to_string())
        .current_dir(&launch.pack_root)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

/// Refuse command construction on every unaccepted optional-pack target.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn platform_command(
    _launch: &VerifiedParserPackLaunch,
    _memory_limits: ParserMemoryLimits,
) -> Result<Command, ParserSupervisorError> {
    Err(ParserSupervisorError::UnsupportedContainment {
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    })
}

/// Return the READY containment identity for one closed artifact target.
const fn containment_for_platform(platform: PackPlatform) -> ParserContainmentKind {
    match platform {
        PackPlatform::LinuxX86_64 => ParserContainmentKind::LinuxLandlockSeccomp,
        PackPlatform::WindowsX86_64 => ParserContainmentKind::WindowsAppContainerJob,
    }
}

/// Generate a fresh session identity from operating-system entropy.
fn fresh_session_identity() -> Result<ParserSessionIdentity, ParserSupervisorError> {
    let mut entropy = [0_u8; PARSER_SESSION_ENTROPY_BYTES];
    getrandom::fill(&mut entropy).map_err(|_source| ParserSupervisorError::EntropyUnavailable)?;
    Ok(ParserSessionIdentity::for_entropy(&entropy))
}

/// Generate one parent-only marker that a worker cannot forge before a frame.
fn fresh_diagnostic_fence() -> Result<DiagnosticFence, ParserSupervisorError> {
    let mut entropy = [0_u8; PARSER_DIAGNOSTIC_FENCE_BYTES];
    getrandom::fill(&mut entropy).map_err(|_source| ParserSupervisorError::EntropyUnavailable)?;
    Ok(DiagnosticFence(entropy))
}

/// Convert a private I/O thread failure at the public typed boundary.
fn io_thread_error(phase: &'static str, error: &ParserIoThreadError) -> ParserSupervisorError {
    ParserSupervisorError::IoThread {
        phase,
        message: bounded_message(error.to_string()),
    }
}

/// Preserve fail-closed diagnostics while allowing the Windows broker to prove a memory exit.
fn diagnostic_failure_after_exit_observation(
    child: &mut Child,
    phase: &'static str,
    error: &ParserIoThreadError,
    absolute_deadline: Instant,
    last_progress: Instant,
    no_progress_timeout: Duration,
    cancellation: &IndexCancellation,
) -> ParserSupervisorError {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        let no_progress_deadline = last_progress
            .checked_add(no_progress_timeout)
            .unwrap_or(absolute_deadline);
        let observation_deadline = absolute_deadline.min(no_progress_deadline);
        loop {
            match child.try_wait() {
                Ok(Some(status))
                    if status.code() == Some(PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE) =>
                {
                    return ParserSupervisorError::WindowsJobMemoryLimitExceeded { phase };
                }
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            let now = Instant::now();
            if cancellation.is_cancelled() || now >= observation_deadline {
                break;
            }
            thread::sleep(SUPERVISOR_POLL_INTERVAL.min(observation_deadline.duration_since(now)));
        }
    }
    #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
    let _ = (
        child,
        absolute_deadline,
        last_progress,
        no_progress_timeout,
        cancellation,
    );
    io_thread_error(phase, error)
}

/// Bound one internal diagnostic without splitting UTF-8.
fn bounded_message(mut message: String) -> String {
    let mut end = message.len().min(PARSER_MAX_STDERR_BYTES);
    while !message.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    message.truncate(end);
    message
}

/// Render one already bounded diagnostic byte stream safely.
fn bounded_diagnostic(bytes: &[u8]) -> String {
    bounded_message(String::from_utf8_lossy(bytes).into_owned())
}

/// Poll cancellation, the immutable absolute deadline, and meaningful progress age.
fn poll_stop(
    phase: &'static str,
    absolute_deadline: Instant,
    last_progress: Instant,
    no_progress_timeout: Duration,
    cancellation: &IndexCancellation,
) -> Result<(), ParserSupervisorError> {
    if cancellation.is_cancelled() {
        return Err(ParserSupervisorError::Cancelled { phase });
    }
    let now = Instant::now();
    if now >= absolute_deadline {
        return Err(ParserSupervisorError::DeadlineExceeded { phase });
    }
    if now.saturating_duration_since(last_progress) >= no_progress_timeout {
        return Err(ParserSupervisorError::NoProgress { phase });
    }
    Ok(())
}

/// Return the next short wait that preserves every caller-owned bound.
fn next_poll_wait(
    absolute_deadline: Instant,
    last_progress: Instant,
    no_progress_timeout: Duration,
) -> Duration {
    let now = Instant::now();
    let deadline_wait = absolute_deadline.saturating_duration_since(now);
    let progress_wait =
        no_progress_timeout.saturating_sub(now.saturating_duration_since(last_progress));
    SUPERVISOR_POLL_INTERVAL
        .min(deadline_wait)
        .min(progress_wait)
}

/// Take one already buffered frame event before observing child exit state.
fn try_frame_event(
    events: &Receiver<FrameReaderEvent>,
) -> Result<Option<FrameReaderEvent>, ParserSupervisorError> {
    match events.try_recv() {
        Ok(event) => Ok(Some(event)),
        Err(TryRecvError::Empty) => Ok(None),
        Err(TryRecvError::Disconnected) => Err(ParserSupervisorError::IoThread {
            phase: "stdout reader",
            message: "stdout reader closed without a terminal event".to_owned(),
        }),
    }
}

/// Convert one owned frame-reader event at the synchronous request boundary.
fn frame_event_result(
    event: FrameReaderEvent,
    child: &mut Child,
    phase: &'static str,
) -> Result<Vec<u8>, ParserSupervisorError> {
    match event {
        FrameReaderEvent::Frame(frame) => Ok(frame),
        FrameReaderEvent::EndOfStream => {
            let status = wait_for_observed_exit(child, SUPERVISOR_POLL_INTERVAL)?;
            let code = status.and_then(|status| status.code());
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            if code == Some(PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE) {
                return Err(ParserSupervisorError::WindowsJobMemoryLimitExceeded { phase });
            }
            Err(ParserSupervisorError::ChildExited { phase, code })
        }
        FrameReaderEvent::Failure(error) => Err(io_thread_error(phase, &error)),
    }
}

/// Observe a direct-child exit for one short bounded interval.
fn wait_for_observed_exit(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<ExitStatus>, ParserSupervisorError> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    loop {
        if let Some(status) =
            child
                .try_wait()
                .map_err(|source| ParserSupervisorError::IoThread {
                    phase: "child exit observation",
                    message: source.to_string(),
                })?
        {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

/// Terminate the complete Linux worker group.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn terminate_linux_process_group(process_id: u32) -> Result<LinuxProcessGroupTermination, String> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let process_group = i32::try_from(process_id)
        .map_err(|_source| "worker process-group identity exceeds i32".to_owned())?;
    match killpg(Pid::from_raw(process_group), Signal::SIGKILL) {
        Ok(()) => Ok(LinuxProcessGroupTermination::Signalled),
        Err(Errno::ESRCH) => Ok(LinuxProcessGroupTermination::Absent),
        Err(source) => Err(source.to_string()),
    }
}

/// Terminate the complete Linux worker group and fall back to the direct child.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn kill_direct_child(child: &mut Child) -> Result<(), ParserSupervisorError> {
    if child
        .try_wait()
        .map_err(|source| ParserSupervisorError::Cleanup {
            message: source.to_string(),
        })?
        .is_some()
    {
        return Ok(());
    }
    match terminate_linux_process_group(child.id()) {
        Ok(LinuxProcessGroupTermination::Signalled) => Ok(()),
        Ok(LinuxProcessGroupTermination::Absent) => {
            if child
                .try_wait()
                .map_err(|source| ParserSupervisorError::Cleanup {
                    message: source.to_string(),
                })?
                .is_none()
            {
                child
                    .kill()
                    .map_err(|source| ParserSupervisorError::Cleanup {
                        message: format!(
                            "worker process group was absent and direct-child termination failed: {source}"
                        ),
                    })?;
            }
            Ok(())
        }
        Err(group_error) => {
            let direct_error = child.kill().err().map(|error| error.to_string());
            Err(ParserSupervisorError::Cleanup {
                message: direct_error.map_or_else(
                    || format!("process-group termination failed: {group_error}"),
                    |direct_error| {
                        format!(
                            "process-group termination failed: {group_error}; direct-child termination failed: {direct_error}"
                        )
                    },
                ),
            })
        }
    }
}

/// Terminate the direct Windows broker or unsupported-host child.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn kill_direct_child(child: &mut Child) -> Result<(), ParserSupervisorError> {
    if child
        .try_wait()
        .map_err(|source| ParserSupervisorError::Cleanup {
            message: source.to_string(),
        })?
        .is_none()
    {
        child
            .kill()
            .map_err(|source| ParserSupervisorError::Cleanup {
                message: source.to_string(),
            })?;
    }
    Ok(())
}

/// Reap an incompletely constructed launch without an unbounded process wait.
fn cleanup_partial_launch(
    child: &mut Child,
    handles: Vec<JoinHandle<()>>,
    diagnostic_handle: Option<JoinHandle<Result<Vec<u8>, ParserIoThreadError>>>,
    frame_events: Option<Receiver<FrameReaderEvent>>,
    diagnostic_events: Option<Receiver<DiagnosticReaderEvent>>,
) -> Result<(), ParserSupervisorError> {
    drop(frame_events);
    drop(diagnostic_events);
    let mut failures = Vec::new();
    if let Err(error) = kill_direct_child(child) {
        failures.push(error.to_string());
    }
    let deadline = Instant::now()
        .checked_add(SUPERVISOR_CLEANUP_TIMEOUT)
        .unwrap_or_else(Instant::now);
    let mut reaped = false;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_status)) => reaped = true,
            Ok(None) => {}
            Err(source) => {
                failures.push(source.to_string());
                break;
            }
        }
        if reaped
            && handles.iter().all(JoinHandle::is_finished)
            && thread_finished(diagnostic_handle.as_ref())
        {
            break;
        }
        thread::sleep(SUPERVISOR_POLL_INTERVAL);
    }
    if !reaped {
        failures.push("incomplete direct child was not reaped".to_owned());
    }
    for handle in handles {
        if !handle.is_finished() {
            failures.push("incomplete launch thread did not terminate".to_owned());
        } else if handle.join().is_err() {
            failures.push("incomplete launch thread panicked".to_owned());
        }
    }
    if thread_finished(diagnostic_handle.as_ref()) {
        if let Err(error) = join_diagnostic_thread(diagnostic_handle) {
            failures.push(error.to_string());
        }
    } else if diagnostic_handle.is_some() {
        failures.push("incomplete diagnostic thread did not terminate".to_owned());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ParserSupervisorError::Cleanup {
            message: bounded_message(failures.join("; ")),
        })
    }
}

/// Preserve an operation failure together with any mandatory cleanup failure.
fn attach_cleanup(
    operation: ParserSupervisorError,
    cleanup: Result<(), ParserSupervisorError>,
) -> ParserSupervisorError {
    match cleanup {
        Ok(()) => operation,
        Err(cleanup) => ParserSupervisorError::OperationAndCleanup {
            operation: Box::new(operation),
            cleanup: Box::new(cleanup),
        },
    }
}

/// Discard bounded reader events so capacity-one senders can finish during cleanup.
fn drain_reader_events(
    frames: &Receiver<FrameReaderEvent>,
    diagnostics: &Receiver<DiagnosticReaderEvent>,
) {
    while frames.try_recv().is_ok() {}
    while diagnostics.try_recv().is_ok() {}
}

/// Return whether one optional owned thread has terminated.
fn thread_finished<T>(handle: Option<&JoinHandle<T>>) -> bool {
    handle.is_none_or(JoinHandle::is_finished)
}

/// Join one unit-returning thread and reject a panic.
fn join_unit_thread(
    handle: Option<JoinHandle<()>>,
    name: &'static str,
) -> Result<(), ParserSupervisorError> {
    let Some(handle) = handle else {
        return Ok(());
    };
    handle
        .join()
        .map_err(|_panic| ParserSupervisorError::Cleanup {
            message: format!("{name} thread panicked"),
        })
}

/// Join the diagnostic thread and retain its bounded bytes.
fn join_diagnostic_thread(
    handle: Option<JoinHandle<Result<Vec<u8>, ParserIoThreadError>>>,
) -> Result<Vec<u8>, ParserSupervisorError> {
    let Some(handle) = handle else {
        return Ok(Vec::new());
    };
    let result = handle
        .join()
        .map_err(|_panic| ParserSupervisorError::Cleanup {
            message: "diagnostic reader thread panicked".to_owned(),
        })?;
    result.map_err(|error| ParserSupervisorError::Cleanup {
        message: bounded_message(error.to_string()),
    })
}

/// Synchronous owner of the one process-wide optional parser session.
pub struct OptionalParserSupervisor {
    /// Exact artifact root revalidated after observed mutation.
    pack_root: PathBuf,
    /// Current verified launch authority.
    launch: VerifiedParserPackLaunch,
    /// Worker and process-tree ceilings applied to every resident session.
    memory_limits: ParserMemoryLimits,
    /// Current grammar-affined child session, when healthy.
    resident: Option<ResidentParserSession>,
}

impl OptionalParserSupervisor {
    /// Open and verify one installed immutable optional-parser artifact.
    ///
    /// This performs no process creation. Unsupported hosts fail before artifact
    /// acquisition by higher layers, worker launch, or source transfer.
    ///
    /// # Errors
    ///
    /// Returns a typed unsupported-host, path, manifest, payload, or digest error.
    pub fn open(pack_root: impl AsRef<Path>) -> Result<Self, ParserSupervisorError> {
        Self::open_with_memory_limits(pack_root, ParserMemoryLimits::PRODUCTION)
    }

    /// Open one verified artifact with caller-owned release-probe ceilings.
    fn open_with_memory_limits(
        pack_root: impl AsRef<Path>,
        memory_limits: ParserMemoryLimits,
    ) -> Result<Self, ParserSupervisorError> {
        let pack_root = pack_root.as_ref().to_path_buf();
        let launch = VerifiedParserPackLaunch::load(&pack_root)?;
        Ok(Self {
            pack_root,
            launch,
            memory_limits: memory_limits.checked()?,
            resident: None,
        })
    }

    /// Borrow the exact artifact-manifest identity verified during open.
    #[must_use]
    pub(crate) const fn artifact_identity(&self) -> &ParserArtifactIdentity {
        &self.launch.artifact
    }

    /// Borrow the canonical root bound to this verified launch authority.
    #[must_use]
    pub(crate) fn pack_root(&self) -> &Path {
        &self.pack_root
    }

    /// Return whether the verified artifact accepts one canonical language identity.
    #[must_use]
    pub(crate) fn accepts_language(&self, language_id: &str) -> bool {
        self.launch
            .accepted_grammars
            .binary_search_by(|candidate| candidate.as_str().cmp(language_id))
            .is_ok()
    }

    /// Parse bounded raw source through one grammar-affined contained worker.
    ///
    /// `absolute_deadline` is never extended by progress. One pre-READY epoch
    /// covers currentness, reload, sealing, admission, and opening. A newly
    /// validated READY or later identity-validated advancing progress resets
    /// `no_progress_timeout`.
    /// `cancellation` is polled while waiting for admission, writes, and output.
    ///
    /// # Errors
    ///
    /// Returns a typed artifact, grammar, containment, protocol, worker,
    /// cancellation, timeout, I/O, or mandatory cleanup failure. Any failed
    /// operation destroys the resident session before returning.
    pub fn parse(
        &mut self,
        language_id: &str,
        source: &[u8],
        limits: ParserRequestLimits,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<ParserCompletionEvidence, ParserSupervisorError> {
        let last_progress = Instant::now();
        poll_stop(
            "request admission",
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        )?;
        let source_identity = ParserSourceIdentity::for_bytes(source)?;
        self.refresh_changed_artifact(
            language_id,
            last_progress,
            absolute_deadline,
            no_progress_timeout,
            cancellation,
        )?;
        if let Some(resident) = self.resident.as_ref() {
            let grammar_changed = self
                .launch
                .require_grammar(language_id)
                .map_or(true, |grammar| resident.grammar != grammar);
            if grammar_changed {
                self.shutdown_resident()?;
            }
        }
        let mut resident_opened = false;
        if self.resident.is_none() {
            let grammar = self.launch.require_grammar(language_id)?;
            self.resident = Some(ResidentParserSession::launch(
                &self.launch,
                grammar,
                self.memory_limits,
                last_progress,
                absolute_deadline,
                no_progress_timeout,
                cancellation,
            )?);
            resident_opened = true;
        }
        let request_last_progress = if resident_opened {
            Instant::now()
        } else {
            last_progress
        };
        let result = self
            .resident
            .as_mut()
            .ok_or_else(|| ParserSupervisorError::IoThread {
                phase: "request admission",
                message: "resident session was not retained".to_owned(),
            })?
            .parse(
                source,
                source_identity,
                limits,
                request_last_progress,
                absolute_deadline,
                no_progress_timeout,
                cancellation,
            );
        match result {
            Ok(evidence) => Ok(evidence),
            Err(operation) => {
                if operation.is_caller_stop()
                    && let Some(resident) = self.resident.as_mut()
                {
                    resident.termination_requested = true;
                }
                match self.take_and_shutdown_resident() {
                    Ok(()) => Err(operation),
                    Err(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                        operation: Box::new(operation),
                        cleanup: Box::new(cleanup),
                    }),
                }
            }
        }
    }

    /// Close, drain, and reap the resident session when one exists.
    ///
    /// # Errors
    ///
    /// Returns a typed cleanup failure when the direct child or an owned I/O
    /// thread cannot be verified as terminated within the cleanup deadline.
    pub fn shutdown(&mut self) -> Result<(), ParserSupervisorError> {
        self.shutdown_resident()
    }

    /// Replace launch authority only after observed artifact mutation.
    fn refresh_changed_artifact(
        &mut self,
        language_id: &str,
        last_progress: Instant,
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<(), ParserSupervisorError> {
        let control = ArtifactIoControl {
            absolute_deadline,
            last_progress,
            no_progress_timeout,
            cancellation,
        };
        let probe = self.launch.currentness_probe(language_id);
        let current = match run_bounded_artifact_currentness(probe, &control) {
            Ok(current) => current,
            Err(operation) => {
                return Err(attach_cleanup(operation, self.take_and_shutdown_resident()));
            }
        };
        if current {
            return Ok(());
        }
        self.shutdown_resident()?;
        let refreshed = VerifiedParserPackLaunch::load_controlled(
            &self.pack_root,
            language_id,
            last_progress,
            absolute_deadline,
            no_progress_timeout,
            cancellation,
        )?;
        self.replace_verified_launch(refreshed)
    }

    /// Replace launch observations only when the content-addressed artifact identity is unchanged.
    fn replace_verified_launch(
        &mut self,
        refreshed: VerifiedParserPackLaunch,
    ) -> Result<(), ParserSupervisorError> {
        if refreshed.artifact != self.launch.artifact {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: self.pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
                reason: "artifact identity changed inside its immutable slot",
            });
        }
        self.launch = refreshed;
        Ok(())
    }

    /// Close the current resident session and clear its grammar affinity.
    fn shutdown_resident(&mut self) -> Result<(), ParserSupervisorError> {
        self.take_and_shutdown_resident()
    }

    /// Take and terminate the current session exactly once.
    fn take_and_shutdown_resident(&mut self) -> Result<(), ParserSupervisorError> {
        match self.resident.take() {
            Some(resident) => resident.shutdown(),
            None => Ok(()),
        }
    }
}

/// Exercise the exact packaged worker under a deliberately reduced memory ceiling.
///
/// Release verification first admits every grammar under the production ceilings. This
/// differential probe then reuses the same supervisor and platform adapter with a smaller
/// ceiling, requires the OS-specific memory failure, and accepts the result only when mandatory
/// process-tree cleanup also succeeds.
///
/// # Errors
///
/// Returns the first artifact, containment, protocol, cleanup, or unexpected probe outcome.
pub fn probe_optional_parser_memory_boundary(
    pack_root: impl AsRef<Path>,
    logical: &OptionalParserPackManifest,
) -> Result<ParserPackMemoryProbe, ParserSupervisorError> {
    let pack_root = pack_root.as_ref();
    logical.validate()?;
    let grammar =
        logical
            .grammars()
            .first()
            .ok_or_else(|| ParserSupervisorError::PayloadMismatch {
                path: pack_root.to_path_buf(),
                reason: "optional parser manifest has no grammar for the memory probe",
            })?;
    let platform = host_pack_platform().ok_or(ParserSupervisorError::UnsupportedContainment {
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    })?;
    let process_bytes = match platform {
        PackPlatform::LinuxX86_64 => OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES,
        PackPlatform::WindowsX86_64 => OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES,
    };
    let memory_limits = ParserMemoryLimits {
        process_bytes,
        process_tree_bytes: process_bytes,
    }
    .checked()?;
    let probe_source = memory_probe_source(platform, grammar.fixtures.positive.source.as_bytes());
    let mut supervisor =
        OptionalParserSupervisor::open_with_memory_limits(pack_root, memory_limits)?;
    let limits = ParserRequestLimits::new(
        PARSER_MAX_OUTPUT_BYTES,
        PARSER_MAX_NODE_COUNT,
        PARSER_MAX_TREE_DEPTH,
    )?;
    let deadline = Instant::now()
        .checked_add(ARTIFACT_ADMISSION_TIMEOUT)
        .ok_or(ParserSupervisorError::DeadlineExceeded {
            phase: "memory probe deadline calculation",
        })?;
    let operation = supervisor.parse(
        &grammar.language_id,
        &probe_source,
        limits,
        deadline,
        ARTIFACT_ADMISSION_NO_PROGRESS_TIMEOUT,
        &IndexCancellation::new(),
    );
    let cleanup = supervisor.shutdown();
    let failure = match operation {
        Ok(_evidence) => match cleanup {
            Ok(()) => {
                return Err(ParserSupervisorError::MemoryProbeDidNotBreach { process_bytes });
            }
            Err(error) => error,
        },
        Err(error) => attach_cleanup(error, cleanup),
    };

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if let ParserSupervisorError::ResidentMemoryLimitExceeded {
        accounting,
        observed_bytes,
        maximum_bytes,
        observation_interval_millis,
        ..
    } = &failure
    {
        let (control, interval, peak, overshoot) = match *accounting {
            ParserMemoryAccountingKind::LinuxCgroupV2 => {
                (ParserPackMemoryControl::LinuxCgroupV2, None, None, None)
            }
            ParserMemoryAccountingKind::LinuxProcStatus => (
                ParserPackMemoryControl::LinuxProcStatus,
                Some(*observation_interval_millis),
                Some(*observed_bytes),
                Some(observed_bytes.saturating_sub(*maximum_bytes)),
            ),
        };
        return Ok(ParserPackMemoryProbe {
            control,
            process_limit_bytes: *maximum_bytes,
            process_tree_limit_bytes: memory_limits.process_tree_bytes,
            observation_interval_millis: interval,
            peak_observed_bytes: peak,
            maximum_observed_overshoot_bytes: overshoot,
            limit_enforced: ParserPackVerifiedControl::Verified,
            process_tree_cleaned: ParserPackVerifiedControl::Verified,
        });
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    if matches!(
        &failure,
        ParserSupervisorError::WindowsJobMemoryLimitExceeded { .. }
    ) {
        return Ok(ParserPackMemoryProbe {
            control: ParserPackMemoryControl::WindowsJobObject,
            process_limit_bytes: memory_limits.process_bytes,
            process_tree_limit_bytes: memory_limits.process_tree_bytes,
            observation_interval_millis: None,
            peak_observed_bytes: None,
            maximum_observed_overshoot_bytes: None,
            limit_enforced: ParserPackVerifiedControl::Verified,
            process_tree_cleaned: ParserPackVerifiedControl::Verified,
        });
    }

    Err(failure)
}

/// Keep Linux's startup probe small while forcing post-admission allocation on Windows.
fn memory_probe_source(platform: PackPlatform, fixture: &[u8]) -> Vec<u8> {
    match platform {
        PackPlatform::LinuxX86_64 => fixture.to_vec(),
        PackPlatform::WindowsX86_64 => fixture
            .iter()
            .copied()
            .cycle()
            .take(WINDOWS_MEMORY_PROBE_SOURCE_BYTES.min(PARSER_MAX_SOURCE_BYTES as usize))
            .collect(),
    }
}

/// Admit every accepted grammar through its exact positive and negative fixtures.
///
/// The supplied supervisor must have been opened from the same artifact represented by
/// `logical`. One grammar-affined session is reused for its fixture pair, and every session
/// is explicitly shut down before this function returns.
///
/// # Errors
///
/// Returns the first artifact, containment, protocol, fixture-expectation, worker, timeout,
/// cancellation, or cleanup failure. When fixture execution and mandatory shutdown both fail,
/// both typed failures are retained in [`ParserSupervisorError::OperationAndCleanup`].
pub fn admit_optional_parser_artifact(
    mut supervisor: OptionalParserSupervisor,
    logical: &OptionalParserPackManifest,
) -> Result<(), ParserSupervisorError> {
    let cancellation = IndexCancellation::new();
    let limits = ParserRequestLimits::new(
        PARSER_MAX_OUTPUT_BYTES,
        PARSER_MAX_NODE_COUNT,
        PARSER_MAX_TREE_DEPTH,
    )?;
    let aggregate_deadline = Instant::now()
        .checked_add(ARTIFACT_ADMISSION_AGGREGATE_TIMEOUT)
        .ok_or(ParserSupervisorError::DeadlineExceeded {
            phase: "artifact admission aggregate deadline calculation",
        })?;
    let operation = (|| {
        for grammar in logical.grammars() {
            let grammar_deadline = Instant::now()
                .checked_add(ARTIFACT_ADMISSION_TIMEOUT)
                .ok_or(ParserSupervisorError::DeadlineExceeded {
                    phase: "artifact admission deadline calculation",
                })?;
            let deadline = grammar_deadline.min(aggregate_deadline);
            for (fixture, expected) in [
                (&grammar.fixtures.positive, false),
                (&grammar.fixtures.negative, true),
            ] {
                let evidence = supervisor.parse(
                    &grammar.language_id,
                    fixture.source.as_bytes(),
                    limits,
                    deadline,
                    ARTIFACT_ADMISSION_NO_PROGRESS_TIMEOUT,
                    &cancellation,
                )?;
                let actual = evidence.root_has_error();
                if actual != expected {
                    return Err(ParserSupervisorError::FixtureExpectationMismatch {
                        language_id: grammar.language_id.clone(),
                        case_name: fixture.case_name.clone(),
                        actual,
                        expected,
                    });
                }
            }
        }
        Ok(())
    })();
    let cleanup = supervisor.shutdown();
    match operation {
        Ok(()) => cleanup,
        Err(operation) => Err(attach_cleanup(operation, cleanup)),
    }
}

impl Drop for OptionalParserSupervisor {
    fn drop(&mut self) {
        drop(self.take_and_shutdown_resident());
    }
}

/// Execute the process-owning supervisor against the test-only hostile protocol peer.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn run_adversarial_process_suite(peer: &Path) -> io::Result<()> {
    #[derive(Clone, Copy)]
    enum ExpectedFailure {
        Cancelled,
        Deadline,
        InvalidAdmission,
        Io,
        NoProgress,
        Progress(&'static str),
        Ready(&'static str),
        Response(&'static str),
        Limit(&'static str),
        InvalidControl(ParserFrameKind),
        Worker(ParserFailureCode),
    }

    struct Case {
        scenario: &'static str,
        expected: ExpectedFailure,
        source_bytes: usize,
        cancel_before_launch: bool,
        cancellation_after_launch: Option<Duration>,
        deadline: Duration,
        deadline_after_launch: Option<Duration>,
        no_progress: Duration,
        limits: ParserRequestLimits,
    }

    fn default_limits() -> io::Result<ParserRequestLimits> {
        ParserRequestLimits::new(4 * 1024, 16, 16)
            .map_err(|error| io::Error::other(error.to_string()))
    }

    fn case(scenario: &'static str, expected: ExpectedFailure) -> io::Result<Case> {
        Ok(Case {
            scenario,
            expected,
            source_bytes: 32,
            cancel_before_launch: false,
            cancellation_after_launch: None,
            deadline: Duration::from_secs(2),
            deadline_after_launch: None,
            no_progress: Duration::from_millis(500),
            limits: default_limits()?,
        })
    }

    fn error_matches(error: &ParserSupervisorError, expected: ExpectedFailure) -> bool {
        match (error, expected) {
            (ParserSupervisorError::Cancelled { .. }, ExpectedFailure::Cancelled)
            | (ParserSupervisorError::DeadlineExceeded { .. }, ExpectedFailure::Deadline)
            | (ParserSupervisorError::InvalidAdmission, ExpectedFailure::InvalidAdmission)
            | (ParserSupervisorError::IoThread { .. }, ExpectedFailure::Io)
            | (ParserSupervisorError::NoProgress { .. }, ExpectedFailure::NoProgress) => true,
            (
                ParserSupervisorError::Protocol {
                    source: ParserProtocolError::ProgressRegression { field },
                },
                ExpectedFailure::Progress(expected),
            )
            | (
                ParserSupervisorError::Protocol {
                    source: ParserProtocolError::ReadyIdentityMismatch { field },
                },
                ExpectedFailure::Ready(expected),
            )
            | (
                ParserSupervisorError::Protocol {
                    source: ParserProtocolError::ResponseIdentityMismatch { field },
                },
                ExpectedFailure::Response(expected),
            )
            | (
                ParserSupervisorError::Protocol {
                    source: ParserProtocolError::RequestLimitExceeded { field, .. },
                },
                ExpectedFailure::Limit(expected),
            ) => field == &expected,
            (ParserSupervisorError::WorkerFailure { code }, ExpectedFailure::Worker(expected)) => {
                code == &expected
            }
            (
                ParserSupervisorError::Protocol {
                    source: ParserProtocolError::InvalidControlJson { kind, .. },
                },
                ExpectedFailure::InvalidControl(expected),
            ) => kind == &expected,
            _ => false,
        }
    }

    fn command_for(peer: &Path, scenario: &str) -> io::Result<Command> {
        let current_dir = peer
            .parent()
            .ok_or_else(|| io::Error::other("hostile peer path has no parent"))?;
        let mut command = Command::new(peer);
        command
            .arg("--peer")
            .arg(scenario)
            .current_dir(current_dir)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            use std::os::unix::process::CommandExt;

            command.process_group(0);
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            use std::os::windows::process::CommandExt;

            command.creation_flags(0x0800_0000);
        }
        Ok(command)
    }

    fn test_launch(peer: &Path) -> io::Result<VerifiedParserPackLaunch> {
        let pack_root = peer
            .parent()
            .ok_or_else(|| io::Error::other("hostile peer path has no parent"))?
            .to_path_buf();
        let platform = host_pack_platform()
            .ok_or_else(|| io::Error::other("host has no optional-parser containment target"))?;
        Ok(VerifiedParserPackLaunch {
            pack_root: pack_root.clone(),
            platform,
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            containment_broker: Some(peer.to_path_buf()),
            accepted_grammars: vec!["hostile".to_owned()],
            artifact: ParserArtifactIdentity::for_bytes(b"parser-supervisor-hostile-peer"),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            artifact_manifest_bytes: b"parser-supervisor-hostile-peer".to_vec(),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            accepted_manifest_bytes: Vec::new(),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            native_import_policy_bytes: Vec::new(),
            artifact_manifest: FileObservation::unavailable(
                pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
            ),
            payloads: Vec::new(),
            currentness_blocker: None,
        })
    }

    fn require_reused_resident_payload_revalidation(peer: &Path) -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let accepted_bytes = b"accepted-manifest";
        fs::write(
            temp.path().join(ARTIFACT_MANIFEST_FILE_NAME),
            b"parser-supervisor-hostile-peer",
        )?;
        fs::write(
            temp.path().join(ACCEPTED_MANIFEST_FILE_NAME),
            accepted_bytes,
        )?;
        let payload_path = temp.path().join("hostile-grammar");
        fs::write(&payload_path, b"trusted")?;
        let modified = fs::metadata(&payload_path)?.modified()?;

        let mut launch = test_launch(peer)?;
        launch.pack_root = temp.path().to_path_buf();
        launch.artifact_manifest =
            FileObservation::capture(temp.path().join(ARTIFACT_MANIFEST_FILE_NAME))
                .map_err(|error| io::Error::other(error.to_string()))?;
        launch.payloads = vec![PayloadObservation {
            file: FileObservation::capture(payload_path.clone())
                .map_err(|error| io::Error::other(error.to_string()))?,
            role: ParserPackPayloadRole::GrammarLibrary {
                language_id: "hostile".to_owned(),
            },
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            bytes: 7,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            sha256: encode_sha256(Sha256::digest(b"trusted")),
        }];
        let grammar = ParserLanguageIdentity::new("hostile")
            .map_err(|error| io::Error::other(error.to_string()))?;
        let cancellation = IndexCancellation::new();
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(2))
            .ok_or_else(|| io::Error::other("resident reuse deadline overflow"))?;
        let resident = ResidentParserSession::launch_command(
            &launch,
            grammar,
            ParserMemoryLimits::PRODUCTION,
            Instant::now(),
            deadline,
            Duration::from_secs(1),
            &cancellation,
            command_for(peer, "idle-close")?,
        )
        .map_err(|error| {
            io::Error::other(format!(
                "resident reuse test launch failed before mutation: {error:?}"
            ))
        })?;
        let mut supervisor = OptionalParserSupervisor {
            pack_root: temp.path().to_path_buf(),
            launch,
            memory_limits: ParserMemoryLimits::PRODUCTION,
            resident: Some(resident),
        };

        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        supervisor.launch.currentness_blocker = Some(std::sync::Arc::new(MetadataProbeBlocker {
            entered: entered_sender,
            release: std::sync::Mutex::new(release_receiver),
        }));
        let blocked_cancellation = IndexCancellation::new();
        let caller_cancellation = blocked_cancellation.clone();
        let blocked_deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .ok_or_else(|| io::Error::other("blocked currentness deadline overflow"))?;
        let blocked_limits = default_limits()?;
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let caller = thread::spawn(move || {
            let result = supervisor.parse(
                "hostile",
                &[b'x'; 32],
                blocked_limits,
                blocked_deadline,
                Duration::from_secs(5),
                &caller_cancellation,
            );
            let _send_result = result_sender.send((result, supervisor));
        });
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|source| io::Error::other(source.to_string()))?;
        blocked_cancellation.cancel();
        let (blocked_result, returned_supervisor) = result_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|source| io::Error::other(source.to_string()))?;
        if !matches!(
            blocked_result,
            Err(ParserSupervisorError::Cancelled {
                phase: ARTIFACT_IO_PHASE
            })
        ) {
            return Err(io::Error::other(format!(
                "blocked currentness returned the wrong result: {blocked_result:?}"
            )));
        }
        if returned_supervisor.resident.is_some() {
            return Err(io::Error::other(
                "blocked currentness retained the canceled resident",
            ));
        }
        if ArtifactIoLease::acquire().is_ok() {
            return Err(io::Error::other(
                "blocked currentness admitted a second artifact reader",
            ));
        }
        release_sender
            .send(())
            .map_err(|_closed| io::Error::other("metadata-probe release receiver closed"))?;
        caller
            .join()
            .map_err(|_panic| io::Error::other("blocked currentness caller panicked"))?;
        let permit_deadline = Instant::now() + Duration::from_secs(1);
        while ARTIFACT_IO_ACTIVE.load(Ordering::Acquire) && Instant::now() < permit_deadline {
            thread::yield_now();
        }
        if ARTIFACT_IO_ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "blocked currentness did not release the artifact reader",
            ));
        }

        supervisor = returned_supervisor;
        supervisor.launch.currentness_blocker = None;
        let cancellation = IndexCancellation::new();
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(2))
            .ok_or_else(|| io::Error::other("resident reuse deadline overflow"))?;
        let grammar = ParserLanguageIdentity::new("hostile")
            .map_err(|error| io::Error::other(error.to_string()))?;
        supervisor.resident = Some(
            ResidentParserSession::launch_command(
                &supervisor.launch,
                grammar,
                ParserMemoryLimits::PRODUCTION,
                Instant::now(),
                deadline,
                Duration::from_secs(1),
                &cancellation,
                command_for(peer, "idle-close")?,
            )
            .map_err(|error| {
                io::Error::other(format!(
                    "resident reuse test relaunch failed after blocked currentness: {error:?}"
                ))
            })?,
        );

        let mutation = fs::write(&payload_path, b"mutated");
        #[cfg(windows)]
        if mutation.is_err() {
            if fs::read(&payload_path)? != b"trusted" {
                return Err(io::Error::other(
                    "Windows write guard reported failure after changing payload bytes",
                ));
            }
            supervisor
                .shutdown()
                .map_err(|error| io::Error::other(error.to_string()))?;
            return Ok(());
        }
        mutation?;
        File::options()
            .write(true)
            .open(&payload_path)?
            .set_times(fs::FileTimes::new().set_modified(modified))?;
        if fs::metadata(&payload_path)?.len() != 7
            || fs::metadata(&payload_path)?.modified()? != modified
        {
            return Err(io::Error::other(
                "resident reuse mutation did not preserve size and modification time",
            ));
        }

        let source = vec![b'x'; 32];
        let operation = supervisor.parse(
            "hostile",
            &source,
            default_limits()?,
            deadline,
            Duration::from_millis(150),
            &cancellation,
        );
        let Err(error) = operation else {
            return Err(io::Error::other(
                "mutated launch payload was accepted by a reused resident",
            ));
        };
        if error.has_mandatory_cleanup_failure() {
            return Err(io::Error::other(format!(
                "mutated launch payload did not cleanly destroy the resident: {error:?}"
            )));
        }
        if supervisor.resident.is_some() {
            return Err(io::Error::other(
                "mutated launch payload retained the resident session",
            ));
        }
        Ok(())
    }

    fn operate(
        peer: &Path,
        case: &Case,
    ) -> Result<ParserCompletionEvidence, ParserSupervisorError> {
        let launch = test_launch(peer).map_err(|source| ParserSupervisorError::IoThread {
            phase: "adversarial test launch",
            message: source.to_string(),
        })?;
        let grammar = ParserLanguageIdentity::new("hostile")?;
        let cancellation = IndexCancellation::new();
        if case.cancel_before_launch {
            cancellation.cancel();
        }
        let now = Instant::now();
        let deadline = now.checked_add(case.deadline).unwrap_or(now);
        let resident = ResidentParserSession::launch_command(
            &launch,
            grammar,
            ParserMemoryLimits::PRODUCTION,
            now,
            deadline,
            case.no_progress,
            &cancellation,
            command_for(peer, case.scenario).map_err(|source| ParserSupervisorError::IoThread {
                phase: "adversarial test command",
                message: source.to_string(),
            })?,
        );
        let cancellation_thread;
        let result = match resident {
            Ok(mut resident) => {
                let source = vec![b'x'; case.source_bytes];
                let source_identity = ParserSourceIdentity::for_bytes(&source)?;
                let operation_started = Instant::now();
                let operation_deadline = case
                    .deadline_after_launch
                    .and_then(|duration| operation_started.checked_add(duration))
                    .unwrap_or(deadline);
                cancellation_thread = case.cancellation_after_launch.map(|delay| {
                    let cancellation = cancellation.clone();
                    thread::spawn(move || {
                        thread::sleep(delay);
                        cancellation.cancel();
                    })
                });
                let operation = resident.parse(
                    &source,
                    source_identity,
                    case.limits,
                    operation_started,
                    operation_deadline,
                    case.no_progress,
                    &cancellation,
                );
                match operation {
                    Ok(evidence) => resident.shutdown().map(|()| evidence),
                    Err(operation) => {
                        if operation.is_caller_stop() {
                            resident.termination_requested = true;
                        }
                        Err(attach_cleanup(operation, resident.shutdown()))
                    }
                }
            }
            Err(error) => {
                cancellation_thread = None;
                Err(error)
            }
        };
        if let Some(handle) = cancellation_thread {
            handle
                .join()
                .map_err(|_panic| ParserSupervisorError::Cleanup {
                    message: "adversarial cancellation thread panicked".to_owned(),
                })?;
        }
        result
    }

    fn require_failure(peer: &Path, hostile: &Case) -> io::Result<()> {
        let Err(error) = operate(peer, hostile) else {
            return Err(io::Error::other(format!(
                "hostile scenario {} unexpectedly succeeded",
                hostile.scenario
            )));
        };
        if error.has_mandatory_cleanup_failure() {
            return Err(io::Error::other(format!(
                "hostile scenario {} did not reap and join cleanly: {error:?}",
                hostile.scenario
            )));
        }
        if !error_matches(&error, hostile.expected) {
            return Err(io::Error::other(format!(
                "hostile scenario {} returned the wrong typed failure: {error:?}",
                hostile.scenario
            )));
        }

        let mut healthy = case("healthy", ExpectedFailure::Io)?;
        healthy.no_progress = healthy.deadline;
        let cleanup_deadline = Instant::now() + healthy.deadline;
        while PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) && Instant::now() < cleanup_deadline {
            thread::yield_now();
        }
        if PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::other(format!(
                "hostile scenario {} did not finish late process cleanup",
                hostile.scenario
            )));
        }
        require_process_spawn_cleanup_health()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let evidence = operate(peer, &healthy).map_err(|error| {
            io::Error::other(format!(
                "healthy restart after {} failed: {error:?}",
                hostile.scenario
            ))
        })?;
        if evidence.root_kind().as_str() != "source_file" {
            return Err(io::Error::other("healthy restart returned other evidence"));
        }
        Ok(())
    }

    require_reused_resident_payload_revalidation(peer)?;

    let mut cases = vec![
        {
            let mut opening_cancel = case("opening-cancel", ExpectedFailure::Cancelled)?;
            opening_cancel.cancel_before_launch = true;
            opening_cancel
        },
        case("pre-ready-stall", ExpectedFailure::NoProgress)?,
        case("ready-session", ExpectedFailure::Ready("session"))?,
        case("ready-artifact", ExpectedFailure::Ready("artifact"))?,
        case("ready-containment", ExpectedFailure::Ready("containment"))?,
        case(
            "ready-malformed",
            ExpectedFailure::InvalidControl(ParserFrameKind::Ready),
        )?,
        case("ready-truncated", ExpectedFailure::Io)?,
        case("ready-oversized", ExpectedFailure::Io)?,
        case("progress-session", ExpectedFailure::Response("session"))?,
        case("progress-request", ExpectedFailure::Response("request_id"))?,
        case("progress-duplicate", ExpectedFailure::Progress("sequence"))?,
        case("progress-gap", ExpectedFailure::Progress("sequence"))?,
        case(
            "progress-regression",
            ExpectedFailure::Progress("completed_work"),
        )?,
        case("progress-endless", ExpectedFailure::Deadline)?,
        case("progress-no-work", ExpectedFailure::NoProgress)?,
        case(
            "completion-malformed",
            ExpectedFailure::InvalidControl(ParserFrameKind::Completion),
        )?,
        case("completion-truncated", ExpectedFailure::Io)?,
        case("completion-oversized", ExpectedFailure::Io)?,
        case(
            "failure-exit",
            ExpectedFailure::Worker(ParserFailureCode::ParseRejected),
        )?,
        case("stderr-flood", ExpectedFailure::Io)?,
        case("stderr-completion", ExpectedFailure::Io)?,
        case(
            "limit-output",
            ExpectedFailure::Limit("completion.output_bytes"),
        )?,
        case(
            "limit-source",
            ExpectedFailure::Limit("evidence.root_end_byte"),
        )?,
        case(
            "limit-nodes",
            ExpectedFailure::Limit("evidence.named_node_count"),
        )?,
        case(
            "limit-depth",
            ExpectedFailure::Limit("evidence.maximum_depth"),
        )?,
    ];
    let mut blocked_cancel = case("blocked-write", ExpectedFailure::Cancelled)?;
    blocked_cancel.source_bytes = 4 * 1024 * 1024;
    blocked_cancel.cancellation_after_launch = Some(Duration::from_millis(75));
    blocked_cancel.no_progress = Duration::from_secs(1);
    cases.push(blocked_cancel);
    let mut blocked_deadline = case("blocked-write", ExpectedFailure::Deadline)?;
    blocked_deadline.source_bytes = 4 * 1024 * 1024;
    blocked_deadline.deadline_after_launch = Some(Duration::from_millis(125));
    blocked_deadline.no_progress = Duration::from_secs(1);
    cases.push(blocked_deadline);
    if let Some(progress_endless) = cases
        .iter_mut()
        .find(|candidate| candidate.scenario == "progress-endless")
    {
        progress_endless.deadline = Duration::from_millis(250);
        progress_endless.no_progress = Duration::from_secs(1);
    }
    if let Some(output_limit) = cases
        .iter_mut()
        .find(|candidate| candidate.scenario == "limit-output")
    {
        output_limit.limits = ParserRequestLimits::new(64, 16, 16)
            .map_err(|error| io::Error::other(error.to_string()))?;
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    cases.extend([
        case("admission-forged", ExpectedFailure::InvalidAdmission)?,
        case("admission-truncated", ExpectedFailure::Io)?,
        case("admission-stall", ExpectedFailure::NoProgress)?,
        case("admission-flood", ExpectedFailure::Io)?,
    ]);

    for hostile in &cases {
        require_failure(peer, hostile)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Protect bounded framing, backpressure, stop polling, and response identity.

    use std::io::Cursor;
    use std::sync::Arc;

    use projectatlas_core::optional_parser_protocol::{
        PARSER_MAX_SOURCE_BYTES, PARSER_PROTOCOL_VERSION, PARSER_WINDOWS_BROKER_ADMISSION_RECORD,
        ParserCompletion, ParserContentDigest, ParserResponseIdentity, ParserSyntaxKind,
    };

    use super::*;

    /// Serializes tests that deliberately hold the process-wide artifact-I/O lease.
    static ARTIFACT_IO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    /// Serializes the one test that deliberately blocks process creation.
    static PROCESS_SPAWN_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    /// Marks the subprocess branch of the blocked-spawn ownership test.
    const PROCESS_SPAWN_CHILD_ENV: &str = "PROJECTATLAS_PROCESS_SPAWN_CHILD";
    /// File written only if an abandoned child survives its mandatory cleanup.
    const PROCESS_SPAWN_CHILD_COMPLETED_ENV: &str = "PROJECTATLAS_PROCESS_SPAWN_CHILD_COMPLETED";

    /// Restores process-wide spawn health after an injected sticky-failure test.
    struct ProcessSpawnCleanupFailureReset;

    impl Drop for ProcessSpawnCleanupFailureReset {
        fn drop(&mut self) {
            if let Ok(mut slot) = PROCESS_SPAWN_CLEANUP_FAILURE.lock() {
                *slot = None;
            }
        }
    }

    /// Clear any one-shot process-spawn race hooks left by an early test return.
    struct ProcessSpawnTestHookReset;

    impl Drop for ProcessSpawnTestHookReset {
        fn drop(&mut self) {
            for slot in [
                &PROCESS_SPAWN_AFTER_RENDEZVOUS_TEST_HOOK,
                &PROCESS_SPAWN_AFTER_FINAL_CHECK_TEST_HOOK,
                &PROCESS_SPAWN_BEFORE_CLEANUP_TEST_HOOK,
            ] {
                if let Ok(mut hook) = slot.lock() {
                    *hook = None;
                }
            }
        }
    }

    #[test]
    fn blocked_process_spawn_child_fixture() {
        if std::env::var_os(PROCESS_SPAWN_CHILD_ENV).is_none() {
            return;
        }
        thread::sleep(Duration::from_secs(30));
        if let Some(path) = std::env::var_os(PROCESS_SPAWN_CHILD_COMPLETED_ENV) {
            let _write = fs::write(path, b"survived");
        }
    }

    /// Reader that injects one transient interrupted read.
    struct InterruptOnceReader {
        /// Remaining deterministic input bytes.
        input: Cursor<Vec<u8>>,
        /// Whether one transient interrupted read has already been injected.
        did_interrupt: bool,
    }

    impl Read for InterruptOnceReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.did_interrupt {
                self.did_interrupt = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.input.read(buffer)
        }
    }

    /// Reader that reports entry and blocks until the test releases it.
    struct BlockingReader {
        /// Signals that the worker entered the in-flight read.
        entered: SyncSender<()>,
        /// Releases the deliberately stalled read.
        release: Receiver<()>,
    }

    impl Read for BlockingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.entered
                .send(())
                .map_err(|_closed| io::Error::other("blocking reader entry receiver closed"))?;
            self.release
                .recv()
                .map_err(|_closed| io::Error::other("blocking reader release sender closed"))?;
            buffer[0] = b'x';
            Ok(1)
        }
    }

    /// Build process-free verified launch metadata for focused supervisor tests.
    fn metadata_only_launch() -> VerifiedParserPackLaunch {
        let pack_root = PathBuf::from("metadata-only-pack");
        VerifiedParserPackLaunch {
            pack_root: pack_root.clone(),
            platform: PackPlatform::LinuxX86_64,
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            containment_broker: Some(pack_root.join("projectatlas-parser-containment.exe")),
            accepted_grammars: vec!["alpha".to_owned(), "zeta".to_owned()],
            artifact: ParserArtifactIdentity::for_bytes(b"artifact"),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            artifact_manifest_bytes: b"artifact".to_vec(),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            accepted_manifest_bytes: Vec::new(),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            native_import_policy_bytes: Vec::new(),
            artifact_manifest: FileObservation::unavailable(
                pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
            ),
            payloads: Vec::new(),
            currentness_blocker: None,
        }
    }

    /// Build a process-free supervisor value for public metadata delegation tests.
    fn metadata_only_supervisor() -> OptionalParserSupervisor {
        let launch = metadata_only_launch();
        OptionalParserSupervisor {
            pack_root: launch.pack_root.clone(),
            launch,
            memory_limits: ParserMemoryLimits::PRODUCTION,
            resident: None,
        }
    }

    #[test]
    fn supervisor_exposes_only_verified_artifact_and_language_metadata() {
        let supervisor = metadata_only_supervisor();
        assert_eq!(
            supervisor.artifact_identity(),
            &ParserArtifactIdentity::for_bytes(b"artifact")
        );
        assert!(supervisor.accepts_language("alpha"));
        assert!(supervisor.accepts_language("zeta"));
        assert!(!supervisor.accepts_language("missing"));
        assert!(!supervisor.accepts_language("INVALID"));
    }

    #[test]
    fn supervisor_rejects_artifact_identity_change_inside_selected_slot() {
        let mut supervisor = metadata_only_supervisor();
        let selected = supervisor.artifact_identity().clone();
        let mut replacement = metadata_only_launch();
        replacement.artifact = ParserArtifactIdentity::for_bytes(b"replacement-artifact");

        assert!(matches!(
            supervisor.replace_verified_launch(replacement),
            Err(ParserSupervisorError::PayloadMismatch {
                reason: "artifact identity changed inside its immutable slot",
                ..
            })
        ));
        assert_eq!(supervisor.artifact_identity(), &selected);
    }

    #[test]
    fn bounded_artifact_read_retries_interruption() -> Result<(), Box<dyn std::error::Error>> {
        let expected = vec![b'x'; ARTIFACT_READ_CHUNK_BYTES * 3];
        let expected_sha256 = encode_sha256(Sha256::digest(&expected));
        let mut reader = InterruptOnceReader {
            input: Cursor::new(expected.clone()),
            did_interrupt: false,
        };
        let mut bytes = Vec::new();
        let mut sha256 = Sha256::new();
        read_bounded_chunks(
            &mut reader,
            Path::new("changed-payload"),
            u64::try_from(ARTIFACT_READ_CHUNK_BYTES * 3)?,
            &mut bytes,
            &mut sha256,
            None,
        )?;

        require_test(
            bytes == expected,
            "bounded artifact read changed bytes after an interrupted read",
        )?;
        require_test(
            encode_sha256(sha256.finalize()) == expected_sha256,
            "bounded artifact read changed its digest after an interrupted read",
        )
        .map_err(Into::into)
    }

    #[test]
    fn changed_artifact_reload_returns_while_reader_is_blocked()
    -> Result<(), Box<dyn std::error::Error>> {
        let _test_guard = ARTIFACT_IO_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("artifact I/O test lock was poisoned"))?;
        let cancellation = IndexCancellation::new();
        let started = Instant::now();
        let absolute_deadline = started + Duration::from_secs(5);
        let no_progress_timeout = Duration::from_secs(5);
        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (finished_sender, finished_receiver) = mpsc::sync_channel(1);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let caller_cancellation = cancellation.clone();
        let worker_cancellation = cancellation.clone();
        let caller = thread::spawn(move || {
            let control = ArtifactIoControl {
                absolute_deadline,
                last_progress: started,
                no_progress_timeout,
                cancellation: &caller_cancellation,
            };
            let result = run_bounded_artifact_io(
                move || {
                    let worker_control = ArtifactIoControl {
                        absolute_deadline,
                        last_progress: started,
                        no_progress_timeout,
                        cancellation: &worker_cancellation,
                    };
                    let mut reader = BlockingReader {
                        entered: entered_sender,
                        release: release_receiver,
                    };
                    let mut bytes = Vec::new();
                    let mut sha256 = Sha256::new();
                    let result = read_bounded_chunks(
                        &mut reader,
                        Path::new("blocked-payload"),
                        u64::try_from(ARTIFACT_READ_CHUNK_BYTES).unwrap_or(u64::MAX),
                        &mut bytes,
                        &mut sha256,
                        Some(&worker_control),
                    );
                    let worker_cancelled = matches!(
                        &result,
                        Err(ParserSupervisorError::Cancelled {
                            phase: ARTIFACT_IO_PHASE
                        })
                    );
                    let _finished_result = finished_sender.send(worker_cancelled);
                    result
                },
                &control,
            );
            let _result_send = result_sender.send(result);
        });

        let entered = entered_receiver.recv_timeout(Duration::from_secs(1));
        cancellation.cancel();
        let result = result_receiver.recv_timeout(Duration::from_secs(1));
        let returned_cancelled = matches!(
            result.as_ref(),
            Ok(Err(ParserSupervisorError::Cancelled {
                phase: ARTIFACT_IO_PHASE
            }))
        );
        let refused_second_reader = matches!(
            ArtifactIoLease::acquire(),
            Err(ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                ..
            })
        );

        let release_result = release_sender.send(());
        caller
            .join()
            .map_err(|_panic| io::Error::other("artifact revalidation caller panicked"))?;
        let worker_cancelled = finished_receiver.recv_timeout(Duration::from_secs(1));
        let permit_deadline = Instant::now() + Duration::from_secs(1);
        while ARTIFACT_IO_ACTIVE.load(Ordering::Acquire) && Instant::now() < permit_deadline {
            thread::yield_now();
        }

        entered.map_err(|source| io::Error::other(source.to_string()))?;
        let _returned = result.map_err(|source| io::Error::other(source.to_string()))?;
        release_result?;
        let worker_cancelled =
            worker_cancelled.map_err(|source| io::Error::other(source.to_string()))?;
        require_test(
            returned_cancelled,
            "blocked artifact read retained the canceled request",
        )?;
        require_test(
            refused_second_reader,
            "blocked artifact read permitted another reload worker",
        )?;
        require_test(
            worker_cancelled,
            "released artifact reader continued after request cancellation",
        )?;
        require_test(
            ArtifactIoLease::acquire().is_ok(),
            "artifact reader permit was not reusable after worker completion",
        )
        .map_err(Into::into)
    }

    #[test]
    fn currentness_probe_returns_while_path_observer_is_blocked()
    -> Result<(), Box<dyn std::error::Error>> {
        let _test_guard = ARTIFACT_IO_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("artifact I/O test lock was poisoned"))?;
        let cancellation = IndexCancellation::new();
        let started = Instant::now();
        let absolute_deadline = started + Duration::from_secs(5);
        let no_progress_timeout = Duration::from_secs(5);
        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let temp = tempfile::tempdir()?;
        let observed_path = temp.path().join("artifact.json");
        fs::write(&observed_path, b"verified")?;
        let observation = FileObservation::capture(observed_path)?;
        let mut file_probe = observation.currentness_probe();
        file_probe.blocker = Some(std::sync::Arc::new(MetadataProbeBlocker {
            entered: entered_sender,
            release: std::sync::Mutex::new(release_receiver),
        }));
        let probe = ArtifactCurrentnessProbe {
            files: vec![file_probe],
        };
        let caller_cancellation = cancellation.clone();
        let caller = thread::spawn(move || {
            let control = ArtifactIoControl {
                absolute_deadline,
                last_progress: started,
                no_progress_timeout,
                cancellation: &caller_cancellation,
            };
            let result = run_bounded_artifact_currentness(probe, &control);
            let _result_send = result_sender.send(result);
        });

        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|source| io::Error::other(source.to_string()))?;
        cancellation.cancel();
        let result = result_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|source| io::Error::other(source.to_string()))?;
        let returned_cancelled = matches!(
            result,
            Err(ParserSupervisorError::Cancelled {
                phase: ARTIFACT_IO_PHASE
            })
        );
        let refused_second_reader = matches!(
            ArtifactIoLease::acquire(),
            Err(ParserSupervisorError::IoThread {
                phase: ARTIFACT_IO_PHASE,
                ..
            })
        );
        release_sender.send(())?;
        caller
            .join()
            .map_err(|_panic| io::Error::other("currentness caller panicked"))?;
        let permit_deadline = Instant::now() + Duration::from_secs(1);
        while ARTIFACT_IO_ACTIVE.load(Ordering::Acquire) && Instant::now() < permit_deadline {
            thread::yield_now();
        }

        require_test(
            returned_cancelled,
            "blocked pathname observation retained the canceled request",
        )?;
        require_test(
            refused_second_reader,
            "blocked pathname observation permitted another artifact reader",
        )?;
        require_test(
            ArtifactIoLease::acquire().is_ok(),
            "artifact reader permit was not reusable after currentness completion",
        )
        .map_err(Into::into)
    }

    #[test]
    fn changed_artifact_refresh_propagates_request_cancellation() {
        let mut supervisor = metadata_only_supervisor();
        let cancellation = IndexCancellation::new();
        cancellation.cancel();

        assert!(matches!(
            supervisor.refresh_changed_artifact(
                "alpha",
                Instant::now(),
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
                &cancellation,
            ),
            Err(ParserSupervisorError::Cancelled {
                phase: ARTIFACT_IO_PHASE
            })
        ));
    }

    #[test]
    fn controlled_artifact_currentness_polls_before_metadata() {
        let launch = metadata_only_launch();
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let control = ArtifactIoControl {
            absolute_deadline: Instant::now() + Duration::from_secs(1),
            last_progress: Instant::now(),
            no_progress_timeout: Duration::from_secs(1),
            cancellation: &cancellation,
        };

        assert!(matches!(
            launch.currentness_probe("alpha").is_current(Some(&control)),
            Err(ParserSupervisorError::Cancelled {
                phase: ARTIFACT_IO_PHASE
            })
        ));
    }

    #[test]
    fn stopped_process_launch_never_invokes_spawn() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PROCESS_SPAWN_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("process-spawn test lock is poisoned"))?;
        require_process_spawn_cleanup_health()?;
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let invoked = Arc::new(AtomicBool::new(false));
        let spawn_invoked = Arc::clone(&invoked);
        let started = Instant::now();
        let result = run_bounded_process_spawn_with(
            Command::new(std::env::current_exe()?),
            started + Duration::from_secs(1),
            started,
            Duration::from_secs(1),
            &cancellation,
            move |_command| {
                spawn_invoked.store(true, Ordering::Release);
                Err(io::Error::other("stopped launch invoked spawn"))
            },
        );
        if !matches!(
            result,
            Err(ParserSupervisorError::Cancelled {
                phase: PROCESS_LAUNCH_PHASE
            })
        ) {
            return Err(io::Error::other(format!(
                "stopped process launch returned the wrong result: {result:?}"
            ))
            .into());
        }
        if invoked.load(Ordering::Acquire) {
            return Err(io::Error::other("stopped process launch invoked spawn").into());
        }
        Ok(())
    }

    #[test]
    fn sticky_late_cleanup_failure_refuses_future_spawn() -> Result<(), Box<dyn std::error::Error>>
    {
        let _guard = PROCESS_SPAWN_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("process-spawn test lock is poisoned"))?;
        require_process_spawn_cleanup_health()?;
        let _reset = ProcessSpawnCleanupFailureReset;
        record_process_spawn_cleanup_failure(&ParserSupervisorError::Cleanup {
            message: "injected cleanup failure".to_owned(),
        });
        let invoked = Arc::new(AtomicBool::new(false));
        let spawn_invoked = Arc::clone(&invoked);
        let cancellation = IndexCancellation::new();
        let started = Instant::now();
        let result = run_bounded_process_spawn_with(
            Command::new(std::env::current_exe()?),
            started + Duration::from_secs(1),
            started,
            Duration::from_secs(1),
            &cancellation,
            move |_command| {
                spawn_invoked.store(true, Ordering::Release);
                Err(io::Error::other("unhealthy launch invoked spawn"))
            },
        );
        let message = match result {
            Err(ParserSupervisorError::Cleanup { message }) => message,
            other => {
                return Err(io::Error::other(format!(
                    "sticky cleanup failure returned the wrong result: {other:?}"
                ))
                .into());
            }
        };
        if !message.contains("injected cleanup failure") {
            return Err(io::Error::other("sticky cleanup failure lost its diagnostic").into());
        }
        if invoked.load(Ordering::Acquire) {
            return Err(io::Error::other("unhealthy process launch invoked spawn").into());
        }
        Ok(())
    }

    #[test]
    fn blocked_process_spawn_releases_caller_and_reaps_late_child()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PROCESS_SPAWN_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("process-spawn test lock is poisoned"))?;
        require_process_spawn_cleanup_health()?;
        let temp = tempfile::tempdir()?;
        let completed = temp.path().join("child-completed");
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--exact")
            .arg("parser_supervisor::tests::blocked_process_spawn_child_fixture")
            .arg("--nocapture")
            .env(PROCESS_SPAWN_CHILD_ENV, "1")
            .env(PROCESS_SPAWN_CHILD_COMPLETED_ENV, &completed)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (pid_sender, pid_receiver) = mpsc::sync_channel(1);
        let cancellation = IndexCancellation::new();
        let caller_cancellation = cancellation.clone();
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let started = Instant::now();
        let caller = thread::spawn(move || {
            let result = run_bounded_process_spawn_with(
                command,
                started + Duration::from_secs(5),
                started,
                Duration::from_secs(5),
                &caller_cancellation,
                move |mut command| {
                    let child = command.spawn()?;
                    pid_sender
                        .send(child.id())
                        .map_err(|_closed| io::Error::other("spawn PID receiver closed"))?;
                    entered_sender
                        .send(())
                        .map_err(|_closed| io::Error::other("spawn blocker receiver closed"))?;
                    release_receiver
                        .recv()
                        .map_err(|_closed| io::Error::other("spawn release sender closed"))?;
                    Ok(child)
                },
            );
            let _send = result_sender.send(result);
        });

        entered_receiver.recv_timeout(Duration::from_secs(5))?;
        let _late_child_pid = pid_receiver.recv_timeout(Duration::from_secs(1))?;
        cancellation.cancel();
        let result = result_receiver.recv_timeout(Duration::from_secs(1))?;
        match result {
            Err(ParserSupervisorError::Cancelled {
                phase: PROCESS_LAUNCH_PHASE,
            }) => {}
            other => {
                return Err(io::Error::other(format!(
                    "blocked process spawn returned the wrong result: {other:?}"
                ))
                .into());
            }
        }
        if ProcessSpawnLease::acquire().is_ok() {
            return Err(io::Error::other(
                "blocked process spawn released its process-wide lease early",
            )
            .into());
        }
        release_sender.send(())?;
        caller
            .join()
            .map_err(|_panic| io::Error::other("blocked-spawn caller panicked"))?;

        let cleanup_deadline = Instant::now() + SUPERVISOR_CLEANUP_TIMEOUT;
        while PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) && Instant::now() < cleanup_deadline {
            thread::yield_now();
        }
        if PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "late process spawn did not release its process-wide lease",
            )
            .into());
        }
        require_process_spawn_cleanup_health()?;
        if completed.exists() {
            return Err(io::Error::other("late child survived mandatory cleanup").into());
        }
        drop(ProcessSpawnLease::acquire()?);
        Ok(())
    }

    #[test]
    fn completed_process_spawn_cancellation_detaches_after_rendezvous()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PROCESS_SPAWN_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("process-spawn test lock is poisoned"))?;
        require_process_spawn_cleanup_health()?;
        let _hook_reset = ProcessSpawnTestHookReset;
        let temp = tempfile::tempdir()?;
        let completed = temp.path().join("completed-spawn-child-completed");
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--exact")
            .arg("parser_supervisor::tests::blocked_process_spawn_child_fixture")
            .arg("--nocapture")
            .env(PROCESS_SPAWN_CHILD_ENV, "1")
            .env(PROCESS_SPAWN_CHILD_COMPLETED_ENV, &completed)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let (rendezvous_entered_sender, rendezvous_entered_receiver) = mpsc::sync_channel(1);
        let (release_rendezvous_sender, release_rendezvous_receiver) = mpsc::sync_channel(1);
        *PROCESS_SPAWN_AFTER_RENDEZVOUS_TEST_HOOK
            .lock()
            .map_err(|_poisoned| io::Error::other("rendezvous test hook lock is poisoned"))? =
            Some(Box::new(move || {
                let _entered = rendezvous_entered_sender.send(());
                let _release = release_rendezvous_receiver.recv();
            }));

        let (cleanup_entered_sender, cleanup_entered_receiver) = mpsc::sync_channel(1);
        let (release_cleanup_sender, release_cleanup_receiver) = mpsc::sync_channel(1);
        *PROCESS_SPAWN_BEFORE_CLEANUP_TEST_HOOK
            .lock()
            .map_err(|_poisoned| io::Error::other("cleanup test hook lock is poisoned"))? =
            Some(Box::new(move || {
                let thread_name = thread::current().name().unwrap_or("unnamed").to_owned();
                let _entered = cleanup_entered_sender.send(thread_name);
                let _release = release_cleanup_receiver.recv();
            }));

        let (pid_sender, pid_receiver) = mpsc::sync_channel(1);
        let cancellation = IndexCancellation::new();
        let caller_cancellation = cancellation.clone();
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let started = Instant::now();
        let caller = thread::spawn(move || {
            let result = run_bounded_process_spawn_with(
                command,
                started + Duration::from_secs(5),
                started,
                Duration::from_secs(5),
                &caller_cancellation,
                move |mut command| {
                    let child = command.spawn()?;
                    pid_sender
                        .send(child.id())
                        .map_err(|_closed| io::Error::other("spawn PID receiver closed"))?;
                    Ok(child)
                },
            );
            let _send = result_sender.send(result);
        });

        rendezvous_entered_receiver.recv_timeout(Duration::from_secs(5))?;
        let _child_pid = pid_receiver.recv_timeout(Duration::from_secs(1))?;
        cancellation.cancel();
        release_rendezvous_sender.send(())?;
        let result = result_receiver.recv_timeout(Duration::from_secs(1))?;
        match result {
            Err(ParserSupervisorError::Cancelled {
                phase: PROCESS_LAUNCH_PHASE,
            }) => {}
            other => {
                return Err(io::Error::other(format!(
                    "completed process spawn returned the wrong result: {other:?}"
                ))
                .into());
            }
        }
        caller
            .join()
            .map_err(|_panic| io::Error::other("completed-spawn caller panicked"))?;

        let cleanup_thread = cleanup_entered_receiver.recv_timeout(Duration::from_secs(1))?;
        if cleanup_thread != "projectatlas-process-spawn" {
            return Err(io::Error::other(format!(
                "unadmitted child cleanup ran on {cleanup_thread:?}"
            ))
            .into());
        }
        if !PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) || ProcessSpawnLease::acquire().is_ok() {
            return Err(io::Error::other(
                "unadmitted child cleanup released its process-wide lease early",
            )
            .into());
        }
        if completed.exists() {
            return Err(io::Error::other("unadmitted child survived before cleanup").into());
        }

        release_cleanup_sender.send(())?;
        let cleanup_deadline = Instant::now() + SUPERVISOR_CLEANUP_TIMEOUT;
        while PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) && Instant::now() < cleanup_deadline {
            thread::yield_now();
        }
        if PROCESS_SPAWN_ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "unadmitted child cleanup did not release its process-wide lease",
            )
            .into());
        }
        require_process_spawn_cleanup_health()?;
        if completed.exists() {
            return Err(io::Error::other("unadmitted child survived mandatory cleanup").into());
        }
        drop(ProcessSpawnLease::acquire()?);
        Ok(())
    }

    #[test]
    fn launch_command_cancellation_after_process_spawn_commit_returns_after_child_cleanup()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PROCESS_SPAWN_TEST_LOCK
            .lock()
            .map_err(|_poisoned| io::Error::other("process-spawn test lock is poisoned"))?;
        require_process_spawn_cleanup_health()?;
        let _hook_reset = ProcessSpawnTestHookReset;
        let temp = tempfile::tempdir()?;
        let completed = temp.path().join("post-commit-child-completed");
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--exact")
            .arg("parser_supervisor::tests::blocked_process_spawn_child_fixture")
            .arg("--nocapture")
            .env(PROCESS_SPAWN_CHILD_ENV, "1")
            .env(PROCESS_SPAWN_CHILD_COMPLETED_ENV, &completed)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let started = Instant::now();
        let cancellation = IndexCancellation::new();
        let hook_cancellation = cancellation.clone();
        *PROCESS_SPAWN_AFTER_FINAL_CHECK_TEST_HOOK
            .lock()
            .map_err(|_poisoned| io::Error::other("final-check test hook lock is poisoned"))? =
            Some(Box::new(move || hook_cancellation.cancel()));
        let result = ResidentParserSession::launch_command(
            &metadata_only_launch(),
            ParserLanguageIdentity::new("alpha")?,
            ParserMemoryLimits::PRODUCTION,
            started,
            started + Duration::from_secs(5),
            Duration::from_secs(5),
            &cancellation,
            command,
        );
        if !cancellation.is_cancelled() {
            return Err(io::Error::other(
                "final-check test did not cancel after ownership commitment",
            )
            .into());
        }
        match result {
            Err(ParserSupervisorError::Cancelled {
                phase: PROCESS_LAUNCH_PHASE,
            }) => {}
            Err(other) => {
                return Err(io::Error::other(format!(
                    "post-commit launch cancellation returned the wrong error: {other:?}"
                ))
                .into());
            }
            Ok(resident) => {
                resident.shutdown()?;
                return Err(io::Error::other(
                    "post-commit launch cancellation returned a resident session",
                )
                .into());
            }
        }
        drop(ProcessSpawnLease::acquire()?);
        require_process_spawn_cleanup_health()?;
        if completed.exists() {
            return Err(io::Error::other("post-commit child survived launch cleanup").into());
        }
        Ok(())
    }

    #[test]
    fn partial_launch_cleanup_releases_full_reader_channels()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let completed = temp.path().join("partial-launch-child-completed");
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("parser_supervisor::tests::blocked_process_spawn_child_fixture")
            .arg("--nocapture")
            .env(PROCESS_SPAWN_CHILD_ENV, "1")
            .env(PROCESS_SPAWN_CHILD_COMPLETED_ENV, &completed)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let (frame_sender, frame_events) = mpsc::sync_channel(1);
        let (frame_entered_sender, frame_entered_receiver) = mpsc::sync_channel(1);
        let frame_handle = thread::spawn(move || {
            let _first = frame_sender.send(FrameReaderEvent::Frame(vec![1]));
            let _entered = frame_entered_sender.send(());
            let _second = frame_sender.send(FrameReaderEvent::Frame(vec![2]));
        });
        let (diagnostic_sender, diagnostic_events) = mpsc::sync_channel(1);
        let (diagnostic_entered_sender, diagnostic_entered_receiver) = mpsc::sync_channel(1);
        let diagnostic_handle = thread::spawn(move || {
            let _first = diagnostic_sender.send(DiagnosticReaderEvent::AdmissionAccepted);
            let _entered = diagnostic_entered_sender.send(());
            let _second = diagnostic_sender.send(DiagnosticReaderEvent::AdmissionAccepted);
            Ok(Vec::new())
        });
        frame_entered_receiver.recv_timeout(Duration::from_secs(1))?;
        diagnostic_entered_receiver.recv_timeout(Duration::from_secs(1))?;

        cleanup_partial_launch(
            &mut child,
            vec![frame_handle],
            Some(diagnostic_handle),
            Some(frame_events),
            Some(diagnostic_events),
        )?;
        if completed.exists() {
            return Err(io::Error::other("partial-launch child survived cleanup").into());
        }
        Ok(())
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn memfd_creation_retries_only_unsupported_modern_flags() {
        use nix::errno::Errno;
        use nix::libc;
        use nix::sys::memfd::MFdFlags;

        let base = MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING;
        let requested = base | MFdFlags::from_bits_retain(libc::MFD_EXEC);
        let mut attempts = Vec::new();
        let descriptor = create_memfd_with_legacy_fallback(base, libc::MFD_EXEC, |flags| {
            attempts.push(flags);
            if attempts.len() == 1 {
                Err(Errno::EINVAL)
            } else {
                Ok(7)
            }
        });
        assert_eq!(descriptor, Ok(7));
        assert_eq!(attempts, vec![requested, base]);

        let mut denied_attempts = Vec::new();
        let denied = create_memfd_with_legacy_fallback(base, libc::MFD_EXEC, |flags| {
            denied_attempts.push(flags);
            Err::<i32, _>(Errno::EPERM)
        });
        assert_eq!(denied, Err(Errno::EPERM));
        assert_eq!(denied_attempts, vec![requested]);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn sealed_linux_payload_is_read_only_complete_and_immutable()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        use nix::fcntl::{FcntlArg, OFlag, SealFlag, fcntl};

        let cancellation = IndexCancellation::new();
        let control = ArtifactIoControl {
            absolute_deadline: Instant::now() + Duration::from_secs(1),
            last_progress: Instant::now(),
            no_progress_timeout: Duration::from_secs(1),
            cancellation: &cancellation,
        };
        let payload = SealedLinuxPayload::from_verified_bytes(
            "test payload",
            "projectatlas-sealed-payload-test",
            b"verified authority",
            false,
            &control,
        )?;
        require_test(
            payload.file.metadata()?.permissions().mode() & 0o777 == 0o400,
            "sealed document authority retained executable or writable mode bits",
        )?;
        let status = fcntl(&payload.file, FcntlArg::F_GETFL)?;
        require_test(
            status & OFlag::O_ACCMODE.bits() == OFlag::O_RDONLY.bits(),
            "sealed document authority was not reopened read-only",
        )?;
        let seals = fcntl(&payload.file, FcntlArg::F_GET_SEALS)?;
        let required = SealFlag::F_SEAL_WRITE
            | SealFlag::F_SEAL_GROW
            | SealFlag::F_SEAL_SHRINK
            | SealFlag::F_SEAL_SEAL;
        require_test(
            seals & required.bits() == required.bits(),
            "sealed document authority omitted a required seal",
        )?;

        let mut reader = payload.file.try_clone()?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        require_test(
            bytes == b"verified authority",
            "sealed document authority changed after reopening",
        )?;
        require_test(
            payload.file.set_len(0).is_err(),
            "sealed document authority allowed truncation",
        )?;
        let mut writer = payload.file.try_clone()?;
        require_test(
            writer.write_all(b"attacker").is_err(),
            "sealed document authority allowed mutation",
        )?;

        let executable = SealedLinuxPayload::from_verified_bytes(
            "test worker",
            "projectatlas-sealed-worker-test",
            b"verified executable authority",
            true,
            &control,
        )?;
        require_test(
            executable.file.metadata()?.permissions().mode() & 0o777 == 0o500,
            "sealed executable authority retained writable or unexpected mode bits",
        )?;
        Ok(())
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn sealed_linux_payload_legacy_fallback_preserves_exact_modes()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        use nix::errno::Errno;
        use nix::libc;
        use nix::sys::memfd::{MFdFlags, memfd_create};

        let cancellation = IndexCancellation::new();
        let control = ArtifactIoControl {
            absolute_deadline: Instant::now() + Duration::from_secs(1),
            last_progress: Instant::now(),
            no_progress_timeout: Duration::from_secs(1),
            cancellation: &cancellation,
        };
        let base = MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING;
        for (name, executable, expected_mode) in [
            ("projectatlas-legacy-document-test", false, 0o400),
            ("projectatlas-legacy-executable-test", true, 0o500),
        ] {
            let mode_flag = if executable {
                libc::MFD_EXEC
            } else {
                libc::MFD_NOEXEC_SEAL
            };
            let mut attempts = Vec::new();
            let payload = SealedLinuxPayload::from_verified_bytes_with_create(
                "legacy payload",
                name,
                b"verified authority",
                executable,
                &control,
                |flags| {
                    attempts.push(flags);
                    if attempts.len() == 1 {
                        Err(Errno::EINVAL)
                    } else {
                        // A modern test kernel may make base-only memfds non-executable;
                        // model the legacy kernel's executable-by-default fallback inode.
                        memfd_create(name, flags | MFdFlags::from_bits_retain(libc::MFD_EXEC))
                    }
                },
            )?;
            require_test(
                attempts == vec![base | MFdFlags::from_bits_retain(mode_flag), base],
                "legacy fallback did not retry exactly once with base flags",
            )?;
            require_test(
                payload.file.metadata()?.permissions().mode() & 0o777 == expected_mode,
                "legacy fallback retained an unexpected payload mode",
            )?;
        }
        Ok(())
    }

    #[test]
    fn payload_observation_detects_same_size_same_mtime_change_epoch()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("worker");
        fs::write(&path, b"trusted")?;
        let modified = fs::metadata(&path)?.modified()?;
        let observation = PayloadObservation {
            file: FileObservation::capture(path.clone())?,
            role: ParserPackPayloadRole::Worker,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            bytes: 7,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            sha256: encode_sha256(Sha256::digest(b"trusted")),
        };
        require_test(observation.is_current()?, "initial payload was rejected")?;

        let mutation = fs::write(&path, b"mutated");
        #[cfg(windows)]
        if mutation.is_err() {
            require_test(
                observation.is_current()? && fs::read(&path)? == b"trusted",
                "Windows write guard reported failure after changing payload identity",
            )?;
            return Ok(());
        }
        mutation?;
        File::options()
            .write(true)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(modified))?;
        require_test(
            fs::metadata(&path)?.len() == observation.file.epoch.bytes
                && fs::metadata(&path)?.modified()? == modified,
            "test mutation did not preserve size and modification time",
        )?;
        require_test(
            !observation.is_current()?,
            "same-size same-mtime payload mutation retained launch authority",
        )?;
        Ok(())
    }

    #[test]
    fn bounded_artifact_read_rejects_mismatched_captured_epoch()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("worker");
        fs::write(&path, b"trusted")?;
        let observation = FileObservation::capture(path.clone())?;

        #[cfg(unix)]
        let expected_epoch = {
            let replacement = temp.path().join("replacement");
            let retained = temp.path().join("retained");
            fs::write(&replacement, b"mutated")?;
            fs::rename(&path, retained)?;
            fs::rename(replacement, &path)?;
            observation.epoch
        };
        #[cfg(not(unix))]
        let expected_epoch = {
            let _write_guard = &observation;
            FileChangeEpoch::default()
        };

        let Err(error) = read_bounded_file(&path, expected_epoch, 7, None) else {
            return Err(io::Error::other(
                "replacement bytes were accepted under the captured epoch",
            )
            .into());
        };
        require_test(
            matches!(
                error,
                ParserSupervisorError::PayloadMismatch {
                    reason: "artifact read handle does not match the captured file identity",
                    ..
                }
            ),
            "replacement read did not fail on the captured file epoch",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_observation_releases_delete_share_on_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("guarded-payload");
        fs::write(&path, b"trusted")?;
        let observation = FileObservation::capture(path.clone())?;

        require_test(
            fs::remove_file(&path).is_err() && path.is_file(),
            "Windows observation did not deny payload deletion",
        )?;
        drop(observation);
        fs::remove_file(&path)?;
        require_test(
            !path.exists(),
            "dropping Windows observation did not release payload deletion",
        )?;
        Ok(())
    }

    #[test]
    fn payload_observation_revalidates_only_launch_inputs() {
        let observation = |role| PayloadObservation {
            file: FileObservation::unavailable(PathBuf::new()),
            role,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            bytes: 0,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            sha256: String::new(),
        };

        assert!(observation(ParserPackPayloadRole::Worker).contributes_to_launch("rust"));
        assert!(
            observation(ParserPackPayloadRole::ContainmentBroker).contributes_to_launch("rust")
        );
        assert!(observation(ParserPackPayloadRole::AcceptedManifest).contributes_to_launch("rust"));
        assert!(
            observation(ParserPackPayloadRole::GrammarLibrary {
                language_id: "rust".to_owned(),
            })
            .contributes_to_launch("rust")
        );
        assert!(
            !observation(ParserPackPayloadRole::GrammarLibrary {
                language_id: "python".to_owned(),
            })
            .contributes_to_launch("rust")
        );
        assert!(
            !observation(ParserPackPayloadRole::NativeAuditReport).contributes_to_launch("rust")
        );
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert!(
            observation(ParserPackPayloadRole::NativeImportPolicy).contributes_to_launch("rust")
        );
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        assert!(
            !observation(ParserPackPayloadRole::NativeImportPolicy).contributes_to_launch("rust")
        );
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn linux_currentness_probe_detects_native_policy_drift()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let artifact_path = temp.path().join(ARTIFACT_MANIFEST_FILE_NAME);
        let policy_path = temp.path().join("native-policy");
        fs::write(&artifact_path, b"artifact")?;
        fs::write(&policy_path, b"trusted")?;
        let modified = fs::metadata(&policy_path)?.modified()?;

        let mut launch = metadata_only_launch();
        launch.pack_root = temp.path().to_path_buf();
        launch.artifact_manifest = FileObservation::capture(artifact_path)?;
        launch.payloads = vec![PayloadObservation {
            file: FileObservation::capture(policy_path.clone())?,
            role: ParserPackPayloadRole::NativeImportPolicy,
            bytes: 7,
            sha256: encode_sha256(Sha256::digest(b"trusted")),
        }];
        let probe = launch.currentness_probe("alpha");
        require_test(
            probe.is_current(None)?,
            "initial native policy currentness probe failed",
        )?;

        fs::write(&policy_path, b"changed")?;
        File::options()
            .write(true)
            .open(&policy_path)?
            .set_times(fs::FileTimes::new().set_modified(modified))?;
        require_test(
            fs::metadata(&policy_path)?.len() == 7,
            "native policy mutation did not preserve size",
        )?;
        require_test(
            fs::metadata(&policy_path)?.modified()? == modified,
            "native policy mutation did not preserve modification time",
        )?;
        require_test(
            !probe.is_current(None)?,
            "same-size same-mtime native policy drift retained launch authority",
        )?;
        Ok(())
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn linux_worker_launch_closes_inherited_descriptors_on_exec()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::fd::AsRawFd;

        use nix::fcntl::{FcntlArg, FdFlag, fcntl};

        let cancellation = IndexCancellation::new();
        let control = ArtifactIoControl {
            absolute_deadline: Instant::now() + Duration::from_secs(1),
            last_progress: Instant::now(),
            no_progress_timeout: Duration::from_secs(1),
            cancellation: &cancellation,
        };
        let payload = |name| {
            SealedLinuxPayload::from_verified_bytes(
                "test payload",
                name,
                b"verified authority",
                false,
                &control,
            )
        };
        let authority = LinuxResidentLaunchAuthority {
            worker: payload("projectatlas-test-worker")?,
            artifact_manifest: payload("projectatlas-test-artifact")?,
            accepted_manifest: payload("projectatlas-test-accepted")?,
            native_import_policy: payload("projectatlas-test-policy")?,
            grammar: payload("projectatlas-test-grammar")?,
        };
        let inherited = File::open("/dev/null")?;
        fcntl(&inherited, FcntlArg::F_SETFD(FdFlag::empty()))?;
        let descriptor = inherited.as_raw_fd();
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "test ! -e \"/proc/self/fd/$1\"",
                "parser-worker-fd-check",
            ])
            .arg(descriptor.to_string());
        inherit_linux_authority_on_exec(&mut command, authority);

        if command.status()?.success() {
            Ok(())
        } else {
            Err(io::Error::other("worker inherited an injected descriptor").into())
        }
    }

    #[test]
    fn supervisor_memory_limits_reject_zero_reversed_and_runtime_excess() {
        for candidate in [
            ParserMemoryLimits {
                process_bytes: 0,
                process_tree_bytes: 1,
            },
            ParserMemoryLimits {
                process_bytes: 2,
                process_tree_bytes: 1,
            },
            ParserMemoryLimits {
                process_bytes: PARSER_WORKER_PROCESS_MEMORY_BYTES.saturating_add(1),
                process_tree_bytes: PARSER_WORKER_JOB_MEMORY_BYTES.saturating_add(1),
            },
        ] {
            assert!(matches!(
                candidate.checked(),
                Err(ParserSupervisorError::InvalidMemoryLimits { .. })
            ));
        }
        assert!(ParserMemoryLimits::PRODUCTION.checked().is_ok());
    }

    #[test]
    fn memory_probe_source_preserves_linux_and_bounds_windows() {
        let fixture = b".\n";
        assert_eq!(
            memory_probe_source(PackPlatform::LinuxX86_64, fixture),
            fixture
        );
        let windows = memory_probe_source(PackPlatform::WindowsX86_64, fixture);
        assert_eq!(windows.len(), WINDOWS_MEMORY_PROBE_SOURCE_BYTES);
        assert_eq!(&windows[..4], b".\n.\n");
        assert!(windows.len() <= PARSER_MAX_SOURCE_BYTES as usize);
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn windows_job_memory_exit_code_is_reserved_and_typed() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut memory_command = Command::new("cmd.exe");
        memory_command
            .args(["/D", "/Q", "/C"])
            .arg(format!(
                "set /p _= & exit /B {PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE}"
            ))
            .stdin(Stdio::piped());
        let mut memory_exit = memory_command.spawn()?;
        let mut memory_input = memory_exit
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("delayed broker memory-limit stdin is absent"))?;
        require_test(
            memory_exit.try_wait()?.is_none(),
            "delayed broker memory-limit process exited before observation",
        )?;
        let release_memory_exit = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            memory_input.write_all(b"\n")
        });
        let diagnostic = ParserIoThreadError::UnexpectedDiagnostic {
            diagnostic: "tree-sitter failed to allocate 8".to_owned(),
        };
        let observation_timeout = Duration::from_secs(5);
        let started = Instant::now();
        let memory_diagnostic_result = diagnostic_failure_after_exit_observation(
            &mut memory_exit,
            "request response",
            &diagnostic,
            started + observation_timeout,
            started,
            observation_timeout,
            &IndexCancellation::new(),
        );
        release_memory_exit
            .join()
            .map_err(|_panic| io::Error::other("delayed broker memory-limit release panicked"))??;
        if !matches!(
            memory_diagnostic_result,
            ParserSupervisorError::WindowsJobMemoryLimitExceeded {
                phase: "request response"
            }
        ) {
            return Err(std::io::Error::other(format!(
                "reserved broker memory-limit status did not override diagnostic bytes: {memory_diagnostic_result:?}"
            ))
            .into());
        }
        let memory_result =
            frame_event_result(FrameReaderEvent::EndOfStream, &mut memory_exit, "READY");
        if !matches!(
            memory_result,
            Err(ParserSupervisorError::WindowsJobMemoryLimitExceeded { phase: "READY" })
        ) {
            return Err(std::io::Error::other(format!(
                "reserved broker memory-limit status produced {memory_result:?}"
            ))
            .into());
        }

        let mut ordinary_exit = Command::new("cmd.exe")
            .args(["/D", "/C", "exit", "125"])
            .spawn()?;
        ordinary_exit.wait()?;
        let ordinary_diagnostic_result = diagnostic_failure_after_exit_observation(
            &mut ordinary_exit,
            "request response",
            &diagnostic,
            started + Duration::from_secs(1),
            started,
            Duration::from_secs(1),
            &IndexCancellation::new(),
        );
        if !matches!(
            ordinary_diagnostic_result,
            ParserSupervisorError::IoThread {
                phase: "request response",
                ..
            }
        ) {
            return Err(std::io::Error::other(format!(
                "ordinary broker failure status replaced fail-closed diagnostics: {ordinary_diagnostic_result:?}"
            ))
            .into());
        }
        let ordinary_result =
            frame_event_result(FrameReaderEvent::EndOfStream, &mut ordinary_exit, "READY");
        if !matches!(
            ordinary_result,
            Err(ParserSupervisorError::ChildExited {
                phase: "READY",
                code: Some(125)
            })
        ) {
            return Err(std::io::Error::other(format!(
                "ordinary broker failure status produced {ordinary_result:?}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn cleanup_error_helper_includes_nested_combined_failures() {
        let nested = ParserSupervisorError::OperationAndCleanup {
            operation: Box::new(ParserSupervisorError::Cancelled { phase: "test" }),
            cleanup: Box::new(ParserSupervisorError::OperationAndCleanup {
                operation: Box::new(ParserSupervisorError::DeadlineExceeded { phase: "test" }),
                cleanup: Box::new(ParserSupervisorError::Cleanup {
                    message: "reap failed".to_owned(),
                }),
            }),
        };
        assert!(nested.has_mandatory_cleanup_failure());
        assert_eq!(
            nested.to_string(),
            "optional parser operation failed: optional parser operation was cancelled during test; cleanup also failed: optional parser operation failed: optional parser absolute deadline elapsed during test; cleanup also failed: optional parser cleanup failed: reap failed"
        );
        assert!(
            ParserSupervisorError::Cleanup {
                message: "drain failed".to_owned()
            }
            .has_mandatory_cleanup_failure()
        );
        assert!(
            !ParserSupervisorError::Cancelled { phase: "test" }.has_mandatory_cleanup_failure()
        );
    }

    /// Writer that reports entry and blocks until the test releases one write.
    struct GateWriter {
        /// Reports each entered write call.
        entered: SyncSender<()>,
        /// Releases each entered write call.
        release: Receiver<()>,
    }

    impl Write for GateWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.entered
                .send(())
                .map_err(|error| io::Error::other(error.to_string()))?;
            self.release
                .recv()
                .map_err(|error| io::Error::other(error.to_string()))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Create one acknowledged writer command.
    fn writer_command(
        bytes: Vec<u8>,
    ) -> (WriterCommand, Receiver<Result<(), ParserIoThreadError>>) {
        let (acknowledgement, result) = mpsc::sync_channel(1);
        (
            WriterCommand {
                bytes,
                acknowledgement,
            },
            result,
        )
    }

    /// Return a fallible test failure without panicking.
    fn require_test(condition: bool, message: &'static str) -> io::Result<()> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message))
        }
    }

    /// Reject an oversized declaration after only the fixed header was read.
    #[test]
    fn frame_reader_bounds_header_before_payload_allocation() {
        let declared = PARSER_MAX_SOURCE_BYTES.saturating_add(1).to_be_bytes();
        let bytes = [
            b'P',
            b'A',
            PARSER_PROTOCOL_VERSION,
            ParserFrameKind::RawSource.as_u8(),
            declared[0],
            declared[1],
            declared[2],
            declared[3],
            b'x',
        ];
        let mut input = Cursor::new(bytes);
        assert!(matches!(
            read_one_frame(&mut input),
            Err(ParserIoThreadError::FrameHeader {
                source: ParserProtocolError::FramePayloadTooLarge { .. }
            })
        ));
        assert_eq!(input.position(), PARSER_FRAME_HEADER_BYTES as u64);
    }

    /// Keep at most one pending large write behind the blocked writer thread.
    #[test]
    fn writer_queue_has_one_pending_slot() -> Result<(), Box<dyn std::error::Error>> {
        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (commands, receiver) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            writer_loop(
                GateWriter {
                    entered: entered_sender,
                    release: release_receiver,
                },
                &receiver,
            );
        });

        let (first, first_result) = writer_command(vec![1]);
        commands.send(first)?;
        entered_receiver.recv_timeout(Duration::from_secs(1))?;
        let (second, second_result) = writer_command(vec![2]);
        commands.send(second)?;
        let (third, _third_result) = writer_command(vec![3]);
        require_test(
            matches!(commands.try_send(third), Err(TrySendError::Full(_))),
            "writer queue accepted more than one pending write",
        )?;

        release_sender.send(())?;
        first_result.recv_timeout(Duration::from_secs(1))??;
        entered_receiver.recv_timeout(Duration::from_secs(1))?;
        release_sender.send(())?;
        second_result.recv_timeout(Duration::from_secs(1))??;
        drop(commands);
        handle
            .join()
            .map_err(|_panic| io::Error::other("writer test thread panicked"))?;
        Ok(())
    }

    /// Poll cancellation, absolute deadline, and no-progress independently.
    #[test]
    fn stop_polling_preserves_independent_bounds() {
        let cancellation = IndexCancellation::new();
        let future = Instant::now()
            .checked_add(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        assert!(
            poll_stop(
                "test",
                future,
                Instant::now(),
                Duration::from_secs(1),
                &cancellation,
            )
            .is_ok()
        );
        cancellation.cancel();
        assert!(matches!(
            poll_stop(
                "test",
                future,
                Instant::now(),
                Duration::from_secs(1),
                &cancellation,
            ),
            Err(ParserSupervisorError::Cancelled { .. })
        ));
        assert!(matches!(
            poll_stop(
                "test",
                Instant::now(),
                Instant::now(),
                Duration::from_secs(1),
                &IndexCancellation::new(),
            ),
            Err(ParserSupervisorError::DeadlineExceeded { .. })
        ));
        assert!(matches!(
            poll_stop(
                "test",
                future,
                Instant::now(),
                Duration::ZERO,
                &IndexCancellation::new(),
            ),
            Err(ParserSupervisorError::NoProgress { .. })
        ));
    }

    /// Preserve `root_has_error` while rejecting a completion replayed to another session.
    #[test]
    fn completion_keeps_root_error_and_rejects_cross_session_replay()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = b"broken";
        let session = ParserSessionIdentity::for_entropy(b"session-one");
        let artifact = ParserArtifactIdentity::new(ParserContentDigest::for_bytes(b"artifact"));
        let language = ParserLanguageIdentity::new("abl")?;
        let limits = ParserRequestLimits::new(1024, 100, 100)?;
        let request = ParserRequest::new(
            session,
            ParserRequestIdentity::new(1)?,
            artifact.clone(),
            language.clone(),
            ParserSourceIdentity::for_bytes(source)?,
            limits,
        );
        let evidence = ParserCompletionEvidence::new(
            ParserSyntaxKind::new("source_file")?,
            0,
            u32::try_from(source.len())?,
            true,
            1,
            1,
            0,
            1,
        )?;
        let encoded = encode_parser_control(&ParserControl::Completion(ParserCompletion::new(
            ParserResponseIdentity::for_request(&request),
            evidence,
        )))?;
        let decoded =
            decode_parser_completion_for_request(ParserFrame::decode_exact(&encoded)?, &request)?;
        require_test(
            decoded.evidence().root_has_error(),
            "completion lost root_has_error",
        )?;

        let replay_target = ParserRequest::new(
            ParserSessionIdentity::for_entropy(b"session-two"),
            ParserRequestIdentity::new(1)?,
            artifact,
            language,
            ParserSourceIdentity::for_bytes(source)?,
            limits,
        );
        require_test(
            decode_parser_completion_for_request(
                ParserFrame::decode_exact(&encoded)?,
                &replay_target,
            )
            .is_err(),
            "cross-session completion replay was accepted",
        )?;
        Ok(())
    }

    /// Accept only the exact Windows admission prefix before exposing diagnostics.
    #[test]
    fn diagnostic_reader_validates_admission_before_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut bytes = PARSER_WINDOWS_BROKER_ADMISSION_RECORD.to_vec();
        bytes.extend_from_slice(b"bounded diagnostic");
        let (events, receiver) = mpsc::sync_channel(2);
        let diagnostics = diagnostic_reader_loop(
            Cursor::new(bytes),
            true,
            DiagnosticFence([0xA5; PARSER_DIAGNOSTIC_FENCE_BYTES]),
            &events,
        )?;
        require_test(
            matches!(
                receiver.recv_timeout(Duration::from_secs(1))?,
                DiagnosticReaderEvent::AdmissionAccepted
            ),
            "diagnostics became visible before admission",
        )?;
        require_test(
            matches!(
                receiver.recv_timeout(Duration::from_secs(1))?,
                DiagnosticReaderEvent::Failure(ParserIoThreadError::UnexpectedDiagnostic { .. })
            ),
            "bounded diagnostic bytes did not fail closed",
        )?;
        require_test(
            diagnostics == b"bounded diagnostic",
            "diagnostic bytes changed",
        )?;
        Ok(())
    }

    /// Parse exact Linux RSS and cgroup counters without accepting unit or field drift.
    #[test]
    fn linux_memory_accounting_records_are_strict() {
        assert_eq!(
            parse_process_rss("Name:\tworker\nVmRSS:\t4096 kB\n").ok(),
            Some(4 * 1024 * 1024)
        );
        assert!(parse_process_rss("VmRSS:\t4096 MB\n").is_err());
        assert!(parse_process_rss("VmSize:\t4096 kB\n").is_err());
        assert_eq!(
            parse_cgroup_event("low 0\nhigh 1\nmax 7\noom 0\n", "max").ok(),
            Some(7)
        );
        assert!(parse_cgroup_event("max 7 extra\n", "max").is_err());
        assert!(has_cgroup_token("cpu io memory", "memory"));
        assert!(has_cgroup_token("+cpu +memory", "memory"));
        assert!(!has_cgroup_token("cpu memory.swap", "memory"));
        assert!(matches!(
            parse_unified_cgroup_path("0::/user.slice/projectatlas.scope/worker\n"),
            Ok(Some(path)) if path == Path::new("user.slice/projectatlas.scope/worker")
        ));
        assert!(matches!(
            parse_unified_cgroup_path("0::/\n"),
            Ok(Some(path)) if path.as_os_str().is_empty()
        ));
        assert!(parse_unified_cgroup_path("0::/safe/../escape\n").is_err());
        assert!(parse_unified_cgroup_path("0::/one\n0::/two\n").is_err());
        assert_eq!(
            PARSER_LINUX_RSS_OBSERVATION_INTERVAL,
            SUPERVISOR_POLL_INTERVAL
        );
    }

    /// A worker that releases its address space before becoming waitable is classified as exited,
    /// not as a mandatory cleanup failure.
    #[test]
    fn linux_memory_exit_transition_observes_waitable_child() -> io::Result<()> {
        let exit_checks = std::cell::Cell::new(0_u8);
        let observation = resolve_linux_memory_exit_transition(
            io::Error::new(io::ErrorKind::InvalidData, "VmRSS is absent"),
            SUPERVISOR_POLL_INTERVAL,
            || {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "VmRSS is absent",
                ))
            },
            || {
                let checks = exit_checks.get();
                exit_checks.set(checks.saturating_add(1));
                Ok((checks > 0).then_some(LinuxChildExit { code: Some(17) }))
            },
        )?;
        require_test(
            matches!(
                observation,
                LinuxMemoryObservation::ChildExited { code: Some(17) }
            ),
            "exit transition did not become waitable",
        )
    }

    /// Memory accounting that returns during the short transition remains authoritative.
    #[test]
    fn linux_memory_exit_transition_enforces_recovered_observation() -> io::Result<()> {
        let observation = resolve_linux_memory_exit_transition(
            io::Error::new(io::ErrorKind::InvalidData, "VmRSS is absent"),
            SUPERVISOR_POLL_INTERVAL,
            || {
                Ok(Some(LinuxMemoryBreach {
                    accounting: ParserMemoryAccountingKind::LinuxProcStatus,
                    observed_bytes: 4096,
                }))
            },
            || Ok(None),
        )?;
        require_test(
            matches!(
                observation,
                LinuxMemoryObservation::Memory(Some(LinuxMemoryBreach {
                    accounting: ParserMemoryAccountingKind::LinuxProcStatus,
                    observed_bytes: 4096,
                }))
            ),
            "recovered memory accounting was not retained",
        )
    }

    /// A live non-waitable worker with unreadable accounting still fails closed.
    #[test]
    fn linux_memory_exit_transition_retains_unreadable_failure() -> io::Result<()> {
        let Err(error) = resolve_linux_memory_exit_transition(
            io::Error::new(io::ErrorKind::InvalidData, "VmRSS is absent"),
            Duration::ZERO,
            || {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "VmRSS remains absent",
                ))
            },
            || Ok(None),
        ) else {
            return Err(io::Error::other(
                "unreadable live accounting did not fail closed",
            ));
        };
        require_test(
            error.to_string() == "VmRSS remains absent",
            "unreadable memory failure changed",
        )
    }
}
