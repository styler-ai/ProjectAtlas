//! Process-wide admission and scheduling for the optional parser-pack worker.

use super::{
    SymbolBuildOptions, SymbolParseJob, SymbolParseOutcome, admit_symbol_job_source,
    parse_admitted_symbol_job,
};
use crate::CliError;
use projectatlas_cli::optional_parser_lifecycle::{
    OptionalParserPackLifecycle, OptionalParserPackLifecycleError,
    OptionalParserPackProjectSelection, OptionalParserPackSelectionKey,
    VerifiedOptionalParserPackSelection,
};
use projectatlas_cli::parser_supervisor::ParserSupervisorError;
use projectatlas_core::language::{BROAD_PARSER_PACK_ID, language_capability};
use projectatlas_core::optional_parser_protocol::{
    PARSER_MAX_NODE_COUNT, PARSER_MAX_OUTPUT_BYTES, PARSER_MAX_TREE_DEPTH, ParserRequestLimits,
};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkStage};
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

/// Per-file ceiling proven by the fresh-artifact release verifier.
const OPTIONAL_PARSE_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum interval without identity-validated worker progress.
const OPTIONAL_PARSE_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(5);
/// Cooperative polling interval while another project owns optional parsing.
const OPTIONAL_RUNTIME_ADMISSION_POLL: Duration = Duration::from_millis(10);
/// Maximum built-in jobs retained in one Rayon result batch.
const BUILT_IN_PARSE_BATCH_SIZE: usize = 64;

/// One process-wide optional worker owner shared by CLI, watcher, and MCP paths.
static OPTIONAL_PARSER_RUNTIME: OnceLock<Mutex<OptionalParserRuntime>> = OnceLock::new();
/// Whether an inactive project requested cleanup while another project held the worker lease.
static OPTIONAL_PARSER_DEACTIVATION_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether the process-global coordinator is inside one optional parsing group.
static OPTIONAL_PARSER_GROUP_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Closed process-wide optional parser state.
enum OptionalParserRuntimeState<T = Box<VerifiedOptionalParserPackSelection>> {
    /// No optional worker has been admitted.
    Inactive,
    /// One verified selection owns the current grammar-affined supervisor.
    Resident {
        /// Verified process owner retaining immutable-slot execution authority.
        verified: T,
    },
    /// Cleanup could not be proved; retained authority blocks cross-process slot mutation.
    UnavailableAfterCleanupFailure {
        /// Verified selection retained until process exit when child cleanup is uncertain.
        retained: Option<T>,
    },
}

impl<T> OptionalParserRuntimeState<T> {
    /// Quarantine the runtime without dropping an execution owner whose cleanup is uncertain.
    fn retain_after_cleanup_failure(&mut self) {
        let previous = std::mem::replace(
            self,
            Self::UnavailableAfterCleanupFailure { retained: None },
        );
        let retained = match previous {
            Self::Resident { verified } => Some(verified),
            Self::UnavailableAfterCleanupFailure { retained } => retained,
            Self::Inactive => None,
        };
        *self = Self::UnavailableAfterCleanupFailure { retained };
    }
}

/// Synchronous coordinator protected for exactly one optional job group at a time.
struct OptionalParserRuntime {
    /// Current process-wide worker state.
    state: OptionalParserRuntimeState,
}

/// Panic-safe process-global optional-group activity signal.
struct OptionalParserGroupLease {
    /// Whether dropping this lease still needs to clear the activity signal.
    active: bool,
}

impl OptionalParserGroupLease {
    /// Publish one active group after the coordinator mutex has been acquired.
    fn acquire() -> Self {
        OPTIONAL_PARSER_GROUP_ACTIVE.store(true, Ordering::Release);
        Self { active: true }
    }

    /// Clear activity before servicing a pending transition under the same mutex lease.
    fn release(&mut self) {
        OPTIONAL_PARSER_GROUP_ACTIVE.store(false, Ordering::Release);
        self.active = false;
    }
}

