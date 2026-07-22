//! Explicit local lifecycle for the separately shipped optional parser pack.

use crate::parser_supervisor::{
    OptionalParserSupervisor, ParserSupervisorError, admit_optional_parser_artifact,
};
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_ID, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES, OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES,
    OPTIONAL_PARSER_PACK_MAX_FILE_BYTES, OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES,
    OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION, OptionalParserPackArtifactManifest,
    OptionalParserPackManifest, OptionalParserPackManifestError, PackPlatform, PackRelativePath,
};
use projectatlas_core::optional_parser_protocol::{ParserArtifactIdentity, ParserContentDigest};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
use std::process::{Command, Stdio};
use std::sync::OnceLock;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
use std::thread;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
use std::time::{Duration, Instant};
#[cfg(test)]
use tar::EntryType;
use tempfile::{NamedTempFile, TempDir};
use thiserror::Error;

/// Canonical top-level directory inside every completed parser-pack archive.
const ARCHIVE_ROOT: &str = "projectatlas-broad-parser";
/// Logical capability manifest at the artifact root.
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
/// Platform artifact manifest at the artifact root.
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
/// Project-local selection metadata, deliberately inside the excluded atlas state directory.
pub const OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH: &str =
    ".projectatlas/optional-parser-pack.json";
/// Current strict project-selection schema.
const PROJECT_SELECTION_SCHEMA_VERSION: u32 = 1;
/// Maximum project-selection bytes accepted from disk.
const PROJECT_SELECTION_MAX_BYTES: u64 = 16 * 1024;
/// Maximum decompressed tar framing beyond manifest-bounded file payloads.
const TAR_FRAMING_ALLOWANCE_BYTES: u64 = 1024 * 1024;
/// Canonical regular payload mode in a completed archive.
const PAYLOAD_MODE: u32 = 0o644;
/// Canonical parser-worker mode in a completed archive.
const WORKER_MODE: u32 = 0o755;
/// Maximum lifecycle-owned directory entries inspected by one metadata operation.
const LIFECYCLE_METADATA_ENTRY_LIMIT: usize = 1_024;
/// Stable sibling lease retained across logical-pack removal and process crashes.
const OPTIONAL_PARSER_PACK_LEASE_FILE_NAME: &str = ".projectatlas-broad-parser.lifecycle.lock";
/// Stable project-local lease serializing selection read-modify-write transitions.
/// Operations needing both leases always acquire the pack lease first.
const OPTIONAL_PARSER_SELECTION_LEASE_FILE_NAME: &str = "optional-parser-pack.selection.lock";
/// Exact artifact-bound Windows containment broker file.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_CONTAINMENT_BROKER_FILE_NAME: &str = "projectatlas-parser-containment.exe";
/// Closed broker operation that removes only its own artifact-scoped profile and ACEs.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_PROFILE_CLEANUP_ARGUMENT: &str = "cleanup-artifact-profile";
/// Fixed successful cleanup result emitted by the trusted broker.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_PROFILE_CLEANUP_RESULT: &str = "[parser-containment] artifact profile cleanup passed";
/// Hard deadline for one trusted profile cleanup command.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_PROFILE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard post-operation ceiling for termination, reap, and pipe-reader joins.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_PROFILE_CLEANUP_REAP_TIMEOUT: Duration = Duration::from_secs(5);
/// Bounded stdout and stderr retained from one cleanup command.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_PROFILE_CLEANUP_OUTPUT_BYTES: u64 = 64 * 1024;
/// Prefix for a unique slot tombstone whose profile cleanup is still pending.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_REMOVING_TOMBSTONE_PREFIX: &str = ".projectatlas-parser-removing-";
/// Prefix for a unique tombstone whose profile is cleaned and only deletion remains.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const WINDOWS_CLEANED_TOMBSTONE_PREFIX: &str = ".projectatlas-parser-cleaned-";

/// Failure while inspecting or changing the local optional-parser lifecycle.
#[derive(Debug, Error)]
pub enum OptionalParserPackLifecycleError {
    /// This host has no accepted parser containment adapter.
    #[error("optional parser containment is unsupported on {os}/{architecture}")]
    UnsupportedContainment {
        /// Host operating-system identity.
        os: &'static str,
        /// Host architecture identity.
        architecture: &'static str,
    },
    /// A required user-owned storage location could not be derived.
    #[error("could not determine the user-owned optional parser-pack storage root")]
    StorageRootUnavailable,
    /// Another process currently holds an incompatible parser-pack lifecycle lease.
    #[error(
        "optional parser-pack lifecycle is busy at {path:?}; retry after the active operation finishes"
    )]
    Busy {
        /// Exact user-owned lease file that could not be locked.
        path: PathBuf,
    },
    /// One filesystem operation failed.
    #[error("{operation} failed for {path:?}: {source}")]
    Io {
        /// Stable operation description.
        operation: &'static str,
        /// Exact lifecycle-owned path involved.
        path: PathBuf,
        /// Source filesystem failure.
        #[source]
        source: io::Error,
    },
    /// An archive or lifecycle record violated a closed contract.
    #[error("optional parser-pack lifecycle data is invalid: {reason}")]
    InvalidData {
        /// Bounded rejection reason.
        reason: String,
    },
    /// A logical or artifact manifest failed validation.
    #[error("{0}")]
    Manifest(#[from] OptionalParserPackManifestError),
    /// A verified artifact failed supervisor admission.
    #[error("{0}")]
    Supervisor(#[from] ParserSupervisorError),
    /// Strict JSON decoding or encoding failed.
    #[error("optional parser-pack JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    /// One or more exact slot cleanup attempts failed after all slots were attempted.
    #[error("optional parser-pack cleanup was incomplete: {message}")]
    CleanupIncomplete {
        /// Bounded aggregate of artifact-scoped cleanup failures.
        message: String,
    },
    /// A lifecycle operation failed and mandatory child cleanup also failed.
    #[error("optional parser-pack lifecycle operation failed and cleanup also failed")]
    OperationAndCleanup {
        /// Original typed lifecycle failure.
        operation: Box<Self>,
        /// Typed mandatory cleanup failure.
        cleanup: Box<Self>,
    },
}

impl OptionalParserPackLifecycleError {
    /// Return whether this failure is the typed unsupported-containment boundary.
    #[must_use]
    pub const fn is_unsupported_containment(&self) -> bool {
        matches!(self, Self::UnsupportedContainment { .. })
    }
}

/// Preserve both a lifecycle operation failure and its mandatory cleanup failure.
fn finish_with_cleanup<T>(
    operation: Result<T, OptionalParserPackLifecycleError>,
    cleanup: Result<(), OptionalParserPackLifecycleError>,
) -> Result<T, OptionalParserPackLifecycleError> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(operation), Ok(())) => Err(operation),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(operation), Err(cleanup)) => {
            Err(OptionalParserPackLifecycleError::OperationAndCleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            })
        }
    }
}

/// Explicit parser-pack lifecycle operation represented in structured output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionalParserPackOperation {
    /// Validate a completed archive without installing it.
    Verify,
    /// Install a completed archive into an immutable user-owned slot.
    Install,
    /// Select an already installed immutable slot for the current project.
    Enable,
    /// Install and atomically select a replacement while retaining rollback identity.
    Update,
    /// Remove the current project's selection without deleting installed slots.
    Disable,
    /// Disable this project and delete this logical pack's user-owned slots.
    Remove,
    /// Inspect content-free local lifecycle state.
    Status,
}

/// Stable content-free lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionalParserPackState {
    /// No accepted containment adapter exists on this host.
    UnsupportedContainment,
    /// No installed slot or project selection exists.
    Absent,
    /// At least one slot is installed but this project is disabled.
    InstalledDisabled,
    /// This project selects a present immutable slot.
    Enabled,
    /// This project selects a present slot and retains a present rollback slot.
    RollbackReady,
    /// Local selection or slot metadata is malformed, missing, or structurally unsafe.
    Stale,
}

/// Content-free identity and presence of one immutable parser-pack slot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OptionalParserPackSlotReport {
    /// `ProjectAtlas` release line that owns the slot namespace.
    pub projectatlas_version: String,
    /// BLAKE3 identity of the exact artifact-manifest bytes.
    pub artifact: String,
    /// Whether the exact immutable slot directory is present.
    pub present: bool,
}

/// Structured result shared by lifecycle commands and later settings integration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OptionalParserPackLifecycleReport {
    /// Operation that produced this report.
    pub operation: OptionalParserPackOperation,
    /// Current content-free lifecycle state.
    pub state: OptionalParserPackState,
    /// Stable identity of the one logical optional parser pack.
    pub pack_id: &'static str,
    /// Whether the current host has an accepted containment adapter.
    pub supported: bool,
    /// Accepted target triple for this host, when supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<&'static str>,
    /// Number of installed immutable slots observed within the bounded status scan.
    pub installed_slots: usize,
    /// Whether lifecycle metadata traversal reached its directory-entry bound.
    pub installed_slots_truncated: bool,
    /// Current project selection, including missing-slot state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<OptionalParserPackSlotReport>,
    /// Previous project selection retained by the last successful update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback: Option<OptionalParserPackSlotReport>,
    /// Artifact targeted by verify, install, enable, or update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<OptionalParserPackSlotReport>,
    /// Whether this command changed durable lifecycle state.
    pub changed: bool,
}

/// Owns cleanup of an artifact profile created from a temporary pack root.
///
/// Callers must either hold the parser-pack lifecycle's exclusive lease or run
/// on an isolated host where no installed copy of the same artifact can be in
/// use. Normal control flow must call [`Self::cleanup`]; `Drop` is only a
/// best-effort fallback for unwinding or a forgotten terminal action.
#[must_use = "temporary parser artifact profiles require cleanup or installed-slot transfer"]
pub struct TemporaryParserArtifactProfile {
    /// Extracted pack root containing the exact cleanup broker and manifest.
    pack_root: PathBuf,
    /// Identity of the exact artifact manifest that owns the profile.
    artifact: ParserArtifactIdentity,
    /// Whether cleanup ownership has not been completed or transferred.
    cleanup_pending: bool,
}

impl TemporaryParserArtifactProfile {
    /// Arm cleanup ownership from an already verified parser artifact authority.
    ///
    /// The caller must hold the lifecycle's exclusive pack lease or run on an
    /// isolated verifier host before allowing packaged code to execute.
    pub fn for_verified_supervisor(supervisor: &OptionalParserSupervisor) -> Self {
        Self {
            pack_root: supervisor.pack_root().to_path_buf(),
            artifact: supervisor.artifact_identity().clone(),
            cleanup_pending: true,
        }
    }

    /// Build a cleanup owner for focused lifecycle state tests.
    #[cfg(test)]
    fn new(pack_root: impl Into<PathBuf>, artifact: ParserArtifactIdentity) -> Self {
        Self {
            pack_root: pack_root.into(),
            artifact,
            cleanup_pending: true,
        }
    }

    /// Perform the one explicit fallible artifact-profile cleanup attempt.
    ///
    /// # Errors
    ///
    /// Returns a typed lifecycle error when the exact broker cannot validate or
    /// remove its artifact-scoped profile and access-control entries.
    pub fn cleanup(mut self) -> Result<(), OptionalParserPackLifecycleError> {
        self.cleanup_pending_profile()
    }

    /// Transfer cleanup ownership to the newly published immutable slot.
    fn transfer_to_installed_slot(&mut self) {
        self.cleanup_pending = false;
    }

    /// Run cleanup at most once while the temporary pack root still exists.
    fn cleanup_pending_profile(&mut self) -> Result<(), OptionalParserPackLifecycleError> {
        if !self.cleanup_pending {
            return Ok(());
        }
        cleanup_platform_profile(&self.pack_root, &self.artifact)?;
        self.cleanup_pending = false;
        Ok(())
    }
}

impl Drop for TemporaryParserArtifactProfile {
    fn drop(&mut self) {
        drop(self.cleanup_pending_profile());
    }
}

/// Concrete owner of project selection and user-owned immutable pack slots.
#[derive(Clone, Debug)]
pub struct OptionalParserPackLifecycle {
    /// Selected project whose local enablement is owned by this lifecycle.
    project_root: PathBuf,
    /// User-owned root containing versioned immutable slots.
    storage_root: OnceLock<Option<PathBuf>>,
    /// Accepted containment target for the current host.
    platform: Option<PackPlatform>,
    /// Test-only failure seam proving admission precedes lifecycle mutation.
    #[cfg(test)]
    admission_failure: Option<fn(&Path) -> ParserSupervisorError>,
    /// Deterministically fail selection publication after update staging in focused tests.
    #[cfg(test)]
    selection_publication_failure: bool,
}

/// Validated content-free identity of one selected immutable parser-pack slot.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OptionalParserPackSelectionKey {
    /// Stable opaque key for derivation fingerprints and cache ownership.
    value: String,
    /// `ProjectAtlas` release namespace that owns the slot.
    projectatlas_version: String,
    /// Authenticated identity of the selected artifact manifest.
    artifact: ParserArtifactIdentity,
}

impl OptionalParserPackSelectionKey {
    /// Borrow the stable opaque selection key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Borrow the authenticated selected-artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> &ParserArtifactIdentity {
        &self.artifact
    }
}

/// Content-free project selection used by derivation and source-admission policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionalParserPackProjectSelection {
    /// No optional parser pack is selected for this project.
    Inactive,
    /// One validated artifact identity is selected for this project.
    Selected(OptionalParserPackSelectionKey),
}

impl OptionalParserPackProjectSelection {
    /// Borrow the selected slot key, or return `None` for inactive default-core operation.
    #[must_use]
    pub const fn selection_key(&self) -> Option<&OptionalParserPackSelectionKey> {
        match self {
            Self::Inactive => None,
            Self::Selected(selection) => Some(selection),
        }
    }

    /// Borrow the selected artifact identity, or return `None` when inactive.
    #[must_use]
    pub const fn artifact(&self) -> Option<&ParserArtifactIdentity> {
        match self {
            Self::Inactive => None,
            Self::Selected(selection) => Some(selection.artifact()),
        }
    }
}

/// Already-open immutable slot after manifest, digest, and permission verification.
struct OpenedOptionalParserPackSlot {
    /// Durable selected-slot identity.
    selection_key: OptionalParserPackSelectionKey,
    /// Already-open process authority; no child has been launched.
    supervisor: OptionalParserSupervisor,
}

/// Cooperative mode for the one stable cross-process parser-pack lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OptionalParserPackLeaseMode {
    /// Multiple verified readers and executions may coexist.
    Shared,
    /// One immutable-slot mutation excludes every reader and other writer.
    Exclusive,
}

/// RAII-held cross-process authority over immutable optional-parser slots.
struct OptionalParserPackLease {
    /// Open locked file; process exit closes it even when destructors do not run.
    file: File,
}

impl Drop for OptionalParserPackLease {
    fn drop(&mut self) {
        drop(self.file.unlock());
    }
}

/// Verified current-project handoff for runtime staging.
///
/// The handoff owns the already-open, still-unlaunched supervisor so runtime
/// integration cannot accidentally reopen and rehash the exact selected slot.
pub struct VerifiedOptionalParserPackSelection {
    /// Durable selected-slot identity.
    selection_key: OptionalParserPackSelectionKey,
    /// Already-open verified process authority; no child has been launched.
    supervisor: OptionalParserSupervisor,
    /// Shared cross-process authority retained for the complete worker lifetime.
    _execution_lease: OptionalParserPackLease,
}

impl VerifiedOptionalParserPackSelection {
    /// Borrow the durable content-free selected-slot key.
    #[must_use]
    pub const fn selection_key(&self) -> &OptionalParserPackSelectionKey {
        &self.selection_key
    }

    /// Borrow the verified artifact-manifest identity.
    #[must_use]
    pub const fn artifact(&self) -> &ParserArtifactIdentity {
        self.selection_key.artifact()
    }

    /// Return whether the selected artifact admits one canonical language identity.
    #[must_use]
    pub fn accepts_language(&self, language_id: &str) -> bool {
        self.supervisor.accepts_language(language_id)
    }

    /// Mutably borrow the already-open supervisor without releasing execution authority.
    pub fn supervisor_mut(&mut self) -> &mut OptionalParserSupervisor {
        &mut self.supervisor
    }
}

