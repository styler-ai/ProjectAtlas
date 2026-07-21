//! Fail-closed Linux containment for the optional parser worker.
//!
//! The worker observes its exact protocol descriptors, eager system-runtime
//! mappings, and single-threaded starting state before applying irreversible
//! restrictions. Call [`observe_parser_worker_preconditions`] followed by
//! [`enforce_parser_worker_containment`] before reading `SessionOpen`; after any
//! error, terminate the partially restricted worker without reading input.

#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use landlock::{
    ABI, Access, AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus,
};
use nix::errno::Errno;
use nix::libc;
use nix::sys::prctl::{get_no_new_privs, set_no_new_privs};
use nix::sys::resource::{Resource, getrlimit, rlim_t, setrlimit};
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch, apply_filter};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process;
use thiserror::Error;

use projectatlas_core::optional_parser_protocol::PARSER_WORKER_PROCESS_MEMORY_BYTES;

/// Maximum cumulative CPU time before the supervisor must replace the worker.
const CPU_SECONDS: rlim_t = 300;
/// Maximum file bytes the worker may create.
const FILE_SIZE_BYTES: rlim_t = 0;
/// Maximum descriptors available after caller-owned descriptor admission.
const OPEN_FILE_COUNT: rlim_t = 32;
/// Maximum processes available to the worker's real user identity.
const PROCESS_COUNT: rlim_t = 1;
/// Maximum core-dump bytes.
const CORE_DUMP_BYTES: rlim_t = 0;
/// Maximum bytes admitted from the kernel-generated process status record.
const PROCESS_STATUS_MAX_BYTES: u64 = 64 * 1024;
/// Maximum bytes admitted from the kernel descriptor-information record.
const DESCRIPTOR_INFO_MAX_BYTES: u64 = 4 * 1024;
/// Maximum bytes admitted from the kernel runtime-mapping record.
const PROCESS_MAPPINGS_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Exact inherited descriptors used by the parser protocol and bounded diagnostics.
const PROTOCOL_DESCRIPTORS: [u32; 3] = [0, 1, 2];
/// System roots allowed to own already-mapped runtime objects outside the pack.
const SYSTEM_RUNTIME_ROOTS: [&str; 4] = ["/lib", "/lib64", "/usr/lib", "/usr/lib64"];
/// Maximum exact eager system-runtime identities admitted from the artifact policy.
const MAX_EAGER_RUNTIME_LIBRARIES: usize = 32;

/// Process creation and replacement syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const PROCESS_AND_EXEC_SYSCALLS: &[i64] = &[
    libc::SYS_clone,
    libc::SYS_clone3,
    libc::SYS_execve,
    libc::SYS_execveat,
    libc::SYS_fork,
    libc::SYS_vfork,
];
/// Direct network and socket-IPC syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const NETWORK_SYSCALLS: &[i64] = &[
    libc::SYS_accept,
    libc::SYS_accept4,
    libc::SYS_bind,
    libc::SYS_connect,
    libc::SYS_listen,
    libc::SYS_recvfrom,
    libc::SYS_recvmmsg,
    libc::SYS_recvmsg,
    libc::SYS_sendmmsg,
    libc::SYS_sendmsg,
    libc::SYS_sendto,
    libc::SYS_shutdown,
    libc::SYS_socket,
    libc::SYS_socketpair,
];
/// Namespace and mount syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const NAMESPACE_SYSCALLS: &[i64] = &[
    libc::SYS_chroot,
    libc::SYS_fsconfig,
    libc::SYS_fsmount,
    libc::SYS_fsopen,
    libc::SYS_mount,
    libc::SYS_mount_setattr,
    libc::SYS_move_mount,
    libc::SYS_open_tree,
    libc::SYS_pivot_root,
    libc::SYS_setns,
    libc::SYS_umount2,
    libc::SYS_unshare,
];
/// Cross-process inspection, mutation, and signaling syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const PROCESS_ESCAPE_SYSCALLS: &[i64] = &[
    libc::SYS_kill,
    libc::SYS_pidfd_getfd,
    libc::SYS_pidfd_open,
    libc::SYS_pidfd_send_signal,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_prlimit64,
    libc::SYS_ptrace,
    libc::SYS_rt_sigqueueinfo,
    libc::SYS_rt_tgsigqueueinfo,
    libc::SYS_sched_setaffinity,
    libc::SYS_sched_setattr,
    libc::SYS_sched_setparam,
    libc::SYS_sched_setscheduler,
    libc::SYS_setpgid,
    libc::SYS_setpriority,
    libc::SYS_setsid,
    libc::SYS_tgkill,
    libc::SYS_tkill,
];
/// Kernel interfaces that can widen the worker's effective capability surface.
#[cfg(target_arch = "x86_64")]
const KERNEL_ESCAPE_SYSCALLS: &[i64] = &[
    libc::SYS_add_key,
    libc::SYS_bpf,
    libc::SYS_capset,
    libc::SYS_delete_module,
    libc::SYS_finit_module,
    libc::SYS_init_module,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_io_uring_setup,
    libc::SYS_ioprio_set,
    libc::SYS_kexec_file_load,
    libc::SYS_kexec_load,
    libc::SYS_keyctl,
    libc::SYS_memfd_create,
    libc::SYS_name_to_handle_at,
    libc::SYS_open_by_handle_at,
    libc::SYS_perf_event_open,
    libc::SYS_process_madvise,
    libc::SYS_reboot,
    libc::SYS_request_key,
    libc::SYS_userfaultfd,
];
/// Filesystem metadata mutation and working-directory syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const FILESYSTEM_METADATA_AND_DIRECTORY_SYSCALLS: &[i64] = &[
    libc::SYS_chdir,
    libc::SYS_fchdir,
    libc::SYS_chmod,
    libc::SYS_fchmod,
    libc::SYS_fchmodat,
    libc::SYS_fchmodat2,
    libc::SYS_chown,
    libc::SYS_fchown,
    libc::SYS_lchown,
    libc::SYS_fchownat,
    libc::SYS_setxattr,
    libc::SYS_lsetxattr,
    libc::SYS_fsetxattr,
    libc::SYS_removexattr,
    libc::SYS_lremovexattr,
    libc::SYS_fremovexattr,
    libc::SYS_utime,
    libc::SYS_utimes,
    libc::SYS_futimesat,
    libc::SYS_utimensat,
];
/// Persistent System V and POSIX message-queue IPC syscalls denied after admission.
#[cfg(target_arch = "x86_64")]
const PERSISTENT_IPC_SYSCALLS: &[i64] = &[
    libc::SYS_shmget,
    libc::SYS_shmat,
    libc::SYS_shmdt,
    libc::SYS_shmctl,
    libc::SYS_semget,
    libc::SYS_semop,
    libc::SYS_semtimedop,
    libc::SYS_semctl,
    libc::SYS_msgget,
    libc::SYS_msgsnd,
    libc::SYS_msgrcv,
    libc::SYS_msgctl,
    libc::SYS_mq_open,
    libc::SYS_mq_unlink,
    libc::SYS_mq_timedsend,
    libc::SYS_mq_timedreceive,
    libc::SYS_mq_notify,
    libc::SYS_mq_getsetattr,
];

