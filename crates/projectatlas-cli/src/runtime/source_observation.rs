//! Process-local source observation for long-lived verified index reads.

use super::{
    ScanRuntimePlan, SourceVerificationWork, WatchChangeSet,
    open_atlas_store_read_only_for_project, open_exact_fresh_atlas_store_for_project_controlled,
    open_exact_saved_source_matches_index_controlled, publication_input_error,
    source_changed_during_derivation, source_inspection_error,
    verify_saved_source_matches_index_controlled,
};
use crate::CliError;
use blake3::Hasher;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use projectatlas_core::graph::ProjectInstanceId;
use projectatlas_core::{IndexGeneration, IndexWorkControl, IndexWorkStage};
use projectatlas_db::{AtlasStore, CapturedProjectBinding, IndexPublicationState};
use std::collections::HashMap;
use std::fmt;
#[cfg(test)]
use std::fs;
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Take};
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
    pub(crate) fn verify(&self) -> Result<(), CliError> {
        match &self.witness {
            MutationSourceWitness::Observed { entry, epoch } => {
                Self::verify_observed(entry, epoch, &self.control)
            }
            MutationSourceWitness::Exact {
                binding,
                stamp,
                contract_fingerprint,
                policy_witness,
            } => Self::verify_exact(
                binding,
                stamp,
                contract_fingerprint,
                policy_witness,
                &self.control,
            ),
        }
    }

    /// Revalidate an observer-backed mutation witness around exact saved source.
    fn verify_observed(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        if let Err(error) = Self::verify_observation(entry, epoch, control) {
            entry.invalidate();
            return Err(error);
        }
        if let Err(error) = verify_saved_source_matches_index_controlled(
            &entry.binding.database,
            &entry.binding.root,
            entry.binding.config.as_deref(),
            control,
        ) {
            entry.invalidate();
            return Err(error);
        }
        if let Err(error) = Self::verify_observation(entry, epoch, control) {
            entry.invalidate();
            return Err(error);
        }
        Ok(())
    }

    /// Require the retained observer epoch to remain current.
    fn verify_observation(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
        match SourceObservationRegistry::accepts_observed_result(entry, epoch, control) {
            Ok(true) => Ok(()),
            Ok(false) => Err(source_changed_during_derivation(&entry.binding.root, ".")),
            Err(error) => Err(error),
        }
    }

    /// Revalidate exact saved source plus durable identity immediately before commit.
    fn verify_exact(
        binding: &SourceBinding,
        stamp: &VerifiedReadStamp,
        contract_fingerprint: &str,
        policy_witness: &str,
        control: &IndexWorkControl,
    ) -> Result<(), CliError> {
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
            Ok::<_, CliError>((publication, captured, after))
        })();
        let finished = exact.store.finish_index_read_snapshot();
        let (publication, captured, after) = verification?;
        finished?;
        if before == after
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

/// Exact root/database/config identity used to isolate one observer.
#[derive(Clone, Debug, Eq)]
struct SourceBinding {
    /// Canonical source root watched by this observer.
    root: PathBuf,
    /// Database excluded from source-change invalidation.
    database: PathBuf,
    /// Optional scanner configuration watched with the source root.
    config: Option<PathBuf>,
}

impl PartialEq for SourceBinding {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.database == other.database && self.config == other.config
    }
}

impl Hash for SourceBinding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.root.hash(state);
        self.database.hash(state);
        self.config.hash(state);
    }
}