impl OptionalParserPackLifecycle {
    /// Construct lifecycle ownership without opening archives or touching the filesystem.
    ///
    /// `storage_root` is a hidden test/managed-install override. Normal callers pass `None`;
    /// the platform's user-owned application data directory is resolved only if a later
    /// operation actually needs parser-pack storage.
    ///
    /// # Errors
    ///
    /// Construction currently preserves the fallible public signature for compatibility but
    /// performs no storage lookup. A storage-dependent operation reports
    /// [`OptionalParserPackLifecycleError::StorageRootUnavailable`] when the deferred platform
    /// root cannot be derived.
    pub fn new(
        project_root: impl Into<PathBuf>,
        storage_root: Option<PathBuf>,
    ) -> Result<Self, OptionalParserPackLifecycleError> {
        let deferred_storage_root = OnceLock::new();
        if let Some(storage_root) = storage_root
            && deferred_storage_root.set(Some(storage_root)).is_err()
        {
            return Err(OptionalParserPackLifecycleError::StorageRootUnavailable);
        }
        Ok(Self {
            project_root: project_root.into(),
            storage_root: deferred_storage_root,
            platform: host_pack_platform(),
            #[cfg(test)]
            admission_failure: None,
            #[cfg(test)]
            selection_publication_failure: false,
        })
    }

    /// Verify and fixture-probe one local completed archive without installing it.
    ///
    /// # Errors
    ///
    /// Returns unsupported containment before opening the archive, or an archive,
    /// manifest, digest, platform, path, bound, or supervisor-admission failure.
    pub fn verify(
        &self,
        archive: &Path,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        let platform = self.require_supported()?;
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        let _lease = self.acquire_pack_lease(OptionalParserPackLeaseMode::Exclusive)?;
        #[cfg(test)]
        self.fail_admission_if_injected(archive)?;
        let verified = Self::verify_archive(archive, platform, None)?;
        let artifact = verified.slot_report(false);
        verified.cleanup_profile()?;
        Ok(self.report(OptionalParserPackOperation::Verify, false, Some(artifact)))
    }

    /// Install one local completed archive into an immutable versioned slot.
    ///
    /// Installation never enables the slot for this or another project.
    ///
    /// # Errors
    ///
    /// Returns unsupported containment before opening the archive or mutating storage,
    /// or a validation, admission, publication, or filesystem failure.
    pub fn install(
        &self,
        archive: &Path,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        let platform = self.require_supported()?;
        let _lease = self.acquire_pack_lease(OptionalParserPackLeaseMode::Exclusive)?;
        let (slot, changed) = self.install_archive(archive, platform)?;
        Ok(self.report(
            OptionalParserPackOperation::Install,
            changed,
            Some(slot.report(true)),
        ))
    }

    /// Enable an explicitly named already installed slot for the current project.
    ///
    /// Passing the artifact reported in `status.rollback` is the canonical explicit rollback:
    /// the retained slot becomes selected and the displaced selection becomes rollback-ready.
    ///
    /// # Errors
    ///
    /// Returns unsupported containment before state inspection or mutation, or a slot,
    /// selection, manifest, digest, admission, or atomic-publication failure.
    pub fn enable(
        &self,
        artifact: &str,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        let _platform = self.require_supported()?;
        let _pack_lease = self.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?;
        let _selection_lease = self.acquire_selection_mutation_lease()?;
        let slot = PackSlotIdentity::current(artifact)?;
        self.admit_installed_slot(&slot)?;
        let previous = self.read_selection()?;
        let changed = previous.as_ref().map(|value| &value.selected) != Some(&slot);
        if changed {
            let rollback = previous.map(|value| value.selected);
            self.write_selection(&ProjectSelection::new(slot.clone(), rollback))?;
        }
        Ok(self.report(
            OptionalParserPackOperation::Enable,
            changed,
            Some(slot.report(true)),
        ))
    }

    /// Install and atomically select a replacement, retaining the previous exact slot.
    ///
    /// Selection publication is the update commit point. A failed install or verification
    /// publishes no candidate. If selection publication fails after a new immutable candidate
    /// was installed, the prior selection bytes, selected slot, and rollback slot stay exact;
    /// the verified candidate remains installed so an identical retry can reuse it deterministically.
    ///
    /// # Errors
    ///
    /// Returns unsupported containment before archive or state access, rejects an absent or
    /// invalid prior selection, and propagates archive, admission, or atomic-publication failures.
    pub fn update(
        &self,
        archive: &Path,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        let platform = self.require_supported()?;
        let _pack_lease = self.acquire_pack_lease(OptionalParserPackLeaseMode::Exclusive)?;
        let _selection_lease = self.acquire_selection_mutation_lease()?;
        let previous = self.read_selection()?.ok_or_else(|| {
            invalid_data("update requires an enabled current-project parser-pack selection")
        })?;
        self.open_verified_installed_slot(&previous.selected)?;
        let (slot, installed) = self.install_archive(archive, platform)?;
        let selection_changed = self.publish_installed_update(&previous, &slot)?;
        Ok(self.report(
            OptionalParserPackOperation::Update,
            installed || selection_changed,
            Some(slot.report(true)),
        ))
    }

    /// Atomically commit one already-installed update candidate to project selection.
    fn publish_installed_update(
        &self,
        previous: &ProjectSelection,
        slot: &PackSlotIdentity,
    ) -> Result<bool, OptionalParserPackLifecycleError> {
        if slot == &previous.selected {
            return Ok(false);
        }
        self.write_selection(&ProjectSelection::new(
            slot.clone(),
            Some(previous.selected.clone()),
        ))?;
        Ok(true)
    }

    /// Disable the optional parser pack for the current project.
    ///
    /// This cleanup remains supported and idempotent when containment is unsupported or the
    /// selection file is stale, malformed, or from another lifecycle schema.
    ///
    /// # Errors
    ///
    /// Returns only a filesystem cleanup or bounded status-inspection failure.
    pub fn disable(
        &self,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        if !self.selection_mutation_needed()? {
            return Ok(self.report(OptionalParserPackOperation::Disable, false, None));
        }
        let _selection_lease = self.acquire_selection_mutation_lease()?;
        let changed = self.remove_selection_if_present()?;
        Ok(self.report(OptionalParserPackOperation::Disable, changed, None))
    }

    /// Disable this project and remove this logical pack's user-owned immutable slots.
    ///
    /// Other projects are never scanned or mutated; their now-missing selections are reported
    /// as stale when they next inspect status. Cleanup is safe and idempotent on unsupported hosts.
    ///
    /// # Errors
    ///
    /// Returns only an exact lifecycle-owned filesystem cleanup or bounded status failure.
    pub fn remove(
        &self,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        let acquire_pack_lease = if self.platform.is_none() {
            let storage_root = self.storage_root()?;
            match direct_directory_state(storage_root)? {
                DirectDirectoryState::Missing => false,
                DirectDirectoryState::Real => !matches!(
                    direct_directory_state(&storage_root.join(OPTIONAL_PARSER_PACK_ID))?,
                    DirectDirectoryState::Missing
                ),
                DirectDirectoryState::Unsafe => true,
            }
        } else {
            true
        };
        let _pack_lease = acquire_pack_lease
            .then(|| self.acquire_pack_lease(OptionalParserPackLeaseMode::Exclusive))
            .transpose()?;
        let _selection_lease = self
            .selection_mutation_needed()?
            .then(|| self.acquire_selection_mutation_lease())
            .transpose()?;
        let pack_root = self.pack_root()?;
        let slots = installed_slot_paths(&pack_root)?;
        let selection_changed = self.remove_selection_if_present()?;
        let storage_changed = self.remove_installed_pack(&pack_root, slots)?;
        Ok(self.report(
            OptionalParserPackOperation::Remove,
            selection_changed || storage_changed,
            None,
        ))
    }

    /// Inspect bounded content-free lifecycle metadata without hashing or loading pack assets.
    ///
    /// # Errors
    ///
    /// Returns a filesystem error only when exact lifecycle-owned metadata cannot be inspected.
    pub fn status(
        &self,
    ) -> Result<OptionalParserPackLifecycleReport, OptionalParserPackLifecycleError> {
        Ok(self.report(OptionalParserPackOperation::Status, false, None))
    }

    /// Derive the current project's content-free optional-parser selection.
    ///
    /// An absent selection takes the default-core fast path without inspecting
    /// parser-pack storage. A present selection is rejected on unsupported hosts
    /// before its contents are read, then decoded through the strict lifecycle
    /// schema without opening any source file.
    ///
    /// # Errors
    ///
    /// Returns an unsafe project-state-path failure, typed unsupported containment
    /// for any present selection on an unsupported host, or a strict selection
    /// schema or identity failure.
    pub fn derive_project_selection(
        &self,
    ) -> Result<OptionalParserPackProjectSelection, OptionalParserPackLifecycleError> {
        if !self.selection_entry_present()? {
            return Ok(OptionalParserPackProjectSelection::Inactive);
        }
        self.require_supported()?;
        let selection = self
            .read_selection()?
            .ok_or_else(|| invalid_data("project parser-pack selection disappeared"))?;
        Ok(OptionalParserPackProjectSelection::Selected(
            selection.selected.selection_key()?,
        ))
    }

    /// Resolve the current project's enabled slot for normal runtime staging.
    ///
    /// An absent selection returns `Ok(None)` on every host without inspecting pack storage.
    /// A present selection refuses unsupported hosts before reading its contents. Supported
    /// selections are returned only after immutable-slot, manifest, digest, and supervisor
    /// admission verification.
    ///
    /// # Errors
    ///
    /// Returns typed unsupported containment for a present selection on an unsupported host,
    /// or a strict selection, slot, manifest, digest, immutability, or admission failure.
    pub fn resolve_selected_pack(
        &self,
    ) -> Result<Option<VerifiedOptionalParserPackSelection>, OptionalParserPackLifecycleError> {
        let selection = self.derive_project_selection()?;
        let OptionalParserPackProjectSelection::Selected(selection_key) = selection else {
            return Ok(None);
        };
        let execution_lease = self.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?;
        let slot = PackSlotIdentity::from_selection_key(&selection_key);
        let verified = self.open_verified_installed_slot(&slot)?;
        if verified.selection_key != selection_key {
            return Err(invalid_data(
                "verified optional parser-pack slot differs from project selection",
            ));
        }
        Ok(Some(VerifiedOptionalParserPackSelection {
            selection_key: verified.selection_key,
            supervisor: verified.supervisor,
            _execution_lease: execution_lease,
        }))
    }

    /// Refuse unsupported hosts before callers access archives, state, or storage.
    fn require_supported(&self) -> Result<PackPlatform, OptionalParserPackLifecycleError> {
        self.platform
            .ok_or(OptionalParserPackLifecycleError::UnsupportedContainment {
                os: env::consts::OS,
                architecture: env::consts::ARCH,
            })
    }

    /// Install after the unsupported-host guard has already passed.
    fn install_archive(
        &self,
        archive: &Path,
        platform: PackPlatform,
    ) -> Result<(PackSlotIdentity, bool), OptionalParserPackLifecycleError> {
        #[cfg(test)]
        self.fail_admission_if_injected(archive)?;
        self.ensure_storage_roots()?;
        let versions_root = self.versions_root()?;
        let mut verified = Self::verify_archive(archive, platform, Some(&versions_root))?;
        let slot = verified.slot_identity();
        let operation = (|| {
            ensure_direct_directory(
                &versions_root,
                &versions_root.join(&slot.projectatlas_version),
            )?;
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            if windows_slot_cleanup_in_progress(
                &versions_root.join(&slot.projectatlas_version),
                &slot.artifact,
            )? {
                return Err(invalid_data(
                    "the selected parser-pack artifact still has a cleanup tombstone",
                ));
            }
            let destination = self.slot_path(&slot)?;
            if self.slot_path_is_real(&slot)? {
                self.open_verified_installed_slot(&slot)?;
                return Ok((slot.clone(), false));
            }
            if fs::symlink_metadata(&destination).is_ok() {
                return Err(invalid_data(
                    "immutable slot path is occupied by a non-directory entry",
                ));
            }
            if let Err(operation) = seal_immutable_tree(&verified.pack_root) {
                return finish_with_cleanup(
                    Err(operation),
                    make_tree_writable(&verified.pack_root),
                );
            }
            match fs::rename(&verified.pack_root, &destination) {
                Ok(()) => {
                    verified.transfer_profile_to_installed_slot();
                    Ok((slot.clone(), true))
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    make_tree_writable(&verified.pack_root)?;
                    self.open_verified_installed_slot(&slot)?;
                    Ok((slot.clone(), false))
                }
                Err(source) => finish_with_cleanup(
                    Err(io_error(
                        "publish immutable parser-pack slot",
                        destination,
                        source,
                    )),
                    make_tree_writable(&verified.pack_root),
                ),
            }
        })();
        let cleanup = verified.cleanup_profile();
        finish_with_cleanup(operation, cleanup)
    }