/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const PROCESS_AND_EXEC_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const NETWORK_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const NAMESPACE_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const PROCESS_ESCAPE_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const KERNEL_ESCAPE_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const FILESYSTEM_METADATA_AND_DIRECTORY_SYSCALLS: &[i64] = &[];
/// Empty policies keep unsupported Linux architectures compilable but unavailable.
#[cfg(not(target_arch = "x86_64"))]
const PERSISTENT_IPC_SYSCALLS: &[i64] = &[];

/// Closed containment stages used for typed failure routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParserWorkerContainmentStage {
    /// Caller-owned process admission has not completed.
    Preconditions,
    /// The immutable pack root cannot be resolved or opened.
    PackRoot,
    /// The worker inherited a privileged identity or capability set.
    ProcessIdentity,
    /// A hard process resource limit could not be applied or verified.
    ResourceLimits,
    /// `no_new_privs` could not be applied or verified.
    NoNewPrivileges,
    /// The Landlock filesystem boundary could not be fully enforced.
    Filesystem,
    /// The seccomp syscall boundary could not be compiled or installed.
    Syscalls,
}

/// Failure to establish the complete Linux parser-worker boundary.
#[derive(Debug, Error)]
pub(crate) enum ParserWorkerContainmentError {
    /// Inspecting the inherited protocol descriptors failed.
    #[error("could not inspect parser worker protocol descriptors: {source}")]
    InspectProtocolDescriptors {
        /// Kernel descriptor-inspection failure.
        #[source]
        source: io::Error,
    },
    /// The inherited descriptor set or access modes were not exact.
    #[error("parser worker protocol descriptor admission failed: {reason}")]
    InvalidProtocolDescriptors {
        /// Stable content-free rejection reason.
        reason: &'static str,
    },
    /// Inspecting already-mapped runtime objects failed.
    #[error("could not inspect parser worker runtime mappings: {source}")]
    InspectRuntimeMappings {
        /// Kernel mapping-inspection failure.
        #[source]
        source: io::Error,
    },
    /// Runtime mappings were absent, deleted, malformed, or outside admitted roots.
    #[error("parser worker runtime mapping admission failed: {reason}")]
    InvalidRuntimeMappings {
        /// Stable content-free rejection reason.
        reason: &'static str,
    },
    /// Inspecting the starting thread set failed.
    #[error("could not inspect parser worker thread state: {source}")]
    InspectThreadState {
        /// Kernel task-inspection failure.
        #[source]
        source: io::Error,
    },
    /// The worker did not begin as exactly its one main thread.
    #[error("parser worker must start with exactly one thread; observed {observed}")]
    InvalidThreadState {
        /// Numeric task entries reported by the kernel.
        observed: usize,
    },
    /// Linux optional parsing is intentionally limited to the accepted pack architecture.
    #[error("parser worker containment is unsupported on Linux architecture {architecture}")]
    UnsupportedArchitecture {
        /// Architecture reported by the Rust target.
        architecture: &'static str,
    },
    /// Canonicalizing the immutable pack root failed.
    #[error("could not canonicalize parser pack root {path}: {source}")]
    CanonicalizePackRoot {
        /// Requested immutable pack root.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The resolved pack root is not a directory.
    #[error("parser pack root is not a directory: {path}")]
    PackRootNotDirectory {
        /// Resolved invalid pack root.
        path: PathBuf,
    },
    /// Opening the pack root for a Landlock rule failed.
    #[error("could not open parser pack root for Landlock: {source}")]
    OpenPackRoot {
        /// Landlock path-descriptor failure.
        #[source]
        source: landlock::PathFdError,
    },
    /// Reading the bounded kernel process-identity record failed.
    #[error("could not read parser worker process identity: {source}")]
    ReadProcessIdentity {
        /// Kernel status-read failure.
        #[source]
        source: io::Error,
    },
    /// The kernel process-identity record was missing or malformed.
    #[error("parser worker process identity field is missing or malformed: {field}")]
    InvalidProcessIdentity {
        /// Stable kernel status field identity.
        field: &'static str,
    },
    /// The kernel process-identity values were individually valid but not identical.
    #[error("parser worker process identity values are inconsistent: {field}")]
    InconsistentProcessIdentity {
        /// Stable inconsistent identity field.
        field: &'static str,
    },
    /// The worker already holds a privileged identity before containment.
    #[error("parser worker process identity is privileged: {field}")]
    PrivilegedProcessIdentity {
        /// Stable privileged identity or capability field.
        field: &'static str,
    },
    /// Applying one hard resource limit failed.
    #[error("could not set hard parser worker resource limit {resource}: {source}")]
    SetResourceLimit {
        /// Stable resource-limit identity.
        resource: &'static str,
        /// Operating-system failure.
        #[source]
        source: Errno,
    },
    /// Reading back one resource limit failed.
    #[error("could not verify hard parser worker resource limit {resource}: {source}")]
    ReadResourceLimit {
        /// Stable resource-limit identity.
        resource: &'static str,
        /// Operating-system failure.
        #[source]
        source: Errno,
    },
    /// A resource limit did not match its required hard value after application.
    #[error(
        "hard parser worker resource limit {resource} is ({actual_soft}, {actual_hard}); expected ({expected}, {expected})"
    )]
    ResourceLimitMismatch {
        /// Stable resource-limit identity.
        resource: &'static str,
        /// Required soft and hard value.
        expected: rlim_t,
        /// Observed soft value.
        actual_soft: rlim_t,
        /// Observed hard value.
        actual_hard: rlim_t,
    },
    /// Applying `no_new_privs` failed.
    #[error("could not set no_new_privs for parser worker: {source}")]
    SetNoNewPrivileges {
        /// Operating-system failure.
        #[source]
        source: Errno,
    },
    /// Reading back `no_new_privs` failed.
    #[error("could not verify no_new_privs for parser worker: {source}")]
    ReadNoNewPrivileges {
        /// Operating-system failure.
        #[source]
        source: Errno,
    },
    /// `no_new_privs` remained unset after the kernel accepted the setter call.
    #[error("no_new_privs is not enforced for parser worker")]
    NoNewPrivilegesNotEnforced,
    /// Constructing or applying the Landlock rule set failed.
    #[error("could not enforce parser worker Landlock rules: {source}")]
    Landlock {
        /// Landlock policy or syscall failure.
        #[source]
        source: landlock::RulesetError,
    },
    /// Landlock reported less than complete enforcement.
    #[error("parser worker Landlock rules were not fully enforced")]
    LandlockNotFullyEnforced,
    /// Compiling the fixed seccomp policy failed.
    #[error("could not compile parser worker seccomp policy: {source}")]
    CompileSeccomp {
        /// Seccomp policy compiler failure.
        #[source]
        source: seccompiler::BackendError,
    },
    /// Installing the fixed seccomp policy failed.
    #[error("could not install parser worker seccomp policy: {source}")]
    InstallSeccomp {
        /// Seccomp installation failure.
        #[source]
        source: seccompiler::Error,
    },
}