impl Drop for OptionalParserGroupLease {
    fn drop(&mut self) {
        if self.active {
            OPTIONAL_PARSER_GROUP_ACTIVE.store(false, Ordering::Release);
        }
    }
}

impl OptionalParserRuntime {
    /// Create the process-global coordinator without launching a worker.
    const fn new() -> Self {
        Self {
            state: OptionalParserRuntimeState::Inactive,
        }
    }

    /// Activate one verified selection, reusing only the exact same immutable artifact.
    fn activate(
        &mut self,
        verified: VerifiedOptionalParserPackSelection,
    ) -> Result<(), ParserSupervisorError> {
        let selection = verified.selection_key().clone();
        if matches!(
            &self.state,
            OptionalParserRuntimeState::Resident {
                verified: current,
            } if current.selection_key() == &selection
        ) {
            return Ok(());
        }
        if matches!(
            self.state,
            OptionalParserRuntimeState::UnavailableAfterCleanupFailure { .. }
        ) {
            return Err(ParserSupervisorError::Cleanup {
                message: "optional parser runtime is unavailable after an earlier cleanup failure"
                    .to_owned(),
            });
        }
        let previous = std::mem::replace(&mut self.state, OptionalParserRuntimeState::Inactive);
        if let OptionalParserRuntimeState::Resident { mut verified } = previous
            && let Err(error) = verified.supervisor_mut().shutdown()
        {
            self.state = OptionalParserRuntimeState::UnavailableAfterCleanupFailure {
                retained: Some(verified),
            };
            return Err(error);
        }
        self.state = OptionalParserRuntimeState::Resident {
            verified: Box::new(verified),
        };
        Ok(())
    }

    /// Shut down a resident selection while keeping default-core work available.
    fn deactivate(&mut self) -> Result<(), ParserSupervisorError> {
        let previous = std::mem::replace(&mut self.state, OptionalParserRuntimeState::Inactive);
        match previous {
            OptionalParserRuntimeState::Resident { mut verified } => {
                match verified.supervisor_mut().shutdown() {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        self.state = OptionalParserRuntimeState::UnavailableAfterCleanupFailure {
                            retained: Some(verified),
                        };
                        Err(error)
                    }
                }
            }
            OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained } => {
                self.state =
                    OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained };
                Ok(())
            }
            OptionalParserRuntimeState::Inactive => Ok(()),
        }
    }

    /// Mark poisoned or cleanup-uncertain state unavailable after one best-effort shutdown.
    fn quarantine(&mut self) -> Result<(), ParserSupervisorError> {
        let cleanup = self.deactivate();
        if cleanup.is_ok() {
            self.state =
                OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained: None };
        }
        cleanup
    }
}