    /// Validate a local completed archive into a temporary directory.
    fn verify_archive(
        archive: &Path,
        platform: PackPlatform,
        staging_parent: Option<&Path>,
    ) -> Result<VerifiedArchive, OptionalParserPackLifecycleError> {
        let before = sha256_file(archive, OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES)?;
        let extracted = extract_archive(archive, staging_parent)?;
        let accepted_bytes = read_bounded_file(
            &extracted.pack_root.join(ACCEPTED_MANIFEST_FILE_NAME),
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)
                .map_err(|source| invalid_data(source.to_string()))?,
        )?;
        let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
        let artifact_bytes = read_bounded_file(
            &extracted.pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)
                .map_err(|source| invalid_data(source.to_string()))?,
        )?;
        let artifact: OptionalParserPackArtifactManifest = serde_json::from_slice(&artifact_bytes)?;
        artifact.validate(&logical)?;
        if artifact.platform != platform {
            return Err(invalid_data(format!(
                "archive target {} does not match current host target {}",
                artifact.platform.as_str(),
                platform.as_str()
            )));
        }
        require_archive_name(archive, platform)?;
        validate_observed_inventory(&extracted.observed, &artifact)?;
        let projectatlas_version = artifact.projectatlas_version;
        let supervisor = OptionalParserSupervisor::open(&extracted.pack_root)?;
        let artifact_identity = supervisor.artifact_identity().clone();
        let temporary_profile =
            TemporaryParserArtifactProfile::for_verified_supervisor(&supervisor);
        if let Err(error) = admit_optional_parser_artifact(supervisor, &logical) {
            return finish_with_cleanup(Err(error.into()), temporary_profile.cleanup());
        }
        let after = match sha256_file(archive, OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES) {
            Ok(after) => after,
            Err(operation) => {
                return finish_with_cleanup(Err(operation), temporary_profile.cleanup());
            }
        };
        if before != after {
            return finish_with_cleanup(
                Err(invalid_data(
                    "completed archive changed during verification",
                )),
                temporary_profile.cleanup(),
            );
        }
        Ok(VerifiedArchive {
            temporary_profile,
            _directory: extracted.directory,
            pack_root: extracted.pack_root,
            artifact: artifact_identity,
            projectatlas_version,
        })
    }

    /// Execute the complete current-host admission probe before selecting an installed slot.
    fn admit_installed_slot(
        &self,
        slot: &PackSlotIdentity,
    ) -> Result<(), OptionalParserPackLifecycleError> {
        let root = self.slot_path(slot)?;
        #[cfg(test)]
        if let Some(failure) = self.admission_failure {
            return Err(failure(&root).into());
        }
        let logical_bytes = read_bounded_file(
            &root.join(ACCEPTED_MANIFEST_FILE_NAME),
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)
                .map_err(|source| invalid_data(source.to_string()))?,
        )?;
        let logical = OptionalParserPackManifest::from_json(&logical_bytes)?;
        let verified = self.open_verified_installed_slot(slot)?;
        admit_optional_parser_artifact(verified.supervisor, &logical)?;
        Ok(())
    }

    /// Verify exact installed identity, manifest bindings, and immutable permissions.
    fn open_verified_installed_slot(
        &self,
        slot: &PackSlotIdentity,
    ) -> Result<OpenedOptionalParserPackSlot, OptionalParserPackLifecycleError> {
        let root = self.slot_path(slot)?;
        if !self.slot_path_is_real(slot)? {
            return Err(invalid_data(
                "selected optional parser-pack slot is not installed",
            ));
        }
        verify_immutable_tree(&root)?;
        let selection_key = slot.selection_key()?;
        let supervisor = OptionalParserSupervisor::open(root)?;
        if supervisor.artifact_identity() != selection_key.artifact() {
            return Err(invalid_data(
                "installed slot identity differs from its artifact manifest",
            ));
        }
        Ok(OpenedOptionalParserPackSlot {
            selection_key,
            supervisor,
        })
    }

    /// Attempt exact artifact-scoped cleanup for every installed slot before removing storage.
    fn remove_installed_pack(
        &self,
        pack_root: &Path,
        slots: Vec<InstalledSlotPath>,
    ) -> Result<bool, OptionalParserPackLifecycleError> {
        let mut changed = false;
        let mut failures = Vec::new();
        for slot in slots {
            let result = self.remove_installed_slot(&slot);
            match result {
                Ok(slot_changed) => changed |= slot_changed,
                Err(error) => {
                    if failures.len() < 16 {
                        failures.push(format!("{}: {error}", slot.bounded_label()));
                    }
                }
            }
        }
        if !failures.is_empty() {
            return Err(OptionalParserPackLifecycleError::CleanupIncomplete {
                message: failures.join("; "),
            });
        }
        Ok(remove_tree_if_present(pack_root)? || changed)
    }

    /// Complete one exact slot's platform cleanup and bounded tree deletion.
    fn remove_installed_slot(
        &self,
        slot: &InstalledSlotPath,
    ) -> Result<bool, OptionalParserPackLifecycleError> {
        if self.platform == Some(PackPlatform::WindowsX86_64) {
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            {
                return Self::remove_windows_slot(slot);
            }
            #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
            {
                return Err(OptionalParserPackLifecycleError::UnsupportedContainment {
                    os: env::consts::OS,
                    architecture: env::consts::ARCH,
                });
            }
        }
        remove_tree_if_present(&slot.entry_root)
    }

    /// Atomically isolate, profile-clean, and delete one Windows artifact slot.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    fn remove_windows_slot(
        slot: &InstalledSlotPath,
    ) -> Result<bool, OptionalParserPackLifecycleError> {
        if slot.state == InstalledSlotCleanupState::ProfileCleaned {
            return remove_tree_if_present(&slot.entry_root);
        }
        let pending = if slot.state == InstalledSlotCleanupState::Installed {
            transition_slot_to_removing_tombstone(slot)?
        } else {
            slot.clone()
        };
        let pack_root = pending
            .pack_root
            .as_deref()
            .ok_or_else(|| invalid_data("pending parser-pack tombstone has no pack root"))?;
        let identity = PackSlotIdentity {
            projectatlas_version: pending.projectatlas_version.clone(),
            artifact: pending.artifact.clone(),
        };
        identity.validate()?;
        verify_immutable_tree(pack_root)?;
        let supervisor = OptionalParserSupervisor::open(pack_root)?;
        if supervisor.artifact_identity().digest().as_str() != identity.artifact {
            return Err(invalid_data(
                "cleanup tombstone identity differs from its artifact manifest",
            ));
        }
        cleanup_platform_profile(pack_root, supervisor.artifact_identity())?;
        let cleaned = transition_tombstone_to_profile_cleaned(&pending)?;
        remove_tree_if_present(&cleaned.entry_root)?;
        Ok(true)
    }

    /// Render current state after one operation without opening or hashing pack payloads.
    fn report(
        &self,
        operation: OptionalParserPackOperation,
        changed: bool,
        artifact: Option<OptionalParserPackSlotReport>,
    ) -> OptionalParserPackLifecycleReport {
        let selection = self.read_selection_for_status();
        let (installed_slots, installed_slots_truncated, cleanup_pending, mut unsafe_storage) =
            match self
                .pack_root()
                .and_then(|root| count_installed_slots(&root))
            {
                Ok(value) => (value.0, value.1, value.2, false),
                Err(_) => (0, false, false, true),
            };
        let selection_stale = selection.is_err();
        let selection = selection.ok().flatten();
        let selected = selection.as_ref().map(|value| {
            let (present, unsafe_path) = self.slot_presence(&value.selected);
            unsafe_storage |= unsafe_path;
            value.selected.report(present)
        });
        let rollback = selection.as_ref().and_then(|value| {
            value.rollback.as_ref().map(|slot| {
                let (present, unsafe_path) = self.slot_presence(slot);
                unsafe_storage |= unsafe_path;
                slot.report(present)
            })
        });
        let selected_missing = selected.as_ref().is_some_and(|slot| !slot.present);
        let rollback_present = rollback.as_ref().is_some_and(|slot| slot.present);
        let state = if selection_stale
            || unsafe_storage
            || selected_missing
            || installed_slots_truncated
            || cleanup_pending
        {
            OptionalParserPackState::Stale
        } else if selected.is_some() && rollback_present {
            OptionalParserPackState::RollbackReady
        } else if selected.is_some() {
            OptionalParserPackState::Enabled
        } else if installed_slots > 0 {
            OptionalParserPackState::InstalledDisabled
        } else if self.platform.is_none() {
            OptionalParserPackState::UnsupportedContainment
        } else {
            OptionalParserPackState::Absent
        };
        OptionalParserPackLifecycleReport {
            operation,
            state,
            pack_id: OPTIONAL_PARSER_PACK_ID,
            supported: self.platform.is_some(),
            platform: self.platform.map(PackPlatform::as_str),
            installed_slots,
            installed_slots_truncated,
            selected,
            rollback,
            artifact,
            changed,
        }
    }

    /// Return whether any exact selection entry exists without reading its contents.
    fn selection_entry_present(&self) -> Result<bool, OptionalParserPackLifecycleError> {
        match direct_directory_state(&self.selection_parent())? {
            DirectDirectoryState::Missing => return Ok(false),
            DirectDirectoryState::Real => {}
            DirectDirectoryState::Unsafe => {
                return Err(invalid_data(
                    "project .projectatlas selection parent is not a real directory",
                ));
            }
        }
        let path = self.selection_path();
        match fs::symlink_metadata(&path) {
            Ok(_) => Ok(true),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(io_error(
                "inspect project parser-pack selection",
                path,
                source,
            )),
        }
    }

    /// Read strict selection for mutating supported operations.
    fn read_selection(&self) -> Result<Option<ProjectSelection>, OptionalParserPackLifecycleError> {
        match direct_directory_state(&self.selection_parent())? {
            DirectDirectoryState::Missing => return Ok(None),
            DirectDirectoryState::Real => {}
            DirectDirectoryState::Unsafe => {
                return Err(invalid_data(
                    "project .projectatlas selection parent is not a real directory",
                ));
            }
        }
        let path = self.selection_path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let bytes = read_bounded_file(&path, PROJECT_SELECTION_MAX_BYTES)?;
                let selection: ProjectSelection = serde_json::from_slice(&bytes)?;
                selection.validate()?;
                Ok(Some(selection))
            }
            Ok(_) => Err(invalid_data(
                "project parser-pack selection is not a regular file",
            )),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(io_error(
                "inspect project parser-pack selection",
                path,
                source,
            )),
        }
    }

    /// Read selection for status, preserving malformed metadata as typed stale state.
    fn read_selection_for_status(
        &self,
    ) -> Result<Option<ProjectSelection>, OptionalParserPackLifecycleError> {
        self.read_selection()
    }

    /// Atomically replace current-project selection after every candidate is verified.
    fn write_selection(
        &self,
        selection: &ProjectSelection,
    ) -> Result<(), OptionalParserPackLifecycleError> {
        selection.validate()?;
        let path = self.selection_path();
        ensure_anchor_directory(&self.project_root)?;
        let parent = self.selection_parent();
        ensure_direct_directory(&self.project_root, &parent)?;
        let bytes = serde_json::to_vec_pretty(selection)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > PROJECT_SELECTION_MAX_BYTES {
            return Err(invalid_data(
                "project parser-pack selection exceeds its byte bound",
            ));
        }
        let mut temporary = NamedTempFile::new_in(&parent)
            .map_err(|source| io_error("create temporary project selection", &parent, source))?;
        temporary.write_all(&bytes).map_err(|source| {
            io_error("write temporary project selection", path.clone(), source)
        })?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|source| io_error("sync temporary project selection", path.clone(), source))?;
        #[cfg(test)]
        if self.selection_publication_failure {
            return Err(invalid_data(
                "injected project selection publication failure",
            ));
        }
        temporary.persist(&path).map_err(|error| {
            io_error("publish project parser-pack selection", path, error.error)
        })?;
        Ok(())
    }

    /// Exact project-local selection path.
    fn selection_path(&self) -> PathBuf {
        self.project_root
            .join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH)
    }

    /// Exact direct product-owned parent of project selection metadata.
    fn selection_parent(&self) -> PathBuf {
        self.project_root.join(".projectatlas")
    }

    /// Return whether selection cleanup has an exact direct entry to serialize.
    fn selection_mutation_needed(&self) -> Result<bool, OptionalParserPackLifecycleError> {
        match direct_directory_state(&self.selection_parent())? {
            DirectDirectoryState::Missing | DirectDirectoryState::Unsafe => Ok(false),
            DirectDirectoryState::Real => self.selection_entry_present(),
        }
    }

    /// Serialize one project-local selection read-modify-write transition.
    fn acquire_selection_mutation_lease(
        &self,
    ) -> Result<OptionalParserPackLease, OptionalParserPackLifecycleError> {
        ensure_anchor_directory(&self.project_root)?;
        let parent = self.selection_parent();
        ensure_direct_directory(&self.project_root, &parent)?;
        let path = parent.join(OPTIONAL_PARSER_SELECTION_LEASE_FILE_NAME);
        let file = open_or_create_direct_lease_file(&path)?;
        match file.try_lock() {
            Ok(()) => {
                require_direct_lease_path(&path)?;
                Ok(OptionalParserPackLease { file })
            }
            Err(fs::TryLockError::WouldBlock) => {
                Err(OptionalParserPackLifecycleError::Busy { path })
            }
            Err(fs::TryLockError::Error(source)) => {
                Err(io_error("lock project parser-pack selection", path, source))
            }
        }
    }

    /// Remove only a selection inside a real direct project-owned parent.
    fn remove_selection_if_present(&self) -> Result<bool, OptionalParserPackLifecycleError> {
        match direct_directory_state(&self.selection_parent())? {
            DirectDirectoryState::Missing | DirectDirectoryState::Unsafe => Ok(false),
            DirectDirectoryState::Real => remove_file_if_present(&self.selection_path()),
        }
    }

    /// Exact user-owned logical pack root.
    fn storage_root(&self) -> Result<&Path, OptionalParserPackLifecycleError> {
        self.storage_root
            .get_or_init(|| default_storage_root().ok())
            .as_deref()
            .ok_or(OptionalParserPackLifecycleError::StorageRootUnavailable)
    }

    /// Exact user-owned logical pack root, resolved only for storage operations.
    fn pack_root(&self) -> Result<PathBuf, OptionalParserPackLifecycleError> {
        Ok(self.storage_root()?.join(OPTIONAL_PARSER_PACK_ID))
    }

    /// Version namespace containing immutable artifact slots.
    fn versions_root(&self) -> Result<PathBuf, OptionalParserPackLifecycleError> {
        Ok(self.pack_root()?.join("versions"))
    }

    /// Exact path for one validated immutable slot identity.
    fn slot_path(
        &self,
        slot: &PackSlotIdentity,
    ) -> Result<PathBuf, OptionalParserPackLifecycleError> {
        Ok(self
            .versions_root()?
            .join(&slot.projectatlas_version)
            .join(&slot.artifact))
    }

    /// Require every product-owned slot ancestor and the slot leaf to be a real directory.
    fn slot_path_is_real(
        &self,
        slot: &PackSlotIdentity,
    ) -> Result<bool, OptionalParserPackLifecycleError> {
        let pack_root = self.pack_root()?;
        let versions_root = self.versions_root()?;
        for component in [
            pack_root,
            versions_root.clone(),
            versions_root.join(&slot.projectatlas_version),
            self.slot_path(slot)?,
        ] {
            match direct_directory_state(&component)? {
                DirectDirectoryState::Real => {}
                DirectDirectoryState::Missing => return Ok(false),
                DirectDirectoryState::Unsafe => {
                    return Err(invalid_data(
                        "optional parser-pack slot path contains an unsafe owned component",
                    ));
                }
            }
        }
        Ok(true)
    }

    /// Return content-free presence plus whether an unsafe owned component was observed.
    fn slot_presence(&self, slot: &PackSlotIdentity) -> (bool, bool) {
        match self.slot_path_is_real(slot) {
            Ok(present) => (present, false),
            Err(_) => (false, true),
        }
    }

    /// Create and verify each product-owned storage component below the caller-owned anchor.
    fn ensure_storage_roots(&self) -> Result<(), OptionalParserPackLifecycleError> {
        let storage_root = self.storage_root()?;
        ensure_anchor_directory(storage_root)?;
        let pack_root = self.pack_root()?;
        ensure_direct_directory(storage_root, &pack_root)?;
        ensure_direct_directory(&pack_root, &self.versions_root()?)
    }

    /// Acquire one nonblocking shared execution or exclusive storage-mutation lease.
    fn acquire_pack_lease(
        &self,
        mode: OptionalParserPackLeaseMode,
    ) -> Result<OptionalParserPackLease, OptionalParserPackLifecycleError> {
        let storage_root = self.storage_root()?;
        ensure_anchor_directory(storage_root)?;
        let path = storage_root.join(OPTIONAL_PARSER_PACK_LEASE_FILE_NAME);
        let file = open_or_create_direct_lease_file(&path)?;
        let result = match mode {
            OptionalParserPackLeaseMode::Shared => file.try_lock_shared(),
            OptionalParserPackLeaseMode::Exclusive => file.try_lock(),
        };
        match result {
            Ok(()) => {
                require_direct_lease_path(&path)?;
                Ok(OptionalParserPackLease { file })
            }
            Err(fs::TryLockError::WouldBlock) => {
                Err(OptionalParserPackLifecycleError::Busy { path })
            }
            Err(fs::TryLockError::Error(source)) => Err(io_error(
                "lock optional parser-pack lifecycle",
                path,
                source,
            )),
        }
    }

    #[cfg(test)]
    fn for_test(
        project_root: PathBuf,
        storage_root: PathBuf,
        platform: Option<PackPlatform>,
    ) -> Self {
        Self {
            project_root,
            storage_root: OnceLock::from(Some(storage_root)),
            platform,
            admission_failure: None,
            selection_publication_failure: false,
        }
    }

    /// Inject one deterministic admission failure without exposing a production seam.
    #[cfg(test)]
    fn with_admission_failure(mut self, failure: fn(&Path) -> ParserSupervisorError) -> Self {
        self.admission_failure = Some(failure);
        self
    }

    /// Inject one deterministic selection-publication failure without a production seam.
    #[cfg(test)]
    fn with_selection_publication_failure(mut self) -> Self {
        self.selection_publication_failure = true;
        self
    }

    /// Return one injected admission failure before any lifecycle mutation.
    #[cfg(test)]
    fn fail_admission_if_injected(
        &self,
        artifact: &Path,
    ) -> Result<(), OptionalParserPackLifecycleError> {
        match self.admission_failure {
            Some(failure) => Err(failure(artifact).into()),
            None => Ok(()),
        }
    }
}

/// Strict current-project selection record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectSelection {
    /// Strict lifecycle selection schema.
    schema_version: u32,
    /// Stable identity of the one supported logical pack.
    pack_id: String,
    /// Currently enabled immutable slot.
    selected: PackSlotIdentity,
    /// Previous slot retained by a successful update.
    #[serde(skip_serializing_if = "Option::is_none")]
    rollback: Option<PackSlotIdentity>,
}

impl ProjectSelection {
    /// Construct one current-schema project selection.
    fn new(selected: PackSlotIdentity, rollback: Option<PackSlotIdentity>) -> Self {
        Self {
            schema_version: PROJECT_SELECTION_SCHEMA_VERSION,
            pack_id: OPTIONAL_PARSER_PACK_ID.to_owned(),
            selected,
            rollback,
        }
    }

    /// Validate strict schema, logical identity, and distinct canonical slots.
    fn validate(&self) -> Result<(), OptionalParserPackLifecycleError> {
        if self.schema_version != PROJECT_SELECTION_SCHEMA_VERSION {
            return Err(invalid_data(
                "project parser-pack selection schema is unsupported",
            ));
        }
        if self.pack_id != OPTIONAL_PARSER_PACK_ID {
            return Err(invalid_data(
                "project parser-pack selection has another pack identity",
            ));
        }
        self.selected.validate()?;
        if let Some(rollback) = &self.rollback {
            rollback.validate()?;
            if rollback == &self.selected {
                return Err(invalid_data(
                    "project parser-pack rollback duplicates selected slot",
                ));
            }
        }
        Ok(())
    }
}

/// Validated immutable slot identity retained in project metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PackSlotIdentity {
    /// `ProjectAtlas` release namespace that owns the slot.
    projectatlas_version: String,
    /// BLAKE3 identity of exact artifact-manifest bytes.
    artifact: String,
}

impl PackSlotIdentity {
    /// Validate one artifact identity in the current release namespace.
    fn current(artifact: &str) -> Result<Self, OptionalParserPackLifecycleError> {
        let slot = Self {
            projectatlas_version: OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION.to_owned(),
            artifact: artifact.to_owned(),
        };
        slot.validate()?;
        Ok(slot)
    }

    /// Validate release compatibility and canonical artifact digest.
    fn validate(&self) -> Result<(), OptionalParserPackLifecycleError> {
        if self.projectatlas_version != OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION {
            return Err(invalid_data(
                "parser-pack slot belongs to another ProjectAtlas release line",
            ));
        }
        ParserContentDigest::new(self.artifact.clone())
            .map_err(|source| invalid_data(source.to_string()))?;
        Ok(())
    }