impl ParserWorkerContainmentError {
    /// Return the closed stage at which containment failed.
    pub(crate) const fn stage(&self) -> ParserWorkerContainmentStage {
        match self {
            Self::InspectProtocolDescriptors { .. }
            | Self::InvalidProtocolDescriptors { .. }
            | Self::InspectRuntimeMappings { .. }
            | Self::InvalidRuntimeMappings { .. }
            | Self::InspectThreadState { .. }
            | Self::InvalidThreadState { .. }
            | Self::UnsupportedArchitecture { .. } => ParserWorkerContainmentStage::Preconditions,
            Self::CanonicalizePackRoot { .. }
            | Self::PackRootNotDirectory { .. }
            | Self::OpenPackRoot { .. } => ParserWorkerContainmentStage::PackRoot,
            Self::ReadProcessIdentity { .. }
            | Self::InvalidProcessIdentity { .. }
            | Self::InconsistentProcessIdentity { .. }
            | Self::PrivilegedProcessIdentity { .. } => {
                ParserWorkerContainmentStage::ProcessIdentity
            }
            Self::SetResourceLimit { .. }
            | Self::ReadResourceLimit { .. }
            | Self::ResourceLimitMismatch { .. } => ParserWorkerContainmentStage::ResourceLimits,
            Self::SetNoNewPrivileges { .. }
            | Self::ReadNoNewPrivileges { .. }
            | Self::NoNewPrivilegesNotEnforced => ParserWorkerContainmentStage::NoNewPrivileges,
            Self::Landlock { .. } | Self::LandlockNotFullyEnforced => {
                ParserWorkerContainmentStage::Filesystem
            }
            Self::CompileSeccomp { .. } | Self::InstallSeccomp { .. } => {
                ParserWorkerContainmentStage::Syscalls
            }
        }
    }
}

/// Proof that process admission was observed before self-containment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParserWorkerContainmentPreconditions {
    /// Prevent construction without kernel-backed observation.
    _private: (),
}

/// One hard resource-limit rule.
#[derive(Clone, Copy, Debug)]
struct ResourceLimitRule {
    /// Stable diagnostic identity.
    name: &'static str,
    /// Operating-system resource selector.
    resource: Resource,
    /// Exact soft and hard limit.
    value: rlim_t,
}

/// Return the complete fixed hard-limit policy.
fn resource_limit_policy() -> [ResourceLimitRule; 6] {
    [
        ResourceLimitRule {
            name: "address_space_bytes",
            resource: Resource::RLIMIT_AS,
            value: PARSER_WORKER_PROCESS_MEMORY_BYTES,
        },
        ResourceLimitRule {
            name: "cpu_seconds",
            resource: Resource::RLIMIT_CPU,
            value: CPU_SECONDS,
        },
        ResourceLimitRule {
            name: "file_size_bytes",
            resource: Resource::RLIMIT_FSIZE,
            value: FILE_SIZE_BYTES,
        },
        ResourceLimitRule {
            name: "open_file_count",
            resource: Resource::RLIMIT_NOFILE,
            value: OPEN_FILE_COUNT,
        },
        ResourceLimitRule {
            name: "process_count",
            resource: Resource::RLIMIT_NPROC,
            value: PROCESS_COUNT,
        },
        ResourceLimitRule {
            name: "core_dump_bytes",
            resource: Resource::RLIMIT_CORE,
            value: CORE_DUMP_BYTES,
        },
    ]
}

/// Return the only filesystem permissions granted beneath the immutable pack root.
fn pack_read_access() -> BitFlags<AccessFs> {
    AccessFs::ReadFile | AccessFs::ReadDir
}

/// Read one kernel-owned text record within an exact byte ceiling.
fn read_bounded_kernel_text(path: &Path, maximum: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "kernel record exceeds parser worker observation bound",
        ));
    }
    String::from_utf8(bytes).map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))
}

/// Return whether one procfs descriptor target is an anonymous pipe.
fn is_pipe_target(target: &Path) -> bool {
    target
        .to_str()
        .is_some_and(|value| value.starts_with("pipe:[") && value.ends_with(']'))
}

