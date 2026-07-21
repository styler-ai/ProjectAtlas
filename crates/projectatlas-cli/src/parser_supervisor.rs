//! Bounded process supervision for the separately shipped optional parser pack.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

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
    PARSER_MAX_STDERR_BYTES, PARSER_MAX_TREE_DEPTH, PARSER_SESSION_ENTROPY_BYTES,
    PARSER_WINDOWS_BROKER_ADMISSION_RECORD, ParserArtifactIdentity, ParserCompletionEvidence,
    ParserContainmentKind, ParserControl, ParserFailureCode, ParserFrame, ParserFrameHeader,
    ParserFrameKind, ParserLanguageIdentity, ParserProgress, ParserProgressDisposition,
    ParserProtocolError, ParserRequest, ParserRequestIdentity, ParserRequestLimits,
    ParserSessionIdentity, ParserSessionOpen, ParserSourceIdentity,
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
/// Only accepted worker operation.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const WORKER_SERVE_ARGUMENT: &str = "--serve";
/// Only accepted Windows broker operation.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const BROKER_SERVE_ARGUMENT: &str = "serve-worker";
/// Poll interval for cancellation and bounded child state.
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(20);
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
    #[error("optional parser operation failed and cleanup also failed")]
    OperationAndCleanup {
        /// Original typed operation failure.
        operation: Box<Self>,
        /// Typed cleanup failure.
        cleanup: Box<Self>,
    },
}