impl SourceBinding {
    /// Resolve one root, database, and optional configuration into a stable binding.
    fn new(database: &Path, root: &Path, config: Option<&Path>) -> Result<Self, CliError> {
        let root = root.canonicalize().map_err(|source| CliError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let database = absolute_path_from(&root, database);
        let config = config.map(|path| absolute_path_from(&root, path));
        Ok(Self {
            root,
            database,
            config,
        })
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
}

/// Mutable verified-epoch state serialized for one source binding.
#[derive(Debug, Default)]
struct ObservationState {
    /// Monotonic epoch sequence within the owning observer.
    next_epoch: u64,
    /// Last source epoch proven safe for warm reads.
    verified: Option<VerifiedSourceEpoch>,
}

/// Concrete watcher and state for one exact source binding.
struct SourceObservationEntry {
    /// Exact source, database, and configuration identity.
    binding: SourceBinding,
    /// Native watcher kept alive for the entry lifetime.
    _watcher: RecommendedWatcher,
    /// Bounded watcher event receiver drained during reconciliation.
    receiver: Mutex<Receiver<Event>>,
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
    /// Start a bounded native watcher for one source binding.
    fn start(binding: SourceBinding) -> Result<Self, CliError> {
        let (sender, receiver) = sync_channel(SOURCE_OBSERVATION_QUEUE_CAPACITY);
        #[cfg(test)]
        let test_sender = sender.clone();
        let ingress_sequence = Arc::new(AtomicU64::new(0));
        let continuity_lost = Arc::new(AtomicBool::new(false));
        let mut watcher = notify::recommended_watcher(watcher_callback(
            sender,
            Arc::clone(&ingress_sequence),
            Arc::clone(&continuity_lost),
        ))
        .map_err(|source| observer_error(&binding.root, &source))?;
        watcher
            .watch(&binding.root, RecursiveMode::Recursive)
            .map_err(|source| observer_error(&binding.root, &source))?;
        if let Some(config) = binding.config.as_deref()
            && !config.starts_with(&binding.root)
            && let Some(parent) = config.parent()
        {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .map_err(|source| observer_error(&binding.root, &source))?;
        }
        Ok(Self {
            binding,
            _watcher: watcher,
            receiver: Mutex::new(receiver),
            ingress_sequence,
            continuity_lost,
            reconcile: Mutex::new(()),
            state: Mutex::new(ObservationState::default()),
            #[cfg(test)]
            test_sender,
        })
    }

    /// Discard the current verified epoch after any uncertainty.
    fn invalidate(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.verified = None;
        }
    }

    /// Reset continuity and drain events before sampling exact source truth.
    fn clear_before_exact_verification(&self) -> Result<(), CliError> {
        self.invalidate();
        self.continuity_lost.store(false, Ordering::Release);
        let receiver = self
            .receiver
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation receiver"))?;
        loop {
            match receiver.try_recv() {
                Ok(_event) => {}
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    self.continuity_lost.store(true, Ordering::Release);
                    return Ok(());
                }
            }
        }
    }

    /// Return whether any potentially relevant event arrived since exact truth was sampled.
    fn changed_since_exact_verification(
        &self,
        scan_options: &projectatlas_fs::ScanOptions,
    ) -> Result<bool, CliError> {
        if self.continuity_lost.load(Ordering::Acquire) {
            return Ok(true);
        }
        let receiver = self
            .receiver
            .lock()
            .map_err(|_poisoned| lock_error(&self.binding.root, "source observation receiver"))?;
        let mut changes = WatchChangeSet::default();
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    let event_changes = observer_event_changes(&self.binding, scan_options, &event);
                    changes.requires_full_scan |= event_changes.requires_full_scan;
                    changes.paths.extend(event_changes.paths);
                    if self
                        .binding
                        .config
                        .as_ref()
                        .is_some_and(|config| event.paths.iter().any(|path| path == config))
                    {
                        changes.requires_full_scan = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.continuity_lost.store(true, Ordering::Release);
                    return Ok(true);
                }
            }
        }
        let changed = changes.requires_full_scan || !changes.paths.is_empty();
        if !changed {
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

    /// Install a new verified epoch after exact source and policy reconciliation.
    fn install_epoch(
        &self,
        process_nonce: [u8; 16],
        binding: &CapturedProjectBinding,
        generation: IndexGeneration,
        ingress_sequence: u64,
        contract_fingerprint: String,
        policy_witness: String,
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
            contract_fingerprint,
            policy_witness,
        };
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

/// Classify source events without letting this binding's own `SQLite` files self-invalidate it.
fn observer_event_changes(
    binding: &SourceBinding,
    scan_options: &projectatlas_fs::ScanOptions,
    event: &Event,
) -> WatchChangeSet {
    let mut filtered = event.clone();
    let metadata_directory = binding.root.join(".projectatlas");
    filtered.paths.retain(|path| {
        let candidate = super::absolute_watch_path(&binding.root, path);
        !same_native_path(&candidate, &metadata_directory)
            && !is_database_runtime_path(&candidate, &binding.database)
    });
    if filtered.paths.is_empty() && !event.need_rescan() {
        return WatchChangeSet::default();
    }
    super::notify_event_changes(&binding.root, scan_options, &filtered)
}

/// Return whether a path is the database or one of its runtime sidecar files.
fn is_database_runtime_path(candidate: &Path, database: &Path) -> bool {
    if same_native_path(candidate, database) {
        return true;
    }
    let Some(database_name) = database.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    candidate.parent().is_some_and(|parent| {
        database
            .parent()
            .is_some_and(|database_parent| same_native_path(parent, database_parent))
    }) && candidate
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.strip_prefix(database_name)
                .is_some_and(|suffix| matches!(suffix, "-wal" | "-shm" | "-journal"))
        })
}