/// Validate exact, distinct protocol-pipe targets and the transient observer descriptor.
fn validate_protocol_descriptor_targets(
    descriptors: &mut BTreeMap<u32, PathBuf>,
    observer_target: &Path,
) -> Result<(), ParserWorkerContainmentError> {
    let mut protocol_targets = BTreeSet::new();
    for descriptor in PROTOCOL_DESCRIPTORS {
        let target = descriptors.remove(&descriptor).ok_or(
            ParserWorkerContainmentError::InvalidProtocolDescriptors {
                reason: "a required protocol descriptor is absent",
            },
        )?;
        if !is_pipe_target(&target) {
            return Err(ParserWorkerContainmentError::InvalidProtocolDescriptors {
                reason: "a protocol descriptor is not an anonymous pipe",
            });
        }
        if !protocol_targets.insert(target) {
            return Err(ParserWorkerContainmentError::InvalidProtocolDescriptors {
                reason: "protocol descriptors do not use distinct pipes",
            });
        }
    }
    if descriptors.len() > 1 || descriptors.values().any(|target| target != observer_target) {
        return Err(ParserWorkerContainmentError::InvalidProtocolDescriptors {
            reason: "an inherited descriptor exists outside the protocol set",
        });
    }
    Ok(())
}

/// Read the access mode of one inherited descriptor from procfs.
fn descriptor_access_mode(descriptor: u32) -> Result<u32, ParserWorkerContainmentError> {
    let path = PathBuf::from(format!("/proc/self/fdinfo/{descriptor}"));
    let info = read_bounded_kernel_text(&path, DESCRIPTOR_INFO_MAX_BYTES)
        .map_err(|source| ParserWorkerContainmentError::InspectProtocolDescriptors { source })?;
    let flags = info
        .lines()
        .find_map(|line| line.strip_prefix("flags:"))
        .map(str::trim)
        .ok_or(ParserWorkerContainmentError::InvalidProtocolDescriptors {
            reason: "descriptor flags are missing",
        })?;
    let flags = u32::from_str_radix(flags, 8).map_err(|_source| {
        ParserWorkerContainmentError::InvalidProtocolDescriptors {
            reason: "descriptor flags are malformed",
        }
    })?;
    Ok(flags & u32::try_from(libc::O_ACCMODE).unwrap_or(3))
}

/// Require only stdin/stdout/stderr plus the transient procfs observer descriptor.
fn observe_protocol_descriptors() -> Result<(), ParserWorkerContainmentError> {
    let descriptor_root = Path::new("/proc/self/fd");
    let observer_target = PathBuf::from(format!("/proc/{}/fd", process::id()));
    let mut descriptors = BTreeMap::new();
    let mut entries = fs::read_dir(descriptor_root)
        .map_err(|source| ParserWorkerContainmentError::InspectProtocolDescriptors { source })?;
    for entry in entries.by_ref() {
        let entry =
            entry.map_err(
                |source| ParserWorkerContainmentError::InspectProtocolDescriptors { source },
            )?;
        let descriptor = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(ParserWorkerContainmentError::InvalidProtocolDescriptors {
                reason: "descriptor identity is malformed",
            })?;
        let target = fs::read_link(entry.path()).map_err(|source| {
            ParserWorkerContainmentError::InspectProtocolDescriptors { source }
        })?;
        descriptors.insert(descriptor, target);
    }
    drop(entries);

    validate_protocol_descriptor_targets(&mut descriptors, &observer_target)?;
    if descriptor_access_mode(0)? != u32::try_from(libc::O_RDONLY).unwrap_or(0)
        || descriptor_access_mode(1)? != u32::try_from(libc::O_WRONLY).unwrap_or(1)
        || descriptor_access_mode(2)? != u32::try_from(libc::O_WRONLY).unwrap_or(1)
    {
        return Err(ParserWorkerContainmentError::InvalidProtocolDescriptors {
            reason: "protocol descriptor access modes are not exact",
        });
    }
    Ok(())
}

/// Validate the exact executable and policy-bound eager system-runtime mappings.
fn validate_runtime_mappings(
    mappings: &str,
    pack_root: &Path,
    worker_executable: &Path,
    expected_runtime_libraries: &[String],
) -> Result<(), ParserWorkerContainmentError> {
    if expected_runtime_libraries.is_empty()
        || expected_runtime_libraries.len() > MAX_EAGER_RUNTIME_LIBRARIES
        || expected_runtime_libraries
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || expected_runtime_libraries
            .iter()
            .any(|library| !is_runtime_library_basename(library))
    {
        return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
            reason: "the artifact runtime-mapping policy is not a bounded sorted basename set",
        });
    }
    let system_roots = SYSTEM_RUNTIME_ROOTS.map(Path::new);
    let expected = expected_runtime_libraries
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut observed_worker = false;
    let mut observed_vdso = false;
    let mut observed_runtime_libraries = BTreeSet::new();
    for line in mappings.lines() {
        let mut remaining = line;
        for _field in 0..5 {
            take_mapping_field(&mut remaining).ok_or(
                ParserWorkerContainmentError::InvalidRuntimeMappings {
                    reason: "a runtime mapping row is malformed",
                },
            )?;
        }
        let mapped =
            remaining.trim_start_matches(|character: char| character.is_ascii_whitespace());
        if mapped.is_empty() {
            continue;
        }
        if mapped == "[vdso]" {
            if observed_vdso {
                return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                    reason: "the Linux vDSO mapping is duplicated",
                });
            }
            observed_vdso = true;
            continue;
        }
        // Other kernel-owned anonymous regions have no file or DSO identity.
        if mapped.starts_with('[') && mapped.ends_with(']') {
            continue;
        }
        if mapped.ends_with(" (deleted)") {
            return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                reason: "a mapped runtime object is deleted",
            });
        }
        let mapped = Path::new(mapped);
        if !mapped.is_absolute() {
            return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                reason: "a mapped runtime object is not absolute",
            });
        }
        if mapped == worker_executable {
            observed_worker = true;
        } else if mapped.starts_with(pack_root) {
            return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                reason: "a pack object other than the trusted worker was mapped before admission",
            });
        } else if system_roots.iter().any(|root| mapped.starts_with(root)) {
            let library = mapped.file_name().and_then(|name| name.to_str()).ok_or(
                ParserWorkerContainmentError::InvalidRuntimeMappings {
                    reason: "a system runtime mapping has no UTF-8 basename",
                },
            )?;
            if !expected.contains(library) {
                return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                    reason: "a system runtime mapping is absent from the artifact policy",
                });
            }
            observed_runtime_libraries.insert(library);
        } else {
            return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
                reason: "a mapped runtime object is outside admitted roots",
            });
        }
    }
    if !observed_worker || !observed_vdso || observed_runtime_libraries != expected {
        return Err(ParserWorkerContainmentError::InvalidRuntimeMappings {
            reason: "the exact worker, Linux vDSO, and eager runtime mapping set were not all observed",
        });
    }
    Ok(())
}