impl ParserSupervisorError {
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

/// Metadata used to detect mutation of one already verified payload.
#[derive(Debug)]
struct PayloadObservation {
    /// Canonical payload path.
    path: PathBuf,
    /// Exact manifest-bound byte count.
    bytes: u64,
    /// Modification timestamp when the host filesystem exposes one.
    modified: Option<SystemTime>,
}

/// Complete private launch authority derived from one exact immutable artifact.
#[derive(Debug)]
struct VerifiedParserPackLaunch {
    /// Canonical artifact root.
    pack_root: PathBuf,
    /// Accepted target bound by the artifact manifest.
    platform: PackPlatform,
    /// Exact executable launched on Linux.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    worker: PathBuf,
    /// Exact containment broker launched on Windows.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    containment_broker: Option<PathBuf>,
    /// Sorted accepted language identities.
    accepted_grammars: Vec<String>,
    /// Exact artifact-manifest byte identity independently observed by Rust.
    artifact: ParserArtifactIdentity,
    /// Manifest-bound SHA-256 of the logical capability manifest.
    accepted_manifest_sha256: String,
    /// Cheap metadata observations for every already hashed payload.
    payloads: Vec<PayloadObservation>,
}

impl VerifiedParserPackLaunch {
    /// Validate and canonicalize one exact artifact before process creation.
    fn load(pack_root: &Path) -> Result<Self, ParserSupervisorError> {
        let platform =
            host_pack_platform().ok_or(ParserSupervisorError::UnsupportedContainment {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
            })?;
        let pack_root = canonical_directory(pack_root)?;
        let accepted_path = canonical_direct_file(&pack_root, ACCEPTED_MANIFEST_FILE_NAME)?;
        let artifact_path = canonical_direct_file(&pack_root, ARTIFACT_MANIFEST_FILE_NAME)?;
        let accepted_bytes = read_bounded_file(
            &accepted_path,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
        )?;
        let artifact_bytes = read_bounded_file(
            &artifact_path,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
        )?;
        let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
        let artifact_manifest: OptionalParserPackArtifactManifest =
            serde_json::from_slice(&artifact_bytes)
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
        let mut payloads = Vec::with_capacity(artifact_manifest.files.len());
        for payload in &artifact_manifest.files {
            let path = canonical_payload_file(&pack_root, payload.path.as_str())?;
            let bytes = read_bounded_file(&path, payload.bytes)?;
            if u64::try_from(bytes.len()).ok() != Some(payload.bytes) {
                return Err(ParserSupervisorError::PayloadMismatch {
                    path,
                    reason: "payload byte count differs from the artifact manifest",
                });
            }
            if sha256_hex(&bytes) != payload.sha256.as_str() {
                return Err(ParserSupervisorError::PayloadMismatch {
                    path,
                    reason: "payload SHA-256 differs from the artifact manifest",
                });
            }
            let metadata = file_metadata(&path)?;
            payloads.push(PayloadObservation {
                path: path.clone(),
                bytes: payload.bytes,
                modified: metadata.modified().ok(),
            });
            match &payload.role {
                ParserPackPayloadRole::Worker => worker = Some(path),
                ParserPackPayloadRole::ContainmentBroker => containment_broker = Some(path),
                ParserPackPayloadRole::AcceptedManifest => {
                    accepted_payload_sha256 = Some(payload.sha256.as_str().to_owned());
                }
                ParserPackPayloadRole::FixtureCorpus
                | ParserPackPayloadRole::ProjectLicense
                | ParserPackPayloadRole::NativeImportPolicy
                | ParserPackPayloadRole::NativeAuditReport
                | ParserPackPayloadRole::GrammarLibrary { .. } => {}
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
        if sha256_hex(&accepted_bytes) != accepted_manifest_sha256 {
            return Err(ParserSupervisorError::PayloadMismatch {
                path: accepted_path,
                reason: "accepted capability manifest does not match its artifact payload row",
            });
        }
        #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
        let _ = containment_broker;

        Ok(Self {
            pack_root,
            platform,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            worker: expected_worker,
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            containment_broker,
            accepted_grammars: logical
                .grammars()
                .iter()
                .map(|grammar| grammar.language_id.clone())
                .collect(),
            artifact: ParserArtifactIdentity::for_bytes(&artifact_bytes),
            accepted_manifest_sha256,
            payloads,
        })
    }

    /// Return whether all cheap immutable-artifact observations still match.
    fn is_current(&self) -> Result<bool, ParserSupervisorError> {
        let artifact_bytes = read_bounded_file(
            &self.pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
        )?;
        if ParserArtifactIdentity::for_bytes(&artifact_bytes) != self.artifact {
            return Ok(false);
        }
        let accepted_bytes = read_bounded_file(
            &self.pack_root.join(ACCEPTED_MANIFEST_FILE_NAME),
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
        )?;
        if sha256_hex(&accepted_bytes) != self.accepted_manifest_sha256 {
            return Ok(false);
        }
        for payload in &self.payloads {
            let Ok(metadata) = fs::metadata(&payload.path) else {
                return Ok(false);
            };
            if !metadata.is_file()
                || metadata.len() != payload.bytes
                || metadata.modified().ok() != payload.modified
            {
                return Ok(false);
            }
        }
        Ok(true)
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

/// Read one exact regular file without permitting growth beyond its bound.
fn read_bounded_file(path: &Path, maximum: u64) -> Result<Vec<u8>, ParserSupervisorError> {
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
    if metadata.len() > maximum {
        return Err(ParserSupervisorError::ArtifactFileTooLarge {
            path: path.to_path_buf(),
            actual: metadata.len(),
            maximum,
        });
    }
    let capacity = usize::try_from(metadata.len()).unwrap_or(ARTIFACT_READ_CHUNK_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    let mut bounded = Read::by_ref(&mut file).take(maximum.saturating_add(1));
    bounded
        .read_to_end(&mut bytes)
        .map_err(|source| ParserSupervisorError::ArtifactRead {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(ParserSupervisorError::ArtifactFileTooLarge {
            path: path.to_path_buf(),
            actual: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(bytes)
}

/// Compute lowercase SHA-256 for one exact payload.
fn sha256_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        encoded.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
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
    /// The bounded diagnostic stream exceeded its fixed byte ceiling.
    #[error("diagnostic stream exceeded {maximum} bytes")]
    DiagnosticOverflow {
        /// Inclusive byte ceiling.
        maximum: usize,
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
    /// Terminal bounded reader failure.
    Failure(ParserIoThreadError),
}

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

/// Own worker stdout until EOF or the first bounded framing failure.
fn frame_reader_loop(mut stdout: ChildStdout, events: &SyncSender<FrameReaderEvent>) {
    loop {
        let event = match read_one_frame(&mut stdout) {
            Ok(Some(frame)) => FrameReaderEvent::Frame(frame),
            Ok(None) => FrameReaderEvent::EndOfStream,
            Err(error) => FrameReaderEvent::Failure(error),
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
    events: &SyncSender<DiagnosticReaderEvent>,
) -> Result<Vec<u8>, ParserIoThreadError> {
    if expect_windows_admission {
        let mut observed = [0_u8; PARSER_WINDOWS_BROKER_ADMISSION_RECORD.len()];
        stderr
            .read_exact(&mut observed)
            .map_err(|source| ParserIoThreadError::Stream {
                operation: "read Windows admission record",
                source,
            })?;
        if observed != PARSER_WINDOWS_BROKER_ADMISSION_RECORD {
            let error = ParserIoThreadError::AdmissionMismatch;
            drop(events.send(DiagnosticReaderEvent::Failure(error)));
            return Err(ParserIoThreadError::AdmissionMismatch);
        }
    }
    if events
        .send(DiagnosticReaderEvent::AdmissionAccepted)
        .is_err()
    {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    let mut chunk = [0_u8; 4 * 1024];
    loop {
        let count = match stderr.read(&mut chunk) {
            Ok(0) => return Ok(diagnostics),
            Ok(count) => count,
            Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                let message = source.to_string();
                drop(events.try_send(DiagnosticReaderEvent::Failure(
                    ParserIoThreadError::Stream {
                        operation: "read diagnostic stream",
                        source,
                    },
                )));
                return Err(ParserIoThreadError::Stream {
                    operation: "read diagnostic stream",
                    source: io::Error::other(message),
                });
            }
        };
        if count > PARSER_MAX_STDERR_BYTES.saturating_sub(diagnostics.len()) {
            drop(events.try_send(DiagnosticReaderEvent::Failure(
                ParserIoThreadError::DiagnosticOverflow {
                    maximum: PARSER_MAX_STDERR_BYTES,
                },
            )));
            return Err(ParserIoThreadError::DiagnosticOverflow {
                maximum: PARSER_MAX_STDERR_BYTES,
            });
        }
        diagnostics.extend_from_slice(&chunk[..count]);
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
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxMemoryBreach {
    /// Active accounting path.
    accounting: ParserMemoryAccountingKind,
    /// Last observed resident or cgroup memory bytes.
    observed_bytes: u64,
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
        /// Process-group termination failure, when the first kill attempt failed.
        termination_error: Option<String>,
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
        drop(self.stop.try_send(()));
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
                termination_error: linux_monitor_termination_error(process_id),
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
        absolute_deadline: Instant,
        no_progress_timeout: Duration,
        cancellation: &IndexCancellation,
    ) -> Result<Self, ParserSupervisorError> {
        let session = fresh_session_identity()?;
        let containment = containment_for_platform(launch.platform);
        let memory_limits = memory_limits.checked()?;
        let mut command = platform_command(launch, memory_limits)?;
        let program = PathBuf::from(command.get_program());
        let mut child = command
            .spawn()
            .map_err(|source| ParserSupervisorError::Spawn { program, source })?;
        let stdin = child
            .stdin
            .take()
            .ok_or(ParserSupervisorError::MissingPipe { stream: "stdin" });
        let stdout = child
            .stdout
            .take()
            .ok_or(ParserSupervisorError::MissingPipe { stream: "stdout" });
        let stderr = child
            .stderr
            .take()
            .ok_or(ParserSupervisorError::MissingPipe { stream: "stderr" });
        let (stdin, stdout, stderr) = match (stdin, stdout, stderr) {
            (Ok(stdin), Ok(stdout), Ok(stderr)) => (stdin, stdout, stderr),
            (stdin, stdout, stderr) => {
                let operation = stdin
                    .err()
                    .or_else(|| stdout.err())
                    .or_else(|| stderr.err())
                    .unwrap_or(ParserSupervisorError::MissingPipe { stream: "unknown" });
                return Err(attach_cleanup(
                    operation,
                    cleanup_partial_launch(&mut child, Vec::new()),
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
                    cleanup_partial_launch(&mut child, Vec::new()),
                ));
            }
        };
        let (frame_sender, frame_events) = mpsc::sync_channel(1);
        let frame_handle = thread::Builder::new()
            .name("parser-supervisor-stdout".to_owned())
            .spawn(move || frame_reader_loop(stdout, &frame_sender))
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
                    cleanup_partial_launch(&mut child, vec![writer_handle]),
                ));
            }
        };
        let (diagnostic_sender, diagnostic_events) = mpsc::sync_channel(1);
        let expect_windows_admission = launch.platform == PackPlatform::WindowsX86_64;
        let diagnostic_handle = thread::Builder::new()
            .name("parser-supervisor-stderr".to_owned())
            .spawn(move || {
                diagnostic_reader_loop(stderr, expect_windows_admission, &diagnostic_sender)
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
                    cleanup_partial_launch(&mut child, vec![writer_handle, frame_handle]),
                ));
            }
        };

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
        let started = Instant::now();
        let opening = (|| {
            resident.wait_for_admission(
                absolute_deadline,
                started,
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
                started,
                no_progress_timeout,
                cancellation,
            )?;
            let ready_bytes = resident.wait_for_frame(
                "READY",
                absolute_deadline,
                started,
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

        let mut last_progress = Instant::now();
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
                    self.enforce_memory_bound("request failure", true)?;
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
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            self.enforce_memory_bound(phase, false)?;
            if let Some(event) = try_frame_event(&self.frame_reader.events)? {
                return self.finish_frame_event(event, phase);
            }
            self.check_diagnostic_reader(phase)?;
            match self.frame_reader.events.recv_timeout(next_poll_wait(
                absolute_deadline,
                last_progress,
                no_progress_timeout,
            )) {
                Ok(event) => return self.finish_frame_event(event, phase),
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

    /// Surface diagnostic overflow or premature diagnostic-stream closure.
    fn check_diagnostic_reader(
        &mut self,
        phase: &'static str,
    ) -> Result<(), ParserSupervisorError> {
        match self.diagnostic_reader.events.try_recv() {
            Ok(DiagnosticReaderEvent::Failure(error)) => Err(io_thread_error(phase, &error)),
            Ok(DiagnosticReaderEvent::AdmissionAccepted)
            | Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(()),
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
                if let Some(status) = self.child.try_wait().map_err(|wait_source| {
                    ParserSupervisorError::ResidentMemoryObservationFailed {
                        phase,
                        accounting: ParserMemoryAccountingKind::LinuxProcStatus,
                        message: bounded_message(format!(
                            "memory observation failed: {source}; child-state observation also failed: {wait_source}"
                        )),
                    }
                })? {
                    return Err(ParserSupervisorError::ChildExited {
                        phase,
                        code: status.code(),
                    });
                }
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
        self.termination_requested = true;
        let (operation, termination_error) = match event {
            LinuxMemoryMonitorEvent::Limit {
                breach,
                termination_error,
            } => (
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
            ),
            LinuxMemoryMonitorEvent::ObservationFailed {
                accounting,
                message,
                termination_error,
            } => (
                ParserSupervisorError::ResidentMemoryObservationFailed {
                    phase,
                    accounting,
                    message,
                },
                termination_error,
            ),
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

/// Build the one accepted platform command with closed arguments and environment.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn platform_command(
    launch: &VerifiedParserPackLaunch,
    _memory_limits: ParserMemoryLimits,
) -> Result<Command, ParserSupervisorError> {
    use std::os::unix::process::CommandExt;

    if launch.platform != PackPlatform::LinuxX86_64 {
        return Err(ParserSupervisorError::PayloadMismatch {
            path: launch.pack_root.clone(),
            reason: "Linux supervisor received another platform artifact",
        });
    }
    let mut command = Command::new(&launch.worker);
    command
        .arg(WORKER_SERVE_ARGUMENT)
        .current_dir(&launch.pack_root)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
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

/// Convert a private I/O thread failure at the public typed boundary.
fn io_thread_error(phase: &'static str, error: &ParserIoThreadError) -> ParserSupervisorError {
    ParserSupervisorError::IoThread {
        phase,
        message: bounded_message(error.to_string()),
    }
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
) -> Result<(), ParserSupervisorError> {
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
        if reaped && handles.iter().all(JoinHandle::is_finished) {
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
    /// `absolute_deadline` is never extended by progress. Only identity-validated
    /// progress that advances stage or work resets `no_progress_timeout`.
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
        poll_stop(
            "request admission",
            absolute_deadline,
            Instant::now(),
            no_progress_timeout,
            cancellation,
        )?;
        self.refresh_changed_artifact()?;
        let grammar = self.launch.require_grammar(language_id)?;
        let source_identity = ParserSourceIdentity::for_bytes(source)?;
        if self
            .resident
            .as_ref()
            .is_some_and(|resident| resident.grammar != grammar)
        {
            self.shutdown_resident()?;
        }
        if self.resident.is_none() {
            self.resident = Some(ResidentParserSession::launch(
                &self.launch,
                grammar,
                self.memory_limits,
                absolute_deadline,
                no_progress_timeout,
                cancellation,
            )?);
        }
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
                absolute_deadline,
                no_progress_timeout,
                cancellation,
            );
        match result {
            Ok(evidence) => Ok(evidence),
            Err(operation) => match self.take_and_shutdown_resident() {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(ParserSupervisorError::OperationAndCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            },
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
    fn refresh_changed_artifact(&mut self) -> Result<(), ParserSupervisorError> {
        if let Ok(true) = self.launch.is_current() {
            return Ok(());
        }
        self.shutdown_resident()?;
        self.launch = VerifiedParserPackLaunch::load(&self.pack_root)?;
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
        grammar.fixtures.positive.source.as_bytes(),
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

#[cfg(test)]
mod tests {
    //! Protect bounded framing, backpressure, stop polling, and response identity.

    use std::io::Cursor;

    use projectatlas_core::optional_parser_protocol::{
        PARSER_MAX_SOURCE_BYTES, PARSER_PROTOCOL_VERSION, PARSER_WINDOWS_BROKER_ADMISSION_RECORD,
        ParserCompletion, ParserContentDigest, ParserResponseIdentity, ParserSyntaxKind,
    };

    use super::*;

    /// Build a process-free supervisor value for public metadata delegation tests.
    fn metadata_only_supervisor() -> OptionalParserSupervisor {
        let pack_root = PathBuf::from("metadata-only-pack");
        OptionalParserSupervisor {
            pack_root: pack_root.clone(),
            launch: VerifiedParserPackLaunch {
                pack_root: pack_root.clone(),
                platform: PackPlatform::LinuxX86_64,
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                worker: pack_root.join("projectatlas-parser-worker"),
                #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                containment_broker: Some(pack_root.join("projectatlas-parser-containment.exe")),
                accepted_grammars: vec!["alpha".to_owned(), "zeta".to_owned()],
                artifact: ParserArtifactIdentity::for_bytes(b"artifact"),
                accepted_manifest_sha256: "0".repeat(64),
                payloads: Vec::new(),
            },
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

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn windows_job_memory_exit_code_is_reserved_and_typed() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut memory_exit = Command::new("cmd.exe")
            .args([
                "/D",
                "/C",
                "exit",
                &PARSER_WINDOWS_BROKER_MEMORY_LIMIT_EXIT_CODE.to_string(),
            ])
            .spawn()?;
        memory_exit.wait()?;
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
        let (events, receiver) = mpsc::sync_channel(1);
        let diagnostics = diagnostic_reader_loop(Cursor::new(bytes), true, &events)?;
        require_test(
            matches!(
                receiver.recv_timeout(Duration::from_secs(1))?,
                DiagnosticReaderEvent::AdmissionAccepted
            ),
            "diagnostics became visible before admission",
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
}
