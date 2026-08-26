//! Target-local `SQLite` backup hydration for independently writable worktree atlases.

use crate::schema::sqlite_sidecar_path;
use crate::{
    AtlasStore, DbError, DbResult, IndexPublicationState, ProjectRootTransition, set_metadata,
    validate_database_location, verify_project_database,
};
use projectatlas_core::graph::ProjectInstanceId;
#[cfg(test)]
use projectatlas_core::normalize_native_path_display;
use projectatlas_core::{CanonicalProjectRoot, IndexGeneration, IndexWorkControl, IndexWorkStage};
use rusqlite::Connection;
use rusqlite::backup::{Backup, StepResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::{Builder, TempPath};

/// Number of pages copied per cooperative online-backup step.
const HYDRATION_BACKUP_PAGES_PER_STEP: i32 = 256;
/// Pause between transiently blocked online-backup steps.
const HYDRATION_BACKUP_BUSY_PAUSE: Duration = Duration::from_millis(1);
/// Consecutive busy/locked steps admitted before deterministic fallback.
const HYDRATION_BACKUP_BUSY_ATTEMPTS: usize = 5_000;
/// Private target-local candidate filename prefix.
const HYDRATION_CANDIDATE_PREFIX: &str = ".projectatlas-hydration-";
/// Durable diagnostic provenance keys retained only by the target atlas.
const HYDRATION_SOURCE_PROJECT_KEY: &str = "worktree_hydration_source_project_instance_id";
/// Complete source generation whose graph became the detached baseline.
const HYDRATION_SOURCE_GENERATION_KEY: &str = "worktree_hydration_source_generation";
/// Wall-clock epoch at which the private target candidate was prepared.
const HYDRATION_PREPARED_AT_KEY: &str = "worktree_hydration_prepared_at_epoch";

/// Disposable target-local database prepared from a consistent control-atlas backup.
#[derive(Debug)]
pub struct WorktreeHydrationCandidate {
    /// Auto-cleaned unpublished database path.
    path: Option<TempPath>,
    /// Exact no-clobber publication path.
    destination_database: PathBuf,
    /// Canonical root the copied database now owns.
    target_root: CanonicalProjectRoot,
    /// Source atlas identity retained only for diagnostics.
    source_project_instance_id: ProjectInstanceId,
    /// New target atlas identity.
    target_project_instance_id: ProjectInstanceId,
    /// Rebound baseline generation that normal reconciliation must supersede.
    baseline_generation: IndexGeneration,
    /// Caller-confirmed exact source verification when no publication was needed.
    source_state_verified: bool,
}

impl WorktreeHydrationCandidate {
    /// Borrow the unpublished candidate database path.
    ///
    /// # Errors
    ///
    /// Returns an error only after activation has consumed the private path.
    pub fn path(&self) -> DbResult<&Path> {
        self.path
            .as_deref()
            .ok_or(DbError::WorktreeHydrationInvalid {
                reason: "hydration candidate path was already consumed",
            })
    }

    /// Borrow the no-clobber activation destination.
    #[must_use]
    pub fn destination_database(&self) -> &Path {
        &self.destination_database
    }

    /// Return the source control-atlas identity captured by the backup.
    #[must_use]
    pub const fn source_project_instance_id(&self) -> ProjectInstanceId {
        self.source_project_instance_id
    }

    /// Return the new independently writable target-atlas identity.
    #[must_use]
    pub const fn target_project_instance_id(&self) -> ProjectInstanceId {
        self.target_project_instance_id
    }

    /// Return the generation containing the rebound source baseline.
    #[must_use]
    pub const fn baseline_generation(&self) -> IndexGeneration {
        self.baseline_generation
    }

    /// Accept an exact no-delta source verification from the normal runtime scanner.
    ///
    /// # Errors
    ///
    /// Returns an error when the candidate no longer exposes its complete rebound baseline or
    /// cancellation has been requested. Callers must invoke this only after comparing the exact
    /// target source and derivation contract with the candidate.
    pub fn accept_verified_source_state(&mut self, control: &IndexWorkControl) -> DbResult<()> {
        control.check(IndexWorkStage::Publication)?;
        let store = AtlasStore::open_for_project(self.path()?, self.target_root.as_path())?;
        let publication = store.index_publication()?;
        if !publication.as_ref().is_some_and(|publication| {
            publication.state == IndexPublicationState::Complete
                && publication.generation == self.baseline_generation
        }) {
            return Err(DbError::WorktreeHydrationNotReconciled {
                baseline: self.baseline_generation,
                found: publication.map_or(IndexGeneration::ZERO, |value| value.generation),
            });
        }
        self.source_state_verified = true;
        Ok(())
    }

    /// Verify post-backup reconciliation, checkpoint WAL, and prepare for publication.
    ///
    /// # Errors
    ///
    /// Returns an error while the candidate is incomplete, busy, canceled, or corrupt. Failure
    /// leaves the destination untouched and removes the unpublished candidate.
    pub fn prepare_activation(
        mut self,
        control: &IndexWorkControl,
    ) -> DbResult<PreparedWorktreeHydrationCandidate> {
        control.check(IndexWorkStage::Publication)?;
        let candidate_path = self.path()?.to_path_buf();
        let store = AtlasStore::open_for_project(&candidate_path, self.target_root.as_path())?;
        let publication = store.index_publication()?.filter(|publication| {
            publication.state == IndexPublicationState::Complete
                && (publication.generation > self.baseline_generation
                    || self.source_state_verified
                        && publication.generation == self.baseline_generation)
        });
        let found_generation = publication
            .as_ref()
            .map_or(IndexGeneration::ZERO, |publication| publication.generation);
        if publication.is_none() {
            return Err(DbError::WorktreeHydrationNotReconciled {
                baseline: self.baseline_generation,
                found: found_generation,
            });
        }
        let (busy, log_frames, checkpointed_frames) =
            store
                .connection
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;
        if busy != 0 || log_frames != checkpointed_frames {
            return Err(DbError::WorktreeHydrationInvalid {
                reason: "candidate WAL checkpoint remained busy or incomplete",
            });
        }
        drop(store);

        verify_project_database(&candidate_path, self.target_root.as_path())?;
        fs::OpenOptions::new()
            .write(true)
            .open(&candidate_path)
            .and_then(|file| file.sync_all())
            .map_err(|source| DbError::WorktreeHydrationIo {
                path: candidate_path.clone(),
                source,
            })?;
        remove_candidate_sidecars(&candidate_path)?;
        control.check(IndexWorkStage::Publication)?;

        let path = self.path.take().ok_or(DbError::WorktreeHydrationInvalid {
            reason: "hydration candidate path was already consumed",
        })?;
        Ok(PreparedWorktreeHydrationCandidate {
            path: Some(path),
            destination_database: self.destination_database.clone(),
            source_project_instance_id: self.source_project_instance_id,
            target_project_instance_id: self.target_project_instance_id,
            baseline_generation: self.baseline_generation,
            reconciled_generation: found_generation,
        })
    }

    /// Verify and publish this candidate without replacing an existing destination.
    ///
    /// This compatibility convenience performs both phases. Lifecycle-sensitive callers should
    /// call [`Self::prepare_activation`] before acquiring external writer exclusion, then publish
    /// the returned [`PreparedWorktreeHydrationCandidate`] inside that short critical section.
    ///
    /// # Errors
    ///
    /// Returns an error while preparation fails or another process creates the destination first.
    pub fn activate(self, control: &IndexWorkControl) -> DbResult<WorktreeHydrationActivation> {
        self.prepare_activation(control)?.activate(control)
    }
}

/// Fully verified target-local hydration candidate awaiting only no-clobber publication.
#[derive(Debug)]
pub struct PreparedWorktreeHydrationCandidate {
    /// Auto-cleaned unpublished database path.
    path: Option<TempPath>,
    /// Exact no-clobber publication path.
    destination_database: PathBuf,
    /// Source atlas identity retained only for diagnostics.
    source_project_instance_id: ProjectInstanceId,
    /// New target atlas identity.
    target_project_instance_id: ProjectInstanceId,
    /// Rebound baseline generation.
    baseline_generation: IndexGeneration,
    /// Complete reconciled generation verified before writer exclusion.
    reconciled_generation: IndexGeneration,
}

impl PreparedWorktreeHydrationCandidate {
    /// Publish the already verified candidate without replacing an existing destination.
    ///
    /// # Errors
    ///
    /// Returns an error on cancellation, a publication collision, or an I/O failure. Failure
    /// preserves an existing destination. A directory-sync failure leaves the newly published
    /// database intact but prevents callers from binding it.
    pub fn activate(mut self, control: &IndexWorkControl) -> DbResult<WorktreeHydrationActivation> {
        control.check(IndexWorkStage::Publication)?;

        let path = self.path.take().ok_or(DbError::WorktreeHydrationInvalid {
            reason: "hydration candidate path was already consumed",
        })?;
        path.persist_noclobber(&self.destination_database)
            .map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    DbError::WorktreeHydrationDestinationExists {
                        path: self.destination_database.clone(),
                    }
                } else {
                    DbError::WorktreeHydrationIo {
                        path: self.destination_database.clone(),
                        source: error.error,
                    }
                }
            })?;
        #[cfg(unix)]
        sync_activation_directory(&self.destination_database)?;

        Ok(WorktreeHydrationActivation {
            database: self.destination_database.clone(),
            source_project_instance_id: self.source_project_instance_id,
            target_project_instance_id: self.target_project_instance_id,
            baseline_generation: self.baseline_generation,
            reconciled_generation: self.reconciled_generation,
        })
    }
}