/// Parse built-in jobs in Rayon and accepted optional jobs in deterministic grammar/path order.
pub(super) fn parse_symbol_jobs_controlled(
    project_root: &Path,
    project_selection: &OptionalParserPackProjectSelection,
    pool: &rayon::ThreadPool,
    jobs: &[SymbolParseJob],
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<Vec<SymbolParseOutcome>, CliError> {
    if matches!(
        project_selection,
        OptionalParserPackProjectSelection::Inactive
    ) {
        deactivate_if_initialized(control)?;
        let built_in = jobs.iter().collect::<Vec<_>>();
        return parse_built_in_jobs(pool, &built_in, options, control);
    }

    let lifecycle = OptionalParserPackLifecycle::new(project_root, None)?;
    let verified = lifecycle.resolve_selected_pack()?.ok_or_else(|| {
        CliError::ParserPack(OptionalParserPackLifecycleError::InvalidData {
            reason: "optional parser selection disappeared before source staging".to_owned(),
        })
    })?;
    if project_selection.selection_key() != Some(verified.selection_key()) {
        return Err(CliError::ParserPack(
            OptionalParserPackLifecycleError::InvalidData {
                reason: "optional parser selection changed before source staging".to_owned(),
            },
        ));
    }
    let expected_selection = verified.selection_key().clone();

    let mut built_in = Vec::with_capacity(jobs.len());
    let mut optional = Vec::new();
    for job in jobs {
        match optional_language_id(job) {
            Some(language) if verified.accepts_language(language) => {
                optional.push((language, job));
            }
            _ => built_in.push(job),
        }
    }
    sort_optional_jobs(&mut optional);

    let mut outcomes = parse_built_in_jobs(pool, &built_in, options, control)?;
    if outcomes.iter().any(terminal_parse_outcome) {
        return Ok(outcomes);
    }

    let mut runtime = lock_optional_runtime(control)?;
    let mut group_lease = OptionalParserGroupLease::acquire();
    let operation = (|| {
        service_pending_deactivation(&mut runtime)?;
        ensure_selection_current(&lifecycle, &expected_selection)?;
        runtime.activate(verified).map_err(supervisor_error)?;
        let limits = ParserRequestLimits::new(
            PARSER_MAX_OUTPUT_BYTES,
            PARSER_MAX_NODE_COUNT,
            PARSER_MAX_TREE_DEPTH,
        )
        .map_err(ParserSupervisorError::from)
        .map_err(supervisor_error)?;

        for (language, job) in optional {
            control.check(IndexWorkStage::SymbolParsing)?;
            let content = match admit_symbol_job_source(job, options, control) {
                Ok(content) => content,
                Err(outcome) if matches!(&*outcome, SymbolParseOutcome::BinaryOrNonUtf8 { .. }) => {
                    outcomes.push(*outcome);
                    continue;
                }
                Err(outcome) => {
                    outcomes.push(*outcome);
                    return Ok(outcomes);
                }
            };
            ensure_selection_current(&lifecycle, &expected_selection)?;
            let deadline = optional_parse_deadline(control)?;
            let result = match &mut runtime.state {
                OptionalParserRuntimeState::Resident { verified } => {
                    verified.supervisor_mut().parse(
                        language,
                        content.as_bytes(),
                        limits,
                        deadline,
                        OPTIONAL_PARSE_NO_PROGRESS_TIMEOUT,
                        control.cancellation(),
                    )
                }
                OptionalParserRuntimeState::Inactive
                | OptionalParserRuntimeState::UnavailableAfterCleanupFailure { .. } => {
                    return Err(runtime_unavailable_error());
                }
            };
            if let Err(error) = result {
                retain_runtime_after_parser_failure(&mut runtime.state, &error);
                return Err(supervisor_error(error));
            }
            let outcome = parse_admitted_symbol_job(
                job,
                &content,
                Some(projectatlas_core::symbols::ParserKind::TreeSitter),
                options,
                control,
            );
            let terminal = terminal_parse_outcome(&outcome);
            outcomes.push(outcome);
            if terminal {
                break;
            }
        }
        Ok(outcomes)
    })();
    group_lease.release();
    let cleanup = service_pending_deactivation(&mut runtime);
    combine_optional_operation_and_cleanup(operation, cleanup)
}

/// Quarantine one concrete execution owner only when the supervisor reports uncertain cleanup.
fn retain_runtime_after_parser_failure<T>(
    state: &mut OptionalParserRuntimeState<T>,
    error: &ParserSupervisorError,
) {
    if error.has_mandatory_cleanup_failure() {
        state.retain_after_cleanup_failure();
    }
}

/// Return the canonical optional grammar identity for one registry-owned job.
fn optional_language_id(job: &SymbolParseJob) -> Option<&str> {
    let capability = language_capability(job.language.as_deref()?)?;
    (capability.optional_pack == Some(BROAD_PARSER_PACK_ID)).then_some(capability.id)
}

/// Keep each grammar contiguous and paths stable inside its worker session.
fn sort_optional_jobs(jobs: &mut [(&str, &SymbolParseJob)]) {
    jobs.sort_by(|(left_language, left_job), (right_language, right_job)| {
        left_language
            .cmp(right_language)
            .then_with(|| left_job.path.cmp(&right_job.path))
    });
}

/// Parse non-optional jobs without changing the existing Rayon ownership.
fn parse_built_in_jobs(
    pool: &rayon::ThreadPool,
    jobs: &[&SymbolParseJob],
    options: &SymbolBuildOptions,
    control: &IndexWorkControl,
) -> Result<Vec<SymbolParseOutcome>, CliError> {
    let mut outcomes = Vec::with_capacity(jobs.len());
    for batch in jobs.chunks(BUILT_IN_PARSE_BATCH_SIZE) {
        control.check(IndexWorkStage::SymbolParsing)?;
        outcomes.extend(pool.install(|| {
            batch
                .par_iter()
                .map(|job| super::parse_symbol_job_controlled(job, options, control))
                .collect::<Vec<_>>()
        }));
    }
    Ok(outcomes)
}

/// Return whether later jobs must not start after this outcome.
fn terminal_parse_outcome(outcome: &SymbolParseOutcome) -> bool {
    matches!(
        outcome,
        SymbolParseOutcome::SourceChanged { .. }
            | SymbolParseOutcome::Io { .. }
            | SymbolParseOutcome::IndexWork(_)
    )
}

/// Acquire the one process-global owner without hiding cancellation behind a blocking lock.
fn lock_optional_runtime(
    control: &IndexWorkControl,
) -> Result<MutexGuard<'static, OptionalParserRuntime>, CliError> {
    let runtime = OPTIONAL_PARSER_RUNTIME.get_or_init(|| Mutex::new(OptionalParserRuntime::new()));
    lock_runtime(runtime, control)
}