/// Return whether one artifact-provided runtime identity is a safe basename.
fn is_runtime_library_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Consume one fixed whitespace-delimited procfs mapping column.
fn take_mapping_field<'a>(remaining: &mut &'a str) -> Option<&'a str> {
    *remaining = remaining.trim_start_matches(|character: char| character.is_ascii_whitespace());
    if remaining.is_empty() {
        return None;
    }
    if let Some(separator) = remaining.find(|character: char| character.is_ascii_whitespace()) {
        let field = &remaining[..separator];
        *remaining = &remaining[separator..];
        Some(field)
    } else {
        let field = *remaining;
        *remaining = "";
        Some(field)
    }
}

/// Observe eager file-backed runtime mappings before Landlock closes the filesystem.
fn observe_runtime_mappings(
    pack_root: &Path,
    expected_runtime_libraries: &[String],
) -> Result<(), ParserWorkerContainmentError> {
    let mappings =
        read_bounded_kernel_text(Path::new("/proc/self/maps"), PROCESS_MAPPINGS_MAX_BYTES)
            .map_err(|source| ParserWorkerContainmentError::InspectRuntimeMappings { source })?;
    let worker_executable = fs::canonicalize("/proc/self/exe")
        .map_err(|source| ParserWorkerContainmentError::InspectRuntimeMappings { source })?;
    validate_runtime_mappings(
        &mappings,
        pack_root,
        &worker_executable,
        expected_runtime_libraries,
    )
}

/// Require exactly the process main thread before installing seccomp.
fn observe_single_thread() -> Result<(), ParserWorkerContainmentError> {
    let mut observed = 0_usize;
    let expected = process::id().to_string();
    for entry in fs::read_dir("/proc/self/task")
        .map_err(|source| ParserWorkerContainmentError::InspectThreadState { source })?
    {
        let entry =
            entry.map_err(|source| ParserWorkerContainmentError::InspectThreadState { source })?;
        let identity = entry.file_name();
        if identity.to_str().is_some_and(|value| value == expected) {
            observed = observed.saturating_add(1);
        } else if identity
            .to_str()
            .is_some_and(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        {
            observed = observed.saturating_add(1);
        } else {
            return Err(ParserWorkerContainmentError::InvalidThreadState { observed });
        }
    }
    if observed != 1 {
        return Err(ParserWorkerContainmentError::InvalidThreadState { observed });
    }
    Ok(())
}

/// Observe every Linux admission prerequisite from kernel-owned process state.
///
/// # Errors
///
/// Returns a typed precondition failure for extra or misdirected descriptors,
/// non-eager runtime mappings, or any additional starting thread.
pub(crate) fn observe_parser_worker_preconditions(
    pack_root: &Path,
    expected_runtime_libraries: &[String],
) -> Result<ParserWorkerContainmentPreconditions, ParserWorkerContainmentError> {
    accepted_target_architecture()?;
    let canonical_pack_root = pack_root.canonicalize().map_err(|source| {
        ParserWorkerContainmentError::CanonicalizePackRoot {
            path: pack_root.to_path_buf(),
            source,
        }
    })?;
    if !canonical_pack_root.is_dir() {
        return Err(ParserWorkerContainmentError::PackRootNotDirectory {
            path: canonical_pack_root,
        });
    }
    observe_protocol_descriptors()?;
    observe_runtime_mappings(&canonical_pack_root, expected_runtime_libraries)?;
    observe_single_thread()?;
    Ok(ParserWorkerContainmentPreconditions { _private: () })
}

/// Return the accepted seccompiler target or reject an unshipped Linux pack target.
fn accepted_target_architecture() -> Result<TargetArch, ParserWorkerContainmentError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(TargetArch::x86_64),
        architecture => Err(ParserWorkerContainmentError::UnsupportedArchitecture { architecture }),
    }
}

/// Build the fixed syscall-denial map without allocating rule conditions.
fn seccomp_policy() -> BTreeMap<i64, Vec<seccompiler::SeccompRule>> {
    PROCESS_AND_EXEC_SYSCALLS
        .iter()
        .chain(NETWORK_SYSCALLS)
        .chain(NAMESPACE_SYSCALLS)
        .chain(PROCESS_ESCAPE_SYSCALLS)
        .chain(KERNEL_ESCAPE_SYSCALLS)
        .chain(FILESYSTEM_METADATA_AND_DIRECTORY_SYSCALLS)
        .chain(PERSISTENT_IPC_SYSCALLS)
        .map(|number| (*number, Vec::new()))
        .collect()
}

/// Select a finite hard ceiling without raising an inherited soft or hard limit.
fn effective_resource_limit(
    inherited_soft: rlim_t,
    inherited_hard: rlim_t,
    configured: rlim_t,
) -> rlim_t {
    inherited_soft.min(inherited_hard).min(configured)
}

/// Apply and read back every hard process resource limit.
fn enforce_resource_limits() -> Result<(), ParserWorkerContainmentError> {
    for rule in resource_limit_policy() {
        let (inherited_soft, inherited_hard) = getrlimit(rule.resource).map_err(|source| {
            ParserWorkerContainmentError::ReadResourceLimit {
                resource: rule.name,
                source,
            }
        })?;
        let effective = effective_resource_limit(inherited_soft, inherited_hard, rule.value);
        setrlimit(rule.resource, effective, effective).map_err(|source| {
            ParserWorkerContainmentError::SetResourceLimit {
                resource: rule.name,
                source,
            }
        })?;
        let (actual_soft, actual_hard) = getrlimit(rule.resource).map_err(|source| {
            ParserWorkerContainmentError::ReadResourceLimit {
                resource: rule.name,
                source,
            }
        })?;
        if (actual_soft, actual_hard) != (effective, effective) {
            return Err(ParserWorkerContainmentError::ResourceLimitMismatch {
                resource: rule.name,
                expected: effective,
                actual_soft,
                actual_hard,
            });
        }
    }
    Ok(())
}