    /// Project one validated opaque selection key for derivation ownership.
    fn selection_key(
        &self,
    ) -> Result<OptionalParserPackSelectionKey, OptionalParserPackLifecycleError> {
        self.validate()?;
        let artifact = ParserArtifactIdentity::new(
            ParserContentDigest::new(self.artifact.clone())
                .map_err(|source| invalid_data(source.to_string()))?,
        );
        Ok(OptionalParserPackSelectionKey {
            value: format!(
                "{}:{}:{}",
                OPTIONAL_PARSER_PACK_ID, self.projectatlas_version, self.artifact
            ),
            projectatlas_version: self.projectatlas_version.clone(),
            artifact,
        })
    }

    /// Reconstruct an internal slot identity from an already validated selection key.
    fn from_selection_key(selection: &OptionalParserPackSelectionKey) -> Self {
        Self {
            projectatlas_version: selection.projectatlas_version.clone(),
            artifact: selection.artifact.digest().as_str().to_owned(),
        }
    }

    /// Project one content-free slot report.
    fn report(&self, present: bool) -> OptionalParserPackSlotReport {
        OptionalParserPackSlotReport {
            projectatlas_version: self.projectatlas_version.clone(),
            artifact: self.artifact.clone(),
            present,
        }
    }
}

/// Verified extracted artifact kept alive until it is discarded or atomically published.
struct VerifiedArchive {
    /// Armed cleanup owner, declared before the temporary directory so fallback
    /// cleanup runs while the exact broker and manifest still exist.
    temporary_profile: TemporaryParserArtifactProfile,
    /// Temporary directory whose drop cleans an unpublished extraction.
    _directory: TempDir,
    /// Canonical extracted artifact root.
    pack_root: PathBuf,
    /// Identity of exact validated artifact-manifest bytes.
    artifact: ParserArtifactIdentity,
    /// Manifest-bound `ProjectAtlas` release line.
    projectatlas_version: String,
}

impl VerifiedArchive {
    /// Construct the immutable slot identity for this verified archive.
    fn slot_identity(&self) -> PackSlotIdentity {
        PackSlotIdentity {
            projectatlas_version: self.projectatlas_version.clone(),
            artifact: self.artifact.digest().as_str().to_owned(),
        }
    }

    /// Project one content-free slot report.
    fn slot_report(&self, present: bool) -> OptionalParserPackSlotReport {
        self.slot_identity().report(present)
    }

    /// Complete temporary profile cleanup before the extraction is discarded.
    fn cleanup_profile(self) -> Result<(), OptionalParserPackLifecycleError> {
        let Self {
            temporary_profile,
            _directory: directory,
            ..
        } = self;
        let result = temporary_profile.cleanup();
        drop(directory);
        result
    }

    /// Make the immutable installed slot the sole remaining profile owner.
    fn transfer_profile_to_installed_slot(&mut self) {
        self.temporary_profile.transfer_to_installed_slot();
    }
}

/// One exact extracted file observation.
struct ObservedFile {
    /// Exact extracted file bytes.
    bytes: u64,
    /// SHA-256 of exact extracted file bytes.
    sha256: String,
}

/// Temporary archive extraction and exact observed inventory.
struct ExtractedArchive {
    /// Temporary directory owning the extraction until publish or drop.
    directory: TempDir,
    /// Canonical root inside the temporary extraction.
    pack_root: PathBuf,
    /// Exact extracted files keyed by canonical artifact-relative path.
    observed: BTreeMap<String, ObservedFile>,
}

/// Closed cleanup state for one lifecycle-owned slot or tombstone entry.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstalledSlotCleanupState {
    /// Canonical deterministic installed slot awaiting an atomic removal transition.
    Installed,
    /// Unique tombstone whose artifact profile still requires idempotent cleanup.
    ProfilePending,
    /// Unique tombstone whose profile is already cleaned and only deletion remains.
    ProfileCleaned,
}

/// One direct slot or unique tombstone retained for bounded cleanup.
#[derive(Clone, Debug)]
struct InstalledSlotPath {
    /// Direct version directory name, retained even when stale.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    projectatlas_version: String,
    /// Canonical artifact identity parsed from the slot or tombstone name.
    artifact: String,
    /// Exact lifecycle-owned direct slot or tombstone container.
    entry_root: PathBuf,
    /// Exact pack root while artifact/profile verification remains necessary.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    pack_root: Option<PathBuf>,
    /// Current closed cleanup transition state.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    state: InstalledSlotCleanupState,
}

impl InstalledSlotPath {
    /// Return a bounded content-free label for aggregate cleanup diagnostics.
    fn bounded_label(&self) -> String {
        self.artifact.chars().take(64).collect()
    }
}

/// Reader that rejects decompression beyond one hard byte ceiling.
struct BoundedReader<R> {
    /// Wrapped decompressor.
    inner: R,
    /// Maximum bytes the reader may return.
    maximum: u64,
    /// Bytes returned so far.
    consumed: u64,
}

impl<R> BoundedReader<R> {
    /// Wrap one decompressor with a hard returned-byte ceiling.
    const fn new(inner: R, maximum: u64) -> Self {
        Self {
            inner,
            maximum,
            consumed: 0,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.consumed >= self.maximum {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expanded archive exceeded its hard byte ceiling",
                )),
            };
        }
        let remaining = self.maximum.saturating_sub(self.consumed);
        let allowed = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.consumed = self
            .consumed
            .checked_add(u64::try_from(read).map_err(io::Error::other)?)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "archive byte count overflowed")
            })?;
        Ok(read)
    }
}

/// Stream and validate one canonical completed archive.
fn extract_archive(
    path: &Path,
    staging_parent: Option<&Path>,
) -> Result<ExtractedArchive, OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect optional parser-pack archive", path, source))?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES
    {
        return Err(invalid_data(
            "archive is not a bounded non-empty regular file",
        ));
    }
    let directory = match staging_parent {
        Some(parent) => TempDir::new_in(parent)
            .map_err(|source| io_error("create parser-pack staging directory", parent, source))?,
        None => TempDir::new().map_err(|source| {
            io_error("create parser-pack verification directory", path, source)
        })?,
    };
    let pack_root = directory.path().join(ARCHIVE_ROOT);
    fs::create_dir(&pack_root)
        .map_err(|source| io_error("create extracted parser-pack root", &pack_root, source))?;
    let input = File::open(path)
        .map_err(|source| io_error("open optional parser-pack archive", path, source))?;
    let decoder = zstd::Decoder::new(BufReader::new(input))
        .map_err(|source| io_error("decode optional parser-pack archive", path, source))?;
    let maximum_tar_bytes = OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES
        .checked_add(TAR_FRAMING_ALLOWANCE_BYTES)
        .ok_or_else(|| invalid_data("tar expansion bound overflowed"))?;
    let bounded = BoundedReader::new(decoder, maximum_tar_bytes);
    let mut archive = tar::Archive::new(bounded);
    let mut observed = BTreeMap::new();
    let mut previous_path: Option<String> = None;
    let mut expanded_bytes = 0u64;
    let entries = archive
        .entries()
        .map_err(|source| io_error("read optional parser-pack archive entries", path, source))?;
    for entry in entries {
        let mut entry = entry
            .map_err(|source| io_error("read optional parser-pack archive entry", path, source))?;
        if observed.len() >= OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES.saturating_add(1) {
            return Err(invalid_data("archive exceeded its file-entry ceiling"));
        }
        if !entry.header().entry_type().is_file() {
            return Err(invalid_data("archive contains a non-regular entry"));
        }
        let raw_path = entry.path_bytes();
        let archive_path = std::str::from_utf8(raw_path.as_ref())
            .map_err(|source| invalid_data(source.to_string()))?;
        let prefix = format!("{ARCHIVE_ROOT}/");
        let relative = archive_path
            .strip_prefix(&prefix)
            .ok_or_else(|| invalid_data("archive entry is outside the canonical pack root"))?;
        let relative = PackRelativePath::new(relative)?;
        if previous_path
            .as_ref()
            .is_some_and(|previous| previous.as_str() >= relative.as_str())
        {
            return Err(invalid_data(
                "archive entries are not strictly path-sorted and unique",
            ));
        }
        previous_path = Some(relative.as_str().to_owned());
        let bytes = entry
            .header()
            .size()
            .map_err(|source| io_error("read parser-pack entry size", path, source))?;
        if bytes == 0 || bytes > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES {
            return Err(invalid_data(
                "archive entry is empty or exceeds its file bound",
            ));
        }
        let expected_mode = if matches!(
            relative.as_str(),
            "projectatlas-parser-worker" | "projectatlas-parser-worker.exe"
        ) {
            WORKER_MODE
        } else {
            PAYLOAD_MODE
        };
        let header = entry.header();
        if header
            .uid()
            .map_err(|source| invalid_data(source.to_string()))?
            != 0
            || header
                .gid()
                .map_err(|source| invalid_data(source.to_string()))?
                != 0
            || header
                .mtime()
                .map_err(|source| invalid_data(source.to_string()))?
                != 0
            || header
                .mode()
                .map_err(|source| invalid_data(source.to_string()))?
                != expected_mode
        {
            return Err(invalid_data("archive entry metadata is not canonical"));
        }
        expanded_bytes = expanded_bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid_data("expanded payload byte count overflowed"))?;
        if expanded_bytes > OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES {
            return Err(invalid_data(
                "archive exceeded its expanded payload ceiling",
            ));
        }
        let destination = pack_root.join(Path::new(relative.as_str()));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                io_error("create parser-pack payload directory", parent, source)
            })?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|source| {
                io_error("create extracted parser-pack file", &destination, source)
            })?;
        let mut hasher = Sha256::new();
        let copied = copy_and_hash(&mut entry, &mut output, &mut hasher)?;
        if copied != bytes {
            return Err(invalid_data(
                "archive entry size differs from its tar header",
            ));
        }
        output
            .sync_all()
            .map_err(|source| io_error("sync extracted parser-pack file", &destination, source))?;
        #[cfg(unix)]
        set_extracted_mode(&destination, expected_mode)?;
        let key = relative.as_str().to_owned();
        if observed
            .insert(
                key,
                ObservedFile {
                    bytes,
                    sha256: lowercase_hex(hasher.finalize().as_ref()),
                },
            )
            .is_some()
        {
            return Err(invalid_data("archive contains a duplicate payload path"));
        }
    }
    let mut bounded = archive.into_inner();
    require_zero_tar_padding(&mut bounded)?;
    Ok(ExtractedArchive {
        directory,
        pack_root,
        observed,
    })
}

/// Require exact manifest-listed files plus the self-excluded artifact manifest.
fn validate_observed_inventory(
    observed: &BTreeMap<String, ObservedFile>,
    artifact: &OptionalParserPackArtifactManifest,
) -> Result<(), OptionalParserPackLifecycleError> {
    let expected_count = artifact
        .files
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid_data("expected artifact file count overflowed"))?;
    if observed.len() != expected_count {
        return Err(invalid_data(format!(
            "artifact contains {} files; expected {expected_count}",
            observed.len()
        )));
    }
    for file in &artifact.files {
        let actual = observed
            .get(file.path.as_str())
            .ok_or_else(|| invalid_data(format!("artifact is missing {:?}", file.path.as_str())))?;
        if actual.bytes != file.bytes || actual.sha256 != file.sha256.as_str() {
            return Err(invalid_data(format!(
                "payload {:?} differs from its artifact manifest",
                file.path.as_str()
            )));
        }
    }
    if !observed.contains_key(ARTIFACT_MANIFEST_FILE_NAME) {
        return Err(invalid_data(
            "artifact manifest is missing from archive inventory",
        ));
    }
    Ok(())
}

/// Require the canonical archive basename for the current platform.
fn require_archive_name(
    path: &Path,
    platform: PackPlatform,
) -> Result<(), OptionalParserPackLifecycleError> {
    let expected = format!("{ARCHIVE_ROOT}-{}.tar.zst", platform.as_str());
    if path.file_name().and_then(std::ffi::OsStr::to_str) != Some(expected.as_str()) {
        return Err(invalid_data(format!(
            "archive basename must be {expected:?} for {}",
            platform.as_str()
        )));
    }
    Ok(())
}

/// Hash one bounded non-empty regular file for TOCTOU comparison.
fn sha256_file(
    path: &Path,
    maximum: u64,
) -> Result<(String, u64), OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect bounded lifecycle file", path, source))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(invalid_data(
            "lifecycle file is not a bounded non-empty regular file",
        ));
    }
    let mut input = BufReader::new(
        File::open(path).map_err(|source| io_error("open bounded lifecycle file", path, source))?,
    );
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut total = 0u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|source| io_error("hash bounded lifecycle file", path, source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(u64::try_from(read).map_err(|source| invalid_data(source.to_string()))?)
            .ok_or_else(|| invalid_data("lifecycle file byte count overflowed"))?;
        if total > maximum {
            return Err(invalid_data("lifecycle file exceeded its byte ceiling"));
        }
    }
    if total != metadata.len() {
        return Err(invalid_data(
            "lifecycle file changed while it was being hashed",
        ));
    }
    Ok((lowercase_hex(hasher.finalize().as_ref()), total))
}

/// Read one exact bounded non-empty regular file.
fn read_bounded_file(
    path: &Path,
    maximum: u64,
) -> Result<Vec<u8>, OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect bounded lifecycle file", path, source))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(invalid_data(
            "lifecycle file is not a bounded non-empty regular file",
        ));
    }
    let capacity =
        usize::try_from(metadata.len()).map_err(|source| invalid_data(source.to_string()))?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|source| io_error("open bounded lifecycle file", path, source))?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| io_error("read bounded lifecycle file", path, source))?;
    if bytes.len() != capacity {
        return Err(invalid_data(
            "lifecycle file changed while it was being read",
        ));
    }
    Ok(bytes)
}

/// Copy one archive entry while hashing exact bytes.
fn copy_and_hash(
    input: &mut impl Read,
    output: &mut File,
    hasher: &mut Sha256,
) -> Result<u64, OptionalParserPackLifecycleError> {
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut total = 0u64;
    loop {
        let read = input.read(&mut buffer).map_err(|source| {
            io_error(
                "read parser-pack archive entry",
                Path::new(ARCHIVE_ROOT),
                source,
            )
        })?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(|source| {
            io_error(
                "write extracted parser-pack file",
                Path::new(ARCHIVE_ROOT),
                source,
            )
        })?;
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(u64::try_from(read).map_err(|source| invalid_data(source.to_string()))?)
            .ok_or_else(|| invalid_data("archive entry byte count overflowed"))?;
        if total > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES {
            return Err(invalid_data("archive entry exceeded its file byte ceiling"));
        }
    }
    Ok(total)
}

/// Require canonical zero padding after the tar terminator.
fn require_zero_tar_padding(input: &mut impl Read) -> Result<(), OptionalParserPackLifecycleError> {
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = input.read(&mut buffer).map_err(|source| {
            io_error(
                "read parser-pack tar padding",
                Path::new(ARCHIVE_ROOT),
                source,
            )
        })?;
        if read == 0 {
            return Ok(());
        }
        if buffer[..read].iter().any(|byte| *byte != 0) {
            return Err(invalid_data(
                "archive contains non-zero data after its tar terminator",
            ));
        }
    }
}

/// Enumerate exact canonical installed slots under the lifecycle-owned namespace.
fn installed_slot_paths(
    pack_root: &Path,
) -> Result<Vec<InstalledSlotPath>, OptionalParserPackLifecycleError> {
    match fs::symlink_metadata(pack_root) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(source) => {
            return Err(io_error(
                "inspect parser-pack storage root",
                pack_root,
                source,
            ));
        }
    }
    let versions = pack_root.join("versions");
    match fs::symlink_metadata(&versions) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(source) => {
            return Err(io_error(
                "inspect parser-pack versions root",
                versions,
                source,
            ));
        }
    }
    let mut slots = Vec::new();
    let mut observed_entries = 0usize;
    for version in fs::read_dir(&versions)
        .map_err(|source| io_error("list parser-pack versions", &versions, source))?
    {
        if observed_entries == LIFECYCLE_METADATA_ENTRY_LIMIT {
            return Err(invalid_data(
                "parser-pack metadata entries exceed the cleanup bound",
            ));
        }
        observed_entries = observed_entries.saturating_add(1);
        let version = version
            .map_err(|source| io_error("read parser-pack version entry", &versions, source))?;
        if !version
            .file_type()
            .map_err(|source| {
                io_error("inspect parser-pack version entry", version.path(), source)
            })?
            .is_dir()
        {
            continue;
        }
        let projectatlas_version_name = version
            .file_name()
            .into_string()
            .map_err(|_name| invalid_data("parser-pack version directory is not UTF-8"))?;
        for slot in fs::read_dir(version.path())
            .map_err(|source| io_error("list parser-pack slots", version.path(), source))?
        {
            if observed_entries == LIFECYCLE_METADATA_ENTRY_LIMIT {
                return Err(invalid_data(
                    "parser-pack metadata entries exceed the cleanup bound",
                ));
            }
            observed_entries = observed_entries.saturating_add(1);
            let slot = slot.map_err(|source| {
                io_error("read parser-pack slot entry", version.path(), source)
            })?;
            if !slot
                .file_type()
                .map_err(|source| io_error("inspect parser-pack slot entry", slot.path(), source))?
                .is_dir()
            {
                continue;
            }
            let entry_name = slot
                .file_name()
                .into_string()
                .map_err(|_name| invalid_data("parser-pack artifact directory is not UTF-8"))?;
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            if let Some((state, artifact)) = parse_windows_tombstone_name(&entry_name) {
                let entry_root = slot.path();
                slots.push(InstalledSlotPath {
                    projectatlas_version: projectatlas_version_name.clone(),
                    artifact,
                    pack_root: (state == InstalledSlotCleanupState::ProfilePending)
                        .then(|| entry_root.clone()),
                    entry_root,
                    state,
                });
                continue;
            }
            let entry_root = slot.path();
            slots.push(InstalledSlotPath {
                #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                projectatlas_version: projectatlas_version_name.clone(),
                artifact: entry_name,
                #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                pack_root: Some(entry_root.clone()),
                entry_root,
                #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
                state: InstalledSlotCleanupState::Installed,
            });
        }
        #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
        drop(projectatlas_version_name);
    }
    Ok(slots)
}

