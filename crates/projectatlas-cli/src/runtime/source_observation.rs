//! Process-local source observation for long-lived verified index reads.

use super::{
    ScanRuntimePlan, SourceVerificationWork, WatchChangeSet,
    open_atlas_store_read_only_for_project, open_exact_fresh_atlas_store_for_project_controlled,
    open_exact_saved_source_matches_index_controlled,
    policy_watch::{PolicyWatchInputs, PolicyWatchSelection},
    publication_input_error, source_changed_during_derivation, source_inspection_error,
    verify_saved_source_matches_index_controlled,
};
use crate::CliError;
use blake3::Hasher;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use projectatlas_core::graph::ProjectInstanceId;
use projectatlas_core::{CanonicalProjectRoot, IndexGeneration, IndexWorkControl, IndexWorkStage};
use projectatlas_db::{
    AtlasStore, CapturedProjectBinding, DatabaseFileIdentity, IndexPublicationState,
};
use std::collections::{BTreeSet, HashMap};
#[cfg(windows)]
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Take};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum independent project bindings observed by one MCP process.
const SOURCE_OBSERVATION_CAPACITY: usize = 16;
/// Maximum unprocessed native watcher events retained per project.
const SOURCE_OBSERVATION_QUEUE_CAPACITY: usize = 1_024;
/// Fixed attempts used to reconcile an edit that overlaps a query or verification.
const VERIFIED_READ_ATTEMPTS: usize = 3;
/// Maximum bytes read from any one external source-selection policy input.
const MAX_POLICY_INPUT_BYTES: u64 = 16 * 1_024 * 1_024;
/// Fixed SQLite sidecar names belonging to one captured database namespace.
const DATABASE_RUNTIME_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

/// Send test-only native observer evidence through libtest's captured diagnostics.
#[cfg(test)]
#[expect(
    clippy::print_stderr,
    reason = "Native observer test evidence must use libtest capture"
)]
pub(super) fn report_observation_test_diagnostic(message: fmt::Arguments<'_>) {
    eprintln!("{message}");
}

/// Whether exact verification may repair a bounded delta before returning a read.
#[derive(Clone, Copy)]
enum SourceRepairPolicy {
    /// Ordinary navigation may reconcile a safe incremental delta.
    Incremental,
    /// Refresh no-op admission requires an already current publication.
    RequireCurrent,
}

/// Read admission either proves current state or transfers exact discovery to refresh.
pub(crate) enum SourceReadAdmission<T> {
    /// The requested snapshot/result is backed by current source evidence.
    Current(T),
    /// No mutation occurred; the refresh owner can consume this exact assessment.
    Reconcile(Box<super::IndexFreshnessAssessment>),
}

impl<T> SourceReadAdmission<T> {
    /// Preserve typed guidance for callers that do not own a refresh operation.
    pub(crate) fn require_current(self) -> Result<T, CliError> {
        match self {
            Self::Current(value) => Ok(value),
            Self::Reconcile(assessment) => Err(CliError::RefreshRequired(Box::new(
                assessment.delta.map_or_else(
                    || super::index_policy_refresh_required(&assessment.source.root),
                    |delta| delta.report,
                ),
            ))),
        }
    }
}

impl SourceRepairPolicy {
    /// Open exact source evidence with the caller's explicit mutation policy.
    fn open_exact(
        self,
        binding: &SourceBinding,
        before_plan: Option<&ScanRuntimePlan>,
        control: &IndexWorkControl,
    ) -> Result<SourceReadAdmission<super::ExactFreshIndexRead>, CliError> {
        match self {
            Self::Incremental => open_exact_fresh_atlas_store_for_project_controlled(
                &binding.database,
                &binding.root,
                binding.config.as_deref(),
                control,
            )
            .map(SourceReadAdmission::Current),
            Self::RequireCurrent => {
                let store =
                    open_atlas_store_read_only_for_project(&binding.database, &binding.root)?;
                let fresh_plan;
                let plan = if let Some(plan) = before_plan {
                    plan
                } else {
                    fresh_plan = ScanRuntimePlan::for_path_controlled(
                        binding.config.as_deref(),
                        &binding.root,
                        None,
                        control,
                    )
                    .map_err(|source| publication_input_error(&binding.root, source))?;
                    &fresh_plan
                };
                let assessment = super::assess_open_index_against_plan(
                    &store,
                    plan,
                    projectatlas_fs::ScanLimits::default(),
                    control,
                )?;
                if assessment.delta.is_some() {
                    store.finish_index_read_snapshot()?;
                    Ok(SourceReadAdmission::Reconcile(Box::new(assessment)))
                } else {
                    Ok(SourceReadAdmission::Current(super::ExactFreshIndexRead {
                        store,
                        work: assessment.work,
                    }))
                }
            }
        }
    }
}

/// Exact process-local evidence attached to one accepted `SQLite` read snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedReadStamp {
    /// Nonce distinguishing this server process from every restart.
    pub(crate) process_nonce: [u8; 16],
    /// Monotonic verified epoch inside this process and source binding.
    pub(crate) epoch: u64,
    /// Durable complete publication generation read by the `SQLite` snapshot.
    pub(crate) generation: IndexGeneration,
    /// Durable project identity captured by the same root-bound snapshot.
    pub(crate) project_instance_id: ProjectInstanceId,
}

/// Measured freshness work consumed before one result was accepted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct VerifiedReadWork {
    /// Exact full-source verifications performed for this accepted call.
    pub(crate) exact_verifications: u64,
    /// Repository entries inspected by exact verification.
    pub(crate) filesystem_entries: u64,
    /// Repository source bytes hashed by exact verification.
    pub(crate) filesystem_bytes: u64,
    /// `SQLite` read statements owned directly by freshness verification.
    pub(crate) sqlite_read_statements: u64,
    /// Full indexed-node rows decoded by freshness verification.
    pub(crate) decoded_nodes: u64,
    /// Provisional query results discarded after an overlapping invalidation.
    pub(crate) retries: u64,
    /// Wall time for verification, bounded query construction, and acceptance.
    pub(crate) elapsed: Duration,
    /// Rendered result bytes supplied by the adapter after acceptance.
    pub(crate) output_bytes: u64,
}

impl VerifiedReadWork {
    /// Accumulate one exact source verification into this accepted call's work.
    fn add_exact(&mut self, work: SourceVerificationWork) {
        self.exact_verifications = self.exact_verifications.saturating_add(1);
        self.filesystem_entries = self
            .filesystem_entries
            .saturating_add(work.filesystem_entries);
        self.filesystem_bytes = self.filesystem_bytes.saturating_add(work.filesystem_bytes);
        self.sqlite_read_statements = self
            .sqlite_read_statements
            .saturating_add(work.sqlite_read_statements);
        self.decoded_nodes = self.decoded_nodes.saturating_add(work.decoded_nodes);
    }
}

/// Accepted owned result plus its epoch stamp and measured freshness work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedReadOutcome<T> {
    /// Result built entirely inside the accepted `SQLite` snapshot.
    pub(crate) value: T,
    /// Process epoch and durable generation that accepted the result.
    pub(crate) stamp: VerifiedReadStamp,
    /// Work consumed before the result was accepted.
    pub(crate) work: VerifiedReadWork,
}

impl<T> VerifiedReadOutcome<T> {
    /// Record the already-rendered output size without rescanning or reopening `SQLite`.
    pub(crate) fn with_output_bytes(mut self, output_bytes: usize) -> Self {
        self.work.output_bytes = u64::try_from(output_bytes).unwrap_or(u64::MAX);
        self
    }
}

/// Source authority retained through one purpose mutation commit boundary.
enum MutationSourceWitness {
    /// Native observation plus the accepted source epoch.
    Observed {
        /// Exact source binding and observer continuity retained by this admission.
        entry: Arc<SourceObservationEntry>,
        /// Exact durable generation and policy epoch accepted before mutation.
        epoch: VerifiedSourceEpoch,
    },
    /// Exact-per-call compatibility witness when no observer can be admitted.
    Exact {
        /// Canonical source, database, and configuration identity.
        binding: SourceBinding,
        /// Durable generation and project identity accepted before mutation.
        stamp: VerifiedReadStamp,
        /// Scanner contract accepted around exact source verification.
        contract_fingerprint: String,
        /// Dynamic source-selection policy accepted around exact verification.
        policy_witness: String,
        /// Native database object accepted by the actual exact-read connection.
        database_identity: Option<DatabaseFileIdentity>,
    },
}

/// Saved-source witness retained through one purpose mutation commit boundary.
pub(crate) struct VerifiedMutationAdmission {
    /// Observed or exact-per-call source authority retained by this admission.
    witness: MutationSourceWitness,
    /// Cooperative cancellation and work budget shared through commit admission.
    control: IndexWorkControl,
}

impl VerifiedMutationAdmission {
    /// Revalidate exact saved source, policy, observer continuity, and cancellation before commit.
    pub(crate) fn verify(&self, store: &AtlasStore) -> Result<(), CliError> {
        match &self.witness {
            MutationSourceWitness::Observed { entry, epoch } => {
                Self::verify_observed(entry, epoch, store, &self.control, || {
                    verify_saved_source_matches_index_controlled(
                        &entry.binding.database,
                        &entry.binding.root,
                        entry.binding.config.as_deref(),
                        &self.control,
                    )
                })
            }
            MutationSourceWitness::Exact {
                binding,
                stamp,
                contract_fingerprint,
                policy_witness,
                database_identity,
            } => Self::verify_exact(
                binding,
                stamp,
                contract_fingerprint,
                policy_witness,
                database_identity.as_ref(),
                store,
                &self.control,
            ),
        }
    }

    /// Revalidate an observer-backed mutation witness around exact saved source.
    fn verify_observed(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        store: &AtlasStore,
        control: &IndexWorkControl,
        verify_source: impl FnOnce() -> Result<(), CliError>,
    ) -> Result<(), CliError> {
        if let Err(error) = Self::verify_observation(entry, epoch, store, control) {
            #[cfg(test)]
            report_observation_test_diagnostic(format_args!(
                "mutation observer rejected before exact-source comparison: {entry:?}"
            ));
            entry.invalidate_epoch(epoch);
            return Err(error);
        }
        if let Err(error) = verify_source() {
            #[cfg(test)]
            report_observation_test_diagnostic(format_args!(
                "mutation exact-source comparison rejected: error={error}, entry={entry:?}"
            ));
            if matches!(error, CliError::RefreshRequired(_)) {
                entry.invalidate_after_proven_source_mismatch()?;
            } else {
                entry.invalidate_epoch(epoch);
            }
            return Err(error);
        }
        if let Err(error) = Self::verify_observation(entry, epoch, store, control) {
            #[cfg(test)]
            report_observation_test_diagnostic(format_args!(
                "mutation observer rejected after exact-source comparison: {entry:?}"
            ));
            entry.invalidate_epoch(epoch);
            return Err(error);
        }
        Ok(())
    }

    /// Require the retained observer epoch to remain current.
    fn verify_observation(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        store: &AtlasStore,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        match SourceObservationRegistry::accepts_observed_result(entry, epoch, store, control) {
            Ok(ObservedAcceptance::Accepted) => Ok(()),
            Ok(ObservedAcceptance::Superseded | ObservedAcceptance::Invalidated) => {
                Err(source_changed_during_derivation(&entry.binding.root, "."))
            }
            Err(error) => Err(error),
        }
    }

    /// Revalidate exact saved source plus durable identity immediately before commit.
    fn verify_exact(
        binding: &SourceBinding,
        stamp: &VerifiedReadStamp,
        contract_fingerprint: &str,
        policy_witness: &str,
        database_identity: Option<&DatabaseFileIdentity>,
        mutation_store: &AtlasStore,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        control.check(IndexWorkStage::Publication)?;
        if !binding.matches_database_store(database_identity, mutation_store) {
            return Err(source_changed_during_derivation(&binding.root, "."));
        }
        let before = exact_source_policy(binding, control)?;
        if before.0 != contract_fingerprint || before.1 != policy_witness {
            return Err(source_changed_during_derivation(&binding.root, "."));
        }
        let exact = open_exact_saved_source_matches_index_controlled(
            &binding.database,
            &binding.root,
            binding.config.as_deref(),
            control,
        )?;
        let verification = (|| {
            let publication = exact
                .store
                .index_publication()?
                .ok_or_else(|| source_changed_during_derivation(&binding.root, "."))?;
            let captured = exact.store.captured_project_binding()?;
            let after = exact_source_policy(binding, control)?;
            let database_matches = binding.matches_database_store(database_identity, &exact.store)
                && binding.matches_database_store(database_identity, mutation_store);
            Ok::<_, CliError>((publication, captured, after, database_matches))
        })();
        let finished = exact.store.finish_index_read_snapshot();
        let (publication, captured, after, database_matches) = verification?;
        finished?;
        if database_matches
            && before == after
            && after.0 == contract_fingerprint
            && after.1 == policy_witness
            && publication.generation == stamp.generation
            && captured.project_instance_id == stamp.project_instance_id
        {
            Ok(())
        } else {
            Err(source_changed_during_derivation(&binding.root, "."))
        }
    }
}

/// Acceptance of one observed result against the shared source epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservedAcceptance {
    /// The retained witness is still current.
    Accepted,
    /// Another reconciliation replaced the witness without invalid source evidence.
    Superseded,
    /// Source, policy, or observation continuity no longer supports the witness.
    Invalidated,
}

/// Exact root/database/config identity used to isolate one observer.
#[derive(Clone, Debug, Eq)]
pub(super) struct SourceBinding {
    /// Canonical source root watched by this observer.
    root: PathBuf,
    /// Database excluded from source-change invalidation.
    database: PathBuf,
    /// Resolved runtime namespace captured without retargeting the selected SQLite path.
    database_target: Option<PathBuf>,
    /// Optional scanner configuration watched with the source root.
    config: Option<PathBuf>,
}

impl PartialEq for SourceBinding {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.database == other.database
            && self.database_target == other.database_target
            && self.config == other.config
    }
}

impl Hash for SourceBinding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.root.hash(state);
        self.database.hash(state);
        self.database_target.hash(state);
        self.config.hash(state);
    }
}