/// Read one required hexadecimal capability field from `/proc/self/status`.
fn process_status_capability(
    status: &str,
    field: &'static str,
) -> Result<u128, ParserWorkerContainmentError> {
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ParserWorkerContainmentError::InvalidProcessIdentity { field })?;
    u128::from_str_radix(value, 16)
        .map_err(|_| ParserWorkerContainmentError::InvalidProcessIdentity { field })
}

/// Parse one exact real/effective/saved/filesystem identity tuple from procfs.
fn process_status_identity(
    status: &str,
    field: &'static str,
) -> Result<[u32; 4], ParserWorkerContainmentError> {
    let mut values = status
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .map(str::split_whitespace)
        .ok_or(ParserWorkerContainmentError::InvalidProcessIdentity { field })?;
    let identities = {
        let mut next_identity = || {
            values
                .next()
                .ok_or(ParserWorkerContainmentError::InvalidProcessIdentity { field })?
                .parse::<u32>()
                .map_err(|_source| ParserWorkerContainmentError::InvalidProcessIdentity { field })
        };
        [
            next_identity()?,
            next_identity()?,
            next_identity()?,
            next_identity()?,
        ]
    };
    if values.next().is_some() {
        return Err(ParserWorkerContainmentError::InvalidProcessIdentity { field });
    }
    Ok(identities)
}

/// Reject root or mismatched real/effective/saved/filesystem identities.
fn require_consistent_unprivileged_identity(
    status: &str,
    field: &'static str,
) -> Result<(), ParserWorkerContainmentError> {
    let identities = process_status_identity(status, field)?;
    if identities.contains(&0) {
        return Err(ParserWorkerContainmentError::PrivilegedProcessIdentity { field });
    }
    if identities[1..]
        .iter()
        .any(|identity| *identity != identities[0])
    {
        return Err(ParserWorkerContainmentError::InconsistentProcessIdentity { field });
    }
    Ok(())
}

/// Reject root, mixed identities, and every inherited capability set.
fn verify_unprivileged_process_status(status: &str) -> Result<(), ParserWorkerContainmentError> {
    require_consistent_unprivileged_identity(status, "Uid:")?;
    require_consistent_unprivileged_identity(status, "Gid:")?;
    for field in ["CapInh:", "CapPrm:", "CapEff:", "CapAmb:"] {
        if process_status_capability(status, field)? != 0 {
            return Err(ParserWorkerContainmentError::PrivilegedProcessIdentity { field });
        }
    }
    Ok(())
}

/// Read and validate the kernel-owned process identity before irreversible restriction.
fn enforce_unprivileged_process_identity() -> Result<(), ParserWorkerContainmentError> {
    let mut status = String::new();
    File::open("/proc/self/status")
        .and_then(|file| {
            file.take(PROCESS_STATUS_MAX_BYTES.saturating_add(1))
                .read_to_string(&mut status)
        })
        .map_err(|source| ParserWorkerContainmentError::ReadProcessIdentity { source })?;
    if u64::try_from(status.len()).unwrap_or(u64::MAX) > PROCESS_STATUS_MAX_BYTES {
        return Err(ParserWorkerContainmentError::InvalidProcessIdentity {
            field: "/proc/self/status",
        });
    }
    verify_unprivileged_process_status(&status)
}

/// Apply and read back the irreversible privilege-escalation guard.
fn enforce_no_new_privileges() -> Result<(), ParserWorkerContainmentError> {
    set_no_new_privs()
        .map_err(|source| ParserWorkerContainmentError::SetNoNewPrivileges { source })?;
    let enforced = get_no_new_privs()
        .map_err(|source| ParserWorkerContainmentError::ReadNoNewPrivileges { source })?;
    if !enforced {
        return Err(ParserWorkerContainmentError::NoNewPrivilegesNotEnforced);
    }
    Ok(())
}

/// Restrict filesystem access to read-only pack descendants without execute grants.
fn enforce_landlock(pack_root: PathFd) -> Result<(), ParserWorkerContainmentError> {
    let status = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V3))
        .map_err(|source| ParserWorkerContainmentError::Landlock { source })?
        .create()
        .map_err(|source| ParserWorkerContainmentError::Landlock { source })?
        .set_compatibility(CompatLevel::HardRequirement)
        .add_rule(PathBeneath::new(pack_root, pack_read_access()))
        .map_err(|source| ParserWorkerContainmentError::Landlock { source })?
        .restrict_self()
        .map_err(|source| ParserWorkerContainmentError::Landlock { source })?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(ParserWorkerContainmentError::LandlockNotFullyEnforced);
    }
    Ok(())
}

/// Compile and install one seccomp filter on the worker's only thread.
fn enforce_seccomp() -> Result<(), ParserWorkerContainmentError> {
    let target_arch = accepted_target_architecture()?;
    let filter = SeccompFilter::new(
        seccomp_policy(),
        SeccompAction::Allow,
        SeccompAction::KillProcess,
        target_arch,
    )
    .map_err(|source| ParserWorkerContainmentError::CompileSeccomp { source })?;
    let program: BpfProgram = filter
        .try_into()
        .map_err(|source| ParserWorkerContainmentError::CompileSeccomp { source })?;
    apply_filter(&program).map_err(|source| ParserWorkerContainmentError::InstallSeccomp { source })
}