/// Parse only unique lifecycle-owned Windows tombstone directory names.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn parse_windows_tombstone_name(name: &str) -> Option<(InstalledSlotCleanupState, String)> {
    let (state, remainder) =
        if let Some(remainder) = name.strip_prefix(WINDOWS_REMOVING_TOMBSTONE_PREFIX) {
            (InstalledSlotCleanupState::ProfilePending, remainder)
        } else if let Some(remainder) = name.strip_prefix(WINDOWS_CLEANED_TOMBSTONE_PREFIX) {
            (InstalledSlotCleanupState::ProfileCleaned, remainder)
        } else {
            return None;
        };
    if remainder.len() < 65 {
        return None;
    }
    let (artifact, unique_suffix) = remainder.split_at(64);
    if !unique_suffix.starts_with('-')
        || unique_suffix.len() < 2
        || !artifact
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some((state, artifact.to_owned()))
}

/// Return whether one artifact has any bounded profile-removal tombstone.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn windows_slot_cleanup_in_progress(
    version_root: &Path,
    artifact: &str,
) -> Result<bool, OptionalParserPackLifecycleError> {
    ParserContentDigest::new(artifact.to_owned())
        .map_err(|source| invalid_data(source.to_string()))?;
    let entries = fs::read_dir(version_root)
        .map_err(|source| io_error("list parser-pack cleanup tombstones", version_root, source))?;
    for (index, entry) in entries.enumerate() {
        if index == LIFECYCLE_METADATA_ENTRY_LIMIT {
            return Err(invalid_data(
                "parser-pack cleanup tombstones exceed the lifecycle bound",
            ));
        }
        let entry = entry.map_err(|source| {
            io_error("read parser-pack cleanup tombstone", version_root, source)
        })?;
        if !entry
            .file_type()
            .map_err(|source| {
                io_error(
                    "inspect parser-pack cleanup tombstone",
                    entry.path(),
                    source,
                )
            })?
            .is_dir()
        {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if parse_windows_tombstone_name(&name)
            .is_some_and(|(_state, candidate)| candidate == artifact)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Atomically move a deterministic slot into one unique profile-pending tombstone.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn transition_slot_to_removing_tombstone(
    slot: &InstalledSlotPath,
) -> Result<InstalledSlotPath, OptionalParserPackLifecycleError> {
    if slot.state != InstalledSlotCleanupState::Installed {
        return Err(invalid_data(
            "only an installed slot can enter the removing tombstone state",
        ));
    }
    ParserContentDigest::new(slot.artifact.clone())
        .map_err(|source| invalid_data(source.to_string()))?;
    let parent = slot
        .entry_root
        .parent()
        .ok_or_else(|| invalid_data("parser-pack slot has no version parent"))?;
    let prefix = format!("{WINDOWS_REMOVING_TOMBSTONE_PREFIX}{}-", slot.artifact);
    let reservation = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent)
        .map_err(|source| io_error("reserve unique parser-pack tombstone", parent, source))?;
    let tombstone = reservation.path().to_path_buf();
    reservation.close().map_err(|source| {
        io_error(
            "release parser-pack tombstone reservation",
            &tombstone,
            source,
        )
    })?;
    fs::rename(&slot.entry_root, &tombstone).map_err(|source| {
        io_error(
            "move parser-pack slot into removing tombstone",
            &slot.entry_root,
            source,
        )
    })?;
    Ok(InstalledSlotPath {
        projectatlas_version: slot.projectatlas_version.clone(),
        artifact: slot.artifact.clone(),
        entry_root: tombstone.clone(),
        pack_root: Some(tombstone),
        state: InstalledSlotCleanupState::ProfilePending,
    })
}

/// Atomically record successful profile cleanup in the unique tombstone path itself.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn transition_tombstone_to_profile_cleaned(
    slot: &InstalledSlotPath,
) -> Result<InstalledSlotPath, OptionalParserPackLifecycleError> {
    if slot.state != InstalledSlotCleanupState::ProfilePending {
        return Err(invalid_data(
            "only a profile-pending tombstone can become profile-cleaned",
        ));
    }
    let name = slot
        .entry_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_data("parser-pack cleanup tombstone name is not UTF-8"))?;
    let remainder = name
        .strip_prefix(WINDOWS_REMOVING_TOMBSTONE_PREFIX)
        .ok_or_else(|| invalid_data("parser-pack removing tombstone prefix is invalid"))?;
    let parent = slot
        .entry_root
        .parent()
        .ok_or_else(|| invalid_data("parser-pack cleanup tombstone has no parent"))?;
    let cleaned_root = parent.join(format!("{WINDOWS_CLEANED_TOMBSTONE_PREFIX}{remainder}"));
    if fs::symlink_metadata(&cleaned_root).is_ok() {
        return Err(invalid_data(
            "profile-cleaned parser-pack tombstone path is already occupied",
        ));
    }
    fs::rename(&slot.entry_root, &cleaned_root).map_err(|source| {
        io_error(
            "record parser-pack profile cleanup in tombstone state",
            &slot.entry_root,
            source,
        )
    })?;
    Ok(InstalledSlotPath {
        projectatlas_version: slot.projectatlas_version.clone(),
        artifact: slot.artifact.clone(),
        entry_root: cleaned_root,
        pack_root: None,
        state: InstalledSlotCleanupState::ProfileCleaned,
    })
}

/// Run the exact verified Windows broker cleanup command for its own artifact directory.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn cleanup_platform_profile(
    root: &Path,
    artifact: &ParserArtifactIdentity,
) -> Result<(), OptionalParserPackLifecycleError> {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let manifest_path = root.join(ARTIFACT_MANIFEST_FILE_NAME);
    let manifest_bytes = read_bounded_file(
        &manifest_path,
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)
            .map_err(|source| invalid_data(source.to_string()))?,
    )?;
    let observed = ParserArtifactIdentity::for_bytes(&manifest_bytes);
    if &observed != artifact {
        return Err(invalid_data(
            "artifact manifest changed before profile cleanup",
        ));
    }
    let sha256 = Sha256::digest(&manifest_bytes);
    let profile_name = format!("projectatlas.parser.{}", lowercase_hex(&sha256[..20]));
    let broker = root.join(WINDOWS_CONTAINMENT_BROKER_FILE_NAME);
    let windows_directory = validated_windows_directory()?;
    let mut command = Command::new(&broker);
    command
        .arg(WINDOWS_PROFILE_CLEANUP_ARGUMENT)
        .current_dir(root)
        .env_clear()
        .env("SystemRoot", &windows_directory)
        .env("WINDIR", &windows_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|source| io_error("start artifact profile cleanup broker", &broker, source))?;
    supervise_cleanup_broker(
        &mut child,
        &broker,
        &profile_name,
        WINDOWS_PROFILE_CLEANUP_TIMEOUT,
        WINDOWS_PROFILE_CLEANUP_REAP_TIMEOUT,
    )
}

/// Non-Windows hosts have no durable artifact profile to remove.
#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
// Keep one fallible cross-platform cleanup contract for the RAII owner; only
// the Windows implementation can fail because only Windows creates a profile.
#[allow(clippy::unnecessary_wraps)]
fn cleanup_platform_profile(
    _root: &Path,
    _artifact: &ParserArtifactIdentity,
) -> Result<(), OptionalParserPackLifecycleError> {
    Ok(())
}

/// Own one cleanup child through bounded operation, termination, reap, and pipe drain.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn supervise_cleanup_broker(
    child: &mut std::process::Child,
    broker: &Path,
    profile_name: &str,
    operation_timeout: Duration,
    cleanup_timeout: Duration,
) -> Result<(), OptionalParserPackLifecycleError> {
    let mut operation_failure = None;
    let stdout_reader = if let Some(stdout) = child.stdout.take() {
        Some(thread::spawn(move || read_bounded_cleanup_output(stdout)))
    } else {
        operation_failure = Some(invalid_data("profile cleanup broker stdout was not piped"));
        None
    };
    let stderr_reader = if let Some(stderr) = child.stderr.take() {
        Some(thread::spawn(move || read_bounded_cleanup_output(stderr)))
    } else {
        operation_failure
            .get_or_insert_with(|| invalid_data("profile cleanup broker stderr was not piped"));
        None
    };
    let operation_deadline = Instant::now()
        .checked_add(operation_timeout)
        .unwrap_or_else(Instant::now);
    let mut status = None;
    while operation_failure.is_none() && status.is_none() {
        match child.try_wait() {
            Ok(observed) => status = observed,
            Err(source) => {
                operation_failure = Some(io_error(
                    "wait for artifact profile cleanup broker",
                    broker,
                    source,
                ));
            }
        }
        if status.is_none() && Instant::now() >= operation_deadline {
            operation_failure = Some(invalid_data(format!(
                "profile {profile_name} cleanup exceeded its deadline"
            )));
        }
        if operation_failure.is_none() && status.is_none() {
            thread::sleep(Duration::from_millis(10));
        }
    }

    let cleanup_deadline = Instant::now()
        .checked_add(cleanup_timeout)
        .unwrap_or_else(Instant::now);
    let mut cleanup_failures = Vec::new();
    if status.is_none()
        && let Err(kill_source) = child.kill()
    {
        match child.try_wait() {
            Ok(Some(observed)) => status = Some(observed),
            Ok(None) => cleanup_failures.push(format!(
                "terminate cleanup broker failed: {kill_source}"
            )),
            Err(wait_source) => cleanup_failures.push(format!(
                "terminate cleanup broker failed: {kill_source}; observe child failed: {wait_source}"
            )),
        }
    }
    while status.is_none() && Instant::now() < cleanup_deadline {
        match child.try_wait() {
            Ok(observed) => status = observed,
            Err(source) => {
                cleanup_failures.push(format!("reap cleanup broker failed: {source}"));
                break;
            }
        }
        if status.is_none() {
            thread::sleep(Duration::from_millis(10));
        }
    }
    if status.is_none() {
        cleanup_failures
            .push("cleanup broker was not reaped within its cleanup deadline".to_owned());
    }

    while (!cleanup_reader_finished(stdout_reader.as_ref())
        || !cleanup_reader_finished(stderr_reader.as_ref()))
        && Instant::now() < cleanup_deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    let stdout = join_cleanup_reader(stdout_reader, "stdout", broker, &mut cleanup_failures);
    let stderr = join_cleanup_reader(stderr_reader, "stderr", broker, &mut cleanup_failures);

    if operation_failure.is_none() {
        let operation_result = match (status, stdout, stderr) {
            (Some(status), Some(Ok(stdout)), Some(Ok(stderr))) => match String::from_utf8(stdout) {
                Ok(stdout)
                    if status.success()
                        && stdout.trim_end() == WINDOWS_PROFILE_CLEANUP_RESULT
                        && stderr.is_empty() =>
                {
                    Ok(())
                }
                Ok(_) => Err(invalid_data(format!(
                    "profile {profile_name} cleanup broker returned an invalid bounded result"
                ))),
                Err(source) => Err(invalid_data(source.to_string())),
            },
            (_, Some(Err(source)), _) => Err(io_error(
                "read artifact profile cleanup stdout",
                broker,
                source,
            )),
            (_, _, Some(Err(source))) => Err(io_error(
                "read artifact profile cleanup stderr",
                broker,
                source,
            )),
            _ => Err(invalid_data(
                "profile cleanup broker result was incomplete after reap",
            )),
        };
        if let Err(error) = operation_result {
            operation_failure = Some(error);
        }
    }

    let cleanup_failure = (!cleanup_failures.is_empty()).then(|| {
        OptionalParserPackLifecycleError::CleanupIncomplete {
            message: cleanup_failures.join("; ").chars().take(4_096).collect(),
        }
    });
    match (operation_failure, cleanup_failure) {
        (None, None) => Ok(()),
        (Some(operation), None) => Err(operation),
        (None, Some(cleanup)) => Err(cleanup),
        (Some(operation), Some(cleanup)) => {
            Err(OptionalParserPackLifecycleError::OperationAndCleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            })
        }
    }
}

/// Return whether one optional cleanup output reader has terminated.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn cleanup_reader_finished(
    reader: Option<&thread::JoinHandle<Result<Vec<u8>, io::Error>>>,
) -> bool {
    reader.is_none_or(thread::JoinHandle::is_finished)
}

/// Join one completed cleanup reader without hiding a mandatory drain failure.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn join_cleanup_reader(
    reader: Option<thread::JoinHandle<Result<Vec<u8>, io::Error>>>,
    stream: &'static str,
    broker: &Path,
    cleanup_failures: &mut Vec<String>,
) -> Option<Result<Vec<u8>, io::Error>> {
    let reader = reader?;
    if !reader.is_finished() {
        cleanup_failures.push(format!(
            "cleanup broker {stream} reader did not drain within its cleanup deadline"
        ));
        return None;
    }
    match reader.join() {
        Ok(result) => Some(result),
        Err(_panic) => {
            cleanup_failures.push(format!(
                "cleanup broker {stream} reader panicked for {}",
                broker.display()
            ));
            None
        }
    }
}

/// Resolve and validate the one Windows directory needed to initialize the managed broker.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn validated_windows_directory() -> Result<PathBuf, OptionalParserPackLifecycleError> {
    let configured = env::var_os("SystemRoot")
        .or_else(|| env::var_os("WINDIR"))
        .ok_or_else(|| invalid_data("Windows directory environment is unavailable"))?;
    let path = PathBuf::from(configured);
    if !path.is_absolute() {
        return Err(invalid_data("Windows directory is not absolute"));
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|source| io_error("inspect Windows directory", &path, source))?;
    if !metadata.file_type().is_dir() {
        return Err(invalid_data("Windows directory is not a real directory"));
    }
    fs::canonicalize(&path)
        .map_err(|source| io_error("canonicalize Windows directory", path, source))
}

/// Read one trusted cleanup stream through a hard byte ceiling.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn read_bounded_cleanup_output(input: impl Read) -> Result<Vec<u8>, io::Error> {
    let mut output = Vec::new();
    input
        .take(WINDOWS_PROFILE_CLEANUP_OUTPUT_BYTES.saturating_add(1))
        .read_to_end(&mut output)?;
    if u64::try_from(output.len()).unwrap_or(u64::MAX) > WINDOWS_PROFILE_CLEANUP_OUTPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "profile cleanup output exceeded its byte ceiling",
        ));
    }
    Ok(output)
}

/// Count immutable slot directory rows without opening any pack file.
fn count_installed_slots(
    pack_root: &Path,
) -> Result<(usize, bool, bool), OptionalParserPackLifecycleError> {
    let versions = pack_root.join("versions");
    match fs::symlink_metadata(pack_root) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok((0, false, false)),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(invalid_data("parser-pack storage root is not a directory"));
        }
        Ok(_) => {}
        Err(source) => {
            return Err(io_error(
                "inspect parser-pack storage root",
                pack_root,
                source,
            ));
        }
    }
    match fs::symlink_metadata(&versions) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok((0, false, false)),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(invalid_data("parser-pack versions root is not a directory"));
        }
        Ok(_) => {}
        Err(source) => {
            return Err(io_error(
                "inspect parser-pack versions root",
                versions,
                source,
            ));
        }
    }
    let mut count = 0usize;
    let mut observed_entries = 0usize;
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let mut cleanup_pending = false;
    #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
    let cleanup_pending = false;
    let version_entries = fs::read_dir(&versions)
        .map_err(|source| io_error("list parser-pack versions", &versions, source))?;
    for version in version_entries {
        if observed_entries == LIFECYCLE_METADATA_ENTRY_LIMIT {
            return Ok((count, true, cleanup_pending));
        }
        observed_entries = observed_entries.saturating_add(1);
        let version = version
            .map_err(|source| io_error("read parser-pack version entry", &versions, source))?;
        if !version
            .file_type()
            .map_err(|source| {
                io_error("inspect parser-pack version entry", version.path(), source)
            })?
            .is_dir()
        {
            continue;
        }
        let slots = fs::read_dir(version.path())
            .map_err(|source| io_error("list parser-pack slots", version.path(), source))?;
        for slot in slots {
            if observed_entries == LIFECYCLE_METADATA_ENTRY_LIMIT {
                return Ok((count, true, cleanup_pending));
            }
            observed_entries = observed_entries.saturating_add(1);
            let slot = slot.map_err(|source| {
                io_error("read parser-pack slot entry", version.path(), source)
            })?;
            if !slot
                .file_type()
                .map_err(|source| io_error("inspect parser-pack slot entry", slot.path(), source))?
                .is_dir()
            {
                continue;
            }
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            if slot
                .file_name()
                .to_str()
                .and_then(parse_windows_tombstone_name)
                .is_some()
            {
                cleanup_pending = true;
                continue;
            }
            count = count.saturating_add(1);
        }
    }
    Ok((count, false, cleanup_pending))
}