impl SourceBinding {
    /// Resolve one root, database, and optional configuration into a stable binding.
    pub(super) fn new(
        database: &Path,
        root: &Path,
        config: Option<&Path>,
    ) -> Result<Self, CliError> {
        let root = root.canonicalize().map_err(|source| CliError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let database = absolute_path_from(&root, database);
        let database_target = fs::canonicalize(&database).ok();
        let config = config.map(|path| absolute_path_from(&root, path));
        Ok(Self {
            root,
            database,
            database_target,
            config,
        })
    }

    /// Queue exact runtime spellings for the controlled drain instead of coalescing policy uncertainty.
    /// This is lexical routing, not authority to suppress the event or reuse a retargeted binding.
    fn queues_database_event(&self, event: &Event) -> bool {
        if event.need_rescan() || event.paths.is_empty() || self.database_target.is_none() {
            return false;
        }
        let structural_main = matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
        event.paths.iter().all(|path| {
            let candidate = super::absolute_watch_path(&self.root, path);
            [
                Some(self.database.as_path()),
                self.database_target.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|database| {
                if candidate == database {
                    return !structural_main;
                }
                DATABASE_RUNTIME_SUFFIXES.into_iter().any(|suffix| {
                    let mut sidecar = database.as_os_str().to_os_string();
                    sidecar.push(suffix);
                    candidate == Path::new(&sidecar)
                })
            })
        })
    }

    /// Recognize only captured literal names, without consulting the filesystem in a callback.
    fn has_database_path(&self, event: &Event) -> bool {
        event.paths.iter().any(|path| {
            let candidate = super::absolute_watch_path(&self.root, path);
            [
                Some(self.database.as_path()),
                self.database_target.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|database| {
                candidate == database
                    || DATABASE_RUNTIME_SUFFIXES.into_iter().any(|suffix| {
                        let mut sidecar = database.as_os_str().to_os_string();
                        sidecar.push(suffix);
                        candidate == Path::new(&sidecar)
                    })
            })
        })
    }

    /// Recognize a target-file hint without authorizing its suppression in the callback.
    fn is_database_target_creation(&self, event: &Event) -> bool {
        !event.need_rescan()
            && matches!(
                event.kind,
                EventKind::Create(notify::event::CreateKind::File)
            )
            && !event.paths.is_empty()
            && self.database_target.as_ref().is_some_and(|target| {
                event
                    .paths
                    .iter()
                    .all(|path| super::absolute_watch_path(&self.root, path) == *target)
            })
    }

    /// Verify the selected alias still resolves to the admitted target and file object.
    fn matches_database_store(
        &self,
        identity: Option<&DatabaseFileIdentity>,
        store: &AtlasStore,
    ) -> bool {
        identity.is_none_or(|identity| {
            fs::canonicalize(&self.database).ok() == self.database_target
                && identity.matches_path(&self.database)
                && identity.matches_store(store)
        })
    }
}

/// One bounded runtime wake, independent of policy uncertainty and source continuity.
#[derive(Clone)]
pub(super) struct RuntimeEventIngress {
    /// Captured paths route events but never authorize suppressing a changed native binding.
    binding: SourceBinding,
    /// One representative runtime notification remains queued until the controlled drain.
    pending: Arc<AtomicBool>,
}

impl RuntimeEventIngress {
    /// Capture one runtime namespace and its independent wake state.
    pub(super) fn new(binding: SourceBinding) -> Self {
        Self {
            binding,
            pending: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Preserve captured database events even when they are unrelated to policy filenames.
    fn may_affect(&self, event: &Event) -> bool {
        self.binding.has_database_path(event)
    }

    /// Publish ordinary runtime work without arming a policy recheck or overflowing on self-writes.
    pub(super) fn observe(&self, selection: &PolicyWatchSelection, event: &Event) -> bool {
        if !super::event_kind_affects_index(event.kind) && !event.need_rescan() {
            return false;
        }
        // FSEvents can report creation of an already-opened database. Keep this
        // hint queued, without I/O or coalescing, for the controlled identity check.
        if self.binding.is_database_target_creation(event) {
            return true;
        }
        if self.binding.queues_database_event(event) {
            return !self.pending.swap(true, Ordering::AcqRel);
        }
        let selected = selection.observe(event);
        let main_replaced = !event.need_rescan()
            && matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
            )
            && event.paths.iter().any(|path| {
                let candidate = super::absolute_watch_path(&self.binding.root, path);
                candidate == self.binding.database
                    || self.binding.database_target.as_ref() == Some(&candidate)
            });
        if main_replaced {
            #[cfg(test)]
            report_observation_test_diagnostic(format_args!(
                "source observer database namespace invalidated: event={event:?}"
            ));
            selection.require_recursive_rebind();
        }
        selected || self.may_affect(event)
    }

    /// Clear before consuming the representative event; MCP callers hold the receiver fence.
    pub(super) fn begin_drain(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// One successfully verified source epoch retained only by this process.
#[derive(Clone, Debug, Eq, PartialEq)]
struct VerifiedSourceEpoch {
    /// Accepted process and durable publication identity.
    stamp: VerifiedReadStamp,
    /// Latest watcher ingress sequence covered by this epoch.
    ingress_sequence: u64,
    /// Source publication contract covered by this epoch.
    contract_fingerprint: String,
    /// Exact external source-selection policy covered by this epoch.
    policy_witness: String,
    /// Native object bound to the exact SQLite connection used for this epoch.
    database_identity: Option<DatabaseFileIdentity>,
}

/// Mutable verified-epoch state serialized for one source binding.
#[derive(Debug, Default)]
struct ObservationState {
    /// Monotonic epoch sequence within the owning observer.
    next_epoch: u64,
    /// Last source epoch proven safe for warm reads.
    verified: Option<VerifiedSourceEpoch>,
    /// Preserve the prior object across epoch invalidation and exact admission retries.
    last_database_identity: Option<DatabaseFileIdentity>,
}

/// Native registrations acknowledged by one watcher, including partial failed updates.
struct SourceWatchCoverage {
    /// Existing native watcher; callbacks retain their bounded ingress handshake.
    watcher: RecommendedWatcher,
    /// External directories whose registration has not been successfully retired.
    directories: BTreeSet<PathBuf>,
}

/// Concrete watcher and state for one exact source binding.
struct SourceObservationEntry {
    /// Exact source, database, and configuration identity.
    binding: SourceBinding,
    /// Policy namespace shared with the callback without coupling it to epoch locks.
    policy_selection: PolicyWatchSelection,
    /// Coalesced runtime wake shared with callback publication.
    runtime_ingress: RuntimeEventIngress,
    /// Native watcher kept alive for the entry lifetime.
    watcher: Mutex<SourceWatchCoverage>,
    /// Bounded receiver whose lock serializes callback publication with drain acknowledgment.
    receiver: Arc<Mutex<Receiver<Event>>>,
    /// Monotonic count of relevant events accepted by the callback.
    ingress_sequence: Arc<AtomicU64>,
    /// Whether overflow, disconnection, or rescan invalidated event continuity.
    continuity_lost: Arc<AtomicBool>,
    /// Serializes exact verification and epoch installation for this binding.
    reconcile: Mutex<()>,
    /// Current process-local verified epoch.
    state: Mutex<ObservationState>,
    #[cfg(test)]
    /// Deterministic event ingress used only by owning tests.
    test_sender: SyncSender<Event>,
    #[cfg(test)]
    /// One event injected after an acceptance drain for owning race tests.
    acceptance_event: Mutex<Option<Event>>,
    #[cfg(test)]
    /// Pause immediately before native identity validation while holding the receiver fence.
    database_acceptance_gate: Mutex<Option<IngressFenceGate>>,
    #[cfg(test)]
    /// Continuity loss injected during the next locked event drain for owning race tests.
    drain_continuity_invalidations: AtomicU64,
}

#[cfg(windows)]
impl Drop for SourceObservationEntry {
    fn drop(&mut self) {
        // Cleanup must also cover registrations retained by a partial failed update.
        let coverage = self
            .watcher
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(coverage.watcher.unwatch(&self.binding.root));
        PolicyWatchInputs::unwatch(&mut coverage.watcher, &coverage.directories);
        // Configure acknowledges preceding native cancellation/completion work.
        drop(coverage.watcher.configure(notify::Config::default()));
    }
}

impl fmt::Debug for SourceObservationEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceObservationEntry")
            .field("binding", &self.binding)
            .field(
                "ingress_sequence",
                &self.ingress_sequence.load(Ordering::Acquire),
            )
            .field(
                "continuity_lost",
                &self.continuity_lost.load(Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

impl SourceObservationEntry {
    /// Start an isolated observer for owning tests with the normal work budget.
    #[cfg(test)]
    fn start(binding: SourceBinding) -> Result<Self, CliError> {
        Self::start_controlled(binding, &super::standalone_index_work_control())
    }

    /// Start native observation within the caller's original work budget.
    fn start_controlled(
        binding: SourceBinding,
        control: &IndexWorkControl,
    ) -> Result<Self, CliError> {
        let (sender, receiver) = sync_channel(SOURCE_OBSERVATION_QUEUE_CAPACITY);
        let receiver = Arc::new(Mutex::new(receiver));
        #[cfg(test)]
        let test_sender = sender.clone();
        let ingress_sequence = Arc::new(AtomicU64::new(0));
        let continuity_lost = Arc::new(AtomicBool::new(false));
        let policy_selection = PolicyWatchSelection::new(binding.root.clone());
        let runtime_ingress = RuntimeEventIngress::new(binding.clone());
        let watcher = notify::recommended_watcher(selected_watcher_callback(
            &runtime_ingress,
            policy_selection.clone(),
            sender,
            Arc::clone(&receiver),
            Arc::clone(&ingress_sequence),
            Arc::clone(&continuity_lost),
            #[cfg(test)]
            None,
        ))
        .map_err(|source| observer_error(&binding.root, &source))?;
        let mut entry = Self {
            binding,
            policy_selection,
            runtime_ingress,
            watcher: Mutex::new(SourceWatchCoverage {
                watcher,
                directories: BTreeSet::new(),
            }),
            receiver,
            ingress_sequence,
            continuity_lost,
            reconcile: Mutex::new(()),
            state: Mutex::new(ObservationState::default()),
            #[cfg(test)]
            test_sender,
            #[cfg(test)]
            acceptance_event: Mutex::new(None),
            #[cfg(test)]
            database_acceptance_gate: Mutex::new(None),
            #[cfg(test)]
            drain_continuity_invalidations: AtomicU64::new(0),
        };
        entry
            .watcher
            .get_mut()
            .map_err(|_poisoned| lock_error(&entry.binding.root, "source observation watcher"))?
            .watcher
            .watch(&entry.binding.root, RecursiveMode::Recursive)
            .map_err(|source| observer_error(&entry.binding.root, &source))?;
        entry.refresh_policy_watches(control)?;
        Ok(entry)
    }

    /// Update native coverage before sampling source, without holding the receiver lock.
    fn refresh_policy_watches(&self, control: &IndexWorkControl) -> Result<(), CliError> {
        // No old epoch can outlive this namespace transition. Publish before a
        // newly watched input can emit an event; never hold the selector lock
        // while native watch acknowledgement might need callback progress.
        self.invalidate();
        let mut coverage = self
            .watcher
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation watcher"))?;
        let SourceWatchCoverage {
            watcher,
            directories,
        } = &mut *coverage;
        let update = self
            .policy_selection
            .rebind_recursive_if_required(watcher, control)
            .and_then(|()| {
                PolicyWatchInputs::install_selected(
                    &self.binding.root,
                    self.binding.config.as_deref(),
                    &self.policy_selection,
                    watcher,
                    directories,
                    control,
                )
            });
        if let Err(error) = update {
            self.continuity_lost.store(true, Ordering::Release);
            self.invalidate();
            return Err(error);
        }
        Ok(())
    }

    /// Publish one deterministic test event through the production handshake.
    #[cfg(test)]
    fn publish_test_event(&self, event: Event) -> Result<(), CliError> {
        let _receiver = self
            .receiver
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation receiver"))?;
        self.test_sender.try_send(event).map_err(|source| {
            CliError::InvalidInput(format!(
                "deterministic source observer event injection failed: {source}"
            ))
        })?;
        self.ingress_sequence.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Discard the current verified epoch after any uncertainty.
    fn invalidate(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.verified = None;
        }
    }

    /// Capture request authority without cloning policy/contract strings.
    fn current_stamp(&self) -> Result<Option<VerifiedReadStamp>, CliError> {
        self.state
            .lock()
            .map(|state| state.verified.as_ref().map(|epoch| epoch.stamp.clone()))
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation state"))
    }

    /// Discard only the failed operation's epoch without clearing a successor.
    fn invalidate_epoch(&self, epoch: &VerifiedSourceEpoch) {
        self.invalidate_stamp(&epoch.stamp);
    }

    /// Keep a successor installed after the failed request sampled its authority.
    fn invalidate_stamp(&self, stamp: &VerifiedReadStamp) {
        if let Ok(mut state) = self.state.lock()
            && state
                .verified
                .as_ref()
                .is_some_and(|current| current.stamp == *stamp)
        {
            state.verified = None;
        }
    }

    /// Reject all cached and in-flight evidence after exact source verification fails.
    fn invalidate_after_proven_source_mismatch(&self) -> Result<(), CliError> {
        let _receiver = self
            .receiver
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation receiver"))?;
        self.continuity_lost.store(true, Ordering::Release);
        self.invalidate();
        Ok(())
    }

    /// Reset continuity and drain events before sampling exact source truth.
    fn clear_before_exact_verification(&self, control: &IndexWorkControl) -> Result<(), CliError> {
        self.clear_before_exact_verification_with_update(control, || {
            self.refresh_policy_watches(control)
        })
    }

    /// Fence preexisting ingress before a watch update; retain all later uncertainty.
    fn clear_before_exact_verification_with_update(
        &self,
        control: &IndexWorkControl,
        update: impl FnOnce() -> Result<(), CliError>,
    ) -> Result<(), CliError> {
        {
            let receiver = self.receiver.lock().map_err(|_poisoned| {
                lock_error(&self.binding.root, "source observation receiver")
            })?;
            self.invalidate();
            self.continuity_lost.store(false, Ordering::Release);
            self.runtime_ingress.begin_drain();
            loop {
                control.check(IndexWorkStage::RepositoryTraversal)?;
                match receiver.try_recv() {
                    Ok(_event) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.continuity_lost.store(true, Ordering::Release);
                        break;
                    }
                }
            }
        }
        // Leave events/overflow received during coverage discovery and native
        // installation pending. The final verification drain must retry them.
        update()
    }

    /// Return whether any potentially relevant event arrived since exact truth was sampled.
    fn changed_since_exact_verification(
        &self,
        scan_options: &projectatlas_fs::ScanOptions,
        control: &IndexWorkControl,
    ) -> Result<bool, CliError> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation receiver"))?;
        self.drain_source_events(&receiver, scan_options, None, control)
    }

    /// Drain and invalidate atomically with callback publication and epoch installation.
    fn drain_source_events(
        &self,
        receiver: &Receiver<Event>,
        scan_options: &projectatlas_fs::ScanOptions,
        candidate_identity: Option<&DatabaseFileIdentity>,
        control: &IndexWorkControl,
    ) -> Result<bool, CliError> {
        self.runtime_ingress.begin_drain();
        #[cfg(test)]
        if self
            .drain_continuity_invalidations
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            self.continuity_lost.store(true, Ordering::Release);
        }
        if self.continuity_lost.load(Ordering::Acquire)
            || self.policy_selection.recursive_rebind_pending()
        {
            #[cfg(test)]
            report_observation_test_diagnostic(format_args!(
                "source observer drain refused: root={}, continuity_lost={}, recursive_rebind={}",
                self.binding.root.display(),
                self.continuity_lost.load(Ordering::Acquire),
                self.policy_selection.recursive_rebind_pending()
            ));
            self.invalidate();
            return Ok(true);
        }
        let mut changes = WatchChangeSet::default();
        let mut batch_policy = None;
        let previous_identity = self
            .state
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation state"))?
            .last_database_identity
            .clone();
        let database_identity = previous_identity.as_ref().or(candidate_identity);
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    self.preserve_consumed_event(
                        control
                            .check(IndexWorkStage::RepositoryTraversal)
                            .map_err(CliError::from),
                    )?;
                    if self.binding.is_database_target_creation(&event)
                        && database_identity.is_some_and(|identity| {
                            fs::canonicalize(&self.binding.database).ok()
                                == self.binding.database_target
                                && identity.matches_path(&self.binding.database)
                        })
                    {
                        continue;
                    }
                    let runtime_candidate = self.binding.has_database_path(&event);
                    let Some(filtered) = filtered_observer_event(&self.binding, &event) else {
                        continue;
                    };
                    // Do not reconstruct policy for database/access events or an
                    // empty queue. Every relevant event in this drain reuses it.
                    if batch_policy.is_none() {
                        let discovered = projectatlas_fs::RootScanPolicy::discover(
                            &self.binding.root,
                            scan_options,
                            control,
                        )
                        .map_err(CliError::from);
                        let discovered = self.preserve_consumed_event(discovered)?;
                        let inputs =
                            self.preserve_consumed_event(self.policy_selection.snapshot())?;
                        batch_policy = Some((discovered, inputs));
                    }
                    if let Some((policy, inputs)) = &batch_policy {
                        let classified = super::notify_event_changes_with_policy(
                            &self.binding.root,
                            scan_options,
                            &filtered,
                            policy,
                        );
                        let mut event_changes = self.preserve_consumed_event(classified)?;
                        event_changes.requires_full_scan |= runtime_candidate
                            || inputs.affects(&filtered)
                            || self.preserve_consumed_event(
                                inputs.ancestor_bindings_changed(&filtered, control),
                            )?;
                        #[cfg(test)]
                        if event_changes.has_changes() {
                            report_observation_test_diagnostic(format_args!(
                                "source observer event invalidated epoch: root={:?}, database={:?}, event={filtered:?}, full={}, source={:?}, documents={:?}",
                                self.binding.root,
                                self.binding.database,
                                event_changes.requires_full_scan,
                                event_changes.paths,
                                event_changes.document_paths
                            ));
                        }
                        changes.merge(event_changes);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.continuity_lost.store(true, Ordering::Release);
                    self.invalidate();
                    return Ok(true);
                }
            }
        }
        if self.policy_selection.take_policy_recheck() {
            let structural = self.policy_selection.take_structural_policy_recheck();
            let inspected = (|| -> Result<bool, CliError> {
                if structural {
                    // Canonical names and bytes can stay equal after replacing
                    // the directory object to which a native watch was bound.
                    return Ok(true);
                }
                let Some(epoch) = self.current_epoch()? else {
                    // A transient policy edit may have affected an in-flight
                    // exact candidate even if its bytes have already reverted.
                    return Ok(true);
                };
                let inputs = self.policy_selection.snapshot()?;
                let current = exact_source_policy(&self.binding, control)?;
                Ok(inputs.bindings_changed(control)?
                    || current.0 != epoch.contract_fingerprint
                    || current.1 != epoch.policy_witness)
            })();
            if inspected.is_err() {
                self.policy_selection.retain_policy_recheck();
            }
            changes.requires_full_scan |= self.preserve_consumed_event(inspected)?;
        }
        let changed = changes.has_changes();
        if changed {
            if changes.requires_full_scan {
                self.policy_selection.require_recursive_rebind();
            }
            // Preserve consumed changes for an exact verification already in flight.
            self.continuity_lost.store(true, Ordering::Release);
            self.invalidate();
        } else {
            let acknowledged = self.ingress_sequence.load(Ordering::Acquire);
            let mut state = self
                .state
                .lock()
                .map_err(|_poisoned| lock_error(&self.binding.root, "source observation state"))?;
            if let Some(epoch) = state.verified.as_mut() {
                epoch.ingress_sequence = acknowledged;
            }
        }
        Ok(changed)
    }

    /// Never acknowledge a consumed event after failed policy inspection.
    fn preserve_consumed_event<T>(&self, result: Result<T, CliError>) -> Result<T, CliError> {
        if result.is_err() {
            self.continuity_lost.store(true, Ordering::Release);
            self.invalidate();
        }
        result
    }

    /// Consume events only while the sampled epoch still owns their selection policy.
    fn drain_epoch_events(
        &self,
        receiver: &Receiver<Event>,
        epoch: &VerifiedSourceEpoch,
        scan_options: &projectatlas_fs::ScanOptions,
        control: &IndexWorkControl,
    ) -> Result<ObservedAcceptance, CliError> {
        let Some(current) = self.current_epoch()? else {
            return Ok(ObservedAcceptance::Invalidated);
        };
        if current.stamp != epoch.stamp {
            return Ok(ObservedAcceptance::Superseded);
        }
        if self.drain_source_events(
            receiver,
            scan_options,
            epoch.database_identity.as_ref(),
            control,
        )? {
            return Ok(ObservedAcceptance::Invalidated);
        }
        Ok(ObservedAcceptance::Accepted)
    }

    /// Install a new verified epoch after exact source and policy reconciliation.
    fn install_epoch(
        &self,
        process_nonce: [u8; 16],
        binding: &CapturedProjectBinding,
        generation: IndexGeneration,
        ingress_sequence: u64,
        source_policy: (String, String),
        database_identity: Option<DatabaseFileIdentity>,
    ) -> Result<VerifiedSourceEpoch, CliError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation state"))?;
        state.next_epoch = state.next_epoch.saturating_add(1);
        let epoch = VerifiedSourceEpoch {
            stamp: VerifiedReadStamp {
                process_nonce,
                epoch: state.next_epoch,
                generation,
                project_instance_id: binding.project_instance_id,
            },
            ingress_sequence,
            contract_fingerprint: source_policy.0,
            policy_witness: source_policy.1,
            database_identity,
        };
        state
            .last_database_identity
            .clone_from(&epoch.database_identity);
        state.verified = Some(epoch.clone());
        Ok(epoch)
    }

    /// Clone the current verified epoch without holding the state lock.
    fn current_epoch(&self) -> Result<Option<VerifiedSourceEpoch>, CliError> {
        self.state
            .lock()
            .map(|state| state.verified.clone())
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation state"))
    }
}

/// Remove access and self-database notifications before any policy I/O.
fn filtered_observer_event(binding: &SourceBinding, event: &Event) -> Option<Event> {
    if !super::event_kind_affects_index(event.kind) && !event.need_rescan() {
        return None;
    }
    let mut filtered = event.clone();
    let metadata_directory = binding.root.join(".projectatlas");
    filtered.paths.retain(|path| {
        let candidate = super::absolute_watch_path(&binding.root, path);
        !same_native_namespace_entry(&candidate, &metadata_directory)
            && !is_database_runtime_path(&candidate, binding, event.kind)
    });
    if filtered.paths.is_empty() && !event.need_rescan() {
        return None;
    }
    Some(filtered)
}

/// Test one standalone notification through the production filtering/classification boundary.
#[cfg(test)]
fn observer_event_changes(
    binding: &SourceBinding,
    scan_options: &projectatlas_fs::ScanOptions,
    event: &Event,
) -> WatchChangeSet {
    let Some(filtered) = filtered_observer_event(binding, event) else {
        return WatchChangeSet::default();
    };
    super::notify_event_changes(&binding.root, scan_options, &filtered)
}

/// Return whether a path is the database or one of its runtime sidecar files.
fn is_database_runtime_path(candidate: &Path, binding: &SourceBinding, kind: EventKind) -> bool {
    let namespaces = [
        Some(binding.database.as_path()),
        binding
            .database_target
            .as_deref()
            .filter(|target| *target != binding.database.as_path()),
    ];
    for database in namespaces.into_iter().flatten() {
        let main = same_native_namespace_entry(candidate, database);
        let runtime = main
            || DATABASE_RUNTIME_SUFFIXES.into_iter().any(|suffix| {
                let mut sidecar = database.as_os_str().to_os_string();
                sidecar.push(suffix);
                let sidecar = Path::new(&sidecar);
                same_native_namespace_entry(candidate, sidecar) || {
                    #[cfg(windows)]
                    {
                        missing_windows_sidecar_matches(candidate, sidecar, database, suffix)
                    }
                    #[cfg(not(windows))]
                    {
                        false
                    }
                }
            });
        if !runtime {
            continue;
        }
        // A namespace captured before an alias retarget must not acknowledge
        // notifications using the old target's authority.
        if binding.database_target.is_none()
            || fs::canonicalize(&binding.database).ok() != binding.database_target
        {
            return false;
        }
        return !main
            || !matches!(
                kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
            );
    }
    false
}

/// Compare a native namespace entry without equating a distinct link with its target.
pub(super) fn same_native_namespace_entry(candidate: &Path, known: &Path) -> bool {
    let (Some(parent), Some(known_parent), Some(leaf), Some(known_leaf)) = (
        candidate.parent(),
        known.parent(),
        candidate.file_name(),
        known.file_name(),
    ) else {
        return false;
    };
    if !same_native_path(parent, known_parent) {
        return false;
    }
    if leaf == known_leaf {
        return true;
    }
    #[cfg(windows)]
    {
        use crate::project_state::windows_filesystem as native_names;
        match (
            native_names::entry_name(candidate),
            native_names::entry_name(known),
        ) {
            (Ok(Some(candidate)), Ok(Some(known))) => candidate == known,
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        fs::symlink_metadata(candidate).is_ok_and(|metadata| !metadata.file_type().is_symlink())
            && fs::symlink_metadata(known).is_ok_and(|metadata| !metadata.file_type().is_symlink())
            && same_native_path(candidate, known)
    }
}

/// Match a removed sidecar through its existing database leaf on ordinary
/// case-insensitive Windows directories. The base-file proof keeps a
/// case-sensitive directory's distinct spelling fail-closed.
#[cfg(windows)]
fn missing_windows_sidecar_matches(
    candidate: &Path,
    sidecar: &Path,
    database: &Path,
    suffix: &str,
) -> bool {
    if candidate.exists() || sidecar.exists() {
        return false;
    }
    let (Some(parent), Some(leaf)) = (candidate.parent(), candidate.file_name()) else {
        return false;
    };
    let suffix_units: Vec<u16> = OsStr::new(suffix).encode_wide().collect();
    let leaf_units: Vec<u16> = leaf.encode_wide().collect();
    if leaf_units.len() <= suffix_units.len()
        || !leaf_units[leaf_units.len() - suffix_units.len()..]
            .iter()
            .zip(&suffix_units)
            .all(|(actual, expected)| {
                let actual = *actual;
                let expected = *expected;
                let fold_ascii = |unit: u16| {
                    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
                        unit + (u16::from(b'a') - u16::from(b'A'))
                    } else {
                        unit
                    }
                };
                actual == expected || fold_ascii(actual) == fold_ascii(expected)
            })
    {
        return false;
    }
    let base = parent.join(OsString::from_wide(
        &leaf_units[..leaf_units.len() - suffix_units.len()],
    ));
    // An unresolved candidate whose base spelling is already exact has no
    // native evidence that the missing suffix belongs to a case-insensitive
    // namespace; leave that event observable instead of guessing.
    if base == database || !same_native_namespace_entry(&base, database) {
        return false;
    }
    let exact_suffix = leaf_units[leaf_units.len() - suffix_units.len()..] == suffix_units;
    exact_suffix
        || crate::project_state::windows_filesystem::case_sensitive(parent)
            .is_ok_and(|sensitive| !sensitive)
}