/// Poll one concrete coordinator lock under the caller's cancellation and deadline.
fn lock_runtime<'a>(
    runtime: &'a Mutex<OptionalParserRuntime>,
    control: &IndexWorkControl,
) -> Result<MutexGuard<'a, OptionalParserRuntime>, CliError> {
    loop {
        control.check(IndexWorkStage::SymbolParsing)?;
        match runtime.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => thread::park_timeout(OPTIONAL_RUNTIME_ADMISSION_POLL),
            Err(TryLockError::Poisoned(poisoned)) => {
                let mut guard = poisoned.into_inner();
                if let Err(error) = guard.quarantine() {
                    return Err(supervisor_error(error));
                }
                return Err(runtime_unavailable_error());
            }
        }
    }
}

/// Request cleanup without making default-core wait behind an active optional group.
fn deactivate_if_initialized(control: &IndexWorkControl) -> Result<(), CliError> {
    let Some(runtime) = OPTIONAL_PARSER_RUNTIME.get() else {
        return Ok(());
    };
    OPTIONAL_PARSER_DEACTIVATION_REQUESTED.store(true, Ordering::Release);
    loop {
        if OPTIONAL_PARSER_GROUP_ACTIVE.load(Ordering::Acquire) {
            return Ok(());
        }
        control.check(IndexWorkStage::SymbolParsing)?;
        match runtime.try_lock() {
            Ok(mut guard) => return service_pending_deactivation(&mut guard),
            // A selected group that begins while this caller polls owns the pending request and
            // will service it before releasing its lease. Otherwise the lock release is imminent.
            Err(TryLockError::WouldBlock) => thread::park_timeout(OPTIONAL_RUNTIME_ADMISSION_POLL),
            Err(TryLockError::Poisoned(poisoned)) => {
                let mut guard = poisoned.into_inner();
                OPTIONAL_PARSER_DEACTIVATION_REQUESTED.store(false, Ordering::Release);
                return guard.quarantine().map_err(supervisor_error);
            }
        }
    }
}