/// Make a newly published directory entry durable before external binding commits.
#[cfg(unix)]
fn sync_activation_directory(destination_database: &Path) -> DbResult<()> {
    let parent = destination_database
        .parent()
        .ok_or(DbError::WorktreeHydrationInvalid {
            reason: "hydration destination database has no parent",
        })?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| DbError::WorktreeHydrationIo {
            path: parent.to_path_buf(),
            source,
        })
}

impl Drop for WorktreeHydrationCandidate {
    fn drop(&mut self) {
        if let Some(path) = self.path.as_deref() {
            remove_candidate_sidecars_best_effort(path);
        }
    }
}

impl Drop for PreparedWorktreeHydrationCandidate {
    fn drop(&mut self) {
        if let Some(path) = self.path.as_deref() {
            remove_candidate_sidecars_best_effort(path);
        }
    }
}

/// Completed no-clobber activation of one exact reconciled worktree atlas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeHydrationActivation {
    /// Activated target database path.
    pub database: PathBuf,
    /// Control-atlas identity captured by the hydration baseline.
    pub source_project_instance_id: ProjectInstanceId,
    /// Independently writable target-atlas identity.
    pub target_project_instance_id: ProjectInstanceId,
    /// Generation created when the copied graph was rebound to the target.
    pub baseline_generation: IndexGeneration,
    /// Complete target generation published by normal source reconciliation.
    pub reconciled_generation: IndexGeneration,
}