/// Compare native paths without converting identity through display text.
pub(super) fn same_native_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let canonical = |path: &Path| -> Option<PathBuf> {
        if let Ok(path) = fs::canonicalize(path) {
            return Some(path);
        }
        let parent = path.parent()?;
        let identity = CanonicalProjectRoot::from_path(parent).ok()?;
        Some(identity.as_path().join(path.file_name()?))
    };
    match (canonical(left), canonical(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Bounded process-local registry shared by every clone of one MCP server.
pub(crate) struct SourceObservationRegistry {
    /// Random identity distinguishing epochs created by this process.
    process_nonce: [u8; 16],
    /// Bounded observers keyed by exact source binding.
    entries: Mutex<HashMap<SourceBinding, Arc<SourceObservationEntry>>>,
    /// Acceptance invalidations forced by owning mutation-admission tests.
    #[cfg(test)]
    mutation_acceptance_invalidations: AtomicU64,
    /// Preparation invalidations forced by owning mutation-admission tests.
    #[cfg(test)]
    preparation_invalidations: AtomicU64,
}

impl fmt::Debug for SourceObservationRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries = self.entries.lock().map_or(0, |entries| entries.len());
        formatter
            .debug_struct("SourceObservationRegistry")
            .field("entries", &entries)
            .field("capacity", &SOURCE_OBSERVATION_CAPACITY)
            .finish_non_exhaustive()
    }
}

impl Default for SourceObservationRegistry {
    fn default() -> Self {
        Self {
            process_nonce: process_nonce(),
            entries: Mutex::new(HashMap::new()),
            #[cfg(test)]
            mutation_acceptance_invalidations: AtomicU64::new(0),
            #[cfg(test)]
            preparation_invalidations: AtomicU64::new(0),
        }
    }
}

impl SourceObservationRegistry {
    /// Run one complete query inside a source-epoch and `SQLite`-generation boundary.
    pub(crate) fn with_verified_read<T, F>(
        &self,
        database: &Path,
        root: &Path,
        config: Option<&Path>,
        control: &IndexWorkControl,
        query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        self.with_read_policy(
            database,
            root,
            config,
            control,
            SourceRepairPolicy::Incremental,
            query,
        )
        .and_then(SourceReadAdmission::require_current)
    }

    /// Verify current source without repairing it, for truthful refresh no-op admission.
    pub(crate) fn with_verified_current_read<T, F>(
        &self,
        database: &Path,
        root: &Path,
        config: Option<&Path>,
        control: &IndexWorkControl,
        query: F,
    ) -> Result<SourceReadAdmission<VerifiedReadOutcome<T>>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        self.with_read_policy(
            database,
            root,
            config,
            control,
            SourceRepairPolicy::RequireCurrent,
            query,
        )
    }

    /// Share epoch, snapshot and acceptance checks across read mutation policies.
    fn with_read_policy<T, F>(
        &self,
        database: &Path,
        root: &Path,
        config: Option<&Path>,
        control: &IndexWorkControl,
        repair: SourceRepairPolicy,
        mut query: F,
    ) -> Result<SourceReadAdmission<VerifiedReadOutcome<T>>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        let binding = SourceBinding::new(database, root, config)?;
        let Some(entry) = self.entry(binding.clone(), control)? else {
            return self.with_exact_fallback(&binding, control, repair, query);
        };
        match self.with_observed_read(&entry, control, repair, &mut query) {
            Err(CliError::Watcher(_unavailable)) => {
                self.with_exact_fallback(&binding, control, repair, query)
            }
            result => result,
        }
    }

    /// Admit a purpose mutation and retain its source witness through commit.
    pub(crate) fn admit_mutation(
        &self,
        database: &Path,
        root: &Path,
        config: Option<&Path>,
        control: &IndexWorkControl,
    ) -> Result<VerifiedMutationAdmission, CliError> {
        let binding = SourceBinding::new(database, root, config)?;
        let Some(entry) = self.entry(binding.clone(), control)? else {
            return self.admit_exact_mutation(binding, control);
        };
        let mut work = VerifiedReadWork::default();
        for attempt in 0..VERIFIED_READ_ATTEMPTS {
            let previous = entry.current_stamp()?;
            if let Err(error) = control.check(IndexWorkStage::Publication) {
                if let Some(stamp) = previous {
                    entry.invalidate_stamp(&stamp);
                }
                return Err(error.into());
            }
            let (store, epoch) =
                match self.prepare_observed_store(&entry, control, &mut work, attempt == 0) {
                    Ok(Some(prepared)) => prepared,
                    Ok(None) => {
                        return self.admit_exact_mutation(binding, control);
                    }
                    Err(CliError::Watcher(_unavailable)) => {
                        return self.admit_exact_mutation(binding, control);
                    }
                    Err(error) => return Err(error),
                };
            #[cfg(test)]
            if self
                .mutation_acceptance_invalidations
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                entry.continuity_lost.store(true, Ordering::Release);
            }
            match Self::accepts_observed_result(&entry, &epoch, &store, control) {
                Ok(ObservedAcceptance::Accepted) => {
                    if let Err(error) = store.finish_index_read_snapshot() {
                        entry.invalidate_epoch(&epoch);
                        return Err(error.into());
                    }
                    return Ok(VerifiedMutationAdmission {
                        witness: MutationSourceWitness::Observed { entry, epoch },
                        control: control.clone(),
                    });
                }
                Ok(ObservedAcceptance::Superseded | ObservedAcceptance::Invalidated) => {}
                Err(error) => {
                    entry.invalidate_epoch(&epoch);
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            }
            drop(store.finish_index_read_snapshot());
        }
        self.admit_exact_mutation(binding, control)
    }

    /// Admit a mutation through exact source when no native observer is available.
    fn admit_exact_mutation(
        &self,
        binding: SourceBinding,
        control: &IndexWorkControl,
    ) -> Result<VerifiedMutationAdmission, CliError> {
        for _attempt in 0..VERIFIED_READ_ATTEMPTS {
            control.check(IndexWorkStage::Publication)?;
            let before = exact_source_policy(&binding, control)?;
            let exact = open_exact_fresh_atlas_store_for_project_controlled(
                &binding.database,
                &binding.root,
                binding.config.as_deref(),
                control,
            )?;
            let after = match exact_source_policy(&binding, control) {
                Ok(after) => after,
                Err(error) => {
                    drop(exact.store.finish_index_read_snapshot());
                    return Err(error);
                }
            };
            if before != after {
                drop(exact.store.finish_index_read_snapshot());
                continue;
            }
            let stamp = match self.exact_stamp(&exact.store, &binding.root) {
                Ok(stamp) => stamp,
                Err(error) => {
                    drop(exact.store.finish_index_read_snapshot());
                    return Err(error);
                }
            };
            let database_identity = exact.store.database_file_identity();
            exact.store.finish_index_read_snapshot()?;
            return Ok(VerifiedMutationAdmission {
                witness: MutationSourceWitness::Exact {
                    binding,
                    stamp,
                    contract_fingerprint: after.0,
                    policy_witness: after.1,
                    database_identity,
                },
                control: control.clone(),
            });
        }
        Err(source_changed_during_derivation(&binding.root, "."))
    }

    /// Inject one deterministic observer event for owning integration tests.
    #[cfg(test)]
    pub(crate) fn inject_test_event(
        &self,
        database: &Path,
        root: &Path,
        config: Option<&Path>,
        event: Event,
    ) -> Result<(), CliError> {
        let binding = SourceBinding::new(database, root, config)?;
        let entry = self
            .entries
            .lock()
            .map_err(|_poisoned| lock_error(&binding.root, "source observation registry"))?
            .get(&binding)
            .cloned()
            .ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "source observer test entry is missing for '{}'",
                    binding.root.display()
                ))
            })?;
        entry.publish_test_event(event)
    }

    /// Reuse or admit the bounded observer for an exact source binding.
    fn entry(
        &self,
        binding: SourceBinding,
        control: &IndexWorkControl,
    ) -> Result<Option<Arc<SourceObservationEntry>>, CliError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_poisoned| lock_error(&binding.root, "source observation registry"))?;
        if let Some(entry) = entries.get(&binding) {
            return Ok(Some(Arc::clone(entry)));
        }
        if entries.len() >= SOURCE_OBSERVATION_CAPACITY {
            return Ok(None);
        }
        control.check(IndexWorkStage::RepositoryTraversal)?;
        let entry = match SourceObservationEntry::start_controlled(binding.clone(), control) {
            Ok(entry) => Arc::new(entry),
            Err(_observer_unavailable) => return Ok(None),
        };
        entries.insert(binding, Arc::clone(&entry));
        Ok(Some(entry))
    }

    /// Retry a query until one observed epoch remains valid through acceptance.
    fn with_observed_read<T, F>(
        &self,
        entry: &SourceObservationEntry,
        control: &IndexWorkControl,
        repair: SourceRepairPolicy,
        mut query: F,
    ) -> Result<SourceReadAdmission<VerifiedReadOutcome<T>>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        let started = Instant::now();
        let mut work = VerifiedReadWork::default();
        for attempt in 0..VERIFIED_READ_ATTEMPTS {
            let previous = entry.current_stamp()?;
            if let Err(error) = control.check(IndexWorkStage::Publication) {
                if let Some(stamp) = previous {
                    entry.invalidate_stamp(&stamp);
                }
                return Err(error.into());
            }
            let (store, epoch) = match self
                .prepare_observed_store_with_policy(entry, control, &mut work, false, repair)
            {
                Ok(SourceReadAdmission::Current(Some(prepared))) => prepared,
                Ok(SourceReadAdmission::Reconcile(assessment)) => {
                    return Ok(SourceReadAdmission::Reconcile(assessment));
                }
                Ok(SourceReadAdmission::Current(None)) => {
                    return Err(source_changed_during_derivation(&entry.binding.root, "."));
                }
                Err(error) => return Err(error),
            };
            let value = match query(&store, epoch.stamp.clone()) {
                Ok(value) => value,
                Err(error) => {
                    if matches!(error, CliError::IndexWork(_) | CliError::Watcher(_)) {
                        entry.invalidate_epoch(&epoch);
                    }
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            };
            match Self::accepts_observed_result(entry, &epoch, &store, control) {
                Ok(ObservedAcceptance::Accepted) => {
                    store.finish_index_read_snapshot()?;
                    work.elapsed = started.elapsed();
                    return Ok(SourceReadAdmission::Current(VerifiedReadOutcome {
                        value,
                        stamp: epoch.stamp,
                        work,
                    }));
                }
                Ok(ObservedAcceptance::Superseded | ObservedAcceptance::Invalidated) => {}
                Err(error) => {
                    entry.invalidate_epoch(&epoch);
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            }
            drop(store.finish_index_read_snapshot());
            if attempt + 1 < VERIFIED_READ_ATTEMPTS {
                work.retries = work.retries.saturating_add(1);
            }
        }
        Err(source_changed_during_derivation(&entry.binding.root, "."))
    }

    /// Open a snapshot backed by a current epoch or establish a new exact epoch.
    fn prepare_observed_store(
        &self,
        entry: &SourceObservationEntry,
        control: &IndexWorkControl,
        work: &mut VerifiedReadWork,
        force_exact: bool,
    ) -> Result<Option<(AtlasStore, VerifiedSourceEpoch)>, CliError> {
        self.prepare_observed_store_with_policy(
            entry,
            control,
            work,
            force_exact,
            SourceRepairPolicy::Incremental,
        )
        .and_then(SourceReadAdmission::require_current)
    }

    /// Establish or reuse an observed epoch with an explicit source-repair policy.
    fn prepare_observed_store_with_policy(
        &self,
        entry: &SourceObservationEntry,
        control: &IndexWorkControl,
        work: &mut VerifiedReadWork,
        force_exact: bool,
        repair: SourceRepairPolicy,
    ) -> Result<SourceReadAdmission<Option<(AtlasStore, VerifiedSourceEpoch)>>, CliError> {
        let _reconcile = entry
            .reconcile
            .lock()
            .map_err(|_poisoned| lock_error(&entry.binding.root, "source reconciliation"))?;
        let prepared = (|| {
            // One-shot refresh must include writes completed before the request. An empty
            // callback queue cannot fence events still pending in an asynchronous backend.
            // Until a causal backend fence is available, require exact refresh admission.
            // Mutation authority also starts from exact saved source under this lock.
            if force_exact || matches!(repair, SourceRepairPolicy::RequireCurrent) {
                entry.invalidate();
            } else if entry.current_epoch()?.is_some() {
                let plan = ScanRuntimePlan::for_path_controlled(
                    entry.binding.config.as_deref(),
                    &entry.binding.root,
                    None,
                    control,
                )
                .map_err(|source| publication_input_error(&entry.binding.root, source))?;
                entry.changed_since_exact_verification(&plan.scan_options, control)?;
                let contract_fingerprint = plan.publication_contract_fingerprint();
                let policy_witness = source_policy_witness(&plan, control)?;
                if let Some(epoch) = entry.current_epoch()?
                    && epoch.contract_fingerprint == contract_fingerprint
                    && epoch.policy_witness == policy_witness
                    && !entry.continuity_lost.load(Ordering::Acquire)
                {
                    let store = open_atlas_store_read_only_for_project(
                        &entry.binding.database,
                        &entry.binding.root,
                    )?;
                    let publication = store.index_publication()?.filter(|publication| {
                        publication.state == IndexPublicationState::Complete
                            && publication.contract_fingerprint.as_deref()
                                == Some(contract_fingerprint.as_str())
                    });
                    let captured = store.captured_project_binding()?;
                    work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
                    if publication
                        .is_some_and(|publication| publication.generation == epoch.stamp.generation)
                        && captured.project_instance_id == epoch.stamp.project_instance_id
                        && entry
                            .binding
                            .matches_database_store(epoch.database_identity.as_ref(), &store)
                    {
                        return Ok(SourceReadAdmission::Current(Some((store, epoch))));
                    }
                    entry.invalidate();
                    drop(store.finish_index_read_snapshot());
                }
            }

            for _attempt in 0..VERIFIED_READ_ATTEMPTS {
                entry.clear_before_exact_verification(control)?;
                let before_plan = ScanRuntimePlan::for_path_controlled(
                    entry.binding.config.as_deref(),
                    &entry.binding.root,
                    None,
                    control,
                )
                .map_err(|source| publication_input_error(&entry.binding.root, source))?;
                let before_contract = before_plan.publication_contract_fingerprint();
                let before_policy = source_policy_witness(&before_plan, control)?;
                let admission = repair.open_exact(&entry.binding, Some(&before_plan), control)?;
                if let SourceReadAdmission::Current(exact) = &admission {
                    work.add_exact(exact.work);
                }
                let Some(after_plan) = source_policy_plan_matching(
                    &entry.binding,
                    &before_contract,
                    &before_policy,
                    control,
                )?
                else {
                    if let SourceReadAdmission::Current(exact) = admission {
                        drop(exact.store.finish_index_read_snapshot());
                    }
                    continue;
                };
                let exact = match admission {
                    SourceReadAdmission::Current(exact) => exact,
                    SourceReadAdmission::Reconcile(assessment) => {
                        return Ok(SourceReadAdmission::Reconcile(assessment));
                    }
                };
                #[cfg(test)]
                if self
                    .preparation_invalidations
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                        remaining.checked_sub(1)
                    })
                    .is_ok()
                {
                    entry.invalidate_after_proven_source_mismatch()?;
                }
                let publication = exact
                    .store
                    .index_publication()?
                    .ok_or_else(|| source_changed_during_derivation(&entry.binding.root, "."))?;
                let captured = exact.store.captured_project_binding()?;
                let database_identity = exact.store.database_file_identity();
                work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
                let receiver = entry.receiver.lock().map_err(|_poisoned| {
                    lock_error(&entry.binding.root, "source observation receiver")
                })?;
                if entry.drain_source_events(
                    &receiver,
                    &after_plan.scan_options,
                    database_identity.as_ref(),
                    control,
                )? || !entry
                    .binding
                    .matches_database_store(database_identity.as_ref(), &exact.store)
                {
                    drop(receiver);
                    drop(exact.store.finish_index_read_snapshot());
                    continue;
                }
                let ingress_sequence = entry.ingress_sequence.load(Ordering::Acquire);
                let epoch = entry.install_epoch(
                    self.process_nonce,
                    &captured,
                    publication.generation,
                    ingress_sequence,
                    (before_contract, before_policy),
                    database_identity,
                )?;
                return Ok(SourceReadAdmission::Current(Some((exact.store, epoch))));
            }
            Ok(SourceReadAdmission::Current(None))
        })();
        if !matches!(&prepared, Ok(SourceReadAdmission::Current(Some(_)))) {
            // No successor can be installed until this cleanup and the owning
            // reconciliation guard have both completed.
            entry.invalidate();
        }
        prepared
    }

    /// Confirm that no source, policy, or observer event invalidated a query result.
    fn accepts_observed_result(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        store: &AtlasStore,
        control: &IndexWorkControl,
    ) -> Result<ObservedAcceptance, CliError> {
        for _attempt in 0..VERIFIED_READ_ATTEMPTS {
            control.check(IndexWorkStage::Publication)?;
            let Some(sampled) = entry.current_epoch()? else {
                return Ok(ObservedAcceptance::Invalidated);
            };
            let plan = ScanRuntimePlan::for_path_controlled(
                entry.binding.config.as_deref(),
                &entry.binding.root,
                None,
                control,
            )
            .map_err(|source| publication_input_error(&entry.binding.root, source))?;
            let contract = plan.publication_contract_fingerprint();
            let policy = source_policy_witness(&plan, control)?;
            {
                let receiver = entry.receiver.lock().map_err(|_poisoned| {
                    lock_error(&entry.binding.root, "source observation receiver")
                })?;
                match entry.drain_epoch_events(&receiver, &sampled, &plan.scan_options, control)? {
                    ObservedAcceptance::Accepted => {}
                    ObservedAcceptance::Superseded => continue,
                    ObservedAcceptance::Invalidated => return Ok(ObservedAcceptance::Invalidated),
                }
            }
            #[cfg(test)]
            if let Some(event) = entry
                .acceptance_event
                .lock()
                .map_err(|_poisoned| lock_error(&entry.binding.root, "acceptance test event"))?
                .take()
            {
                entry.publish_test_event(event)?;
            }
            let receiver = entry.receiver.lock().map_err(|_poisoned| {
                lock_error(&entry.binding.root, "source observation receiver")
            })?;
            match entry.drain_epoch_events(&receiver, &sampled, &plan.scan_options, control)? {
                ObservedAcceptance::Accepted => {}
                ObservedAcceptance::Superseded => continue,
                ObservedAcceptance::Invalidated => return Ok(ObservedAcceptance::Invalidated),
            }
            // Keep the actual query/writer connection inside the callback fence.
            // A copied database can preserve project ID and generation while its
            // native object differs, including a replacement-and-restore race.
            #[cfg(test)]
            if let Some((reached, release)) = entry
                .database_acceptance_gate
                .lock()
                .map_err(|_poisoned| lock_error(&entry.binding.root, "database acceptance gate"))?
                .take()
            {
                reached.send(()).map_err(|_closed| {
                    CliError::InvalidInput("database acceptance gate receiver closed".to_string())
                })?;
                release
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_closed| {
                        CliError::InvalidInput(
                            "database acceptance gate was not released".to_string(),
                        )
                    })?;
            }
            if !entry
                .binding
                .matches_database_store(epoch.database_identity.as_ref(), store)
            {
                entry.invalidate_epoch(epoch);
                return Ok(ObservedAcceptance::Invalidated);
            }
            let mut state = entry
                .state
                .lock()
                .map_err(|_poisoned| lock_error(&entry.binding.root, "source observation state"))?;
            let Some(current) = state.verified.as_ref() else {
                return Ok(ObservedAcceptance::Invalidated);
            };
            // A reconciliation during policy sampling requires fresh policy evidence.
            if current.stamp != sampled.stamp {
                continue;
            }
            if current.contract_fingerprint != contract || current.policy_witness != policy {
                state.verified = None;
                return Ok(ObservedAcceptance::Invalidated);
            }
            if current.stamp != epoch.stamp {
                return Ok(ObservedAcceptance::Superseded);
            }
            if current.ingress_sequence == entry.ingress_sequence.load(Ordering::Acquire) {
                return Ok(ObservedAcceptance::Accepted);
            }
        }
        Ok(ObservedAcceptance::Invalidated)
    }

    /// Exact-per-call compatibility path when a native observer cannot be admitted.
    fn with_exact_fallback<T, F>(
        &self,
        binding: &SourceBinding,
        control: &IndexWorkControl,
        repair: SourceRepairPolicy,
        mut query: F,
    ) -> Result<SourceReadAdmission<VerifiedReadOutcome<T>>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        let started = Instant::now();
        let mut work = VerifiedReadWork::default();
        for attempt in 0..VERIFIED_READ_ATTEMPTS {
            let exact = match repair.open_exact(binding, None, control)? {
                SourceReadAdmission::Current(exact) => exact,
                SourceReadAdmission::Reconcile(assessment) => {
                    return Ok(SourceReadAdmission::Reconcile(assessment));
                }
            };
            work.add_exact(exact.work);
            let stamp = self.exact_stamp(&exact.store, &binding.root)?;
            let value = query(&exact.store, stamp.clone())?;
            exact.store.finish_index_read_snapshot()?;

            let post = match repair.open_exact(binding, None, control)? {
                SourceReadAdmission::Current(exact) => exact,
                SourceReadAdmission::Reconcile(assessment) => {
                    return Ok(SourceReadAdmission::Reconcile(assessment));
                }
            };
            work.add_exact(post.work);
            let post_publication = post.store.index_publication()?;
            let post_binding = post.store.captured_project_binding()?;
            post.store.finish_index_read_snapshot()?;
            if post_publication.is_some_and(|candidate| {
                candidate.generation == stamp.generation
                    && post_binding.project_instance_id == stamp.project_instance_id
            }) {
                work.elapsed = started.elapsed();
                return Ok(SourceReadAdmission::Current(VerifiedReadOutcome {
                    value,
                    stamp,
                    work,
                }));
            }
            if attempt + 1 < VERIFIED_READ_ATTEMPTS {
                work.retries = work.retries.saturating_add(1);
            }
        }
        Err(source_changed_during_derivation(&binding.root, "."))
    }

    /// Capture durable identity from one exact current read snapshot.
    fn exact_stamp(&self, store: &AtlasStore, root: &Path) -> Result<VerifiedReadStamp, CliError> {
        let publication = store
            .index_publication()?
            .ok_or_else(|| source_changed_during_derivation(root, "."))?;
        let captured = store.captured_project_binding()?;
        Ok(VerifiedReadStamp {
            process_nonce: self.process_nonce,
            epoch: 0,
            generation: publication.generation,
            project_instance_id: captured.project_instance_id,
        })
    }
}