/// Establish the complete Linux parser-worker self-containment boundary.
///
/// The returned path is the canonical immutable pack root admitted by Landlock.
/// The caller may read the first bounded `SessionOpen` only after this function
/// succeeds.
///
/// # Errors
///
/// Returns a typed stage failure when the accepted architecture, pack root,
/// resource limits, `no_new_privs`, Landlock, or seccomp cannot be proven.
/// Any error may follow irreversible partial restriction; the caller must exit.
pub(crate) fn enforce_parser_worker_containment(
    pack_root: &Path,
    _preconditions: ParserWorkerContainmentPreconditions,
) -> Result<PathBuf, ParserWorkerContainmentError> {
    accepted_target_architecture()?;
    let canonical_pack_root = pack_root.canonicalize().map_err(|source| {
        ParserWorkerContainmentError::CanonicalizePackRoot {
            path: pack_root.to_path_buf(),
            source,
        }
    })?;
    if !canonical_pack_root.is_dir() {
        return Err(ParserWorkerContainmentError::PackRootNotDirectory {
            path: canonical_pack_root,
        });
    }
    let landlock_pack_root = PathFd::new(&canonical_pack_root)
        .map_err(|source| ParserWorkerContainmentError::OpenPackRoot { source })?;

    enforce_unprivileged_process_identity()?;
    enforce_resource_limits()?;
    enforce_no_new_privileges()?;
    enforce_landlock(landlock_pack_root)?;
    enforce_seccomp()?;
    Ok(canonical_pack_root)
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    /// Keep the inherited descriptor contract limited to three distinct anonymous pipes.
    #[test]
    fn protocol_descriptor_targets_are_exact() {
        assert!(is_pipe_target(Path::new("pipe:[123]")));
        assert!(!is_pipe_target(Path::new("/dev/null")));
        assert!(!is_pipe_target(Path::new("socket:[123]")));

        let observer_target = Path::new("/proc/42/fd");
        let mut accepted = BTreeMap::from([
            (0, PathBuf::from("pipe:[100]")),
            (1, PathBuf::from("pipe:[101]")),
            (2, PathBuf::from("pipe:[102]")),
            (3, observer_target.to_path_buf()),
        ]);
        assert!(validate_protocol_descriptor_targets(&mut accepted, observer_target).is_ok());

        let mut aliased = BTreeMap::from([
            (0, PathBuf::from("pipe:[100]")),
            (1, PathBuf::from("pipe:[101]")),
            (2, PathBuf::from("pipe:[101]")),
            (3, observer_target.to_path_buf()),
        ]);
        assert!(matches!(
            validate_protocol_descriptor_targets(&mut aliased, observer_target),
            Err(ParserWorkerContainmentError::InvalidProtocolDescriptors {
                reason: "protocol descriptors do not use distinct pipes"
            })
        ));
    }

    /// Accept only eager pack and system-library mappings.
    #[test]
    fn runtime_mapping_policy_is_fail_closed() {
        let accepted = "00400000-00401000 r-xp 00000000 00:00 1 /opt/pack/projectatlas-parser-worker\n\
                        7f000000-7f001000 r-xp 00000000 00:00 2 /usr/lib/libc.so.6\n\
                        7f000100-7f001100 r-xp 00000000 00:00 4 /usr/lib/libm.so.6\n\
                        7f001000-7f002000 r-xp 00000000 00:00 3 /lib64/ld-linux-x86-64.so.2\n\
                        7f002000-7f003000 r-xp 00000000 00:00 0 [vdso]\n\
                        7f001000-7f002000 rw-p 00000000 00:00 0\n";
        let expected = vec![
            "ld-linux-x86-64.so.2".to_owned(),
            "libc.so.6".to_owned(),
            "libm.so.6".to_owned(),
        ];
        assert!(
            validate_runtime_mappings(
                accepted,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            )
            .is_ok()
        );

        let spaced_pack = accepted.replace(
            "/opt/pack/projectatlas-parser-worker",
            "/opt/Project Atlas/pack/projectatlas-parser-worker",
        );
        assert!(
            validate_runtime_mappings(
                &spaced_pack,
                Path::new("/opt/Project Atlas/pack"),
                Path::new("/opt/Project Atlas/pack/projectatlas-parser-worker"),
                &expected,
            )
            .is_ok()
        );

        let repository_mapping = format!(
            "{accepted}7f002000-7f003000 r--p 00000000 00:00 3 /workspace/repository/input\n"
        );
        assert!(matches!(
            validate_runtime_mappings(
                &repository_mapping,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            ),
            Err(ParserWorkerContainmentError::InvalidRuntimeMappings { .. })
        ));

        let deleted_mapping =
            accepted.replace("/usr/lib/libc.so.6", "/usr/lib/libc.so.6 (deleted)");
        assert!(matches!(
            validate_runtime_mappings(
                &deleted_mapping,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            ),
            Err(ParserWorkerContainmentError::InvalidRuntimeMappings { .. })
        ));

        let injected = accepted.replace(
            "7f001000-7f002000 rw-p",
            "7f003000-7f004000 r-xp 00000000 00:00 4 /usr/lib/libinjected.so\n7f001000-7f002000 rw-p",
        );
        assert!(matches!(
            validate_runtime_mappings(
                &injected,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            ),
            Err(ParserWorkerContainmentError::InvalidRuntimeMappings { .. })
        ));

        let missing_loader = accepted
            .lines()
            .filter(|line| !line.contains("ld-linux-x86-64.so.2"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            validate_runtime_mappings(
                &missing_loader,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            ),
            Err(ParserWorkerContainmentError::InvalidRuntimeMappings { .. })
        ));

        let missing_vdso = accepted
            .lines()
            .filter(|line| !line.contains("[vdso]"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            validate_runtime_mappings(
                &missing_vdso,
                Path::new("/opt/pack"),
                Path::new("/opt/pack/projectatlas-parser-worker"),
                &expected,
            ),
            Err(ParserWorkerContainmentError::InvalidRuntimeMappings { .. })
        ));
    }

    /// Keep every configured process limit finite and hard.
    #[test]
    fn resource_limit_policy_is_bounded() {
        let rules = resource_limit_policy();
        assert_eq!(rules.len(), 6);
        assert!(rules.iter().all(|rule| rule.value != rlim_t::MAX));
        assert_eq!(FILE_SIZE_BYTES, 0);
        assert_eq!(CORE_DUMP_BYTES, 0);
        assert_eq!(PROCESS_COUNT, 1);
        assert_eq!(effective_resource_limit(16, 32, 64), 16);
        assert_eq!(effective_resource_limit(64, 32, 128), 32);
        assert_eq!(effective_resource_limit(128, 128, 64), 64);
    }

    /// Require exact non-root identities and reject every inherited capability set.
    #[test]
    fn process_identity_policy_requires_consistent_unprivileged_ids() {
        let ordinary = "Uid:\t1000\t1000\t1000\t1000\n\
                        Gid:\t1000\t1000\t1000\t1000\n\
                        CapInh:\t0000000000000000\n\
                        CapPrm:\t0000000000000000\n\
                        CapEff:\t0000000000000000\n\
                        CapAmb:\t0000000000000000\n";
        assert!(verify_unprivileged_process_status(ordinary).is_ok());

        let root_uid = ordinary.replace("Uid:\t1000\t1000\t1000\t1000", "Uid:\t0\t0\t0\t0");
        assert!(matches!(
            verify_unprivileged_process_status(&root_uid),
            Err(ParserWorkerContainmentError::PrivilegedProcessIdentity { field: "Uid:" })
        ));

        let root_gid = ordinary.replace("Gid:\t1000\t1000\t1000\t1000", "Gid:\t0\t0\t0\t0");
        assert!(matches!(
            verify_unprivileged_process_status(&root_gid),
            Err(ParserWorkerContainmentError::PrivilegedProcessIdentity { field: "Gid:" })
        ));

        let mismatched_uid = ordinary.replace(
            "Uid:\t1000\t1000\t1000\t1000",
            "Uid:\t1000\t1001\t1000\t1000",
        );
        assert!(matches!(
            verify_unprivileged_process_status(&mismatched_uid),
            Err(ParserWorkerContainmentError::InconsistentProcessIdentity { field: "Uid:" })
        ));

        let mismatched_gid = ordinary.replace(
            "Gid:\t1000\t1000\t1000\t1000",
            "Gid:\t1000\t1000\t1001\t1000",
        );
        assert!(matches!(
            verify_unprivileged_process_status(&mismatched_gid),
            Err(ParserWorkerContainmentError::InconsistentProcessIdentity { field: "Gid:" })
        ));

        let capable = ordinary.replace("CapEff:\t0000000000000000", "CapEff:\t0000000000000001");
        assert!(matches!(
            verify_unprivileged_process_status(&capable),
            Err(ParserWorkerContainmentError::PrivilegedProcessIdentity { field: "CapEff:" })
        ));
        assert!(matches!(
            verify_unprivileged_process_status(
                &ordinary.replace("Uid:\t1000\t1000\t1000\t1000", "Uid:\t1000\t1000\t1000",)
            ),
            Err(ParserWorkerContainmentError::InvalidProcessIdentity { .. })
        ));
        assert!(matches!(
            verify_unprivileged_process_status(&ordinary.replace(
                "Gid:\t1000\t1000\t1000\t1000",
                "Gid:\t1000\t1000\t1000\t1000\t1000",
            )),
            Err(ParserWorkerContainmentError::InvalidProcessIdentity { .. })
        ));
    }

    /// Grant pack reads without granting execution or mutation.
    #[test]
    fn pack_policy_is_read_only_and_non_executable() {
        let access = pack_read_access();
        assert!(access.contains(AccessFs::ReadFile));
        assert!(access.contains(AccessFs::ReadDir));
        assert!(!access.contains(AccessFs::Execute));
        assert!((access & AccessFs::from_write(ABI::V3)).is_empty());
    }

    /// Keep each denied syscall in one closed family and compile the fixed filter.
    #[test]
    fn seccomp_policy_is_complete_and_unique() -> Result<(), Box<dyn std::error::Error>> {
        let family_total = PROCESS_AND_EXEC_SYSCALLS.len()
            + NETWORK_SYSCALLS.len()
            + NAMESPACE_SYSCALLS.len()
            + PROCESS_ESCAPE_SYSCALLS.len()
            + KERNEL_ESCAPE_SYSCALLS.len()
            + FILESYSTEM_METADATA_AND_DIRECTORY_SYSCALLS.len()
            + PERSISTENT_IPC_SYSCALLS.len();
        let policy = seccomp_policy();
        let unique = policy.keys().copied().collect::<BTreeSet<_>>();
        assert_eq!(policy.len(), family_total);
        assert_eq!(unique.len(), family_total);
        assert!(PROCESS_AND_EXEC_SYSCALLS.contains(&libc::SYS_execve));
        assert!(NETWORK_SYSCALLS.contains(&libc::SYS_socket));
        assert!(NAMESPACE_SYSCALLS.contains(&libc::SYS_unshare));
        assert!(PROCESS_ESCAPE_SYSCALLS.contains(&libc::SYS_process_vm_writev));
        assert!(PROCESS_ESCAPE_SYSCALLS.contains(&libc::SYS_rt_sigqueueinfo));
        assert!(PROCESS_ESCAPE_SYSCALLS.contains(&libc::SYS_prlimit64));
        assert!(PROCESS_ESCAPE_SYSCALLS.contains(&libc::SYS_setpgid));
        assert!(PROCESS_ESCAPE_SYSCALLS.contains(&libc::SYS_setsid));
        assert!(KERNEL_ESCAPE_SYSCALLS.contains(&libc::SYS_io_uring_setup));
        assert_eq!(
            FILESYSTEM_METADATA_AND_DIRECTORY_SYSCALLS,
            &[
                libc::SYS_chdir,
                libc::SYS_fchdir,
                libc::SYS_chmod,
                libc::SYS_fchmod,
                libc::SYS_fchmodat,
                libc::SYS_fchmodat2,
                libc::SYS_chown,
                libc::SYS_fchown,
                libc::SYS_lchown,
                libc::SYS_fchownat,
                libc::SYS_setxattr,
                libc::SYS_lsetxattr,
                libc::SYS_fsetxattr,
                libc::SYS_removexattr,
                libc::SYS_lremovexattr,
                libc::SYS_fremovexattr,
                libc::SYS_utime,
                libc::SYS_utimes,
                libc::SYS_futimesat,
                libc::SYS_utimensat,
            ]
        );
        assert_eq!(
            PERSISTENT_IPC_SYSCALLS,
            &[
                libc::SYS_shmget,
                libc::SYS_shmat,
                libc::SYS_shmdt,
                libc::SYS_shmctl,
                libc::SYS_semget,
                libc::SYS_semop,
                libc::SYS_semtimedop,
                libc::SYS_semctl,
                libc::SYS_msgget,
                libc::SYS_msgsnd,
                libc::SYS_msgrcv,
                libc::SYS_msgctl,
                libc::SYS_mq_open,
                libc::SYS_mq_unlink,
                libc::SYS_mq_timedsend,
                libc::SYS_mq_timedreceive,
                libc::SYS_mq_notify,
                libc::SYS_mq_getsetattr,
            ]
        );

        let target = accepted_target_architecture()?;
        let filter = SeccompFilter::new(
            policy,
            SeccompAction::Allow,
            SeccompAction::KillProcess,
            target,
        )?;
        let program: BpfProgram = filter.try_into()?;
        assert!(!program.is_empty());
        Ok(())
    }
}