impl AtlasStore {
    /// Prepare one unpublished target-local worktree database through `SQLite` online backup.
    ///
    /// The caller must first prove through structural Git evidence that source and target belong
    /// to the same repository. The returned candidate must be reconciled with the normal scan
    /// pipeline before [`WorktreeHydrationCandidate::activate`] will publish it.
    ///
    /// # Errors
    ///
    /// Returns an error for incompatible source state, unsafe target placement, an existing
    /// destination, cancellation/deadline, backup contention, invalid copied state, or I/O.
    pub fn prepare_worktree_hydration(
        &self,
        target_root: &Path,
        destination_database: &Path,
        control: &IndexWorkControl,
    ) -> DbResult<WorktreeHydrationCandidate> {
        control.check(IndexWorkStage::Publication)?;
        let source_root = self
            .project_root_identity()?
            .ok_or(DbError::ProjectRootIdentityMissing)?;
        let source_project_instance_id = self
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let source_database =
            self.database_path
                .as_deref()
                .ok_or(DbError::WorktreeHydrationInvalid {
                    reason: "hydration source is not a file-backed database",
                })?;
        if !source_database.exists() {
            return Err(DbError::WorktreeHydrationInvalid {
                reason: "hydration source database is missing",
            });
        }

        let target_root_identity = canonical_target_root(target_root)?;
        if target_root_identity == source_root {
            return Err(DbError::WorktreeHydrationInvalid {
                reason: "hydration target matches the source project root",
            });
        }
        let destination_database =
            validated_target_database(target_root_identity.as_path(), destination_database)?;
        if destination_database.exists() {
            return Err(DbError::WorktreeHydrationDestinationExists {
                path: destination_database,
            });
        }
        validate_database_location(&destination_database)?;
        let destination_parent =
            destination_database
                .parent()
                .ok_or(DbError::WorktreeHydrationInvalid {
                    reason: "hydration destination has no target-local parent",
                })?;
        let reservation = Builder::new()
            .prefix(HYDRATION_CANDIDATE_PREFIX)
            .suffix(".sqlite")
            .tempfile_in(destination_parent)
            .map_err(|source| DbError::WorktreeHydrationIo {
                path: destination_parent.to_path_buf(),
                source,
            })?;
        let candidate_path = reservation.into_temp_path();

        let mut capture = Connection::open(&candidate_path)?;
        copy_online_backup(&self.connection, &mut capture, control)?;
        drop(capture);

        let copied = AtlasStore::open_for_project(&candidate_path, source_root.as_path())?;
        let snapshot = copied.export_derived_graph_snapshot_from_stable_copy()?;
        drop(copied);
        control.check(IndexWorkStage::Publication)?;

        let transition = AtlasStore::transition_project_root(
            &candidate_path,
            target_root_identity.as_path(),
            ProjectRootTransition::Detach,
        )?;
        let mut target =
            AtlasStore::open_for_project(&candidate_path, target_root_identity.as_path())?;
        clear_nontransferable_state(
            &target,
            source_project_instance_id,
            snapshot.metadata().source_generation,
        )?;
        let baseline = target.import_worktree_hydration_snapshot(&snapshot)?;
        if baseline.previous_generation != IndexGeneration::ZERO {
            return Err(DbError::WorktreeHydrationInvalid {
                reason: "hydration baseline did not start from generation zero",
            });
        }
        drop(target);
        verify_project_database(&candidate_path, target_root_identity.as_path())?;

        Ok(WorktreeHydrationCandidate {
            path: Some(candidate_path),
            destination_database,
            target_root: target_root_identity,
            source_project_instance_id,
            target_project_instance_id: transition.project_instance_id,
            baseline_generation: baseline.published_generation,
            source_state_verified: false,
        })
    }
}