/// Open or create the stable direct lease file without accepting an indirect leaf.
fn open_or_create_direct_lease_file(path: &Path) -> Result<File, OptionalParserPackLifecycleError> {
    loop {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                require_direct_lease_file(path, &metadata)?;
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .map_err(|source| {
                        io_error("open optional parser-pack lifecycle lease", path, source)
                    })?;
                require_direct_lease_path(path)?;
                return Ok(file);
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                match OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
                {
                    Ok(file) => {
                        require_direct_lease_path(path)?;
                        return Ok(file);
                    }
                    Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(source) => {
                        return Err(io_error(
                            "create optional parser-pack lifecycle lease",
                            path,
                            source,
                        ));
                    }
                }
            }
            Err(source) => {
                return Err(io_error(
                    "inspect optional parser-pack lifecycle lease",
                    path,
                    source,
                ));
            }
        }
    }
}

/// Revalidate that the stable named lease remains a direct regular file.
fn require_direct_lease_path(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        io_error(
            "revalidate optional parser-pack lifecycle lease",
            path,
            source,
        )
    })?;
    require_direct_lease_file(path, &metadata)?;
    Ok(())
}

/// Reject symlink, Windows reparse-point, directory, and special-file lease leaves.
fn require_direct_lease_file(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), OptionalParserPackLifecycleError> {
    #[cfg(windows)]
    let indirect = {
        use std::os::windows::fs::MetadataExt as _;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    };
    #[cfg(not(windows))]
    let indirect = metadata.file_type().is_symlink();
    if indirect || !metadata.file_type().is_file() {
        return Err(invalid_data(format!(
            "optional parser-pack lifecycle lease is not a direct regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Create the caller-selected storage anchor; its own ancestor policy belongs to the caller.
fn ensure_anchor_directory(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(invalid_data(
            "parser-pack storage anchor is not a directory",
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|source| io_error("create parser-pack storage anchor", path, source)),
        Err(source) => Err(io_error("inspect parser-pack storage anchor", path, source)),
    }
}

/// Create one direct product-owned directory without following a symlink at that component.
fn ensure_direct_directory(
    parent: &Path,
    path: &Path,
) -> Result<(), OptionalParserPackLifecycleError> {
    if path.parent() != Some(parent) {
        return Err(invalid_data(
            "lifecycle directory is not a direct owned child",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(invalid_data(
            "product-owned lifecycle component is not a real directory",
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => fs::create_dir(path)
            .map_err(|source| io_error("create product-owned lifecycle directory", path, source)),
        Err(source) => Err(io_error(
            "inspect product-owned lifecycle directory",
            path,
            source,
        )),
    }
}

/// Closed observation for one product-owned direct directory component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectDirectoryState {
    /// The exact component is absent.
    Missing,
    /// The exact component is a real directory.
    Real,
    /// The exact component is a symlink, junction, or non-directory entry.
    Unsafe,
}

/// Inspect one exact component without following a symlink at that component.
fn direct_directory_state(
    path: &Path,
) -> Result<DirectDirectoryState, OptionalParserPackLifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(DirectDirectoryState::Real),
        Ok(_) => Ok(DirectDirectoryState::Unsafe),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(DirectDirectoryState::Missing)
        }
        Err(source) => Err(io_error(
            "inspect product-owned lifecycle component",
            path,
            source,
        )),
    }
}

/// Remove one exact file or symlink without interpreting its content.
fn remove_file_if_present(path: &Path) -> Result<bool, OptionalParserPackLifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Err(invalid_data(
            "project parser-pack selection path is a directory",
        )),
        Ok(_) => {
            make_path_writable(path)?;
            fs::remove_file(path)
                .map_err(|source| io_error("remove project parser-pack selection", path, source))?;
            Ok(true)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error(
            "inspect project parser-pack selection",
            path,
            source,
        )),
    }
}

/// Remove one exact lifecycle-owned tree, or its symlink leaf, without following it.
fn remove_tree_if_present(path: &Path) -> Result<bool, OptionalParserPackLifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            make_path_writable(path)?;
            remove_symlink_leaf(path)?;
            Ok(true)
        }
        Ok(metadata) if metadata.file_type().is_dir() => {
            make_tree_writable(path)?;
            fs::remove_dir_all(path)
                .map_err(|source| io_error("remove parser-pack storage", path, source))?;
            Ok(true)
        }
        Ok(_) => {
            make_path_writable(path)?;
            fs::remove_file(path)
                .map_err(|source| io_error("remove parser-pack storage entry", path, source))?;
            Ok(true)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error("inspect parser-pack storage", path, source)),
    }
}

/// Remove only a symlink or junction leaf, never its target.
#[cfg(windows)]
fn remove_symlink_leaf(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(directory_error) => fs::remove_file(path).map_err(|file_error| {
            invalid_data(format!(
                "could not remove parser-pack storage link: directory={directory_error}; file={file_error}"
            ))
        }),
    }
}

/// Remove only a symlink leaf, never its target.
#[cfg(not(windows))]
fn remove_symlink_leaf(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    fs::remove_file(path)
        .map_err(|source| io_error("remove parser-pack storage symlink", path, source))
}

/// Recursively remove write permissions so a published slot is immutable to normal lifecycle code.
fn seal_immutable_tree(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect parser-pack staging tree", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_data("parser-pack staging tree contains a symlink"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|source| io_error("list parser-pack staging tree", path, source))?
        {
            let entry =
                entry.map_err(|source| io_error("read parser-pack staging entry", path, source))?;
            seal_immutable_tree(&entry.path())?;
        }
    }
    set_path_immutable(path, metadata.is_dir())
}

/// Verify that an installed slot has no writable file or directory entry.
fn verify_immutable_tree(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect immutable parser-pack slot", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_data(
            "immutable parser-pack slot contains a symlink",
        ));
    }
    verify_path_immutable(path, &metadata)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|source| io_error("list immutable parser-pack slot", path, source))?
        {
            let entry = entry.map_err(|source| {
                io_error("read immutable parser-pack slot entry", path, source)
            })?;
            verify_immutable_tree(&entry.path())?;
        }
    }
    Ok(())
}

/// Restore writable permissions recursively before exact cleanup.
fn make_tree_writable(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_error("inspect parser-pack cleanup tree", path, source)),
    };
    make_path_writable(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in fs::read_dir(path)
            .map_err(|source| io_error("list parser-pack cleanup tree", path, source))?
        {
            let entry =
                entry.map_err(|source| io_error("read parser-pack cleanup entry", path, source))?;
            make_tree_writable(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
/// Restore the canonical archive mode after extraction.
fn set_extracted_mode(path: &Path, mode: u32) -> Result<(), OptionalParserPackLifecycleError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|source| io_error("set extracted parser-pack mode", path, source))
}

#[cfg(unix)]
/// Remove write bits while retaining executable bits from the canonical archive.
fn set_path_immutable(
    path: &Path,
    _directory: bool,
) -> Result<(), OptionalParserPackLifecycleError> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect parser-pack permissions", path, source))?;
    let mode = metadata.permissions().mode() & !0o222;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|source| io_error("seal parser-pack permissions", path, source))
}

#[cfg(windows)]
/// Mark one installed file read-only; directories retain normal traversal semantics.
fn set_path_immutable(
    path: &Path,
    directory: bool,
) -> Result<(), OptionalParserPackLifecycleError> {
    if directory {
        return Ok(());
    }
    let mut permissions = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect parser-pack permissions", path, source))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
        .map_err(|source| io_error("seal parser-pack permissions", path, source))
}

#[cfg(not(any(unix, windows)))]
/// Preserve permissions on hosts that cannot activate the optional pack.
fn set_path_immutable(
    _path: &Path,
    _directory: bool,
) -> Result<(), OptionalParserPackLifecycleError> {
    Ok(())
}

#[cfg(unix)]
/// Require that no installed entry retains a Unix write bit.
fn verify_path_immutable(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), OptionalParserPackLifecycleError> {
    use std::os::unix::fs::PermissionsExt as _;
    if metadata.permissions().mode() & 0o222 != 0 {
        return Err(invalid_data(format!(
            "installed parser-pack entry {} is writable",
            path.file_name().unwrap_or_default().display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
/// Require that every installed regular file retains its read-only marker.
fn verify_path_immutable(
    _path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), OptionalParserPackLifecycleError> {
    if metadata.is_file() && !metadata.permissions().readonly() {
        return Err(invalid_data("installed parser-pack file is writable"));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
/// Accept metadata on hosts that cannot activate the optional pack.
fn verify_path_immutable(
    _path: &Path,
    _metadata: &fs::Metadata,
) -> Result<(), OptionalParserPackLifecycleError> {
    Ok(())
}

#[cfg(unix)]
/// Restore owner traversal and write bits before exact tree cleanup.
fn make_path_writable(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect parser-pack cleanup permissions", path, source))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let mode = metadata.permissions().mode() | 0o700;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|source| io_error("restore parser-pack cleanup permissions", path, source))
}

#[cfg(windows)]
/// Clear only the Windows read-only file attribute before exact tree cleanup.
#[allow(clippy::permissions_set_readonly_false)]
fn make_path_writable(path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect parser-pack cleanup permissions", path, source))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let mut permissions = metadata.permissions();
    // On Windows this toggles only FILE_ATTRIBUTE_READONLY; the Unix world-writable
    // behavior guarded by Clippy is compiled in the separate PermissionsExt branch above.
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)
        .map_err(|source| io_error("restore parser-pack cleanup permissions", path, source))
}

#[cfg(not(any(unix, windows)))]
/// Preserve permissions on hosts that cannot activate the optional pack.
fn make_path_writable(_path: &Path) -> Result<(), OptionalParserPackLifecycleError> {
    Ok(())
}

/// Resolve the user-owned parser-pack storage root for normal operation.
fn default_storage_root() -> Result<PathBuf, OptionalParserPackLifecycleError> {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("ProjectAtlas").join("parser-packs"))
            .ok_or(OptionalParserPackLifecycleError::StorageRootUnavailable)
    }
    #[cfg(target_os = "macos")]
    {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| {
                root.join("Library")
                    .join("Application Support")
                    .join("ProjectAtlas")
                    .join("parser-packs")
            })
            .ok_or(OptionalParserPackLifecycleError::StorageRootUnavailable);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(root)
                .join("projectatlas")
                .join("parser-packs"));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| root.join(".local/share/projectatlas/parser-packs"))
            .ok_or(OptionalParserPackLifecycleError::StorageRootUnavailable)
    }
    #[cfg(not(any(unix, windows)))]
    Err(OptionalParserPackLifecycleError::StorageRootUnavailable)
}

/// Current accepted optional-pack target, if any.
fn host_pack_platform() -> Option<PackPlatform> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Some(PackPlatform::LinuxX86_64),
        ("windows", "x86_64") => Some(PackPlatform::WindowsX86_64),
        _ => None,
    }
}

/// Canonical lowercase hexadecimal without another dependency.
fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Construct one bounded data-contract error.
fn invalid_data(reason: impl Into<String>) -> OptionalParserPackLifecycleError {
    OptionalParserPackLifecycleError::InvalidData {
        reason: reason.into(),
    }
}