/// Apply one pending cleanup transition while the caller owns the process-global state.
fn service_pending_deactivation(runtime: &mut OptionalParserRuntime) -> Result<(), CliError> {
    if !OPTIONAL_PARSER_DEACTIVATION_REQUESTED.swap(false, Ordering::AcqRel) {
        return Ok(());
    }
    runtime.deactivate().map_err(supervisor_error)
}

/// Revalidate the project-local selection immediately before optional source admission.
fn ensure_selection_current(
    lifecycle: &OptionalParserPackLifecycle,
    expected: &OptionalParserPackSelectionKey,
) -> Result<(), CliError> {
    let current = lifecycle.derive_project_selection()?;
    if current.selection_key() == Some(expected) {
        return Ok(());
    }
    Err(CliError::ParserPack(
        OptionalParserPackLifecycleError::InvalidData {
            reason: "optional parser selection changed before source transfer".to_owned(),
        },
    ))
}

/// Preserve both an operation failure and a mandatory process-cleanup failure.
fn combine_optional_operation_and_cleanup<T>(
    operation: Result<T, CliError>,
    cleanup: Result<(), CliError>,
) -> Result<T, CliError> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(operation), Ok(())) => Err(operation),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(operation), Err(cleanup)) => Err(CliError::OptionalParserOperationAndCleanup {
            operation: Box::new(operation),
            cleanup: Box::new(cleanup),
        }),
    }
}

/// Bound one worker request by both the operation deadline and the proven file ceiling.
fn optional_parse_deadline(control: &IndexWorkControl) -> Result<Instant, CliError> {
    control.check(IndexWorkStage::SymbolParsing)?;
    let now = Instant::now();
    let per_file = now.checked_add(OPTIONAL_PARSE_TIMEOUT).ok_or_else(|| {
        CliError::IndexWork(IndexWorkFailure::DeadlineExceeded {
            stage: IndexWorkStage::SymbolParsing,
        })
    })?;
    Ok(control
        .deadline()
        .map_or(per_file, |deadline| deadline.min(per_file)))
}

/// Preserve task cancellation and deadline state across the supervisor boundary.
fn supervisor_error(error: ParserSupervisorError) -> CliError {
    match error {
        ParserSupervisorError::Cancelled { .. } => {
            CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SymbolParsing,
            })
        }
        ParserSupervisorError::DeadlineExceeded { .. } => {
            CliError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::SymbolParsing,
            })
        }
        other => CliError::ParserPack(OptionalParserPackLifecycleError::Supervisor(other)),
    }
}