/// Copy a consistent live source in bounded cooperative pages.
fn copy_online_backup(
    source: &Connection,
    destination: &mut Connection,
    control: &IndexWorkControl,
) -> DbResult<()> {
    let backup = Backup::new(source, destination)?;
    let mut busy_attempts = 0usize;
    loop {
        control.check(IndexWorkStage::Publication)?;
        match backup.step(HYDRATION_BACKUP_PAGES_PER_STEP)? {
            StepResult::Done => return Ok(()),
            StepResult::More => busy_attempts = 0,
            StepResult::Busy | StepResult::Locked => {
                busy_attempts = busy_attempts.saturating_add(1);
                if busy_attempts > HYDRATION_BACKUP_BUSY_ATTEMPTS {
                    return Err(DbError::WorktreeHydrationBackupBusy {
                        attempts: busy_attempts,
                    });
                }
                thread::sleep(HYDRATION_BACKUP_BUSY_PAUSE);
            }
            _ => {
                return Err(DbError::WorktreeHydrationInvalid {
                    reason: "SQLite returned an unsupported online-backup state",
                });
            }
        }
    }
}

/// Canonicalize and require one existing target directory.
fn canonical_target_root(target_root: &Path) -> DbResult<CanonicalProjectRoot> {
    if !target_root.is_absolute() {
        return Err(DbError::WorktreeHydrationInvalid {
            reason: "hydration target root is not absolute",
        });
    }
    CanonicalProjectRoot::from_path(target_root).map_err(DbError::from)
}