/// Construct one filesystem error retaining exact operation and path context.
fn io_error(
    operation: &'static str,
    path: impl Into<PathBuf>,
    source: io::Error,
) -> OptionalParserPackLifecycleError {
    OptionalParserPackLifecycleError::Io {
        operation,
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    /// Child-only storage root used to prove lease release without running destructors.
    const ABRUPT_LEASE_STORAGE_ENV: &str = "PROJECTATLAS_TEST_ABRUPT_LEASE_STORAGE";
    /// Child-only marker proving the shared lease was acquired before abrupt exit.
    const ABRUPT_LEASE_MARKER_ENV: &str = "PROJECTATLAS_TEST_ABRUPT_LEASE_MARKER";
    /// Child-only lease kind selected by the cross-process lifecycle tests.
    const ABRUPT_LEASE_KIND_ENV: &str = "PROJECTATLAS_TEST_ABRUPT_LEASE_KIND";
    /// Deliberate non-success code used by the no-destructor child process.
    const ABRUPT_LEASE_EXIT_CODE: i32 = 86;

    type TestResult = Result<(), Box<dyn Error>>;

    fn require(condition: bool, message: &str) -> TestResult {
        if condition {
            Ok(())
        } else {
            Err(Box::new(io::Error::other(message.to_owned())))
        }
    }

    /// Require one lifecycle call to fail without panicking the test process.
    fn require_lifecycle_error<T>(
        result: Result<T, OptionalParserPackLifecycleError>,
        message: &str,
    ) -> Result<OptionalParserPackLifecycleError, Box<dyn Error>> {
        match result {
            Ok(_) => Err(Box::new(io::Error::other(message.to_owned()))),
            Err(error) => Ok(error),
        }
    }

    fn test_slot(byte: char) -> PackSlotIdentity {
        PackSlotIdentity {
            projectatlas_version: OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION.to_owned(),
            artifact: std::iter::repeat_n(byte, 64).collect(),
        }
    }

    #[test]
    fn installed_slot_transfer_disarms_temporary_profile_cleanup() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut profile = TemporaryParserArtifactProfile::new(
            directory.path(),
            ParserArtifactIdentity::for_bytes(b"artifact-manifest"),
        );
        profile.transfer_to_installed_slot();
        require(
            !profile.cleanup_pending,
            "installed-slot transfer retained temporary cleanup ownership",
        )?;
        profile.cleanup()?;
        Ok(())
    }

    #[test]
    fn lifecycle_operation_and_cleanup_failures_are_both_retained() -> TestResult {
        let error = require_lifecycle_error(
            finish_with_cleanup::<()>(
                Err(invalid_data("operation failed")),
                Err(invalid_data("cleanup failed")),
            ),
            "dual lifecycle failure was accepted",
        )?;
        match error {
            OptionalParserPackLifecycleError::OperationAndCleanup { operation, cleanup } => {
                require(
                    operation.to_string().contains("operation failed")
                        && cleanup.to_string().contains("cleanup failed"),
                    "dual lifecycle failure lost one typed cause",
                )
            }
            other => Err(Box::new(io::Error::other(format!(
                "dual lifecycle failure returned the wrong variant: {other}"
            )))),
        }
    }

    #[test]
    fn shared_pack_leases_exclude_storage_mutation_until_every_reader_releases() -> TestResult {
        let root = tempfile::tempdir()?;
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            storage.clone(),
            Some(PackPlatform::LinuxX86_64),
        );
        fs::create_dir_all(lifecycle.versions_root()?)?;

        let first_reader = lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?;
        let second_reader = lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?;
        let error = require_lifecycle_error(
            lifecycle.remove(),
            "exclusive removal succeeded while shared leases were retained",
        )?;
        require(
            matches!(error, OptionalParserPackLifecycleError::Busy { .. }),
            "shared/exclusive contention did not retain a typed busy failure",
        )?;
        require(
            lifecycle.pack_root()?.is_dir(),
            "contended removal changed immutable pack storage",
        )?;

        drop(first_reader);
        require(
            matches!(
                lifecycle.remove(),
                Err(OptionalParserPackLifecycleError::Busy { .. })
            ),
            "one remaining shared lease did not retain exclusion",
        )?;
        drop(second_reader);
        require(
            lifecycle.remove()?.changed,
            "removal did not proceed after every shared lease released",
        )?;

        let writer = lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Exclusive)?;
        require(
            matches!(
                lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Shared),
                Err(OptionalParserPackLifecycleError::Busy { .. })
            ),
            "exclusive lease did not exclude a later shared reader",
        )?;
        drop(writer);
        let _reader_after_release =
            lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?;
        require(
            storage.join(OPTIONAL_PARSER_PACK_LEASE_FILE_NAME).is_file(),
            "stable lifecycle lease disappeared with logical pack storage",
        )
    }

    #[test]
    fn abrupt_lease_holder_process() -> TestResult {
        let Some(storage) = env::var_os(ABRUPT_LEASE_STORAGE_ENV).map(PathBuf::from) else {
            return Ok(());
        };
        let marker = env::var_os(ABRUPT_LEASE_MARKER_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("abrupt lease marker path is missing"))?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            storage.join("project"),
            storage,
            Some(PackPlatform::LinuxX86_64),
        );
        let kind = env::var(ABRUPT_LEASE_KIND_ENV)?;
        let _lease = match kind.as_str() {
            "pack" => lifecycle.acquire_pack_lease(OptionalParserPackLeaseMode::Shared)?,
            "selection" => lifecycle.acquire_selection_mutation_lease()?,
            _ => return Err(io::Error::other("unknown abrupt lease kind").into()),
        };
        fs::write(marker, kind)?;
        std::thread::sleep(std::time::Duration::from_secs(30));
        std::process::exit(ABRUPT_LEASE_EXIT_CODE);
    }

    fn wait_for_child_marker(marker: &Path) -> TestResult {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.is_file() {
            if std::time::Instant::now() >= deadline {
                return Err(
                    io::Error::other("lease-holder child did not publish readiness").into(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }

    #[test]
    fn live_child_excludes_pack_mutation_and_abrupt_exit_releases_lease() -> TestResult {
        let root = tempfile::tempdir()?;
        let storage = root.path().join("storage");
        let marker = root.path().join("lease-acquired");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            storage.clone(),
            Some(PackPlatform::LinuxX86_64),
        );
        fs::create_dir_all(lifecycle.versions_root()?.join("retained"))?;
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("optional_parser_lifecycle::tests::abrupt_lease_holder_process")
            .arg("--nocapture")
            .env(ABRUPT_LEASE_STORAGE_ENV, &storage)
            .env(ABRUPT_LEASE_MARKER_ENV, &marker)
            .env(ABRUPT_LEASE_KIND_ENV, "pack")
            .spawn()?;
        wait_for_child_marker(&marker)?;
        require(
            matches!(
                lifecycle.remove(),
                Err(OptionalParserPackLifecycleError::Busy { .. })
            ),
            "live child execution did not exclude immutable storage removal",
        )?;
        child.kill()?;
        let _status = child.wait()?;
        require(
            lifecycle.remove()?.changed,
            "post-exit removal did not succeed",
        )
    }

    #[test]
    fn live_child_serializes_selection_mutation_and_abrupt_exit_releases_lease() -> TestResult {
        let root = tempfile::tempdir()?;
        let storage = root.path().join("storage");
        let project = storage.join("project");
        let marker = root.path().join("selection-lease-acquired");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage.clone(),
            Some(PackPlatform::LinuxX86_64),
        );
        lifecycle.write_selection(&ProjectSelection::new(test_slot('a'), None))?;
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("optional_parser_lifecycle::tests::abrupt_lease_holder_process")
            .arg("--nocapture")
            .env(ABRUPT_LEASE_STORAGE_ENV, &storage)
            .env(ABRUPT_LEASE_MARKER_ENV, &marker)
            .env(ABRUPT_LEASE_KIND_ENV, "selection")
            .spawn()?;
        wait_for_child_marker(&marker)?;
        for (operation, result) in [
            ("enable", lifecycle.enable(&test_slot('b').artifact)),
            (
                "update",
                lifecycle.update(&root.path().join("candidate.tar.zst")),
            ),
            ("disable", lifecycle.disable()),
            ("remove", lifecycle.remove()),
        ] {
            require(
                matches!(result, Err(OptionalParserPackLifecycleError::Busy { .. })),
                &format!("live child selection transition did not serialize {operation}"),
            )?;
        }
        require(
            lifecycle.selection_path().is_file(),
            "contended selection operations changed project selection",
        )?;
        child.kill()?;
        let _status = child.wait()?;
        require(
            lifecycle.disable()?.changed,
            "post-exit disable did not succeed",
        )
    }

    /// Create a directory link for hostile-storage tests.
    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    /// Create a directory symlink, falling back to a junction without following it later.
    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(source) if source.raw_os_error() == Some(1314) => {
                let status = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(link)
                    .arg(target)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(source)
                }
            }
            Err(source) => Err(source),
        }
    }

    #[test]
    fn unsupported_operations_refuse_before_archive_or_state_access() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("missing-project");
        let storage = root.path().join("missing-storage");
        let archive = root.path().join("missing-archive.tar.zst");
        let lifecycle = OptionalParserPackLifecycle::for_test(project, storage.clone(), None);

        for error in [
            require_lifecycle_error(lifecycle.verify(&archive), "verify must refuse")?,
            require_lifecycle_error(lifecycle.install(&archive), "install must refuse")?,
            require_lifecycle_error(lifecycle.enable(&"a".repeat(64)), "enable must refuse")?,
            require_lifecycle_error(lifecycle.update(&archive), "update must refuse")?,
        ] {
            require(
                error.is_unsupported_containment(),
                "failure was not typed unsupported containment",
            )?;
        }
        require(!storage.exists(), "unsupported operation created storage")
    }

    #[test]
    fn runtime_handoff_is_absent_everywhere_and_refuses_present_unsupported_state() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle =
            OptionalParserPackLifecycle::for_test(project.clone(), storage.clone(), None);
        require(
            lifecycle.resolve_selected_pack()?.is_none(),
            "absent selection did not preserve default-core operation",
        )?;
        let selection = project.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
        fs::create_dir_all(
            selection
                .parent()
                .ok_or_else(|| io::Error::other("selection parent missing"))?,
        )?;
        fs::write(&selection, b"not inspected on an unsupported host")?;
        let error = require_lifecycle_error(
            lifecycle.resolve_selected_pack(),
            "present unsupported selection must refuse",
        )?;
        require(
            error.is_unsupported_containment(),
            "present selection was not typed unsupported containment",
        )?;
        require(!storage.exists(), "runtime handoff touched pack storage")
    }

    #[test]
    fn project_selection_derivation_is_content_free_strict_and_storage_independent() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage-is-a-file");
        fs::write(&storage, b"must not be inspected")?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        );

        require(
            lifecycle.derive_project_selection()? == OptionalParserPackProjectSelection::Inactive,
            "absent project selection was not inactive",
        )?;
        let selected = test_slot('a');
        lifecycle.write_selection(&ProjectSelection::new(selected.clone(), None))?;
        let derivation = lifecycle.derive_project_selection()?;
        let key = derivation
            .selection_key()
            .ok_or_else(|| io::Error::other("selected derivation omitted its key"))?;
        require(
            key.as_str()
                == format!(
                    "{}:{}:{}",
                    OPTIONAL_PARSER_PACK_ID, selected.projectatlas_version, selected.artifact
                ),
            "selected derivation key was not stable",
        )?;
        require(
            derivation.artifact() == Some(key.artifact()),
            "selected derivation artifact did not delegate to its key",
        )?;
        require(
            OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH == ".projectatlas/optional-parser-pack.json",
            "public selection policy path drifted",
        )?;

        fs::write(lifecycle.selection_path(), b"malformed")?;
        require(
            lifecycle.derive_project_selection().is_err(),
            "supported derivation accepted malformed selection JSON",
        )
    }

    #[test]
    fn public_constructor_defers_storage_and_prioritizes_unsupported_state() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let mut lifecycle = OptionalParserPackLifecycle::new(project.clone(), None)?;
        require(
            lifecycle.storage_root.get().is_none(),
            "public constructor eagerly resolved the user storage root",
        )?;
        lifecycle.platform = None;
        let archive = root.path().join("missing.tar.zst");
        let error = require_lifecycle_error(
            lifecycle.verify(&archive),
            "unsupported verify did not fail",
        )?;
        require(
            error.is_unsupported_containment(),
            "unsupported verify lost typed priority",
        )?;
        require(
            lifecycle.storage_root.get().is_none(),
            "unsupported verify resolved user storage",
        )?;

        let selection = project.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
        fs::create_dir_all(
            selection
                .parent()
                .ok_or_else(|| io::Error::other("selection parent missing"))?,
        )?;
        fs::write(&selection, b"stale")?;
        if lifecycle.storage_root.set(None).is_err() {
            return Err(io::Error::other("deferred storage root was already initialized").into());
        }
        require(
            lifecycle.disable()?.changed,
            "disable required a user storage root",
        )?;
        require(
            !selection.exists(),
            "disable did not remove project selection",
        )
    }

    #[test]
    fn present_unsupported_derivation_refuses_before_malformed_content() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let selection = project.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
        fs::create_dir_all(
            selection
                .parent()
                .ok_or_else(|| io::Error::other("selection parent missing"))?,
        )?;
        fs::write(&selection, b"malformed and must not be read")?;
        let lifecycle =
            OptionalParserPackLifecycle::for_test(project, root.path().join("storage"), None);
        let error = require_lifecycle_error(
            lifecycle.derive_project_selection(),
            "present unsupported derivation did not fail",
        )?;
        require(
            error.is_unsupported_containment(),
            "present unsupported derivation inspected malformed contents",
        )
    }

    #[test]
    fn unsupported_cleanup_is_idempotent_for_stale_metadata() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let selection = project.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
        fs::create_dir_all(
            selection
                .parent()
                .ok_or_else(|| io::Error::other("selection parent missing"))?,
        )?;
        fs::write(&selection, b"stale")?;
        let pack_root = storage.join(OPTIONAL_PARSER_PACK_ID);
        fs::create_dir_all(pack_root.join("versions/stale/slot"))?;
        let payload = pack_root.join("versions/stale/slot/payload.bin");
        fs::write(&payload, b"payload")?;
        let mut permissions = fs::metadata(&payload)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&payload, permissions)?;
        let lifecycle = OptionalParserPackLifecycle::for_test(project, storage, None);

        require(
            lifecycle.disable()?.changed,
            "first disable did not remove stale selection",
        )?;
        require(
            !lifecycle.disable()?.changed,
            "second disable was not idempotent",
        )?;
        require(
            lifecycle.remove()?.changed,
            "first remove did not delete storage",
        )?;
        require(
            !lifecycle.remove()?.changed,
            "second remove was not idempotent",
        )
    }

    #[test]
    fn unsupported_remove_does_not_create_absent_storage() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let source = project.join("src/lib.rs");
        let selection = project.join(OPTIONAL_PARSER_PACK_SELECTION_POLICY_PATH);
        fs::create_dir_all(
            selection
                .parent()
                .ok_or_else(|| io::Error::other("selection parent missing"))?,
        )?;
        fs::create_dir_all(
            source
                .parent()
                .ok_or_else(|| io::Error::other("source parent missing"))?,
        )?;
        fs::write(&source, b"source must survive")?;
        fs::write(&selection, b"stale")?;
        let lifecycle = OptionalParserPackLifecycle::for_test(project, storage.clone(), None);

        require(
            lifecycle.remove()?.changed,
            "first remove did not delete stale selection",
        )?;
        require(!selection.exists(), "stale selection survived remove")?;
        require(!storage.exists(), "remove created absent storage")?;
        require(
            !lifecycle.remove()?.changed,
            "second remove was not idempotent",
        )?;
        require(
            fs::read(&source)? == b"source must survive",
            "unsupported remove touched source",
        )
    }

    #[test]
    fn removal_never_follows_product_owned_storage_links() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let external_pack = root.path().join("external-pack");
        fs::create_dir_all(&storage)?;
        fs::create_dir_all(&external_pack)?;
        let external_marker = external_pack.join("must-survive.txt");
        fs::write(&external_marker, b"outside")?;
        let pack_link = storage.join(OPTIONAL_PARSER_PACK_ID);
        create_directory_link(&external_pack, &pack_link)?;
        let lifecycle = OptionalParserPackLifecycle::for_test(project, storage.clone(), None);

        require(
            lifecycle.remove()?.changed,
            "pack-root link was not removed",
        )?;
        require(
            external_marker.is_file(),
            "pack-root link target was deleted",
        )?;
        require(!pack_link.exists(), "pack-root link leaf survived")?;

        let pack_root = storage.join(OPTIONAL_PARSER_PACK_ID);
        fs::create_dir_all(&pack_root)?;
        let external_versions = root.path().join("external-versions");
        fs::create_dir_all(&external_versions)?;
        let versions_marker = external_versions.join("must-survive.txt");
        fs::write(&versions_marker, b"outside")?;
        create_directory_link(&external_versions, &pack_root.join("versions"))?;

        require(lifecycle.remove()?.changed, "versions link was not removed")?;
        require(
            versions_marker.is_file(),
            "versions link target was deleted",
        )
    }

    #[test]
    fn selection_operations_never_follow_project_state_parent_link() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let external_state = root.path().join("external-state");
        fs::create_dir_all(&project)?;
        fs::create_dir_all(&external_state)?;
        let external_selection = external_state.join("optional-parser-pack.json");
        fs::write(&external_selection, b"must survive")?;
        create_directory_link(&external_state, &project.join(".projectatlas"))?;
        let lifecycle =
            OptionalParserPackLifecycle::for_test(project, root.path().join("storage"), None);

        require(
            lifecycle.status()?.state == OptionalParserPackState::Stale,
            "linked selection parent was not reported stale",
        )?;
        require(
            !lifecycle.disable()?.changed,
            "disable claimed to mutate a linked selection parent",
        )?;
        require(
            external_selection.is_file(),
            "disable deleted an external selection through a linked parent",
        )?;
        let _report = lifecycle.remove()?;
        require(
            external_selection.is_file(),
            "remove deleted an external selection through a linked parent",
        )
    }

    #[test]
    fn status_rejects_and_removal_does_not_follow_selected_slot_link() -> TestResult {
        let root = tempfile::tempdir()?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            root.path().join("storage"),
            None,
        );
        let selected = test_slot('d');
        let slot = lifecycle.slot_path(&selected)?;
        fs::create_dir_all(
            slot.parent()
                .ok_or_else(|| io::Error::other("slot parent missing"))?,
        )?;
        let external = root.path().join("external-slot");
        fs::create_dir_all(&external)?;
        let marker = external.join("must-survive.txt");
        fs::write(&marker, b"outside")?;
        create_directory_link(&external, &slot)?;
        lifecycle.write_selection(&ProjectSelection::new(selected, None))?;

        require(
            lifecycle.status()?.state == OptionalParserPackState::Stale,
            "linked selected slot was reported present",
        )?;
        let _report = lifecycle.remove()?;
        require(marker.is_file(), "selected slot link target was deleted")
    }

    #[test]
    fn install_refuses_product_owned_storage_link_before_archive_open() -> TestResult {
        let Some(platform) = host_pack_platform() else {
            return Ok(());
        };
        let root = tempfile::tempdir()?;
        let storage = root.path().join("storage");
        let external = root.path().join("external");
        fs::create_dir_all(&storage)?;
        fs::create_dir_all(&external)?;
        let marker = external.join("must-survive.txt");
        fs::write(&marker, b"outside")?;
        create_directory_link(&external, &storage.join(OPTIONAL_PARSER_PACK_ID))?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            storage,
            Some(platform),
        );
        let missing_archive = root.path().join("missing.tar.zst");

        require(
            lifecycle.install(&missing_archive).is_err(),
            "install followed a product-owned storage link",
        )?;
        require(marker.is_file(), "install mutated the link target")
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn cleaned_tombstone_makes_partial_slot_removal_retryable() -> TestResult {
        let root = tempfile::tempdir()?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            root.path().join("storage"),
            Some(PackPlatform::WindowsX86_64),
        );
        let identity = test_slot('c');
        let version_root = lifecycle
            .versions_root()?
            .join(&identity.projectatlas_version);
        fs::create_dir_all(&version_root)?;
        let tombstone = version_root.join(format!(
            "{WINDOWS_CLEANED_TOMBSTONE_PREFIX}{}-retry",
            identity.artifact
        ));
        fs::create_dir(&tombstone)?;
        let partial_file = tombstone.join("partial.bin");
        fs::write(&partial_file, b"partial")?;

        require(
            lifecycle.remove()?.changed,
            "partial cleaned tombstone retry did not remove storage",
        )?;
        require(!tombstone.exists(), "successful retry left its tombstone")?;
        require(
            !lifecycle.remove()?.changed,
            "partial tombstone retry was not idempotent",
        )
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn slot_cleanup_uses_unique_atomic_tombstone_states() -> TestResult {
        let root = tempfile::tempdir()?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            root.path().join("storage"),
            Some(PackPlatform::WindowsX86_64),
        );
        let identity = test_slot('e');
        let slot_root = lifecycle.slot_path(&identity)?;
        fs::create_dir_all(&slot_root)?;
        fs::write(slot_root.join("must-move.bin"), b"slot")?;
        let installed = InstalledSlotPath {
            projectatlas_version: identity.projectatlas_version.clone(),
            artifact: identity.artifact.clone(),
            entry_root: slot_root.clone(),
            pack_root: Some(slot_root.clone()),
            state: InstalledSlotCleanupState::Installed,
        };

        let pending = transition_slot_to_removing_tombstone(&installed)?;
        require(
            !slot_root.exists(),
            "deterministic slot survived transition",
        )?;
        require(
            pending.entry_root.is_dir(),
            "profile-pending tombstone was not published",
        )?;
        require(
            parse_windows_tombstone_name(
                pending
                    .entry_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| io::Error::other("pending tombstone name missing"))?,
            ) == Some((
                InstalledSlotCleanupState::ProfilePending,
                identity.artifact.clone(),
            )),
            "profile-pending tombstone name was not strict",
        )?;
        require(
            windows_slot_cleanup_in_progress(
                pending
                    .entry_root
                    .parent()
                    .ok_or_else(|| io::Error::other("pending tombstone parent missing"))?,
                &identity.artifact,
            )?,
            "pending cleanup did not block artifact reuse",
        )?;

        let cleaned = transition_tombstone_to_profile_cleaned(&pending)?;
        require(
            !pending.entry_root.exists() && cleaned.entry_root.is_dir(),
            "profile-cleaned tombstone transition was not atomic",
        )?;
        require(
            windows_slot_cleanup_in_progress(
                cleaned
                    .entry_root
                    .parent()
                    .ok_or_else(|| io::Error::other("cleaned tombstone parent missing"))?,
                &identity.artifact,
            )?,
            "cleaned tombstone did not block artifact reuse",
        )?;
        require(
            remove_tree_if_present(&cleaned.entry_root)?,
            "cleaned tombstone was not removable",
        )
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn reusable_sibling_marker_never_authorizes_profile_cleanup_skip() -> TestResult {
        let root = tempfile::tempdir()?;
        let lifecycle = OptionalParserPackLifecycle::for_test(
            root.path().join("project"),
            root.path().join("storage"),
            Some(PackPlatform::WindowsX86_64),
        );
        let identity = test_slot('f');
        let slot_root = lifecycle.slot_path(&identity)?;
        fs::create_dir_all(&slot_root)?;
        fs::write(slot_root.join(ARTIFACT_MANIFEST_FILE_NAME), b"invalid")?;
        let marker = slot_root
            .parent()
            .ok_or_else(|| io::Error::other("slot parent missing"))?
            .join(format!(".{}.profile-cleaned", identity.artifact));
        fs::write(&marker, b"projectatlas-parser-profile-cleaned-v1\n")?;

        let error = require_lifecycle_error(
            lifecycle.remove(),
            "reusable sibling marker bypassed profile verification",
        )?;
        require(
            matches!(
                error,
                OptionalParserPackLifecycleError::CleanupIncomplete { .. }
            ),
            "invalid marked slot did not fail closed",
        )?;
        require(
            marker.is_file(),
            "legacy marker was unexpectedly consumed as cleanup authority",
        )?;
        require(
            !slot_root.exists(),
            "invalid slot was not isolated from its deterministic path",
        )?;
        let tombstones = installed_slot_paths(&lifecycle.pack_root()?)?;
        require(
            tombstones.iter().any(|slot| {
                slot.artifact == identity.artifact
                    && slot.state == InstalledSlotCleanupState::ProfilePending
            }),
            "failed cleanup did not retain a unique retry tombstone",
        )
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn cleanup_broker_timeout_terminates_reaps_and_drains() -> TestResult {
        let ping = validated_windows_directory()?
            .join("System32")
            .join("PING.EXE");
        let mut child = Command::new(&ping)
            .arg("-n")
            .arg("30")
            .arg("127.0.0.1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let broker = PathBuf::from("cleanup-timeout-test");
        let error = require_lifecycle_error(
            supervise_cleanup_broker(
                &mut child,
                &broker,
                "test-profile",
                Duration::from_millis(50),
                Duration::from_secs(2),
            ),
            "hung cleanup broker unexpectedly succeeded",
        )?;
        require(
            matches!(error, OptionalParserPackLifecycleError::InvalidData { .. }),
            "timeout operation was misclassified as a cleanup failure",
        )?;
        require(
            child.try_wait()?.is_some(),
            "hung cleanup broker was not reaped",
        )
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn cleanup_broker_post_spawn_pipe_fault_still_reaps_child() -> TestResult {
        let ping = validated_windows_directory()?
            .join("System32")
            .join("PING.EXE");
        let mut child = Command::new(&ping)
            .arg("-n")
            .arg("30")
            .arg("127.0.0.1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let retained_stdout = child.stdout.take();
        let broker = PathBuf::from("cleanup-pipe-fault-test");
        let error = require_lifecycle_error(
            supervise_cleanup_broker(
                &mut child,
                &broker,
                "test-profile",
                Duration::from_secs(1),
                Duration::from_secs(2),
            ),
            "missing cleanup pipe unexpectedly succeeded",
        )?;
        require(
            matches!(error, OptionalParserPackLifecycleError::InvalidData { .. }),
            "pipe-fault operation was misclassified as a cleanup failure",
        )?;
        require(
            child.try_wait()?.is_some(),
            "cleanup broker with a post-spawn pipe fault was not reaped",
        )?;
        drop(retained_stdout);
        Ok(())
    }

    #[test]
    fn status_reports_installed_enabled_rollback_and_stale_states() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        );
        require(
            lifecycle.status()?.state == OptionalParserPackState::Absent,
            "initial state was not absent",
        )?;
        let selected = test_slot('a');
        fs::create_dir_all(lifecycle.slot_path(&selected)?)?;
        require(
            lifecycle.status()?.state == OptionalParserPackState::InstalledDisabled,
            "installed slot was not reported disabled",
        )?;
        lifecycle.write_selection(&ProjectSelection::new(selected.clone(), None))?;
        require(
            lifecycle.status()?.state == OptionalParserPackState::Enabled,
            "enabled state missing",
        )?;
        let rollback = test_slot('b');
        fs::create_dir_all(lifecycle.slot_path(&rollback)?)?;
        lifecycle.write_selection(&ProjectSelection::new(selected.clone(), Some(rollback)))?;
        require(
            lifecycle.status()?.state == OptionalParserPackState::RollbackReady,
            "rollback-ready state missing",
        )?;
        fs::remove_dir_all(lifecycle.slot_path(&selected)?)?;
        require(
            lifecycle.status()?.state == OptionalParserPackState::Stale,
            "missing slot was not stale",
        )
    }

    #[test]
    fn lifecycle_metadata_entry_bound_precedes_remove_mutation() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        );
        lifecycle.write_selection(&ProjectSelection::new(test_slot('a'), None))?;
        let selection_before = fs::read(lifecycle.selection_path())?;
        let versions = lifecycle.pack_root()?.join("versions");
        fs::create_dir_all(&versions)?;
        for index in 0..=LIFECYCLE_METADATA_ENTRY_LIMIT {
            fs::create_dir(versions.join(format!("empty-{index:04}")))?;
        }

        let status = lifecycle.status()?;
        require(
            status.installed_slots == 0
                && status.installed_slots_truncated
                && status.state == OptionalParserPackState::Stale,
            "over-limit empty version metadata was not reported as bounded stale state",
        )?;
        let error = require_lifecycle_error(
            lifecycle.remove(),
            "over-limit lifecycle metadata unexpectedly allowed removal",
        )?;
        require(
            matches!(
                error,
                OptionalParserPackLifecycleError::InvalidData { ref reason }
                    if reason == "parser-pack metadata entries exceed the cleanup bound"
            ),
            "over-limit lifecycle metadata returned the wrong removal failure",
        )?;
        require(
            fs::read(lifecycle.selection_path())? == selection_before
                && fs::read_dir(&versions)?.count()
                    == LIFECYCLE_METADATA_ENTRY_LIMIT.saturating_add(1),
            "bounded removal partially mutated selection or storage",
        )?;

        let child_root = tempfile::tempdir()?;
        let child_lifecycle = OptionalParserPackLifecycle::for_test(
            child_root.path().join("project"),
            child_root.path().join("storage"),
            Some(PackPlatform::LinuxX86_64),
        );
        child_lifecycle.write_selection(&ProjectSelection::new(test_slot('b'), None))?;
        let child_selection_before = fs::read(child_lifecycle.selection_path())?;
        let child_version = child_lifecycle.pack_root()?.join("versions").join("0.4.0");
        fs::create_dir_all(&child_version)?;
        for index in 0..LIFECYCLE_METADATA_ENTRY_LIMIT {
            fs::write(child_version.join(format!("stale-{index:04}")), [])?;
        }
        let child_status = child_lifecycle.status()?;
        require(
            child_status.installed_slots == 0
                && child_status.installed_slots_truncated
                && child_status.state == OptionalParserPackState::Stale,
            "over-limit non-directory slot metadata was not reported as bounded stale state",
        )?;
        let child_error = require_lifecycle_error(
            child_lifecycle.remove(),
            "over-limit child metadata unexpectedly allowed removal",
        )?;
        require(
            matches!(
                child_error,
                OptionalParserPackLifecycleError::InvalidData { ref reason }
                    if reason == "parser-pack metadata entries exceed the cleanup bound"
            ),
            "over-limit child metadata returned the wrong removal failure",
        )?;
        require(
            fs::read(child_lifecycle.selection_path())? == child_selection_before
                && fs::read_dir(&child_version)?.count() == LIFECYCLE_METADATA_ENTRY_LIMIT,
            "child metadata overflow partially mutated selection or storage",
        )?;

        let exact_root = tempfile::tempdir()?;
        let exact_lifecycle = OptionalParserPackLifecycle::for_test(
            exact_root.path().join("project"),
            exact_root.path().join("storage"),
            Some(PackPlatform::LinuxX86_64),
        );
        exact_lifecycle.write_selection(&ProjectSelection::new(test_slot('c'), None))?;
        let exact_pack_root = exact_lifecycle.pack_root()?;
        let exact_version = exact_pack_root.join("versions").join("0.4.0");
        fs::create_dir_all(&exact_version)?;
        for index in 0..LIFECYCLE_METADATA_ENTRY_LIMIT.saturating_sub(1) {
            fs::write(exact_version.join(format!("stale-{index:04}")), [])?;
        }
        require(
            !exact_lifecycle.status()?.installed_slots_truncated,
            "exact lifecycle metadata bound was reported as truncated",
        )?;
        let removed = exact_lifecycle.remove()?;
        require(
            removed.changed
                && removed.state == OptionalParserPackState::Absent
                && !exact_lifecycle.selection_path().exists()
                && !exact_pack_root.exists(),
            "exact lifecycle metadata bound did not permit complete removal",
        )
    }

    /// Return one typed process failure for mutation-order tests.
    fn injected_admission_failure(_path: &Path) -> ParserSupervisorError {
        ParserSupervisorError::Cancelled {
            phase: "test artifact admission",
        }
    }

    #[test]
    fn failed_archive_admission_publishes_no_slot_and_preserves_selection() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        )
        .with_admission_failure(injected_admission_failure);
        let selected = test_slot('a');
        lifecycle.write_selection(&ProjectSelection::new(selected, None))?;
        let selection_before = fs::read(lifecycle.selection_path())?;

        let error = require_lifecycle_error(
            lifecycle.install(&root.path().join("candidate.tar.zst")),
            "failed admission unexpectedly installed an archive",
        )?;
        require(
            matches!(
                error,
                OptionalParserPackLifecycleError::Supervisor(
                    ParserSupervisorError::Cancelled { .. }
                )
            ),
            "injected archive admission failure lost its typed source",
        )?;
        let (installed, truncated, cleanup_pending) =
            count_installed_slots(&lifecycle.pack_root()?)?;
        require(
            installed == 0 && !truncated && !cleanup_pending,
            "failed archive admission published an installed slot",
        )?;
        require(
            fs::read(lifecycle.selection_path())? == selection_before,
            "failed archive admission changed project selection",
        )
    }

    #[test]
    fn failed_enable_admission_preserves_previous_selection_bytes() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        )
        .with_admission_failure(injected_admission_failure);
        let selected = test_slot('a');
        let candidate = test_slot('b');
        lifecycle.write_selection(&ProjectSelection::new(selected, None))?;
        let selection_before = fs::read(lifecycle.selection_path())?;

        let error = require_lifecycle_error(
            lifecycle.enable(&candidate.artifact),
            "failed admission unexpectedly selected a candidate",
        )?;
        require(
            matches!(
                error,
                OptionalParserPackLifecycleError::Supervisor(
                    ParserSupervisorError::Cancelled { .. }
                )
            ),
            "injected enable admission failure lost its typed source",
        )?;
        require(
            fs::read(lifecycle.selection_path())? == selection_before,
            "failed enable admission changed project selection",
        )
    }

    #[test]
    fn failed_update_preserves_previous_selection_bytes() -> TestResult {
        let Some(platform) = host_pack_platform() else {
            return Ok(());
        };
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(project, storage, Some(platform));
        let selected = test_slot('a');
        let slot_root = lifecycle.slot_path(&selected)?;
        fs::create_dir_all(&slot_root)?;
        fs::write(slot_root.join(ARTIFACT_MANIFEST_FILE_NAME), b"invalid")?;
        lifecycle.write_selection(&ProjectSelection::new(selected, None))?;
        let before = fs::read(lifecycle.selection_path())?;
        let archive = root.path().join("invalid.tar.zst");
        fs::write(&archive, b"invalid")?;

        require(
            lifecycle.update(&archive).is_err(),
            "invalid update unexpectedly succeeded",
        )?;
        let after = fs::read(lifecycle.selection_path())?;
        require(before == after, "failed update changed the prior selection")
    }

    #[test]
    fn failed_selection_publication_keeps_candidate_for_deterministic_retry() -> TestResult {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        let storage = root.path().join("storage");
        let lifecycle = OptionalParserPackLifecycle::for_test(
            project.clone(),
            storage.clone(),
            Some(PackPlatform::LinuxX86_64),
        );
        let selected = test_slot('a');
        let candidate = test_slot('b');
        let prior_rollback = test_slot('c');
        for slot in [&selected, &candidate, &prior_rollback] {
            fs::create_dir_all(lifecycle.slot_path(slot)?)?;
        }
        let previous = ProjectSelection::new(selected.clone(), Some(prior_rollback.clone()));
        lifecycle.write_selection(&previous)?;
        let selection_before = fs::read(lifecycle.selection_path())?;

        let failing = OptionalParserPackLifecycle::for_test(
            project.clone(),
            storage.clone(),
            Some(PackPlatform::LinuxX86_64),
        )
        .with_selection_publication_failure();
        let error = require_lifecycle_error(
            failing.publish_installed_update(&previous, &candidate),
            "injected selection publication unexpectedly succeeded",
        )?;
        require(
            matches!(
                error,
                OptionalParserPackLifecycleError::InvalidData { ref reason }
                    if reason == "injected project selection publication failure"
            ),
            "selection publication failure lost its typed source",
        )?;
        require(
            fs::read(failing.selection_path())? == selection_before
                && failing.read_selection()?.as_ref() == Some(&previous),
            "failed selection publication changed selected or rollback state",
        )?;
        require(
            failing.slot_path(&selected)?.is_dir()
                && failing.slot_path(&prior_rollback)?.is_dir()
                && failing.slot_path(&candidate)?.is_dir(),
            "failed selection publication removed an immutable lifecycle slot",
        )?;

        let retry = OptionalParserPackLifecycle::for_test(
            project,
            storage,
            Some(PackPlatform::LinuxX86_64),
        );
        require(
            retry.publish_installed_update(&previous, &candidate)?,
            "retry did not publish the retained candidate",
        )?;
        let selected_candidate = retry
            .read_selection()?
            .ok_or_else(|| invalid_data("retried selection is absent"))?;
        require(
            selected_candidate == ProjectSelection::new(candidate.clone(), Some(selected)),
            "retry did not select the candidate with the immediate prior slot as rollback",
        )?;
        let selection_after_retry = fs::read(retry.selection_path())?;
        require(
            !retry.publish_installed_update(&selected_candidate, &candidate)?
                && fs::read(retry.selection_path())? == selection_after_retry,
            "identical retry rewrote or changed the selected candidate",
        )
    }

    #[test]
    fn archive_extraction_rejects_non_regular_entries() -> TestResult {
        let root = tempfile::tempdir()?;
        let archive_path = root.path().join("invalid.tar.zst");
        let output = File::create(&archive_path)?;
        let encoder = zstd::Encoder::new(output, 1)?;
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_mode(PAYLOAD_MODE);
        header.set_cksum();
        builder.append_link(&mut header, format!("{ARCHIVE_ROOT}/payload"), "target")?;
        let encoder = builder.into_inner()?;
        encoder.finish()?.sync_all()?;

        require(
            extract_archive(&archive_path, None).is_err(),
            "non-regular archive entry was accepted",
        )
    }
}