/// Construct the stable refusal used after cleanup uncertainty or lock poisoning.
fn runtime_unavailable_error() -> CliError {
    CliError::ParserPack(OptionalParserPackLifecycleError::InvalidData {
        reason: "optional parser runtime is unavailable until process restart".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn optional_jobs_sort_by_canonical_grammar_then_path() {
        let job = |path: &str, language: &str| SymbolParseJob {
            path: path.to_owned(),
            native_path: path.into(),
            expected_content_hash: "a".repeat(64),
            language: Some(language.to_owned()),
            fallback_summary: None,
            purpose_needs_suggestion: false,
        };
        let jobs = [
            job("z.zig", "zig"),
            job("z.awk", "awk"),
            job("a.zig", "zig"),
            job("a.awk", "awk"),
        ];
        let mut scheduled = [
            ("zig", &jobs[0]),
            ("awk", &jobs[1]),
            ("zig", &jobs[2]),
            ("awk", &jobs[3]),
        ];
        sort_optional_jobs(&mut scheduled);
        assert_eq!(
            scheduled
                .iter()
                .map(|(language, job)| (*language, job.path.as_str()))
                .collect::<Vec<_>>(),
            [
                ("awk", "a.awk"),
                ("awk", "z.awk"),
                ("zig", "a.zig"),
                ("zig", "z.zig"),
            ]
        );
    }

    #[test]
    fn cleanup_failure_quarantines_the_concrete_runtime_state() {
        let mut runtime = OptionalParserRuntime::new();
        runtime.state =
            OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained: None };
        assert!(runtime.deactivate().is_ok());
        assert!(matches!(
            runtime.state,
            OptionalParserRuntimeState::UnavailableAfterCleanupFailure { .. }
        ));
    }

    #[test]
    fn operation_and_cleanup_failures_remain_distinct_at_the_runtime_boundary() {
        let operation = CliError::ParserPack(OptionalParserPackLifecycleError::InvalidData {
            reason: "synthetic operation failure".to_owned(),
        });
        let cleanup = CliError::ParserPack(OptionalParserPackLifecycleError::Supervisor(
            ParserSupervisorError::Cleanup {
                message: "synthetic cleanup failure".to_owned(),
            },
        ));

        let result = combine_optional_operation_and_cleanup::<()>(Err(operation), Err(cleanup));
        assert!(matches!(
            result,
            Err(CliError::OptionalParserOperationAndCleanup { .. })
        ));
    }

    #[test]
    fn cleanup_failure_retains_execution_owner_until_runtime_state_drops() {
        struct DropProbe<'a>(&'a AtomicUsize);

        impl Drop for DropProbe<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drops = AtomicUsize::new(0);
        {
            let mut state = OptionalParserRuntimeState::Resident {
                verified: DropProbe(&drops),
            };
            state.retain_after_cleanup_failure();
            assert!(matches!(
                &state,
                OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained: Some(_) }
            ));
            assert_eq!(drops.load(Ordering::Relaxed), 0);
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn combined_parser_failure_quarantines_and_retains_the_execution_owner() {
        struct DropProbe<'a>(&'a AtomicUsize);

        impl Drop for DropProbe<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drops = AtomicUsize::new(0);
        let mut state = OptionalParserRuntimeState::Resident {
            verified: DropProbe(&drops),
        };
        let error = ParserSupervisorError::OperationAndCleanup {
            operation: Box::new(ParserSupervisorError::Cancelled { phase: "test" }),
            cleanup: Box::new(ParserSupervisorError::Cleanup {
                message: "synthetic cleanup failure".to_owned(),
            }),
        };

        retain_runtime_after_parser_failure(&mut state, &error);
        assert!(matches!(
            &state,
            OptionalParserRuntimeState::UnavailableAfterCleanupFailure { retained: Some(_) }
        ));
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(state);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pending_deactivation_is_consumed_under_the_runtime_lease() {
        OPTIONAL_PARSER_DEACTIVATION_REQUESTED.store(true, Ordering::Release);
        let mut runtime = OptionalParserRuntime::new();
        assert!(service_pending_deactivation(&mut runtime).is_ok());
        assert!(!OPTIONAL_PARSER_DEACTIVATION_REQUESTED.load(Ordering::Acquire));
        assert!(matches!(
            runtime.state,
            OptionalParserRuntimeState::Inactive
        ));
    }

    #[test]
    fn group_activity_is_cleared_when_the_lease_drops() {
        OPTIONAL_PARSER_GROUP_ACTIVE.store(false, Ordering::Release);
        {
            let _lease = OptionalParserGroupLease::acquire();
            assert!(OPTIONAL_PARSER_GROUP_ACTIVE.load(Ordering::Acquire));
        }
        assert!(!OPTIONAL_PARSER_GROUP_ACTIVE.load(Ordering::Acquire));
    }

    #[test]
    fn cancellation_interrupts_global_admission_wait() {
        let runtime = Mutex::new(OptionalParserRuntime::new());
        let _holder = runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let control = IndexWorkControl::new(projectatlas_core::IndexCancellation::new(), None);
        control.cancel();
        let result = lock_runtime(&runtime, &control);
        assert!(matches!(
            result,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SymbolParsing,
            }))
        ));
    }
}