/// Normalize an absent database destination below the exact canonical target root.
fn validated_target_database(target_root: &Path, destination_database: &Path) -> DbResult<PathBuf> {
    if !destination_database.is_absolute() {
        return Err(DbError::WorktreeHydrationInvalid {
            reason: "hydration destination database is not absolute",
        });
    }
    let parent = destination_database
        .parent()
        .ok_or(DbError::WorktreeHydrationInvalid {
            reason: "hydration destination database has no parent",
        })?;
    let parent = fs::canonicalize(parent).map_err(|source| DbError::WorktreeHydrationIo {
        path: parent.to_path_buf(),
        source,
    })?;
    let target_identity = CanonicalProjectRoot::from_path(target_root)?;
    let parent_identity = CanonicalProjectRoot::from_path(&parent)?;
    if parent_identity == target_identity
        || !parent_identity
            .as_path()
            .starts_with(target_identity.as_path())
    {
        return Err(DbError::WorktreeHydrationInvalid {
            reason: "hydration destination is not inside a target-local subdirectory",
        });
    }
    let file_name = destination_database
        .file_name()
        .ok_or(DbError::WorktreeHydrationInvalid {
            reason: "hydration destination database has no file name",
        })?;
    Ok(parent.join(file_name))
}

/// Remove source telemetry, control registry, and transient health state in one target write.
fn clear_nontransferable_state(
    target: &AtlasStore,
    source_project: ProjectInstanceId,
    source_generation: IndexGeneration,
) -> DbResult<()> {
    let prepared_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_source| DbError::WorktreeHydrationInvalid {
            reason: "system clock precedes the Unix epoch",
        })?
        .as_secs();
    target.with_validated_write(|connection| {
        connection.execute_batch(
            "DELETE FROM usage_instance_worktree_origins;
             DELETE FROM worktree_usage_aggregates;
             DELETE FROM worktree_registrations;
             DELETE FROM usage_aggregate_revisions;
             DELETE FROM health_resolutions;",
        )?;
        crate::telemetry::reset_usage_storage_for_hydration(connection)?;
        set_metadata(
            connection,
            HYDRATION_SOURCE_PROJECT_KEY,
            &source_project.to_string(),
        )?;
        set_metadata(
            connection,
            HYDRATION_SOURCE_GENERATION_KEY,
            &source_generation.get().to_string(),
        )?;
        set_metadata(
            connection,
            HYDRATION_PREPARED_AT_KEY,
            &prepared_at.to_string(),
        )?;
        Ok(())
    })
}

/// Remove checkpointed private sidecars before no-clobber publication.
fn remove_candidate_sidecars(path: &Path) -> DbResult<()> {
    for sidecar in [
        sqlite_sidecar_path(path, "-wal"),
        sqlite_sidecar_path(path, "-shm"),
        sqlite_sidecar_path(path, "-journal"),
    ] {
        match fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(DbError::WorktreeHydrationIo {
                    path: sidecar,
                    source,
                });
            }
        }
    }
    Ok(())
}