/// Compare normalized native paths using platform case semantics.
fn same_native_path(left: &Path, right: &Path) -> bool {
    let left = projectatlas_core::normalize_native_path_display(left);
    let right = projectatlas_core::normalize_native_path_display(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
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
        let binding = SourceBinding::new(database, root, config)?;
        let Some(entry) = self.entry(binding.clone())? else {
            return self.with_exact_fallback(&binding, control, query);
        };
        self.with_observed_read(&entry, control, query)
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
        let Some(entry) = self.entry(binding.clone())? else {
            return self.admit_exact_mutation(binding, control);
        };
        // Mutation authority always starts from exact saved source; watcher delivery is advisory.
        entry.invalidate();
        let mut work = VerifiedReadWork::default();
        for _attempt in 0..VERIFIED_READ_ATTEMPTS {
            if let Err(error) = control.check(IndexWorkStage::Publication) {
                entry.invalidate();
                return Err(error.into());
            }
            let (store, epoch) = match self.prepare_observed_store(&entry, control, &mut work) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => {
                    entry.invalidate();
                    return self.admit_exact_mutation(binding, control);
                }
                Err(error) => {
                    entry.invalidate();
                    return Err(error);
                }
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
            match Self::accepts_observed_result(&entry, &epoch, control) {
                Ok(true) => {
                    if let Err(error) = store.finish_index_read_snapshot() {
                        entry.invalidate();
                        return Err(error.into());
                    }
                    return Ok(VerifiedMutationAdmission {
                        witness: MutationSourceWitness::Observed { entry, epoch },
                        control: control.clone(),
                    });
                }
                Ok(false) => {}
                Err(error) => {
                    entry.invalidate();
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            }
            entry.invalidate();
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
            exact.store.finish_index_read_snapshot()?;
            return Ok(VerifiedMutationAdmission {
                witness: MutationSourceWitness::Exact {
                    binding,
                    stamp,
                    contract_fingerprint: after.0,
                    policy_witness: after.1,
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
        entry.ingress_sequence.fetch_add(1, Ordering::AcqRel);
        entry.test_sender.try_send(event).map_err(|source| {
            CliError::InvalidInput(format!(
                "deterministic source observer event injection failed: {source}"
            ))
        })
    }

    /// Reuse or admit the bounded observer for an exact source binding.
    fn entry(
        &self,
        binding: SourceBinding,
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
        let entry = match SourceObservationEntry::start(binding.clone()) {
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
        mut query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        let started = Instant::now();
        let mut work = VerifiedReadWork::default();
        for attempt in 0..VERIFIED_READ_ATTEMPTS {
            if let Err(error) = control.check(IndexWorkStage::Publication) {
                entry.invalidate();
                return Err(error.into());
            }
            let (store, epoch) = match self.prepare_observed_store(entry, control, &mut work) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => {
                    entry.invalidate();
                    return Err(source_changed_during_derivation(&entry.binding.root, "."));
                }
                Err(error) => {
                    entry.invalidate();
                    return Err(error);
                }
            };
            let value = match query(&store, epoch.stamp.clone()) {
                Ok(value) => value,
                Err(error) => {
                    if matches!(error, CliError::IndexWork(_)) {
                        entry.invalidate();
                    }
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            };
            match Self::accepts_observed_result(entry, &epoch, control) {
                Ok(true) => {
                    store.finish_index_read_snapshot()?;
                    work.elapsed = started.elapsed();
                    return Ok(VerifiedReadOutcome {
                        value,
                        stamp: epoch.stamp,
                        work,
                    });
                }
                Ok(false) => {}
                Err(error) => {
                    entry.invalidate();
                    drop(store.finish_index_read_snapshot());
                    return Err(error);
                }
            }
            entry.invalidate();
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
    ) -> Result<Option<(AtlasStore, VerifiedSourceEpoch)>, CliError> {
        let _reconcile = entry
            .reconcile
            .lock()
            .map_err(|_poisoned| lock_error(&entry.binding.root, "source reconciliation"))?;
        let plan = ScanRuntimePlan::for_path_controlled(
            entry.binding.config.as_deref(),
            &entry.binding.root,
            None,
            control,
        )
        .map_err(|source| publication_input_error(&entry.binding.root, source))?;
        if entry.changed_since_exact_verification(&plan.scan_options)? {
            entry.invalidate();
        }
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
            {
                return Ok(Some((store, epoch)));
            }
            entry.invalidate();
            drop(store.finish_index_read_snapshot());
        }

        for _attempt in 0..VERIFIED_READ_ATTEMPTS {
            entry.clear_before_exact_verification()?;
            let before_plan = ScanRuntimePlan::for_path_controlled(
                entry.binding.config.as_deref(),
                &entry.binding.root,
                None,
                control,
            )
            .map_err(|source| publication_input_error(&entry.binding.root, source))?;
            let before_contract = before_plan.publication_contract_fingerprint();
            let before_policy = source_policy_witness(&before_plan, control)?;
            let exact = open_exact_fresh_atlas_store_for_project_controlled(
                &entry.binding.database,
                &entry.binding.root,
                entry.binding.config.as_deref(),
                control,
            )?;
            work.add_exact(exact.work);
            let after_plan = ScanRuntimePlan::for_path_controlled(
                entry.binding.config.as_deref(),
                &entry.binding.root,
                None,
                control,
            )
            .map_err(|source| publication_input_error(&entry.binding.root, source))?;
            let after_contract = after_plan.publication_contract_fingerprint();
            let after_policy = source_policy_witness(&after_plan, control)?;
            #[cfg(test)]
            if self
                .preparation_invalidations
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                entry.continuity_lost.store(true, Ordering::Release);
            }
            let changed = entry.changed_since_exact_verification(&after_plan.scan_options)?;
            if changed || before_contract != after_contract || before_policy != after_policy {
                drop(exact.store.finish_index_read_snapshot());
                continue;
            }
            let publication = exact
                .store
                .index_publication()?
                .ok_or_else(|| source_changed_during_derivation(&entry.binding.root, "."))?;
            let captured = exact.store.captured_project_binding()?;
            work.sqlite_read_statements = work.sqlite_read_statements.saturating_add(1);
            let ingress_sequence = entry.ingress_sequence.load(Ordering::Acquire);
            let epoch = entry.install_epoch(
                self.process_nonce,
                &captured,
                publication.generation,
                ingress_sequence,
                after_contract,
                after_policy,
            )?;
            return Ok(Some((exact.store, epoch)));
        }
        Ok(None)
    }

    /// Confirm that no source, policy, or observer event invalidated a query result.
    fn accepts_observed_result(
        entry: &SourceObservationEntry,
        epoch: &VerifiedSourceEpoch,
        control: &IndexWorkControl,
    ) -> Result<bool, CliError> {
        control.check(IndexWorkStage::Publication)?;
        if entry.continuity_lost.load(Ordering::Acquire) {
            return Ok(false);
        }
        let plan = ScanRuntimePlan::for_path_controlled(
            entry.binding.config.as_deref(),
            &entry.binding.root,
            None,
            control,
        )
        .map_err(|source| publication_input_error(&entry.binding.root, source))?;
        if entry.changed_since_exact_verification(&plan.scan_options)?
            || plan.publication_contract_fingerprint() != epoch.contract_fingerprint
            || source_policy_witness(&plan, control)? != epoch.policy_witness
        {
            return Ok(false);
        }
        Ok(entry.current_epoch()?.is_some_and(|current| {
            current.stamp == epoch.stamp
                && current.contract_fingerprint == epoch.contract_fingerprint
                && current.policy_witness == epoch.policy_witness
                && current.ingress_sequence == entry.ingress_sequence.load(Ordering::Acquire)
        }))
    }

    /// Exact-per-call compatibility path when a native observer cannot be admitted.
    fn with_exact_fallback<T, F>(
        &self,
        binding: &SourceBinding,
        control: &IndexWorkControl,
        mut query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        let started = Instant::now();
        let mut work = VerifiedReadWork::default();
        for attempt in 0..VERIFIED_READ_ATTEMPTS {
            let exact = open_exact_fresh_atlas_store_for_project_controlled(
                &binding.database,
                &binding.root,
                binding.config.as_deref(),
                control,
            )?;
            work.add_exact(exact.work);
            let stamp = self.exact_stamp(&exact.store, &binding.root)?;
            let value = query(&exact.store, stamp.clone())?;
            exact.store.finish_index_read_snapshot()?;

            let post = open_exact_fresh_atlas_store_for_project_controlled(
                &binding.database,
                &binding.root,
                binding.config.as_deref(),
                control,
            )?;
            work.add_exact(post.work);
            let post_publication = post.store.index_publication()?;
            let post_binding = post.store.captured_project_binding()?;
            post.store.finish_index_read_snapshot()?;
            if post_publication.is_some_and(|candidate| {
                candidate.generation == stamp.generation
                    && post_binding.project_instance_id == stamp.project_instance_id
            }) {
                work.elapsed = started.elapsed();
                return Ok(VerifiedReadOutcome { value, stamp, work });
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

/// Convert native watcher callbacks into bounded ingress and continuity state.
fn watcher_callback(
    sender: SyncSender<Event>,
    ingress_sequence: Arc<AtomicU64>,
    continuity_lost: Arc<AtomicBool>,
) -> impl FnMut(notify::Result<Event>) + Send + 'static {
    move |result| match result {
        Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
        Ok(event) => {
            ingress_sequence.fetch_add(1, Ordering::AcqRel);
            if event.need_rescan() {
                continuity_lost.store(true, Ordering::Release);
            }
            match sender.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_event) | TrySendError::Disconnected(_event)) => {
                    continuity_lost.store(true, Ordering::Release);
                }
            }
        }
        Err(_source) => {
            ingress_sequence.fetch_add(1, Ordering::AcqRel);
            continuity_lost.store(true, Ordering::Release);
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
    hash_field(&mut hasher, "root", plan.root.to_string_lossy().as_bytes());
    for path in source_policy_paths(plan, control)? {
        control.check(IndexWorkStage::Publication)?;
        hash_field(&mut hasher, "path", path.to_string_lossy().as_bytes());
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                let file = File::open(&path).map_err(|source| CliError::Io {
                    path: path.clone(),
                    source,
                })?;
                hash_policy_file(&mut hasher, path.as_path(), file, control)?;
            }
            Ok(metadata) if metadata.is_dir() => {
                hash_field(&mut hasher, "state", b"directory");
            }
            Ok(_metadata) => {
                hash_field(&mut hasher, "state", b"other");
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                hash_field(&mut hasher, "state", b"absent");
            }
            Err(source) => {
                return Err(CliError::Io { path, source });
            }
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
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
        Ok((database, source))
    }

    /// Build a bounded non-cancelled control for observer tests.
    fn test_control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(30)))
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
    fn watcher_callback_is_bounded_and_marks_overflow_and_rescan() {
        let (sender, receiver) = sync_channel(1);
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = watcher_callback(sender, Arc::clone(&sequence), Arc::clone(&lost));
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Any,
        )))));
        callback(Ok(Event::new(EventKind::Modify(ModifyKind::Any))));

        assert_eq!(sequence.load(Ordering::Acquire), 2);
        assert!(lost.load(Ordering::Acquire));
        assert!(receiver.try_recv().is_ok());
    }

    #[test]
    fn watcher_callback_ignores_access_events_without_advancing_epoch() {
        let (sender, _receiver) = sync_channel(1);
        let sequence = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let mut callback = watcher_callback(sender, Arc::clone(&sequence), Arc::clone(&lost));
        callback(Ok(Event::new(EventKind::Access(AccessKind::Any))));

        assert_eq!(sequence.load(Ordering::Acquire), 0);
        assert!(!lost.load(Ordering::Acquire));
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
                    entry.ingress_sequence.fetch_add(1, Ordering::AcqRel);
                    entry
                        .test_sender
                        .try_send(
                            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source.clone()),
                        )
                        .map_err(|source_error| {
                            CliError::InvalidInput(format!(
                                "deterministic observation injection failed: {source_error}"
                            ))
                        })?;
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
        admission.verify()?;
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
        admission.verify()?;

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
        exact.verify()?;
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
    fn preparation_continuity_loss_falls_back_only_for_mutations() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let (database, _source) = indexed_project(temp.path())?;
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
        admission.verify()?;

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
            .entry(binding.clone())?
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
        admission.verify()?;
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
        let verification = policy_admission.verify();
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
        let verification = replaced.verify();
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
            matches!(canceled.verify(), Err(CliError::IndexWork(_))),
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
        let verification = stale.verify();
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
        let verification = admission.verify();
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
        let verification = admission.verify();
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