/// Bounded handshake used only by owning tests at the callback ingress fence.
#[cfg(test)]
type IngressFenceGate = (SyncSender<()>, Receiver<()>);

/// Exclude proven unrelated parent-directory traffic before it can consume source queue capacity.
fn selected_watcher_callback(
    runtime: &RuntimeEventIngress,
    selection: PolicyWatchSelection,
    sender: SyncSender<Event>,
    receiver: Arc<Mutex<Receiver<Event>>>,
    ingress_sequence: Arc<AtomicU64>,
    continuity_lost: Arc<AtomicBool>,
    #[cfg(test)] before_fence: Option<IngressFenceGate>,
) -> impl FnMut(notify::Result<Event>) + Send + 'static {
    let mut publish = watcher_callback(
        sender,
        receiver,
        ingress_sequence,
        continuity_lost,
        Some(selection.clone()),
        Some(runtime.clone()),
        #[cfg(test)]
        before_fence,
    );
    let runtime = runtime.clone();
    move |result| {
        if let Ok(event) = &result
            && !selection.may_affect(event)
            && !runtime.may_affect(event)
        {
            return;
        }
        publish(result);
    }
}

/// Convert native watcher callbacks into bounded ingress and continuity state.
fn watcher_callback(
    sender: SyncSender<Event>,
    receiver: Arc<Mutex<Receiver<Event>>>,
    ingress_sequence: Arc<AtomicU64>,
    continuity_lost: Arc<AtomicBool>,
    selection: Option<PolicyWatchSelection>,
    runtime: Option<RuntimeEventIngress>,
    #[cfg(test)] before_fence: Option<IngressFenceGate>,
) -> impl FnMut(notify::Result<Event>) + Send + 'static {
    move |result| match result {
        Ok(event) if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan() => {}
        result => {
            #[cfg(test)]
            if let Some((arrived, release)) = &before_fence
                && (arrived.send(()).is_err()
                    || release.recv_timeout(Duration::from_secs(5)).is_err())
            {
                continuity_lost.store(true, Ordering::Release);
                ingress_sequence.fetch_add(1, Ordering::AcqRel);
                return;
            }
            let _receiver = match receiver.lock() {
                Ok(receiver) => receiver,
                Err(_poisoned) => {
                    continuity_lost.store(true, Ordering::Release);
                    ingress_sequence.fetch_add(1, Ordering::AcqRel);
                    return;
                }
            };
            if let Some(selection) = &selection {
                match &result {
                    Ok(event)
                        if !runtime.as_ref().map_or_else(
                            || selection.observe(event),
                            |runtime| runtime.observe(selection, event),
                        ) =>
                    {
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => selection.require_recursive_rebind(),
                }
            }
            match result {
                Ok(event) => {
                    let needs_rescan = event.need_rescan();
                    match sender.try_send(event) {
                        Ok(()) if !needs_rescan => {}
                        Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                            continuity_lost.store(true, Ordering::Release);
                        }
                    }
                }
                Err(_source) => continuity_lost.store(true, Ordering::Release),
            }
            ingress_sequence.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Hash the bounded external inputs that may change the scanner's source selection.
fn source_policy_witness(
    plan: &ScanRuntimePlan,
    control: &IndexWorkControl,
) -> Result<String, CliError> {
    let mut hasher = Hasher::new();
    hash_field(
        &mut hasher,
        "contract",
        plan.publication_contract_fingerprint().as_bytes(),
    );
    let root_identity = CanonicalProjectRoot::from_path(&plan.root)
        .map_err(|error| CliError::InvalidInput(error.to_string()))?;
    let root_identity_bytes = root_identity
        .encode()
        .map_err(|error| CliError::InvalidInput(error.to_string()))?;
    hash_field(&mut hasher, "root", &root_identity_bytes);
    hash_policy_paths(&mut hasher, &source_policy_paths(plan, control)?, control)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash selected native inputs with the same bounded file and cancellation policy.
pub(super) fn hash_policy_paths<'a>(
    hasher: &mut Hasher,
    paths: impl IntoIterator<Item = &'a PathBuf>,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    for path in paths {
        control.check(IndexWorkStage::Publication)?;
        hash_field(hasher, "path", path.as_os_str().as_encoded_bytes());
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                let file =
                    projectatlas_fs::open_regular_file(path).map_err(|source| CliError::Io {
                        path: path.clone(),
                        source,
                    })?;
                hash_policy_file(hasher, path.as_path(), file, control)?;
            }
            Ok(metadata) if metadata.is_dir() => {
                hash_field(hasher, "state", b"directory");
            }
            Ok(_metadata) => {
                hash_field(hasher, "state", b"other");
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                hash_field(hasher, "state", b"absent");
            }
            Err(source) => {
                return Err(CliError::Io {
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    Ok(())
}

/// Capture the scanner contract and dynamic policy for one exact source binding.
fn exact_source_policy(
    binding: &SourceBinding,
    control: &IndexWorkControl,
) -> Result<(String, String), CliError> {
    let plan = ScanRuntimePlan::for_path_controlled(
        binding.config.as_deref(),
        &binding.root,
        None,
        control,
    )
    .map_err(|source| publication_input_error(&binding.root, source))?;
    let contract_fingerprint = plan.publication_contract_fingerprint();
    let policy_witness = source_policy_witness(&plan, control)?;
    Ok((contract_fingerprint, policy_witness))
}

/// Rebuild policy before accepting either a current read or stale-source handoff.
fn source_policy_plan_matching(
    binding: &SourceBinding,
    contract: &str,
    policy: &str,
    control: &IndexWorkControl,
) -> Result<Option<ScanRuntimePlan>, CliError> {
    let plan = ScanRuntimePlan::for_path_controlled(
        binding.config.as_deref(),
        &binding.root,
        None,
        control,
    )
    .map_err(|source| publication_input_error(&binding.root, source))?;
    if plan.publication_contract_fingerprint() != contract {
        return Ok(None);
    }
    let after_policy = source_policy_witness(&plan, control)?;
    Ok((after_policy == policy).then_some(plan))
}

/// Collect every scanner policy input whose state contributes to source selection.
fn source_policy_paths(
    plan: &ScanRuntimePlan,
    control: &IndexWorkControl,
) -> Result<Vec<PathBuf>, CliError> {
    let mut paths = projectatlas_fs::source_selection_policy_paths_controlled(&plan.root, control)
        .map_err(|source| source_inspection_error(&plan.root, source))?;
    if let Some(config) = plan.selected_config_path.as_ref() {
        paths.push(config.clone());
    } else {
        paths.push(plan.root.join(".projectatlas").join("config.toml"));
        paths.push(plan.root.join("projectatlas.toml"));
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Hash one bounded policy file with cancellation checks between chunks.
fn hash_policy_file(
    hasher: &mut Hasher,
    path: &Path,
    file: File,
    control: &IndexWorkControl,
) -> Result<(), CliError> {
    let mut reader = file.take(MAX_POLICY_INPUT_BYTES.saturating_add(1));
    let mut buffer = [0_u8; 8_192];
    let mut observed = 0_u64;
    loop {
        control.check(IndexWorkStage::Publication)?;
        let read = read_policy_chunk(&mut reader, &mut buffer, path)?;
        if read == 0 {
            break;
        }
        observed = observed.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if observed > MAX_POLICY_INPUT_BYTES {
            return Err(CliError::InvalidInput(format!(
                "source-selection policy input '{}' exceeds the {} byte limit",
                path.display(),
                MAX_POLICY_INPUT_BYTES
            )));
        }
        hasher.update(&buffer[..read]);
    }
    hash_field(hasher, "state", b"present");
    Ok(())
}

/// Read one policy chunk while retaining the file path in any I/O error.
fn read_policy_chunk(
    reader: &mut Take<File>,
    buffer: &mut [u8],
    path: &Path,
) -> Result<usize, CliError> {
    reader.read(buffer).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Append one domain-separated field to a policy witness.
fn hash_field(hasher: &mut Hasher, name: &str, value: &[u8]) {
    hasher.update(name.as_bytes());
    hasher.update(&[0]);
    hasher.update(value);
    hasher.update(&[0xff]);
}

/// Resolve a possibly relative path against the canonical source root.
fn absolute_path_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Generate one process identity, with a deterministic local entropy fallback.
fn process_nonce() -> [u8; 16] {
    let mut nonce = [0_u8; 16];
    if getrandom::fill(&mut nonce).is_ok() {
        return nonce;
    }
    let mut hasher = Hasher::new();
    hasher.update(&std::process::id().to_le_bytes());
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    hasher.update(&time.to_le_bytes());
    nonce.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    nonce
}

/// Convert watcher startup failure into a root-scoped observer diagnostic.
fn observer_error(root: &Path, source: &notify::Error) -> CliError {
    CliError::InvalidInput(format!(
        "source observation is unavailable for '{}': {source}",
        root.display()
    ))
}

/// Build a root-scoped diagnostic for poisoned observer state.
fn lock_error(root: &Path, owner: &str) -> CliError {
    CliError::InvalidInput(format!(
        "{owner} lock is unavailable for project root '{}'",
        root.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{ModifyKind, RenameMode};
    use notify::{EventKind, event::AccessKind};
    use projectatlas_core::{IndexCancellation, PurposeSource};
    use std::error::Error;
    #[cfg(windows)]
    use std::process::Command;

    /// Create and publish a minimal indexed repository for observer tests.
    fn indexed_project(root: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
        fs::create_dir_all(root.join(".projectatlas"))?;
        let source = root.join("source.rs");
        fs::write(&source, "fn original() {}\n")?;
        let database = root.join(".projectatlas").join("projectatlas.db");
        let mut store = super::super::open_atlas_store_for_project(&database, root)?;
        let plan = ScanRuntimePlan::for_path(None, root, None)?;
        super::super::run_scan_pipeline(
            &mut store,
            &plan,
            &super::super::SymbolBuildOptions::new(
                super::super::MAX_SYMBOL_FILE_BYTES,
                Some(1),
                None,
            ),
        )?;
        drop(store);
        // Inject native events beneath the root used for registration, while
        // retaining the caller's explicit database spelling for alias controls.
        Ok((database, plan.root.join("source.rs")))
    }

    /// Keep a warmed epoch while preventing native callbacks from exposing later edits.
    fn detach_native_delivery(
        registry: &SourceObservationRegistry,
        database: &Path,
        root: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let binding = SourceBinding::new(database, root, None)?;
        let mut entries = registry
            .entries
            .lock()
            .map_err(|_poisoned| lock_error(&binding.root, "test source observation registry"))?;
        let entry = entries
            .get_mut(&binding)
            .and_then(Arc::get_mut)
            .ok_or_else(|| std::io::Error::other("test observer is not exclusively owned"))?;
        entry
            .watcher
            .get_mut()
            .map_err(|_poisoned| lock_error(&binding.root, "test watcher"))?
            .watcher = notify::recommended_watcher(|_event| {})?;
        Ok(())
    }

    /// Build a bounded non-cancelled control for observer tests.
    fn test_control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(30)))
    }

    #[test]
    fn exact_refresh_borrows_operation_plan_and_fresh_fallback_rechecks_config()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let (database, _source) = indexed_project(&root)?;
        let config = temp.path().join("selected.toml");
        let canonical_root = root.canonicalize()?;
        let config_root = toml::Value::String(
            canonical_root
                .to_str()
                .ok_or("fixture root is not UTF-8")?
                .to_owned(),
        );
        fs::write(&config, format!("[project]\nroot = {config_root}\n"))?;
        let binding = SourceBinding::new(&database, &root, Some(&config))?;
        let control = test_control();
        let before_plan =
            ScanRuntimePlan::for_path_controlled(Some(&config), &binding.root, None, &control)?;
        let mut store = super::super::open_atlas_store_for_project(&database, &binding.root)?;
        super::super::run_scan_pipeline(
            &mut store,
            &before_plan,
            &super::super::SymbolBuildOptions::new(
                super::super::MAX_SYMBOL_FILE_BYTES,
                Some(1),
                None,
            ),
        )?;
        drop(store);
        fs::write(&config, "invalid = [\n")?;
        let SourceReadAdmission::Current(exact) = SourceRepairPolicy::RequireCurrent.open_exact(
            &binding,
            Some(&before_plan),
            &control,
        )?
        else {
            return Err("unchanged saved source unexpectedly required reconciliation".into());
        };
        exact.store.finish_index_read_snapshot()?;
        require(
            SourceRepairPolicy::RequireCurrent
                .open_exact(&binding, None, &control)
                .is_err(),
            "fresh exact fallback reused the earlier configuration plan",
        )?;
        require(
            ScanRuntimePlan::for_path_controlled(Some(&config), &binding.root, None, &control)
                .is_err(),
            "independent post-admission plan did not inspect the current configuration",
        )
    }

    #[test]
    fn stale_exact_assessment_rechecks_policy_before_refresh_handoff() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let control = test_control();
        let before_plan =
            ScanRuntimePlan::for_path_controlled(None, &binding.root, None, &control)?;
        let contract = before_plan.publication_contract_fingerprint();
        let policy = source_policy_witness(&before_plan, &control)?;
        fs::write(&source, "fn stale_source_requires_refresh() {}\n")?;
        require(
            matches!(
                SourceRepairPolicy::RequireCurrent.open_exact(
                    &binding,
                    Some(&before_plan),
                    &control,
                )?,
                SourceReadAdmission::Reconcile(_)
            ),
            "changed source did not produce a real exact reconciliation assessment",
        )?;
        require(
            source_policy_plan_matching(&binding, &contract, &policy, &control)?.is_some(),
            "unchanged policy refused an ordinary stale-source handoff",
        )?;
        let ignore = binding.root.join(".ignore");
        fs::write(&ignore, "not_present.rs\n")?;
        require(
            source_policy_plan_matching(&binding, &contract, &policy, &control)?.is_none(),
            "policy changed after stale assessment without invalidating its handoff",
        )?;
        fs::remove_file(ignore)?;
        let config = binding.root.join(".projectatlas/config.toml");
        fs::write(&config, "invalid = [\n")?;
        require(
            source_policy_plan_matching(&binding, &contract, &policy, &control).is_err(),
            "invalid current configuration crossed the stale-source handoff boundary",
        )?;
        fs::remove_file(config)?;
        require(
            source_policy_plan_matching(&binding, &contract, &policy, &control)?.is_some(),
            "restored policy could not retry its stale-source reconciliation",
        )
    }

    #[test]
    fn native_observer_filters_target_cohort_through_operator_database_alias()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let (database, _source) = indexed_project(&root)?;
        #[cfg(unix)]
        let selected = temp.path().join("operator-alias.db");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&database, &selected)?;
        #[cfg(windows)]
        let selected = {
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); (New-Object -ComObject Scripting.FileSystemObject).GetFile($env:PROJECTATLAS_TEST_DB_PATH).ShortPath"])
                .env("PROJECTATLAS_TEST_DB_PATH", &database)
                .output()?;
            require(output.status.success(), "native short-name lookup failed")?;
            let selected = PathBuf::from(String::from_utf8(output.stdout)?.trim());
            if selected.file_name() == database.file_name() {
                if std::env::var_os("GITHUB_ACTIONS").is_some() {
                    return Err(std::io::Error::other(
                        "hosted observer proof requires a real database short name",
                    )
                    .into());
                }
                eprintln!("native short-name observer proof unavailable on this filesystem");
                return Ok(());
            }
            selected
        };
        let entry = SourceObservationEntry::start(SourceBinding::new(&selected, &root, None)?)?;
        let store = super::super::open_atlas_store_for_project(&selected, &root)?;
        store.set_purpose("source.rs", "Alias writer", PurposeSource::Agent)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = false;
        let mut captured = Vec::new();
        while Instant::now() < deadline {
            let events: Vec<_> = entry
                .receiver
                .lock()
                .map_err(|_poisoned| std::io::Error::other("observer receiver lock poisoned"))?
                .try_iter()
                .collect();
            for event in events {
                if captured.len() < 16 {
                    captured.push(format!("{event:?}"));
                }
                let runtime_event = event.paths.iter().any(|path| {
                    #[cfg(windows)]
                    let native_name = crate::project_state::windows_filesystem::entry_name(path)
                        .ok()
                        .flatten();
                    #[cfg(windows)]
                    let name = native_name.as_deref().or_else(|| path.file_name());
                    #[cfg(not(windows))]
                    let name = path.file_name();
                    name.is_some_and(|name| {
                        name == "projectatlas.db-wal" || name == "projectatlas.db-shm"
                    })
                });
                if !runtime_event {
                    continue;
                }
                observed = true;
                let changes = observer_event_changes(
                    &entry.binding,
                    &projectatlas_fs::ScanOptions::default(),
                    &event,
                );
                if changes.has_changes() {
                    return Err(std::io::Error::other(format!(
                        "native target-cohort event escaped filtering: event={event:?}, full={}, sources={:?}, documents={:?}",
                        changes.requires_full_scan, changes.paths, changes.document_paths,
                    )).into());
                }
            }
            if observed {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !observed {
            return Err(std::io::Error::other(format!(
                "native SQLite target-cohort event was not observed: {captured:?}"
            ))
            .into());
        }
        drop(store);
        for database in [
            selected.as_path(),
            entry.binding.database_target.as_deref().ok_or_else(|| {
                std::io::Error::other("database target namespace was not captured")
            })?,
        ] {
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut path = database.as_os_str().to_os_string();
                path.push(suffix);
                let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
                    .add_path(PathBuf::from(path));
                require(
                    filtered_observer_event(&entry.binding, &event).is_none(),
                    "removed selected/target SQLite sidecar was not recognized as runtime state",
                )?;
            }
        }
        fs::write(entry.binding.root.join(".projectatlas/config.toml"), "")?;
        await_native_policy_change(&entry)?;
        Ok(())
    }

    #[test]
    fn document_only_events_invalidate_verified_epochs() -> Result<(), Box<dyn Error>> {
        for (policy, leaf) in [
            (".ignore", "target.rs"),
            (".gitignore", "target.rs"),
            (".ignore", "target.asset"),
        ] {
            let temp = tempfile::tempdir()?;
            fs::write(temp.path().join(policy), format!("{leaf}\n"))?;
            fs::write(temp.path().join(leaf), "ignored target\n")?;
            fs::write(
                temp.path().join("README.md"),
                format!("# Links\n[target]({leaf})\n"),
            )?;
            let (database, _source) = indexed_project(temp.path())?;
            let registry = SourceObservationRegistry::default();
            let control = test_control();
            let entry = registry
                .entry(SourceBinding::new(&database, temp.path(), None)?, &control)?
                .ok_or_else(|| std::io::Error::other("source observer unavailable"))?;
            let (store, epoch) = registry
                .prepare_observed_store(&entry, &control, &mut VerifiedReadWork::default(), false)?
                .ok_or_else(|| std::io::Error::other("read epoch unavailable"))?;
            store.finish_index_read_snapshot()?;
            let target = entry.binding.root.join(leaf);
            let event = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(target.clone());
            let changes = observer_event_changes(
                &entry.binding,
                &projectatlas_fs::ScanOptions::default(),
                &event,
            );
            require(
                changes.document_paths.contains(&target)
                    && changes.paths.is_empty()
                    && !changes.requires_full_scan,
                "ignored target event was not document-only",
            )?;
            entry.publish_test_event(event)?;
            require(
                SourceObservationRegistry::accepts_observed_result(
                    &entry, &epoch, &store, &control,
                )? == ObservedAcceptance::Invalidated,
                "document-only event left a warm verified epoch accepted",
            )?;
            require(
                entry.current_epoch()?.is_none()
                    && !entry.policy_selection.recursive_rebind_pending(),
                "document-only invalidation retained an epoch or requested recursive rebind",
            )?;
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn observer_preserves_distinct_source_aliases_to_database_objects() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        for (case, database) in [temp.path().join("external.db"), root.join("inside.db")]
            .into_iter()
            .enumerate()
        {
            let _store = super::super::open_atlas_store_for_project(&database, &root)?;
            let binding = SourceBinding::new(&database, &root, None)?;
            for (leaf, symlink) in [("database-link.rs", true), ("database-hardlink.rs", false)] {
                let source = binding.root.join(format!("{case}-{leaf}"));
                if symlink {
                    std::os::unix::fs::symlink(&database, &source)?;
                } else {
                    fs::hard_link(&database, &source)?;
                }
                let event = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source.clone());
                let changes = observer_event_changes(
                    &binding,
                    &projectatlas_fs::ScanOptions::default(),
                    &event,
                );
                require(
                    changes.document_paths.contains(&source),
                    "a distinct source alias to a database object was treated as runtime state",
                )?;
            }
        }
        let binding = SourceBinding::new(&root.join("inside.db"), &root, None)?;
        let directory_alias = binding.root.join("metadata-link");
        fs::create_dir_all(binding.root.join(".projectatlas"))?;
        std::os::unix::fs::symlink(binding.root.join(".projectatlas"), &directory_alias)?;
        let event =
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(directory_alias.clone());
        require(
            observer_event_changes(&binding, &projectatlas_fs::ScanOptions::default(), &event)
                .document_paths
                .contains(&directory_alias),
            "a source alias to the metadata directory was treated as the directory itself",
        )
    }

    #[cfg(unix)]
    #[test]
    fn database_alias_retarget_and_policy_events_remain_observable() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let actual_root = temp.path().join("repo");
        fs::create_dir(&actual_root)?;
        let root = temp.path().join("selected-root");
        std::os::unix::fs::symlink(&actual_root, &root)?;
        let (database, _source) = indexed_project(&root)?;
        let selected = root.join("selected.db");
        std::os::unix::fs::symlink(&database, &selected)?;
        let binding = SourceBinding::new(&selected, &root, None)?;
        // Match native registration paths while retaining the raw operator selector.
        let native_selected = binding.root.join("selected.db");
        let options = projectatlas_fs::ScanOptions::default();
        for kind in [
            EventKind::Create(notify::event::CreateKind::File),
            EventKind::Remove(notify::event::RemoveKind::File),
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
        ] {
            require(
                observer_event_changes(
                    &binding,
                    &options,
                    &Event::new(kind).add_path(native_selected.clone()),
                )
                .has_changes(),
                "database namespace replacement was acknowledged as an ordinary self-write",
            )?;
        }
        let replacement = temp.path().join("replacement.db");
        fs::copy(&database, &replacement)?;
        fs::remove_file(&selected)?;
        std::os::unix::fs::symlink(&replacement, &selected)?;
        require(
            binding != SourceBinding::new(&selected, &root, None)?,
            "retargeted database alias reused its previous observer binding",
        )?;
        require(
            observer_event_changes(
                &binding,
                &options,
                &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(native_selected),
            )
            .has_changes(),
            "retargeted database alias was filtered by stale target authority",
        )?;
        let config = binding.root.join(".projectatlas/config.toml");
        require(
            filtered_observer_event(
                &binding,
                &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(config),
            )
            .is_some(),
            "selected project configuration was filtered with runtime database state",
        )?;
        let policy = binding.root.join(".ignore");
        fs::write(&policy, "ignored.rs\n")?;
        require(
            observer_event_changes(
                &binding,
                &options,
                &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(policy),
            )
            .has_changes(),
            "local ignore policy was filtered with runtime database state",
        )?;
        let entry = SourceObservationEntry::start(binding.clone())?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        let mut previous_sidecar = binding
            .database_target
            .as_ref()
            .ok_or_else(|| std::io::Error::other("fixture has no captured database target"))?
            .as_os_str()
            .to_os_string();
        previous_sidecar.push("-wal");
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from(previous_sidecar))));
        require(
            entry.changed_since_exact_verification(&options, &test_control())?
                && entry.policy_selection.recursive_rebind_pending(),
            "coalesced previous-target sidecar acknowledged a retargeted database binding",
        )
    }

    #[cfg(unix)]
    #[test]
    fn mutation_event_fixture_preserves_invalidation_below_operator_alias()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("actual-root");
        let selected = temp.path().join("selected-root");
        fs::create_dir(&root)?;
        std::os::unix::fs::symlink(&root, &selected)?;
        let (database, source) = indexed_project(&selected)?;
        let store = super::super::open_atlas_store_for_project(&database, &selected)?;
        let registry = SourceObservationRegistry::default();
        let admission = registry.admit_mutation(&database, &selected, None, &test_control())?;
        let MutationSourceWitness::Observed { entry, .. } = &admission.witness else {
            return Err(std::io::Error::other("native fixture witness unavailable").into());
        };
        *entry
            .acceptance_event
            .lock()
            .map_err(|_poisoned| std::io::Error::other("acceptance event lock poisoned"))? =
            Some(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source));
        require(
            matches!(admission.verify(&store), Err(CliError::RefreshRequired(_))),
            "selected-root fixture spelling bypassed delayed source invalidation",
        )
    }

    #[cfg(unix)]
    #[test]
    fn native_observer_classifies_source_edits_through_selected_root_alias()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("actual-root");
        let selected = temp.path().join("selected-root");
        fs::create_dir(&root)?;
        std::os::unix::fs::symlink(&root, &selected)?;
        let (database, _source) = indexed_project(&selected)?;
        let entry = SourceObservationEntry::start(SourceBinding::new(&database, &selected, None)?)?;
        for (spelling, leaf, content) in [
            (&selected, "selected.rs", "fn selected_alias_edit() {}\n"),
            (
                &entry.binding.root,
                "canonical.rs",
                "fn canonical_root_edit() {}\n",
            ),
        ] {
            let canonical_source = entry.binding.root.join(leaf);
            entry.clear_before_exact_verification(&test_control())?;
            fs::write(spelling.join(leaf), content)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                // The callback takes this same fence before sending. Release it
                // before waiting so native ingress can make progress.
                let event = {
                    entry
                        .receiver
                        .lock()
                        .map_err(|_poisoned| {
                            std::io::Error::other("observer receiver lock poisoned")
                        })?
                        .try_recv()
                };
                if matches!(event, Err(TryRecvError::Disconnected)) {
                    return Err(std::io::Error::other("native observer disconnected").into());
                }
                if let Ok(event) = &event
                    && event.paths.iter().any(|path| path == &canonical_source)
                {
                    let changes = observer_event_changes(
                        &entry.binding,
                        &projectatlas_fs::ScanOptions::default(),
                        event,
                    );
                    require(
                        changes.paths.contains(&canonical_source),
                        "actual native source event was not admitted for verification",
                    )?;
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(std::io::Error::other(
                        "selected-root source edit did not reach its canonical native registration",
                    )
                    .into());
                }
                if matches!(event, Err(TryRecvError::Empty)) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        Ok(())
    }

    /// Return a fallible test failure without panicking inside result-returning tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    #[test]
    fn native_observer_teardown_releases_root_and_external_config() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let external = temp.path().join("settings");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&external)?;
        let config = external.join("config.toml");
        fs::write(&config, "")?;
        let database = root.join("atlas.db");
        let entry =
            SourceObservationEntry::start(SourceBinding::new(&database, &root, Some(&config))?)?;
        fs::write(root.join("source.rs"), "fn observed() {}")?;
        drop(entry);
        require(!database.exists(), "observer cleanup created an index")?;
        fs::remove_dir_all(&root)?;
        fs::remove_dir_all(&external)?;
        require(
            !root.exists() && !external.exists(),
            "watch directories remain",
        )
    }

    #[test]
    fn native_observer_failed_policy_selection_releases_admitted_root() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(&root)?;
        let database = root.join("atlas.db");
        let blocked = temp.path().join("blocked-parent");
        fs::write(&blocked, "not a directory")?;
        let config = blocked.join("config.toml");
        let binding = SourceBinding::new(&database, &root, Some(&config))?;
        let result = SourceObservationEntry::start(binding.clone());
        require(
            result.is_err(),
            "non-directory configuration parent was accepted",
        )?;
        require(
            !database.exists(),
            "failed observer startup created an index",
        )?;
        fs::remove_dir_all(&root)?;
        require(!root.exists(), "failed startup retained its root")?;
        require(
            SourceObservationEntry::start(binding).is_err(),
            "missing root was accepted",
        )
    }

    /// Wait for a real native policy event through production draining, never synthetic ingress.
    fn await_native_policy_change(entry: &SourceObservationEntry) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let control = test_control();
        while Instant::now() < deadline {
            if entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &control,
            )? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(
            std::io::Error::other("selected external input did not reach native observer draining")
                .into(),
        )
    }

    #[test]
    fn native_observer_advances_coverage_as_external_policy_parents_are_created()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let database = root.join("atlas.db");
        let parent = temp.path().join("settings");
        let nested = parent.join("git");
        let config = nested.join("config.toml");
        let entry =
            SourceObservationEntry::start(SourceBinding::new(&database, &root, Some(&config))?)?;
        fs::create_dir(&parent)?;
        await_native_policy_change(&entry)?;
        entry.clear_before_exact_verification(&test_control())?;
        fs::create_dir(&nested)?;
        await_native_policy_change(&entry)?;
        entry.clear_before_exact_verification(&test_control())?;
        fs::write(&config, "")?;
        await_native_policy_change(&entry)?;
        entry.clear_before_exact_verification(&test_control())?;
        fs::write(root.join("source.rs"), "fn observed_after_retirement() {}")?;
        await_native_policy_change(&entry)?;
        require(
            !database.exists(),
            "external watch registration implicitly initialized an index",
        )
    }

    #[test]
    fn native_observer_rebinds_a_replaced_source_root_with_external_database()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let database = temp.path().join("external-atlas.db");
        let entry = SourceObservationEntry::start(SourceBinding::new(&database, &root, None)?)?;
        fs::rename(&root, temp.path().join("retired-root"))?;
        fs::create_dir(&root)?;
        await_native_policy_change(&entry)?;
        entry.clear_before_exact_verification(&test_control())?;
        let source = root.join("source.rs");
        fs::write(&source, "pub fn replacement_source() {}\n")?;
        // Require this exact new-root child, not a late rename/create event
        // from the old root or its external parent registration.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = false;
        while Instant::now() < deadline && !observed {
            {
                let receiver = entry.receiver.lock().map_err(|poisoned| {
                    std::io::Error::other(format!("native event receiver unavailable: {poisoned}"))
                })?;
                observed = receiver.try_iter().any(|event| {
                    !matches!(event.kind, EventKind::Access(_))
                        && event
                            .paths
                            .iter()
                            .any(|path| same_native_path(path, &source))
                });
            }
            if !observed {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        require(
            observed,
            "replacement source-root child did not reach native ingress",
        )?;
        require(
            !database.exists(),
            "root observation initialized an external database",
        )
    }

    #[test]
    fn native_observer_rebinds_a_replaced_external_policy_directory() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let parent = temp.path().join("settings");
        fs::create_dir(&root)?;
        fs::create_dir(&parent)?;
        let config = parent.join("config.toml");
        fs::write(&config, "")?;
        let entry = SourceObservationEntry::start(SourceBinding::new(
            &root.join("atlas.db"),
            &root,
            Some(&config),
        )?)?;
        fs::rename(&parent, temp.path().join("retired-settings"))?;
        fs::create_dir(&parent)?;
        fs::write(&config, "")?;
        await_native_policy_change(&entry)?;
        entry.clear_before_exact_verification(&test_control())?;
        fs::write(&config, "# replacement-only edit\n")?;
        await_native_policy_change(&entry)
    }

    #[test]
    fn policy_watch_update_events_and_overflow_survive_the_exact_verification_fence()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let config = temp.path().join("config.toml");
        fs::write(&config, "")?;
        let entry = SourceObservationEntry::start(SourceBinding::new(
            &root.join("atlas.db"),
            &root,
            Some(&config),
        )?)?;
        let control = test_control();
        entry.clear_before_exact_verification_with_update(&control, || {
            entry.refresh_policy_watches(&control)?;
            entry.publish_test_event(
                Event::new(EventKind::Modify(ModifyKind::Data(
                    notify::event::DataChange::Any,
                )))
                .add_path(config.clone()),
            )
        })?;
        require(
            entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &control,
            )?,
            "policy event received during registration was discarded",
        )?;
        entry.clear_before_exact_verification_with_update(&control, || {
            entry.refresh_policy_watches(&control)?;
            entry.continuity_lost.store(true, Ordering::Release);
            Ok(())
        })?;
        require(
            entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &control,
            )?,
            "registration-time overflow was cleared before exact acceptance",
        )
    }

    #[test]
    fn failed_classification_of_consumed_event_discards_the_verified_epoch()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        registry
            .with_verified_current_read(
                &database,
                temp.path(),
                None,
                &control,
                |_store, _stamp| Ok(()),
            )?
            .require_current()?;
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let entry = registry
            .entry(binding, &control)?
            .ok_or_else(|| std::io::Error::other("native observer unavailable"))?;
        require(
            entry.current_epoch()?.is_some(),
            "fixture did not establish a verified epoch",
        )?;
        entry.publish_test_event(
            Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(source),
        )?;
        let cancelled = test_control();
        cancelled.cancel();
        require(
            entry
                .changed_since_exact_verification(
                    &projectatlas_fs::ScanOptions::default(),
                    &cancelled,
                )
                .is_err(),
            "cancelled event classification was accepted",
        )?;
        require(
            entry.current_epoch()?.is_none() && entry.continuity_lost.load(Ordering::Acquire),
            "consumed event failure retained trusted source evidence",
        )
    }

    #[test]
    fn database_notifications_do_not_arm_unicode_policy_rechecks() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("π-root");
        fs::create_dir(&root)?;
        let database = base.join("atlas.db");
        fs::write(&database, "runtime fixture")?;
        let config = base.join("config.toml");
        fs::write(&config, "")?;
        let inputs = PolicyWatchInputs::discover(&root, Some(&config), &test_control())?;
        let selection = PolicyWatchSelection::new(root.clone());
        selection.publish(Arc::new(inputs))?;
        let (sender, receiver) = sync_channel(8);
        let receiver = Arc::new(Mutex::new(receiver));
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let runtime_ingress =
            RuntimeEventIngress::new(SourceBinding::new(&database, &root, Some(&config))?);
        let mut callback = selected_watcher_callback(
            &runtime_ingress,
            selection.clone(),
            sender,
            Arc::clone(&receiver),
            Arc::clone(&sequence),
            Arc::clone(&lost),
            None,
        );
        let binding = SourceBinding::new(&database, &root, Some(&config))?;
        for (index, event) in [
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(base.join("atlas.db-wal")),
            Event::new(EventKind::Remove(notify::event::RemoveKind::File))
                .add_path(base.join("atlas.db-shm")),
            Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(database.clone()),
        ]
        .into_iter()
        .enumerate()
        {
            require(
                binding.queues_database_event(&event),
                "exact runtime spelling was not routed",
            )?;
            callback(Ok(event));
            require(
                sequence.load(Ordering::Acquire) == index as u64 + 1
                    && !selection.take_policy_recheck()
                    && !selection.recursive_rebind_pending()
                    && !lost.load(Ordering::Acquire),
                "SQLite notification armed Unicode policy uncertainty before runtime filtering",
            )?;
            require(
                receiver
                    .lock()
                    .map_err(|error| std::io::Error::other(error.to_string()))?
                    .try_recv()
                    .is_ok(),
                "runtime notification was discarded instead of reaching controlled native filtering",
            )?;
            runtime_ingress.begin_drain();
        }
        for event in [
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(database.clone()),
            Event::new(EventKind::Remove(notify::event::RemoveKind::File))
                .add_path(database.clone()),
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(database.clone()),
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(config),
            Event::new(EventKind::Modify(ModifyKind::Any))
                .add_path(binding.root.join("source-alias.rs")),
            Event::new(EventKind::Modify(ModifyKind::Any))
                .add_path(database.clone())
                .add_path(base.join("π-rules")),
            Event::new(EventKind::Modify(ModifyKind::Any))
                .add_path(database)
                .set_flag(notify::event::Flag::Rescan),
        ] {
            require(
                !binding.queues_database_event(&event),
                "runtime routing hid structural, policy, alias or rescan uncertainty",
            )?;
        }
        for _ in 0..=SOURCE_OBSERVATION_QUEUE_CAPACITY {
            callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(base.join("atlas.db-wal"))));
        }
        require(
            !lost.load(Ordering::Acquire),
            "SQLite write burst exhausted source continuity instead of sharing a runtime wake",
        )?;
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Both,
        )))
        .add_path(binding.database)));
        require(
            selection.take_policy_recheck() && selection.take_structural_policy_recheck(),
            "main-file replacement lost structural policy uncertainty",
        )
    }

    #[test]
    fn runtime_wake_rearms_after_verified_epoch_drains() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let (database, _source) = indexed_project(&root)?;
        let registry = SourceObservationRegistry::default();
        registry.with_verified_read(&database, &root, None, &test_control(), |_store, stamp| {
            Ok(stamp)
        })?;
        let entry = registry
            .entry(SourceBinding::new(&database, &root, None)?, &test_control())?
            .ok_or_else(|| std::io::Error::other("native observer unavailable"))?;
        let epoch = entry
            .current_epoch()?
            .ok_or_else(|| std::io::Error::other("fixture has no epoch"))?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        for _ in 0..2 {
            callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(database.with_file_name("projectatlas.db-wal"))));
            let receiver = entry
                .receiver
                .lock()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            require(
                entry.runtime_ingress.pending.load(Ordering::Acquire),
                "runtime event did not arm its wake",
            )?;
            require(
                matches!(
                    entry.drain_epoch_events(
                        &receiver,
                        &epoch,
                        &projectatlas_fs::ScanOptions::default(),
                        &test_control()
                    )?,
                    ObservedAcceptance::Accepted
                ),
                "runtime drain invalidated a verified source epoch",
            )?;
            require(
                !entry.runtime_ingress.pending.load(Ordering::Acquire),
                "verified epoch drain consumed its representative without rearming runtime ingress",
            )?;
        }
        Ok(())
    }

    #[test]
    fn unrelated_external_parent_flood_preserves_bounded_source_queue() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let config = base.join("config.toml");
        fs::write(&config, "")?;
        let root = root.canonicalize()?;
        let inputs = PolicyWatchInputs::discover(&root, Some(&config), &test_control())?;
        let selection = PolicyWatchSelection::new(root.clone());
        selection.publish(Arc::new(inputs))?;
        let (sender, receiver) = sync_channel(1);
        let receiver = Arc::new(Mutex::new(receiver));
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = selected_watcher_callback(
            &RuntimeEventIngress::new(SourceBinding::new(&root.join("unused.db"), &root, None)?),
            selection,
            sender,
            Arc::clone(&receiver),
            Arc::clone(&sequence),
            Arc::clone(&lost),
            None,
        );
        for i in 0..=SOURCE_OBSERVATION_QUEUE_CAPACITY {
            callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(base.join(format!("unrelated-{i}")))));
        }
        require(
            sequence.load(Ordering::Acquire) == 0 && !lost.load(Ordering::Acquire),
            "unrelated external traffic consumed observation continuity",
        )?;
        for i in 0..=SOURCE_OBSERVATION_QUEUE_CAPACITY {
            callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(base.join(format!("π-unrelated-{i}")))));
        }
        require(
            sequence.load(Ordering::Acquire) == 1 && !lost.load(Ordering::Acquire),
            "uncertain Unicode sibling traffic overflowed instead of coalescing one policy recheck",
        )?;
        {
            let receiver = receiver
                .lock()
                .map_err(|source| std::io::Error::other(source.to_string()))?;
            require(
                receiver.try_iter().count() == 1,
                "Unicode sibling wake was not bounded",
            )?;
        }
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(config)));
        require(
            sequence.load(Ordering::Acquire) == 2 && !lost.load(Ordering::Acquire),
            "selected external input was not admitted",
        )?;
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(root.join("source.rs"))));
        require(
            lost.load(Ordering::Acquire),
            "real selected-event overflow stopped failing closed",
        )
    }

    #[test]
    fn unicode_sibling_rechecks_preserve_only_a_verified_policy_epoch() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let (database, _source) = indexed_project(&root)?;
        let registry = SourceObservationRegistry::default();
        let first = registry.with_verified_read(
            &database,
            &root,
            None,
            &test_control(),
            |_store, stamp| Ok(stamp),
        )?;
        let entry = registry
            .entry(SourceBinding::new(&database, &root, None)?, &test_control())?
            .ok_or_else(|| std::io::Error::other("native observer unavailable"))?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        let sibling = || {
            Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(base.join("π-unrelated"))
        };
        for _ in 0..=SOURCE_OBSERVATION_QUEUE_CAPACITY {
            callback(Ok(sibling()));
        }
        require(
            !entry.continuity_lost.load(Ordering::Acquire),
            "Unicode sibling storm lost source continuity",
        )?;
        require(
            !entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &test_control(),
            )?,
            "unchanged sibling storm invalidated a verified policy",
        )?;
        let warm = registry.with_verified_read(
            &database,
            &root,
            None,
            &test_control(),
            |_store, stamp| Ok(stamp),
        )?;
        require(
            warm.value == first.value && warm.work.exact_verifications == 0,
            "Unicode sibling storm repeated exact source work",
        )?;
        callback(Ok(sibling()));
        let cancellation = IndexCancellation::new();
        let cancelled = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(5)));
        cancellation.cancel();
        require(
            matches!(
                entry.changed_since_exact_verification(
                    &projectatlas_fs::ScanOptions::default(),
                    &cancelled
                ),
                Err(CliError::IndexWork(_) | CliError::Fs(projectatlas_fs::FsError::IndexWork(_)))
            ),
            "policy recheck ignored cancellation",
        )?;
        require(
            entry.current_epoch()?.is_none() && entry.policy_selection.take_policy_recheck(),
            "cancelled recheck acknowledged pending uncertainty",
        )?;
        entry.policy_selection.retain_policy_recheck();
        registry.with_verified_read(&database, &root, None, &test_control(), |_store, stamp| {
            Ok(stamp)
        })?;
        entry.invalidate();
        callback(Ok(sibling()));
        require(
            entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &test_control(),
            )?,
            "uncertain event certified an in-flight candidate without an epoch",
        )?;
        Ok(())
    }

    #[test]
    fn native_rescan_flag_survives_access_filtering() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let selection = PolicyWatchSelection::new(root.clone());
        let (sender, receiver) = sync_channel(1);
        let receiver = Arc::new(Mutex::new(receiver));
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = selected_watcher_callback(
            &RuntimeEventIngress::new(SourceBinding::new(&root.join("unused.db"), &root, None)?),
            selection,
            sender,
            receiver,
            Arc::clone(&sequence),
            Arc::clone(&lost),
            None,
        );
        let event =
            Event::new(EventKind::Access(AccessKind::Any)).set_flag(notify::event::Flag::Rescan);
        callback(Ok(event.clone()));
        require(
            lost.load(Ordering::Acquire) && sequence.load(Ordering::Acquire) == 1,
            "access kind discarded native loss-of-continuity flag",
        )?;
        require(
            super::super::notify_event_changes(
                &root,
                &projectatlas_fs::ScanOptions::default(),
                &event,
            )
            .requires_full_scan,
            "CLI event classification discarded native rescan flag",
        )
    }

    #[test]
    fn rebind_marker_and_event_share_the_callback_ingress_fence() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("repo");
        fs::create_dir(&root)?;
        let database = base.join("atlas.db");
        fs::write(&database, "runtime fixture")?;
        let config = base.join("config.toml");
        fs::write(&config, "")?;
        let inputs = Arc::new(PolicyWatchInputs::discover(
            &root,
            Some(&config),
            &test_control(),
        )?);
        for (result, rebind, recheck, runtime_pending) in [
            (
                Ok(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                        .add_path(root.clone()),
                ),
                true,
                false,
                false,
            ),
            (
                Err(notify::Error::generic("fixture native observation failure")),
                true,
                false,
                false,
            ),
            (
                Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                    notify::event::DataChange::Any,
                )))
                .add_path(base.join("π-unrelated"))),
                false,
                true,
                false,
            ),
            (
                Ok(
                    Event::new(EventKind::Create(notify::event::CreateKind::File))
                        .add_path(base.join("atlas.db-wal")),
                ),
                false,
                false,
                true,
            ),
            (
                Ok(
                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                        .add_path(database.clone()),
                ),
                true,
                false,
                false,
            ),
        ] {
            let is_error = result.is_err();
            let selection = PolicyWatchSelection::new(root.clone());
            selection.publish(Arc::clone(&inputs))?;
            let runtime =
                RuntimeEventIngress::new(SourceBinding::new(&database, &root, Some(&config))?);
            let (sender, receiver) = sync_channel(1);
            let receiver = Arc::new(Mutex::new(receiver));
            let sequence = Arc::new(AtomicU64::new(0));
            let lost = Arc::new(AtomicBool::new(false));
            let (arrived, arrival) = sync_channel(1);
            let (release, released) = sync_channel(1);
            let mut callback = selected_watcher_callback(
                &runtime,
                selection.clone(),
                sender,
                Arc::clone(&receiver),
                Arc::clone(&sequence),
                Arc::clone(&lost),
                Some((arrived, released)),
            );
            std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
                // Hold the same fence used by final drain/acceptance. The callback
                // acknowledges its exact pre-lock boundary, without timing sleeps.
                let held = receiver
                    .lock()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                let worker = scope.spawn(move || callback(result));
                let reached = arrival.recv_timeout(Duration::from_secs(5));
                let premature_marker = selection.recursive_rebind_pending();
                let premature_recheck = selection.take_policy_recheck();
                let premature_runtime = runtime.pending.load(Ordering::Acquire);
                let premature_sequence = sequence.load(Ordering::Acquire);
                let premature_loss = lost.load(Ordering::Acquire);
                let queue_empty = matches!(held.try_recv(), Err(TryRecvError::Empty));
                let continued = release.try_send(());
                drop(held);
                let completed = worker.join();
                reached?;
                continued?;
                completed.map_err(|payload| {
                    std::io::Error::other(format!("native callback panicked: {payload:?}"))
                })?;
                require(
                    !premature_marker
                        && !premature_recheck
                        && !premature_runtime
                        && premature_sequence == 0
                        && !premature_loss
                        && queue_empty,
                    "acceptance fence observed a marker without its event/sequence",
                )?;
                require(
                    selection.recursive_rebind_pending() == rebind
                        && selection.take_policy_recheck() == recheck
                        && runtime.pending.load(Ordering::Acquire) == runtime_pending
                        && sequence.load(Ordering::Acquire) == 1
                        && lost.load(Ordering::Acquire) == is_error,
                    "callback did not publish structural uncertainty with ingress",
                )?;
                let queued = receiver
                    .lock()
                    .map_err(|error| std::io::Error::other(error.to_string()))?
                    .try_recv();
                require(
                    queued.is_ok() != is_error,
                    "callback publication lost its event/error semantics",
                )
            })?;
        }
        Ok(())
    }

    #[test]
    fn watcher_callback_is_bounded_and_marks_overflow_and_rescan() {
        let (sender, receiver) = sync_channel(1);
        let receiver = Arc::new(Mutex::new(receiver));
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = watcher_callback(
            sender,
            Arc::clone(&receiver),
            Arc::clone(&sequence),
            Arc::clone(&lost),
            None,
            None,
            None,
        );
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Any,
        )))));
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Any))));

        assert_eq!(sequence.load(Ordering::Acquire), 2);
        assert!(lost.load(Ordering::Acquire));
        assert!(
            receiver
                .lock()
                .is_ok_and(|receiver| receiver.try_recv().is_ok())
        );
    }

    #[test]
    fn watcher_callback_ignores_access_events_without_advancing_epoch() {
        let (sender, receiver) = sync_channel(1);
        let receiver = Arc::new(Mutex::new(receiver));
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = watcher_callback(
            sender,
            receiver,
            Arc::clone(&sequence),
            Arc::clone(&lost),
            None,
            None,
            None,
        );
        callback(Ok(Event::new(EventKind::Access(AccessKind::Any))));

        assert_eq!(sequence.load(Ordering::Acquire), 0);
        assert!(!lost.load(Ordering::Acquire));
    }

    #[test]
    fn current_read_reconciles_exactly_without_repair_or_callback_delivery()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        let options = super::super::SymbolBuildOptions::new(
            super::super::MAX_SYMBOL_FILE_BYTES,
            Some(1),
            None,
        );
        let first = registry
            .with_verified_current_read(
                &database,
                temp.path(),
                None,
                &test_control(),
                |store, _stamp| Ok(store.index_publication()?),
            )?
            .require_current()?;
        let second = registry
            .with_verified_current_read(
                &database,
                temp.path(),
                None,
                &test_control(),
                |store, _stamp| Ok(store.index_publication()?),
            )?
            .require_current()?;
        require(
            first.value == second.value && first.stamp.generation == second.stamp.generation,
            "unchanged admission changed publication",
        )?;
        require(
            second.work.exact_verifications == 1
                && second.work.filesystem_entries > 0
                && second.work.filesystem_bytes > 0
                && second.work.decoded_nodes > 0,
            "one-shot admission trusted an unfenced callback queue",
        )?;

        // A source event is not the only invalidation: an external writer advances generation.
        let mut writer = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let generation = first.stamp.generation;
        super::super::run_scan_pipeline(&mut writer, &plan, &options)?;
        let advanced = registry
            .with_verified_current_read(
                &database,
                temp.path(),
                None,
                &test_control(),
                |_store, stamp| Ok(stamp.generation),
            )?
            .require_current()?;
        require(
            advanced.value != generation && advanced.work.exact_verifications > 0,
            "external publication reused the previous verified epoch",
        )?;
        let before = writer.index_publication()?;
        let previous_text = writer.load_file_text("source.rs")?.map(|row| row.content);
        // Simulate an event still pending upstream: keep the accepted epoch but
        // detach native ingress before the source write. No callback can expose it.
        detach_native_delivery(&registry, &database, temp.path())?;
        fs::write(&source, "fn changed_before_observation() {}\n")?;
        let admission = registry.with_verified_current_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |_store, _stamp| Ok(()),
        )?;
        let SourceReadAdmission::Reconcile(assessment) = admission else {
            return Err(
                std::io::Error::other("changed source did not transfer its discovery").into(),
            );
        };
        require(
            writer.index_publication()? == before
                && writer.load_file_text("source.rs")?.map(|row| row.content) == previous_text,
            "refused no-op admission changed the prior publication",
        )?;
        let refresh = super::super::refresh_index_from_assessment_controlled(
            &mut writer,
            &plan,
            &options,
            *assessment,
            &test_control(),
        )?;
        require(
            refresh.symbols.parsed == 1,
            "transferred discovery did not publish the affected source",
        )?;
        require(
            writer
                .load_file_text("source.rs")?
                .is_some_and(|row| row.content == "fn changed_before_observation() {}\n"),
            "transferred discovery did not publish current text",
        )?;
        fs::write(&source, "fn changed_after_handoff() {}\n")?;
        let stale = SourceObservationRegistry::default().with_verified_current_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |_store, _stamp| Ok(()),
        )?;
        let SourceReadAdmission::Reconcile(assessment) = stale else {
            return Err(std::io::Error::other("second change did not transfer discovery").into());
        };
        super::super::run_scan_pipeline(&mut writer, &plan, &options)?;
        let winner = writer.index_publication()?;
        let result = super::super::refresh_index_from_assessment_controlled(
            &mut writer,
            &plan,
            &options,
            *assessment,
            &test_control(),
        );
        require(
            matches!(
                result,
                Err(CliError::Db(
                    projectatlas_db::DbError::PublicationBaseGenerationChanged { .. }
                ))
            ),
            "handoff discarded captured generation and repeated discovery",
        )?;
        require(
            writer.index_publication()? == winner,
            "stale handoff changed the winner",
        )?;
        Ok(())
    }

    #[test]
    fn ordinary_parent_notifications_preserve_the_verified_source_epoch()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let (database, _source) = indexed_project(&root)?;
        let registry = SourceObservationRegistry::default();
        let first = registry.with_verified_read(
            &database,
            &root,
            None,
            &test_control(),
            |_store, stamp| Ok(stamp),
        )?;
        let entry = registry
            .entry(SourceBinding::new(&database, &root, None)?, &test_control())?
            .ok_or_else(|| std::io::Error::other("native observer unavailable"))?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        let parent = entry
            .binding
            .root
            .parent()
            .ok_or_else(|| std::io::Error::other("fixture root has no parent"))?;
        callback(Ok(
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(parent.to_path_buf())
        ));
        require(
            !entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &test_control(),
            )?,
            "ordinary ancestor modification invalidated the source epoch",
        )?;
        let warm = registry.with_verified_read(
            &database,
            &root,
            None,
            &test_control(),
            |_store, stamp| Ok(stamp),
        )?;
        require(
            warm.work.exact_verifications == 0,
            "parent metadata repeated exact verification",
        )?;
        require(
            warm.stamp == first.stamp,
            "parent metadata replaced the verified epoch",
        )?;
        callback(Ok(
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(entry.binding.root.clone())
        ));
        require(
            entry.changed_since_exact_verification(
                &projectatlas_fs::ScanOptions::default(),
                &test_control(),
            )? && entry.policy_selection.recursive_rebind_pending(),
            "a notification on the selected root lost full-verification handling",
        )
    }

    #[test]
    fn verified_epoch_avoids_repeat_tree_and_node_table_work() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();

        let first = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, stamp| Ok((store.overview()?, stamp)),
        )?;
        require(
            (1..=u64::try_from(VERIFIED_READ_ATTEMPTS)?).contains(&first.work.exact_verifications),
            "initial read did not perform a bounded exact verification",
        )?;
        require(
            first.work.filesystem_entries > 0,
            "initial read did not inspect filesystem entries",
        )?;
        require(
            first.work.decoded_nodes > 0,
            "initial read did not decode indexed nodes",
        )?;

        let second = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, stamp| Ok((store.overview()?, stamp)),
        )?;
        require(
            second.work.exact_verifications == 0,
            "warm read unexpectedly repeated exact verification",
        )?;
        require(
            second.work.filesystem_entries == 0,
            "warm read unexpectedly inspected filesystem entries",
        )?;
        require(
            second.work.filesystem_bytes == 0,
            "warm read unexpectedly hashed source bytes",
        )?;
        require(
            second.work.decoded_nodes == 0,
            "warm read unexpectedly decoded the full node table",
        )?;
        require(
            second.work.sqlite_read_statements == 1,
            "warm freshness check used an unexpected SQLite statement count",
        )?;
        require(
            first.stamp == second.stamp,
            "warm read did not reuse the verified epoch",
        )?;
        Ok(())
    }

    #[test]
    fn ignore_policy_changes_invalidate_the_epoch_and_refresh_source_truth()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let first = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;
        require(first.value, "initial indexed source was missing")?;

        fs::write(temp.path().join(".ignore"), "source.rs\n")?;
        let ignored = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        );

        require(
            matches!(ignored, Err(CliError::RefreshRequired(_))),
            "ignore policy change did not require refresh",
        )?;
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let entry = registry
            .entries
            .lock()
            .map_err(|_poisoned| std::io::Error::other("registry lock poisoned"))?
            .get(&binding)
            .cloned()
            .ok_or_else(|| std::io::Error::other("observer entry missing"))?;
        require(
            entry.current_epoch()?.is_none(),
            "ignore policy change left a verified epoch installed",
        )?;
        Ok(())
    }

    #[test]
    fn cold_admission_refreshes_policy_coverage_after_missed_events() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let git = temp.path().join(".git");
        fs::create_dir_all(git.join("info"))?;
        fs::create_dir_all(git.join("objects"))?;
        fs::create_dir_all(git.join("refs"))?;
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(git.join("config"), "[core]\n")?;
        let exclude = git.join("info/exclude");
        fs::write(&exclude, "spared.rs\n")?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let entry = registry
            .entry(binding.clone(), &control)?
            .ok_or("cold observer was not admitted")?;
        require(
            entry.current_epoch()?.is_none(),
            "fixture already has an epoch",
        )?;
        let prior_selection = entry.policy_selection.snapshot()?;
        entry.continuity_lost.store(true, Ordering::Release);
        drop(entry);
        detach_native_delivery(&registry, &database, temp.path())?;
        let modified = fs::metadata(&exclude)?.modified()?;
        fs::write(&exclude, "source.rs\n")?;
        fs::OpenOptions::new()
            .write(true)
            .open(&exclude)?
            .set_modified(modified)?;
        require(
            prior_selection.policy_state_changed(&control)?,
            "policy fixture did not change",
        )?;
        let outcome = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &control,
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;
        require(
            !outcome.value,
            "cold admission returned the old policy's indexed source",
        )?;
        require(
            outcome.work.exact_verifications >= 1,
            "cold admission skipped exact source",
        )?;
        let current = registry
            .entry(binding, &control)?
            .ok_or("cold observer disappeared")?;
        require(
            !current
                .policy_selection
                .snapshot()?
                .policy_state_changed(&control)?,
            "cold exact admission did not refresh policy observation coverage",
        )?;
        Ok(())
    }

    #[test]
    fn git_exclude_changes_are_witnessed_with_a_git_directory() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let git = temp.path().join(".git");
        let git_info = git.join("info");
        fs::create_dir_all(&git_info)?;
        fs::create_dir_all(git.join("objects"))?;
        fs::create_dir_all(git.join("refs"))?;
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(git.join("config"), "[core]\n")?;
        fs::write(git_info.join("exclude"), "")?;
        let registry = SourceObservationRegistry::default();
        let first = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;
        require(first.value, "initial indexed source was missing")?;

        fs::write(git_info.join("exclude"), "source.rs\n")?;
        let ignored = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;

        require(
            !ignored.value,
            "Git exclude change left the excluded source indexed",
        )?;
        require(
            ignored.work.exact_verifications >= 1,
            "Git exclude change reused the stale source epoch",
        )?;
        require(
            ignored.stamp.epoch > first.stamp.epoch,
            "Git exclude change did not advance the verified source epoch",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_git_exclude_target_changes_invalidate_the_epoch() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let policy = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let git = temp.path().join(".git");
        let git_info = git.join("info");
        let exclude_target = policy.path().join("exclude");
        fs::create_dir_all(&git_info)?;
        fs::create_dir_all(git.join("objects"))?;
        fs::create_dir_all(git.join("refs"))?;
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(git.join("config"), "[core]\n")?;
        fs::write(&exclude_target, "")?;
        std::os::unix::fs::symlink(&exclude_target, git_info.join("exclude"))?;
        let registry = SourceObservationRegistry::default();
        let first = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;
        require(first.value, "initial indexed source was missing")?;

        fs::write(&exclude_target, "source.rs\n")?;
        let ignored = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.load_node_by_path("source.rs")?.is_some()),
        )?;

        require(
            !ignored.value,
            "symlinked Git exclude target change left the source indexed",
        )?;
        require(
            ignored.work.exact_verifications >= 1,
            "symlinked Git exclude target change reused the stale source epoch",
        )?;
        require(
            ignored.stamp.epoch > first.stamp.epoch,
            "symlinked Git exclude target change did not advance the verified source epoch",
        )?;
        Ok(())
    }

    #[test]
    fn mid_query_edit_discards_provisional_result_and_reconciles() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let _initial = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.overview()?),
        )?;
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let entry = registry
            .entries
            .lock()
            .map_err(|_poisoned| std::io::Error::other("registry lock poisoned"))?
            .get(&binding)
            .cloned()
            .ok_or_else(|| std::io::Error::other("observer entry missing"))?;
        let mut calls = 0_u64;
        let revised = "fn revised() {}\n";

        let outcome = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| {
                calls = calls.saturating_add(1);
                let hash = store
                    .load_node_by_path("source.rs")?
                    .and_then(|node| node.node.content_hash)
                    .ok_or_else(|| CliError::InvalidInput("source hash missing".to_string()))?;
                if calls == 1 {
                    fs::write(&source, revised).map_err(|source_error| CliError::Io {
                        path: source.clone(),
                        source: source_error,
                    })?;
                    entry.publish_test_event(
                        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source.clone()),
                    )?;
                }
                Ok(hash)
            },
        )?;

        require(calls >= 2, "mid-query edit did not retry the query")?;
        require(
            outcome.work.retries >= 1,
            "mid-query edit was not reported as a retry",
        )?;
        require(
            outcome.work.exact_verifications >= 1,
            "mid-query edit did not trigger exact verification",
        )?;
        require(
            outcome.value == blake3::hash(revised.as_bytes()).to_hex().to_string(),
            "accepted result did not reflect the revised source",
        )?;
        Ok(())
    }

    #[test]
    fn observed_acceptance_does_not_wait_for_exact_reconciliation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let entry = registry
            .entry(
                SourceBinding::new(&database, temp.path(), None)?,
                &test_control(),
            )?
            .ok_or_else(|| std::io::Error::other("source observer unavailable"))?;
        let (store, epoch) = registry
            .prepare_observed_store(&entry, &control, &mut VerifiedReadWork::default(), false)?
            .ok_or_else(|| std::io::Error::other("read epoch unavailable"))?;
        let reconcile = entry
            .reconcile
            .lock()
            .map_err(|_poisoned| std::io::Error::other("source reconciliation lock poisoned"))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let accepted = std::thread::scope(|scope| {
            let checked_entry = &entry;
            let checked_epoch = &epoch;
            let checked_control = &control;
            scope.spawn(move || {
                let accepted = matches!(
                    SourceObservationRegistry::accepts_observed_result(
                        checked_entry,
                        checked_epoch,
                        &store,
                        checked_control
                    ),
                    Ok(ObservedAcceptance::Accepted)
                );
                let finished = store.finish_index_read_snapshot().is_ok();
                let _sent = sender.send(accepted && finished);
            });
            let result = receiver.recv_timeout(Duration::from_secs(5));
            // Release even on failure so the regression cannot strand the test worker.
            drop(reconcile);
            result
        });
        require(
            accepted == Ok(true),
            "observed acceptance waited for exact reconciliation or rejected valid evidence",
        )?;
        Ok(())
    }

    #[test]
    fn older_read_acceptance_preserves_newer_mutation_witness() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let before = fs::read(&source)?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let mut admission = None;
        registry.with_verified_read(&database, temp.path(), None, &control, |store, _stamp| {
            if admission.is_none() {
                admission =
                    Some(registry.admit_mutation(&database, temp.path(), None, &control)?);
            }
            Ok(store.overview()?)
        })?;
        require(
            fs::read(&source)? == before,
            "source changed during read admission",
        )?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Accepted concurrent purpose",
            PurposeSource::Agent,
        )?;
        admission
            .ok_or_else(|| std::io::Error::other("mutation admission missing"))?
            .verify(&store)?;
        transaction.commit()?;
        require(
            store.load_node_by_path("source.rs")?.is_some_and(|node| {
                node.purpose.purpose.as_deref() == Some("Accepted concurrent purpose")
            }),
            "valid concurrent mutation did not commit its purpose",
        )?;
        Ok(())
    }

    #[test]
    fn rejected_preparation_cleans_its_epoch_before_a_successor_can_be_installed()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let prior = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let MutationSourceWitness::Observed { entry, .. } = &prior.witness else {
            return Err(std::io::Error::other("observed fixture missing").into());
        };
        let stamp = entry
            .current_stamp()?
            .ok_or_else(|| std::io::Error::other("initial epoch missing"))?;
        let config = temp.path().join(".projectatlas/config.toml");
        fs::write(&config, "[invalid")?;
        let prepared = registry.prepare_observed_store_with_policy(
            entry,
            &control,
            &mut VerifiedReadWork::default(),
            false,
            SourceRepairPolicy::Incremental,
        );
        require(
            prepared.is_err(),
            "malformed preparation unexpectedly succeeded",
        )?;
        require(
            entry.current_epoch()?.is_none(),
            "failed preparation released authority before cleaning its epoch",
        )?;
        fs::remove_file(&config)?;
        let successor = registry.admit_mutation(&database, temp.path(), None, &control)?;
        // A delayed request error may clean only the stamp it sampled before failure.
        entry.invalidate_stamp(&stamp);
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Accepted successor purpose",
            PurposeSource::Agent,
        )?;
        successor.verify(&store)?;
        transaction.commit()?;
        require(
            store.load_node_by_path("source.rs")?.is_some_and(|node| {
                node.purpose.purpose.as_deref() == Some("Accepted successor purpose")
            }),
            "rejected preparation invalidated its accepted successor",
        )
    }

    #[test]
    fn superseded_mutation_cleanup_preserves_successor_witness() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let previous = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let current = registry.admit_mutation(&database, temp.path(), None, &control)?;
        require(
            matches!(previous.verify(&store), Err(CliError::RefreshRequired(_))),
            "superseded mutation witness was unexpectedly accepted",
        )?;
        current.verify(&store)?;
        Ok(())
    }

    #[test]
    fn exact_source_mismatch_invalidates_successor_without_observer_delivery()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let mut observer = SourceObservationEntry::start(binding.clone())?;
        {
            let coverage = observer
                .watcher
                .get_mut()
                .map_err(|_poisoned| lock_error(&binding.root, "test watcher"))?;
            coverage.watcher.unwatch(&binding.root)?;
            coverage.watcher.configure(notify::Config::default())?;
        }
        let entry = Arc::new(observer);
        registry
            .entries
            .lock()
            .map_err(|_poisoned| std::io::Error::other("observer registry lock poisoned"))?
            .insert(binding, Arc::clone(&entry));
        let control = test_control();
        let previous = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let MutationSourceWitness::Observed { epoch, .. } = &previous.witness else {
            return Err(std::io::Error::other("observed mutation missing").into());
        };
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before_revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        let mut successor = None;
        let result =
            VerifiedMutationAdmission::verify_observed(&entry, epoch, &store, &control, || {
                successor =
                    Some(registry.admit_mutation(&database, temp.path(), None, &control)?);
                store.set_purpose("source.rs", "Uncommitted purpose", PurposeSource::Agent)?;
                fs::write(&source, "fn changed_after_successor() {}\n").map_err(|error| {
                    CliError::Io {
                        path: source.clone(),
                        source: error,
                    }
                })?;
                verify_saved_source_matches_index_controlled(&database, temp.path(), None, &control)
            });
        drop(transaction);
        require(
            matches!(result, Err(CliError::RefreshRequired(_))),
            "exact source mismatch did not reject the previous mutation",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "rejected mutation changed the authored-purpose revision",
        )?;
        require(
            entry.current_epoch()?.is_none(),
            "proven saved-source mismatch left successor evidence reusable",
        )?;
        let successor =
            successor.ok_or_else(|| std::io::Error::other("successor admission missing"))?;
        require(
            matches!(successor.verify(&store), Err(CliError::RefreshRequired(_))),
            "successor survived a proven saved-source mismatch",
        )?;
        Ok(())
    }

    #[test]
    fn cancelled_exact_verification_preserves_successor_witness() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let registry = SourceObservationRegistry::default();
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(30)));
        let previous = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let MutationSourceWitness::Observed { entry, epoch } = &previous.witness else {
            return Err(std::io::Error::other("observed mutation missing").into());
        };
        let mut successor = None;
        let result =
            VerifiedMutationAdmission::verify_observed(entry, epoch, &store, &control, || {
                successor =
                    Some(registry.admit_mutation(&database, temp.path(), None, &test_control())?);
                cancellation.cancel();
                verify_saved_source_matches_index_controlled(&database, temp.path(), None, &control)
            });
        require(
            matches!(result, Err(CliError::IndexWork(_))),
            "exact verification did not return cancellation",
        )?;
        successor
            .ok_or_else(|| std::io::Error::other("successor admission missing"))?
            .verify(&store)?;
        Ok(())
    }

    #[test]
    fn stale_read_source_event_invalidates_newer_mutation_and_rolls_back()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let entry = registry
            .entry(
                SourceBinding::new(&database, temp.path(), None)?,
                &test_control(),
            )?
            .ok_or_else(|| std::io::Error::other("source observer unavailable"))?;
        let (read_store, old_epoch) = registry
            .prepare_observed_store(&entry, &control, &mut VerifiedReadWork::default(), false)?
            .ok_or_else(|| std::io::Error::other("read epoch unavailable"))?;
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Rejected concurrent purpose",
            PurposeSource::Agent,
        )?;
        let event = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source);
        let previous_options = projectatlas_fs::ScanOptions {
            exclude_path_prefixes: vec!["source.rs".to_owned()],
            ..projectatlas_fs::ScanOptions::default()
        };
        let ignored = observer_event_changes(&entry.binding, &previous_options, &event);
        require(
            ignored.paths.is_empty() && !ignored.requires_full_scan,
            "previous policy did not exclude the event in the regression fixture",
        )?;
        entry.publish_test_event(event)?;
        {
            let receiver = entry.receiver.lock().map_err(|_poisoned| {
                std::io::Error::other("source observation receiver lock poisoned")
            })?;
            require(
                entry.drain_epoch_events(
                    &receiver,
                    &old_epoch,
                    &previous_options,
                    &test_control(),
                )? == ObservedAcceptance::Superseded,
                "older policy consumed an event belonging to a successor epoch",
            )?;
        }
        require(
            SourceObservationRegistry::accepts_observed_result(
                &entry, &old_epoch, &store, &control,
            )? == ObservedAcceptance::Invalidated,
            "stale reader treated a source event as mere supersession",
        )?;
        require(
            entry.current_epoch()?.is_none(),
            "source event left newer evidence installed",
        )?;
        let plan = ScanRuntimePlan::for_path_controlled(None, temp.path(), None, &control)?;
        require(
            entry.changed_since_exact_verification(&plan.scan_options, &control)?,
            "consumed source event was lost to concurrent exact verification",
        )?;
        read_store.finish_index_read_snapshot()?;
        let verification = admission.verify(&store);
        transaction.rollback()?;
        require(
            matches!(verification, Err(CliError::RefreshRequired(_))),
            "consumed source event did not reject the mutation witness",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision
                && store
                    .load_node_by_path("source.rs")?
                    .is_some_and(|node| node.purpose == before_purpose),
            "rejected mutation changed authored purpose state",
        )?;
        Ok(())
    }

    #[test]
    fn mutation_admission_reconciles_saved_source_without_waiting_for_observer_delivery()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let _warm = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &control,
            |store, _stamp| Ok(store.overview()?),
        )?;
        detach_native_delivery(&registry, &database, temp.path())?;
        let revised = "fn revised_before_admission() {}\n";
        fs::write(&source, revised)?;

        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let content_hash = store
            .load_node_by_path("source.rs")?
            .and_then(|node| node.node.content_hash)
            .ok_or_else(|| std::io::Error::other("reconciled source hash missing"))?;
        require(
            content_hash == blake3::hash(revised.as_bytes()).to_hex().to_string(),
            "mutation admission reused the warm source epoch",
        )?;
        admission.verify(&store)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mutation_witness_rejects_copied_database_after_target_creation() -> Result<(), Box<dyn Error>>
    {
        for restore_original in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("repo");
            let (database, _source) = indexed_project(&root)?;
            let registry = SourceObservationRegistry::default();
            registry.with_verified_read(
                &database,
                &root,
                None,
                &test_control(),
                |store, _stamp| Ok(store.overview()?),
            )?;
            detach_native_delivery(&registry, &database, &root)?;
            let admission = registry.admit_mutation(&database, &root, None, &test_control())?;
            let MutationSourceWitness::Observed { entry, epoch } = &admission.witness else {
                return Err(std::io::Error::other("native observer was not admitted").into());
            };
            let copied = temp.path().join("copied.db");
            {
                let source = rusqlite::Connection::open(&database)?;
                source.backup(rusqlite::MAIN_DB, &copied, None)?;
            }
            let saved = temp.path().join("original.db");
            fs::rename(&database, &saved)?;
            fs::rename(&copied, &database)?;
            let store = super::super::open_atlas_store_for_project(&database, &root)?;
            require(
                store.captured_project_binding()?.project_instance_id
                    == epoch.stamp.project_instance_id
                    && store.index_publication()?.is_some_and(|publication| {
                        publication.generation == epoch.stamp.generation
                    }),
                "replacement fixture did not preserve project and generation metadata",
            )?;
            let revision = store.authored_purpose_revision()?;
            if restore_original {
                fs::rename(&database, temp.path().join("opened-copy.db"))?;
                fs::rename(&saved, &database)?;
            }
            let transaction = if restore_original {
                None
            } else {
                let transaction = store.begin_purpose_mutation()?;
                store.set_purpose(
                    "source.rs",
                    "Rejected copied-file purpose",
                    PurposeSource::Agent,
                )?;
                Some(transaction)
            };
            let mut callback = selected_watcher_callback(
                &entry.runtime_ingress,
                entry.policy_selection.clone(),
                entry.test_sender.clone(),
                Arc::clone(&entry.receiver),
                Arc::clone(&entry.ingress_sequence),
                Arc::clone(&entry.continuity_lost),
                None,
            );
            let target = entry
                .binding
                .database_target
                .clone()
                .ok_or_else(|| std::io::Error::other("database target missing"))?;
            callback(Ok(Event::new(EventKind::Create(
                notify::event::CreateKind::File,
            ))
            .add_path(target)));
            require(
                !entry.policy_selection.recursive_rebind_pending()
                    && !entry.continuity_lost.load(Ordering::Acquire),
                "replacement fixture bypassed the deferred creation identity check",
            )?;
            require(
                matches!(admission.verify(&store), Err(CliError::RefreshRequired(_))),
                "copied or restored database crossed the admitted native file boundary",
            )?;
            if let Some(transaction) = transaction {
                transaction.rollback()?;
                require(
                    store.authored_purpose_revision()? == revision
                        && store.load_node_by_path("source.rs")?.is_some_and(|node| {
                            node.purpose.purpose.as_deref() != Some("Rejected copied-file purpose")
                        }),
                    "rejected replacement committed authored purpose state",
                )?;
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mutation_witness_rejects_selected_database_link_recreation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let (database, _source) = indexed_project(&root)?;
        let selected = temp.path().join("selected.db");
        std::os::unix::fs::symlink(&database, &selected)?;
        let registry = SourceObservationRegistry::default();
        registry.with_verified_read(&selected, &root, None, &test_control(), |store, _stamp| {
            Ok(store.overview()?)
        })?;
        detach_native_delivery(&registry, &selected, &root)?;
        let admission = registry.admit_mutation(&selected, &root, None, &test_control())?;
        let MutationSourceWitness::Observed { entry, epoch } = &admission.witness else {
            return Err(std::io::Error::other("native observer was not admitted").into());
        };
        let store = super::super::open_atlas_store_for_project(&selected, &root)?;
        let revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Rejected recreated-link purpose",
            PurposeSource::Agent,
        )?;
        fs::remove_file(&selected)?;
        std::os::unix::fs::symlink(&database, &selected)?;
        require(
            epoch.database_identity.as_ref().is_some_and(|identity| {
                identity.matches_path(&selected) && identity.matches_store(&store)
            }),
            "selected-link fixture changed the target database object",
        )?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        callback(Ok(Event::new(EventKind::Create(
            notify::event::CreateKind::File,
        ))
        .add_path(selected)));
        require(
            matches!(admission.verify(&store), Err(CliError::RefreshRequired(_))),
            "recreated selected link was treated as a delayed target-file hint",
        )?;
        transaction.rollback()?;
        require(
            store.authored_purpose_revision()? == revision,
            "rejected selected-link replacement changed authored revision",
        )
    }

    #[cfg(unix)]
    #[test]
    fn database_replacement_waiting_at_receiver_fence_cannot_certify_read()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let (database, _source) = indexed_project(&root)?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        registry.with_verified_read(&database, &root, None, &control, |store, _stamp| {
            Ok(store.overview()?)
        })?;
        detach_native_delivery(&registry, &database, &root)?;
        let entry = registry
            .entry(SourceBinding::new(&database, &root, None)?, &control)?
            .ok_or_else(|| std::io::Error::other("native observer missing"))?;
        let (store, epoch) = registry
            .prepare_observed_store(&entry, &control, &mut VerifiedReadWork::default(), false)?
            .ok_or_else(|| std::io::Error::other("observed epoch missing"))?;
        let copied = temp.path().join("copied.db");
        {
            let source = rusqlite::Connection::open(&database)?;
            source.backup(rusqlite::MAIN_DB, &copied, None)?;
        }
        let (reached, arrival) = sync_channel(1);
        let (release, resume) = sync_channel(1);
        *entry
            .database_acceptance_gate
            .lock()
            .map_err(|_poisoned| std::io::Error::other("test gate poisoned"))? =
            Some((reached, resume));
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        let checked_entry = Arc::clone(&entry);
        let checked_control = control;
        let rejected = std::thread::scope(|scope| -> Result<bool, Box<dyn Error>> {
            let verification = scope.spawn(move || {
                let rejected = matches!(
                    SourceObservationRegistry::accepts_observed_result(
                        &checked_entry,
                        &epoch,
                        &store,
                        &checked_control,
                    ),
                    Ok(ObservedAcceptance::Invalidated)
                );
                let finished = store.finish_index_read_snapshot().is_ok();
                rejected && finished
            });
            let replacement = (|| -> Result<(), Box<dyn Error>> {
                arrival.recv_timeout(Duration::from_secs(5))?;
                fs::rename(&database, temp.path().join("original.db"))?;
                fs::rename(&copied, &database)?;
                Ok(())
            })();
            let target = entry
                .binding
                .database_target
                .clone()
                .ok_or_else(|| std::io::Error::other("database target missing"))?;
            let delivery = scope.spawn(move || {
                callback(Ok(Event::new(EventKind::Create(
                    notify::event::CreateKind::File,
                ))
                .add_path(target)));
            });
            let _released = release.send(());
            let rejected = verification
                .join()
                .map_err(|_panic| std::io::Error::other("verification worker panicked"))?;
            delivery
                .join()
                .map_err(|_panic| std::io::Error::other("delivery worker panicked"))?;
            replacement?;
            Ok(rejected)
        })?;
        require(
            rejected,
            "replacement waiting behind the receiver fence certified a stale read",
        )
    }

    #[cfg(unix)]
    #[test]
    fn mutation_witness_retains_same_database_after_delayed_create() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let original = same_file::Handle::from_path(&database)?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let entry = match &admission.witness {
            MutationSourceWitness::Observed { entry, .. } => Arc::clone(entry),
            MutationSourceWitness::Exact { .. } => {
                return Err(std::io::Error::other("native observer was not admitted").into());
            }
        };
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Retain source responsibility",
            PurposeSource::Agent,
        )?;
        let mut callback = selected_watcher_callback(
            &entry.runtime_ingress,
            entry.policy_selection.clone(),
            entry.test_sender.clone(),
            Arc::clone(&entry.receiver),
            Arc::clone(&entry.ingress_sequence),
            Arc::clone(&entry.continuity_lost),
            None,
        );
        let target = entry
            .binding
            .database_target
            .clone()
            .ok_or_else(|| std::io::Error::other("database target missing"))?;
        callback(Ok(Event::new(EventKind::Create(
            notify::event::CreateKind::File,
        ))
        .add_path(target)));
        require(
            same_file::Handle::from_path(&database)? == original,
            "delayed-create fixture replaced the database object",
        )?;
        admission.verify(&store)?;
        transaction.commit()?;
        require(
            store.authored_purpose_revision()? == revision + 1
                && store.load_node_by_path("source.rs")?.is_some_and(|node| {
                    node.purpose.purpose.as_deref() == Some("Retain source responsibility")
                }),
            "same-object delayed creation prevented the complete purpose transaction",
        )
    }

    #[test]
    fn native_mutation_admission_retains_preexisting_source_repair_through_commit()
    -> Result<(), Box<dyn Error>> {
        for warm in [false, true] {
            for apply in [false, true] {
                let temp = tempfile::tempdir()?;
                let (database, source) = indexed_project(temp.path())?;
                let registry = SourceObservationRegistry::default();
                let control = test_control();
                let queued = {
                    let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
                    store
                        .load_purpose_curation_batch("native-review", &["source.rs".to_string()])?
                };
                let selected = queued
                    .items
                    .first()
                    .ok_or_else(|| std::io::Error::other("queued source missing"))?;
                if warm {
                    registry.with_verified_read(
                        &database,
                        temp.path(),
                        None,
                        &control,
                        |store, _stamp| Ok(store.overview()?),
                    )?;
                }
                let revised = "fn revised_before_admission() {}\n";
                fs::write(&source, revised)?;
                let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
                let binding = SourceBinding::new(&database, temp.path(), None)?;
                require(
                    registry
                        .entries
                        .lock()
                        .map_err(|_poisoned| lock_error(temp.path(), "native test registry"))?
                        .contains_key(&binding),
                    "native mutation regression used observer-unavailable fallback",
                )?;
                report_observation_test_diagnostic(format_args!(
                    "native mutation admitted: warm={warm}, apply={apply}, observed={}",
                    matches!(&admission.witness, MutationSourceWitness::Observed { .. })
                ));
                if !warm {
                    require(
                        matches!(&admission.witness, MutationSourceWitness::Observed { .. }),
                        "cold native mutation regression did not exercise observer verification",
                    )?;
                }
                let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
                let before_revision = store.authored_purpose_revision()?;
                let transaction = store.begin_purpose_mutation()?;
                if apply {
                    store.set_purpose(
                        "source.rs",
                        "Current source responsibility",
                        PurposeSource::Agent,
                    )?;
                } else {
                    let report = super::super::review_purposes(
                        &store,
                        &[super::super::PurposeReviewRequest {
                            path: "source.rs".to_string(),
                            purpose: Some("Current source responsibility".to_string()),
                            confirm_existing: false,
                            task: Some(queued.task.clone()),
                            work_key: Some(selected.work_key.clone()),
                            state_token: Some(selected.state_token.clone()),
                        }],
                        true,
                    )?;
                    require(
                        report.changed == 0
                            && report.conflicts == 1
                            && report.failed == 0
                            && report.items.first().is_some_and(|item| {
                                matches!(&item.action, super::super::PurposeReviewAction::Stale)
                            }),
                        "repaired source did not stale the previous conditional review",
                    )?;
                }
                if let Err(error) = admission.verify(&store) {
                    transaction.rollback()?;
                    return Err(std::io::Error::other(format!(
                        "native purpose commit rejected quiescent repaired source: warm={warm}, apply={apply}, error={error}"
                    )).into());
                }
                transaction.commit()?;
                let node = store
                    .load_node_by_path("source.rs")?
                    .ok_or_else(|| std::io::Error::other("reconciled source missing"))?;
                require(
                    node.node.content_hash.as_deref()
                        == Some(blake3::hash(revised.as_bytes()).to_hex().as_str()),
                    "native mutation admission did not retain the repaired saved source",
                )?;
                require(
                    store.authored_purpose_revision()? == before_revision + u64::from(apply),
                    "native mutation advanced authored revision outside its applied batch",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn mutation_admission_retries_transient_invalidation_and_falls_back_to_exact_source()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before_source = fs::read(&source)?;
        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;

        registry
            .mutation_acceptance_invalidations
            .store(1, Ordering::Release);
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        require(
            registry
                .mutation_acceptance_invalidations
                .load(Ordering::Acquire)
                == 0,
            "transient mutation invalidation was not consumed",
        )?;
        admission.verify(&store)?;

        registry
            .mutation_acceptance_invalidations
            .store(u64::try_from(VERIFIED_READ_ATTEMPTS)?, Ordering::Release);
        let exact = registry.admit_mutation(&database, temp.path(), None, &control)?;
        require(
            matches!(&exact.witness, MutationSourceWitness::Exact { .. }),
            "persistent mutation invalidation did not fall back to exact source",
        )?;
        require(
            registry
                .mutation_acceptance_invalidations
                .load(Ordering::Acquire)
                == 0,
            "mutation invalidation attempts were not bounded",
        )?;
        exact.verify(&store)?;
        require(
            fs::read(&source)? == before_source,
            "mutation admission changed saved source",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "mutation admission changed authored-purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_some_and(|node| node.purpose == before_purpose),
            "mutation admission changed the purpose row",
        )?;
        Ok(())
    }

    #[test]
    fn exact_mismatch_during_preparation_prevents_observed_installation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let registry = SourceObservationRegistry::default();
        registry
            .preparation_invalidations
            .store(u64::try_from(VERIFIED_READ_ATTEMPTS)?, Ordering::Release);

        let admission = registry.admit_mutation(&database, temp.path(), None, &test_control())?;

        require(
            matches!(&admission.witness, MutationSourceWitness::Exact { .. }),
            "preparation continuity loss did not fall back to exact source",
        )?;
        require(
            registry.preparation_invalidations.load(Ordering::Acquire) == 0,
            "preparation invalidation attempts were not bounded",
        )?;
        admission.verify(&store)?;

        registry
            .preparation_invalidations
            .store(u64::try_from(VERIFIED_READ_ATTEMPTS)?, Ordering::Release);
        let read = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.overview()?),
        );
        require(
            matches!(read, Err(CliError::RefreshRequired(_))),
            "observer read did not retain continuity-loss recovery",
        )?;
        Ok(())
    }

    #[test]
    fn mutation_verification_redrains_delayed_observer_events() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let registry = SourceObservationRegistry::default();
        let admission = registry.admit_mutation(&database, temp.path(), None, &test_control())?;
        let MutationSourceWitness::Observed { entry, .. } = &admission.witness else {
            return Err(
                std::io::Error::other("mutation did not retain an observed witness").into(),
            );
        };
        *entry
            .acceptance_event
            .lock()
            .map_err(|_poisoned| std::io::Error::other("acceptance event lock poisoned"))? =
            Some(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(database));

        admission.verify(&store)?;

        require(
            entry
                .acceptance_event
                .lock()
                .map_err(|_poisoned| std::io::Error::other("acceptance event lock poisoned"))?
                .is_none(),
            "delayed irrelevant event was not consumed",
        )?;
        *entry
            .acceptance_event
            .lock()
            .map_err(|_poisoned| std::io::Error::other("acceptance event lock poisoned"))? =
            Some(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source));
        require(
            matches!(admission.verify(&store), Err(CliError::RefreshRequired(_))),
            "delayed relevant event was accepted",
        )?;
        Ok(())
    }

    #[test]
    fn mutation_verification_rechecks_continuity_under_drain_lock() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let registry = SourceObservationRegistry::default();
        let admission = registry.admit_mutation(&database, temp.path(), None, &test_control())?;
        let MutationSourceWitness::Observed { entry, .. } = &admission.witness else {
            return Err(
                std::io::Error::other("mutation did not retain an observed witness").into(),
            );
        };
        entry
            .drain_continuity_invalidations
            .store(1, Ordering::Release);

        require(
            matches!(admission.verify(&store), Err(CliError::RefreshRequired(_))),
            "continuity loss during drain admission was accepted",
        )?;
        require(
            entry.drain_continuity_invalidations.load(Ordering::Acquire) == 0,
            "drain continuity invalidation was not consumed",
        )?;
        Ok(())
    }

    #[test]
    fn mutation_admission_uses_exact_fallback_when_observer_capacity_is_full()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let git = temp.path().join(".git");
        let git_info = git.join("info");
        let git_exclude = git_info.join("exclude");
        fs::create_dir_all(&git_info)?;
        fs::create_dir_all(git.join("objects"))?;
        fs::create_dir_all(git.join("refs"))?;
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(git.join("config"), "[core]\n")?;
        fs::write(&git_exclude, "")?;
        let registry = SourceObservationRegistry::default();
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let filler = registry
            .entry(binding.clone(), &test_control())?
            .ok_or_else(|| std::io::Error::other("source observer was unavailable"))?;
        {
            let mut entries = registry
                .entries
                .lock()
                .map_err(|_poisoned| std::io::Error::other("observer registry was poisoned"))?;
            entries.clear();
            for index in 0..SOURCE_OBSERVATION_CAPACITY {
                let mut filler_binding = binding.clone();
                filler_binding.config = Some(temp.path().join(format!("observer-{index}.toml")));
                entries.insert(filler_binding, Arc::clone(&filler));
            }
        }

        let control = test_control();
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        require(
            matches!(&admission.witness, MutationSourceWitness::Exact { .. }),
            "full observer registry did not use exact mutation admission",
        )?;
        let entries = registry
            .entries
            .lock()
            .map_err(|_poisoned| std::io::Error::other("observer registry was poisoned"))?;
        require(
            entries.len() == SOURCE_OBSERVATION_CAPACITY && !entries.contains_key(&binding),
            "exact mutation admission changed the bounded observer registry",
        )?;
        drop(entries);

        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose("source.rs", "Exact fallback purpose", PurposeSource::Agent)?;
        admission.verify(&store)?;
        transaction.commit()?;
        require(
            store.load_node_by_path("source.rs")?.is_some_and(|node| {
                node.purpose.purpose.as_deref() == Some("Exact fallback purpose")
            }),
            "exact fallback did not commit an unchanged-source mutation",
        )?;

        let policy_admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose("source.rs", "Rejected policy purpose", PurposeSource::Agent)?;
        fs::write(&git_exclude, "never-present-policy-only.tmp\n")?;
        let verification = policy_admission.verify(&store);
        transaction.rollback()?;
        require(
            matches!(verification, Err(CliError::RefreshRequired(_))),
            "exact fallback accepted changed source-selection policy",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision
                && store
                    .load_node_by_path("source.rs")?
                    .is_some_and(|node| node.purpose == before_purpose),
            "policy-drift rejection changed authored purpose",
        )?;

        let replaced = registry.admit_mutation(&database, temp.path(), None, &control)?;
        drop(store);
        fs::write(&source, "fn concurrently_published() {}\n")?;
        let mut publisher = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let plan = ScanRuntimePlan::for_path(None, temp.path(), None)?;
        super::super::run_scan_pipeline(
            &mut publisher,
            &plan,
            &super::super::SymbolBuildOptions::new(
                super::super::MAX_SYMBOL_FILE_BYTES,
                Some(1),
                None,
            ),
        )?;
        drop(publisher);
        verify_saved_source_matches_index_controlled(&database, temp.path(), None, &control)?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Replaced generation purpose",
            PurposeSource::Agent,
        )?;
        let verification = replaced.verify(&store);
        transaction.rollback()?;
        require(
            matches!(verification, Err(CliError::RefreshRequired(_))),
            "exact fallback accepted a replacement publication generation",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision
                && store
                    .load_node_by_path("source.rs")?
                    .is_some_and(|node| node.purpose == before_purpose),
            "replacement-generation rejection changed authored purpose",
        )?;

        let cancellation = IndexCancellation::new();
        let canceled_control =
            IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(30)));
        let canceled = registry.admit_mutation(&database, temp.path(), None, &canceled_control)?;
        cancellation.cancel();
        require(
            matches!(canceled.verify(&store), Err(CliError::IndexWork(_))),
            "exact fallback did not retain cancellation through verification",
        )?;

        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;
        let stale = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose(
            "source.rs",
            "Rejected fallback purpose",
            PurposeSource::Agent,
        )?;
        fs::write(&source, "fn changed_after_exact_admission() {}\n")?;
        let verification = stale.verify(&store);
        transaction.rollback()?;
        require(
            matches!(verification, Err(CliError::RefreshRequired(_))),
            "exact fallback admitted a mutation after saved source changed",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "rejected exact fallback advanced authored-purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_some_and(|node| node.purpose == before_purpose),
            "rejected exact fallback changed the purpose row",
        )?;
        Ok(())
    }

    #[test]
    fn post_admission_edit_rolls_back_without_waiting_for_observer_delivery()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let control = test_control();
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| std::io::Error::other("indexed source missing"))?
            .purpose;
        let before_revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose("source.rs", "Stale purpose", PurposeSource::Agent)?;

        fs::write(&source, "fn changed_after_admission() {}\n")?;
        let verification = admission.verify(&store);
        drop(transaction);
        require(
            matches!(verification, Err(CliError::RefreshRequired(_))),
            "post-admission edit did not invalidate the purpose commit",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "rolled-back mutation advanced authored-purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_some_and(|node| node.purpose == before),
            "rolled-back mutation changed the purpose row",
        )?;
        Ok(())
    }

    #[test]
    fn post_admission_cancellation_rolls_back_purpose_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(30)));
        let admission = registry.admit_mutation(&database, temp.path(), None, &control)?;
        let store = super::super::open_atlas_store_for_project(&database, temp.path())?;
        let before_revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose("source.rs", "Canceled purpose", PurposeSource::Agent)?;

        cancellation.cancel();
        let verification = admission.verify(&store);
        drop(transaction);
        require(
            matches!(verification, Err(CliError::IndexWork(_))),
            "post-admission cancellation did not invalidate the purpose commit",
        )?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "canceled mutation advanced authored-purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_none_or(|node| node.purpose.purpose.as_deref() != Some("Canceled purpose")),
            "canceled mutation changed the purpose row",
        )?;
        Ok(())
    }

    #[test]
    fn same_native_path_rejects_distinct_unresolved_paths() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let left = temp.path().join("missing-left").join("leaf");
        let right = temp.path().join("missing-right").join("leaf");

        require(
            !super::same_native_path(&left, &right),
            "unresolved distinct paths were treated as equal",
        )?;
        Ok(())
    }

    #[test]
    fn same_native_path_matches_paths_below_a_canonicalizable_parent() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let parent = temp.path().join("canonical-parent");
        let nested = parent.join("nested");
        fs::create_dir(&parent)?;
        fs::create_dir(&nested)?;
        let left = parent.join("missing");
        let right = nested.join("..").join("missing");

        require(
            super::same_native_path(&left, &right),
            "equivalent paths below a canonicalizable parent diverged",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn observer_filters_case_variant_database_sidecars_without_rescan() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("MiXeDAtlasRoot");
        let metadata = root.join(".projectatlas");
        fs::create_dir_all(&metadata)?;
        // Native notify paths retain the exact registered canonical root;
        // the temporary directory may instead use a DOS short-name ancestor.
        let root = root.canonicalize()?;
        let metadata = root.join(".projectatlas");
        require(
            !crate::project_state::windows_filesystem::case_sensitive(&metadata)?,
            "case-insensitive observer fixture has a case-sensitive metadata directory",
        )?;
        let database = metadata.join("ProjectAtlas.db");
        fs::write(&database, b"SQLite format 3\0")?;
        let sidecars = ["-wal", "-shm", "-journal"].map(|suffix| {
            let mut sidecar = database.as_os_str().to_os_string();
            sidecar.push(suffix);
            PathBuf::from(sidecar)
        });
        for sidecar in &sidecars {
            fs::write(sidecar, b"SQLite sidecar")?;
        }
        let source = root.join("source.rs");
        fs::write(&source, "fn source() {}\n")?;
        let binding = SourceBinding::new(&database, &root, None)?;
        let scan_options = projectatlas_fs::ScanOptions::default();
        let differently_cased = |path: &Path| -> Result<PathBuf, Box<dyn Error>> {
            let path = path
                .to_str()
                .ok_or_else(|| std::io::Error::other("test path was not UTF-8"))?;
            Ok(PathBuf::from(path.to_ascii_uppercase()))
        };

        for path in std::iter::once(&database).chain(sidecars.iter()) {
            let event =
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(differently_cased(path)?);
            let changes = super::observer_event_changes(&binding, &scan_options, &event);
            require(
                !changes.has_changes(),
                "case-variant SQLite runtime event triggered a rescan",
            )?;
        }

        for sidecar in &sidecars {
            fs::remove_file(sidecar)?;
            let event = Event::new(EventKind::Modify(ModifyKind::Any))
                .add_path(differently_cased(sidecar)?);
            let changes = super::observer_event_changes(&binding, &scan_options, &event);
            require(
                !changes.has_changes(),
                "case-variant removed SQLite sidecar event triggered a rescan",
            )?;
        }

        let event = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source);
        let changes = super::observer_event_changes(&binding, &scan_options, &event);
        require(
            changes.has_changes(),
            "non-runtime source event was incorrectly filtered",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn observer_keeps_case_variant_sidecar_event_in_case_sensitive_directory()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let parent = temp.path().join("case-sensitive-observer");
        fs::create_dir(&parent)?;
        let enabled = Command::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            return Ok(());
        }
        let root = parent.join("Root");
        let metadata = root.join(".projectatlas");
        fs::create_dir_all(&metadata)?;
        let root = root.canonicalize()?;
        let metadata = root.join(".projectatlas");
        require(
            crate::project_state::windows_filesystem::case_sensitive(&metadata)?,
            "case-sensitive fixture directory did not inherit native case sensitivity",
        )?;
        let database = metadata.join("ProjectAtlas.db");
        fs::write(&database, b"SQLite format 3\0")?;
        let binding = SourceBinding::new(&database, &root, None)?;
        let event_path = metadata.join("PROJECTATLAS.DB-WAL");
        let event = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(event_path);
        let changes = super::observer_event_changes(
            &binding,
            &projectatlas_fs::ScanOptions::default(),
            &event,
        );
        require(
            changes.has_changes(),
            "case-variant sidecar event was suppressed in a case-sensitive directory",
        )
    }

    #[test]
    fn cancellation_and_continuity_loss_never_certify_partial_truth() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
        let registry = SourceObservationRegistry::default();
        let initial = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.overview()?),
        )?;
        require(
            initial.work.exact_verifications >= 1,
            "initial read did not establish exact truth",
        )?;
        let binding = SourceBinding::new(&database, temp.path(), None)?;
        let entry = registry
            .entries
            .lock()
            .map_err(|_poisoned| std::io::Error::other("registry lock poisoned"))?
            .get(&binding)
            .cloned()
            .ok_or_else(|| std::io::Error::other("observer entry missing"))?;

        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let canceled_control = IndexWorkControl::new(cancellation, None);
        let canceled = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &canceled_control,
            |store, _stamp| Ok(store.overview()?),
        );
        require(
            matches!(canceled, Err(CliError::IndexWork(_))),
            "cancelled observer read did not return index-work cancellation",
        )?;
        require(
            entry.current_epoch()?.is_none(),
            "cancellation left the previous verified epoch installed",
        )?;

        let recovered = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.overview()?),
        )?;
        require(
            recovered.work.exact_verifications >= 1,
            "read after cancellation did not establish exact truth",
        )?;
        entry.continuity_lost.store(true, Ordering::Release);
        let reverified = registry.with_verified_read(
            &database,
            temp.path(),
            None,
            &test_control(),
            |store, _stamp| Ok(store.overview()?),
        )?;
        require(
            reverified.work.exact_verifications >= 1,
            "continuity loss did not trigger exact verification",
        )?;
        Ok(())
    }
}