/// Best-effort cleanup for an unpublished candidate's transient `SQLite` sidecars.
fn remove_candidate_sidecars_best_effort(path: &Path) {
    for sidecar in [
        sqlite_sidecar_path(path, "-wal"),
        sqlite_sidecar_path(path, "-shm"),
        sqlite_sidecar_path(path, "-journal"),
    ] {
        let _ignored = fs::remove_file(sidecar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorktreeAlias, WorktreeRegistrationState};
    use projectatlas_core::IndexCancellation;
    use rusqlite::params;
    use std::error::Error;
    use std::io;

    /// Return a typed test failure without panic-only assertions.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Seed one complete control atlas with authored state and private control data.
    fn seed_source(store: &mut AtlasStore, target_root: &Path) -> Result<(), Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("source project identity missing"))?;
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind) VALUES('.', 'folder');
             INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                SELECT id, 'Own the repository.', 'agent', 'approved', 'agent'
                  FROM nodes WHERE path = '.';
             INSERT INTO summaries(node_id, summary_level, subject, summary)
                SELECT id, 'node', '', 'Repository summary.' FROM nodes WHERE path = '.';
             INSERT INTO health_resolutions(
                finding_id, category, path, rationale, resolved_by
             ) VALUES('hydration-health', 'test', '.', 'source-only state', 'agent');
             INSERT INTO usage_bucket_dimensions(
                token_savings_bucket, provider, model, tokenizer_backend,
                accuracy, baseline_kind, confidence, accounting_layer, estimate_method,
                denominator_kind, dedupe_scope, overflow
             ) VALUES(
                'navigation_avoidance', 'heuristic', 'unknown', 'chars_div_4',
                'heuristic_estimate', 'selected_candidates', 'inferred',
                'modeled_avoidance', 'heuristic_chars_or_bytes_div_ceil_4',
                'selected_candidates', 'session', 0
             );",
        )?;
        let dimension_id = store.connection.last_insert_rowid();
        store.connection.execute(
            "INSERT INTO usage_global_aggregates(
                 project_instance_id, dimension_id, calls, estimated_without,
                 estimated_with, modeled_without, modeled_with,
                 deduped_modeled_without, deduped_modeled_with
             ) VALUES(?1, ?2, 1, 100, 10, 100, 10, 100, 10)",
            params![project.as_bytes().as_slice(), dimension_id],
        )?;
        store.connection.execute(
            "INSERT INTO usage_aggregate_revisions(project_instance_id, revision)
             VALUES(?1, 1)",
            [project.as_bytes().as_slice()],
        )?;
        let alias = WorktreeAlias::parse("seeded")?;
        let source_root = PathBuf::from(
            store
                .project_root()?
                .ok_or_else(|| io::Error::other("source root missing"))?,
        );
        store.register_worktree(
            &alias,
            &source_root.join(".git"),
            &source_root.join(".git/worktrees/seeded"),
            &"22".repeat(32),
            target_root,
            None,
            1,
        )?;
        let mut publication = store.begin_index_publication("hydration-test")?;
        publication.replace_repository_graph(project, &[], &[], &[], &[])?;
        publication.complete()?;
        Ok(())
    }

    /// Online backup hydration preserves authored baseline state and clears private authority.
    #[test]
    fn hydration_rebinds_reconciles_and_activates_without_private_state_or_clobber()
    -> Result<(), Box<dyn Error>> {
        let fixture = tempfile::tempdir()?;
        let source_root = fixture.path().join("source");
        let target_root = fixture.path().join("target");
        let source_dir = source_root.join(".projectatlas");
        let target_dir = target_root.join(".projectatlas");
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&target_dir)?;
        let source_database = source_dir.join("projectatlas.db");
        let destination_database = target_dir.join("projectatlas.db");
        let mut source = AtlasStore::open_for_project(&source_database, &source_root)?;
        seed_source(&mut source, &target_root)?;
        let source_identity = source
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("source identity missing after seed"))?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let unreconciled =
            source.prepare_worktree_hydration(&target_root, &destination_database, &control)?;
        let unreconciled_path = unreconciled.path()?.to_path_buf();
        let error = match unreconciled.activate(&control) {
            Ok(_activation) => {
                return Err(io::Error::other("unreconciled candidate activated").into());
            }
            Err(error) => error,
        };
        require(
            matches!(error, DbError::WorktreeHydrationNotReconciled { .. }),
            "unreconciled activation returned the wrong typed failure",
        )?;
        require(
            !unreconciled_path.exists() && !destination_database.exists(),
            "unreconciled activation left a candidate or destination",
        )?;

        let verified_root = fixture.path().join("verified-target");
        let verified_dir = verified_root.join(".projectatlas");
        fs::create_dir_all(&verified_dir)?;
        let verified_database = verified_dir.join("projectatlas.db");
        let mut verified =
            source.prepare_worktree_hydration(&verified_root, &verified_database, &control)?;
        let verified_baseline = verified.baseline_generation();
        verified.accept_verified_source_state(&control)?;
        let verified_activation = verified.activate(&control)?;
        require(
            verified_activation.reconciled_generation == verified_baseline
                && verified_database.exists(),
            "exact no-delta source verification did not activate the copied baseline",
        )?;

        let candidate =
            source.prepare_worktree_hydration(&target_root, &destination_database, &control)?;
        let candidate_path = candidate.path()?.to_path_buf();
        let baseline_generation = candidate.baseline_generation();
        let target_identity = candidate.target_project_instance_id();
        require(
            target_identity != source_identity,
            "hydration did not rotate the target identity",
        )?;
        {
            let mut target = AtlasStore::open_for_project(&candidate_path, &target_root)?;
            let copied = target.connection.query_row(
                "SELECT
                    (SELECT purpose FROM purposes JOIN nodes ON nodes.id = purposes.node_id
                       WHERE nodes.path = '.'),
                    (SELECT summary FROM summaries JOIN nodes ON nodes.id = summaries.node_id
                       WHERE nodes.path = '.'),
                    (SELECT COUNT(*) FROM usage_global_aggregates),
                    (SELECT COUNT(*) FROM worktree_registrations),
                    (SELECT COUNT(*) FROM health_resolutions)",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            require(
                copied
                    == (
                        "Own the repository.".to_string(),
                        "Repository summary.".to_string(),
                        0,
                        0,
                        0,
                    ),
                "hydration did not preserve authored state or clear private state",
            )?;
            let provenance = target.connection.query_row(
                "SELECT source.value, generation.value
                   FROM metadata AS source
                   JOIN metadata AS generation
                     ON generation.key = ?2
                  WHERE source.key = ?1",
                params![
                    HYDRATION_SOURCE_PROJECT_KEY,
                    HYDRATION_SOURCE_GENERATION_KEY
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            require(
                provenance.0 == source_identity.to_string() && provenance.1 == "1",
                "hydration provenance does not identify the source baseline",
            )?;
            let mut publication = target.begin_index_projection_refresh("hydration-test")?;
            publication.replace_repository_graph(target_identity, &[], &[], &[], &[])?;
            publication.complete()?;
        }
        let prepared = candidate.prepare_activation(&control)?;
        require(
            candidate_path.exists() && !destination_database.exists(),
            "activation preparation published the candidate early",
        )?;
        let activation = prepared.activate(&control)?;
        require(
            normalize_native_path_display(&activation.database)
                == normalize_native_path_display(&destination_database)
                && activation.baseline_generation == baseline_generation
                && activation.reconciled_generation > baseline_generation
                && activation.target_project_instance_id == target_identity,
            "activation report lost exact hydration identities",
        )?;
        require(
            destination_database.exists() && !candidate_path.exists(),
            "activation did not publish exactly one database path",
        )?;

        let raced_root = fixture.path().join("raced-target");
        let raced_dir = raced_root.join(".projectatlas");
        let raced_database = raced_dir.join("projectatlas.db");
        fs::create_dir_all(&raced_dir)?;
        let mut raced =
            source.prepare_worktree_hydration(&raced_root, &raced_database, &control)?;
        let raced_candidate = raced.path()?.to_path_buf();
        raced.accept_verified_source_state(&control)?;
        fs::write(&raced_database, b"competing initializer")?;
        let raced = raced.prepare_activation(&control)?;
        require(
            matches!(
                raced.activate(&control),
                Err(DbError::WorktreeHydrationDestinationExists { .. })
            ),
            "activation collision did not retain the destination-exists fallback",
        )?;
        require(
            fs::read(&raced_database)? == b"competing initializer" && !raced_candidate.exists(),
            "activation collision changed the winning destination or retained its candidate",
        )?;

        let activated = AtlasStore::open_for_project(&destination_database, &target_root)?;
        require(
            activated.project_instance_id()? == Some(target_identity),
            "activated database identity changed",
        )?;
        let source_private = source.connection.query_row(
            "SELECT
                (SELECT COUNT(*) FROM usage_global_aggregates),
                (SELECT COUNT(*) FROM worktree_registrations WHERE state = ?1)",
            [WorktreeRegistrationState::Active.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        require(
            source_private == (1, 1),
            "hydration mutated source telemetry or registration authority",
        )?;

        let occupied =
            source.prepare_worktree_hydration(&target_root, &destination_database, &control);
        require(
            matches!(
                occupied,
                Err(DbError::WorktreeHydrationDestinationExists { .. })
            ),
            "existing destination was not preserved through a typed no-clobber failure",
        )?;

        let cancelled = IndexWorkControl::new(IndexCancellation::new(), None);
        cancelled.cancel();
        let canceled_result = source.prepare_worktree_hydration(
            &target_root,
            &target_dir.join("cancelled.db"),
            &cancelled,
        );
        require(
            matches!(canceled_result, Err(DbError::IndexWork(_))),
            "canceled hydration did not return the shared typed work failure",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn hydration_preserves_non_utf8_target_identity_and_display_collisions()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStringExt;

        let fixture = tempfile::tempdir()?;
        let source_root = fixture
            .path()
            .join(std::ffi::OsString::from_vec(vec![b's', b'r', b'c', 0x80]));
        let target_root = fixture
            .path()
            .join(std::ffi::OsString::from_vec(vec![b't', b'g', b't', 0x81]));
        let collision_root = fixture.path().join("tgt-�");
        let source_dir = source_root.join(".projectatlas");
        let target_dir = target_root.join(".projectatlas");
        let collision_dir = collision_root.join(".projectatlas");
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&target_dir)?;
        fs::create_dir_all(&collision_dir)?;
        let source_database = source_dir.join("projectatlas.db");
        let target_database = target_dir.join("projectatlas.db");
        let collision_database = collision_dir.join("projectatlas.db");
        let mut source = AtlasStore::open_for_project(&source_database, &source_root)?;
        seed_source(&mut source, &target_root)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);

        let mut candidate =
            source.prepare_worktree_hydration(&target_root, &target_database, &control)?;
        let target_identity = candidate.target_project_instance_id();
        candidate.accept_verified_source_state(&control)?;
        let prepared = candidate.prepare_activation(&control)?;
        let activation = prepared.activate(&control)?;
        require(
            activation.target_project_instance_id == target_identity,
            "non-UTF-8 hydration changed target identity",
        )?;
        verify_project_database(&target_database, &target_root)?;
        let target = AtlasStore::open_read_only_for_project(&target_database, &target_root)?;
        require(
            target.project_root_identity()? == Some(CanonicalProjectRoot::from_path(&target_root)?),
            "hydrated non-UTF-8 target identity was not persisted",
        )?;
        drop(target);

        let mut collision =
            source.prepare_worktree_hydration(&collision_root, &collision_database, &control)?;
        let collision_identity = collision.target_project_instance_id();
        require(
            collision_identity != target_identity,
            "hydration collapsed replacement-character target identity",
        )?;
        collision.accept_verified_source_state(&control)?;
        let collision = collision.prepare_activation(&control)?;
        collision.activate(&control)?;
        verify_project_database(&collision_database, &collision_root)?;
        Ok(())
    }
}
