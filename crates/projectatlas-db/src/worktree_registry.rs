//! Durable `ProjectAtlas` worktree registrations owned by one control atlas.

use crate::{
    AtlasStore, DbError, DbResult, WorktreeUsageSnapshot, WorktreeUsageSyncState, telemetry,
};
use projectatlas_core::{
    CanonicalProjectRoot, MAX_GIT_WORKTREE_REGISTRATIONS, graph::ProjectInstanceId,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::fmt;
use std::path::Path;

/// Reserved alias for the selected control checkout.
pub const MAIN_WORKTREE_ALIAS: &str = "main";
/// Maximum bytes admitted for one worktree alias.
pub const MAX_WORKTREE_ALIAS_BYTES: usize = 64;
/// Maximum normalized bytes stored for one worktree identity path.
const MAX_WORKTREE_REGISTRATION_PATH_BYTES: usize = 128 * 1_024;
/// Lowercase hexadecimal bytes in one opaque Git administrative identity.
const GIT_ADMINISTRATIVE_IDENTITY_BYTES: usize = 64;

/// Validated short selector for one registered non-control worktree.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorktreeAlias(String);

impl WorktreeAlias {
    /// Validate one caller-supplied worktree alias.
    ///
    /// # Errors
    ///
    /// Returns an error for the reserved `main` alias, excessive length, or
    /// characters outside lowercase ASCII letters, digits, `.`, `_`, and `-`.
    pub fn parse(value: &str) -> DbResult<Self> {
        if value.is_empty() {
            return invalid_alias(value, "alias is empty");
        }
        if value.len() > MAX_WORKTREE_ALIAS_BYTES {
            return invalid_alias(value, "alias exceeds 64 UTF-8 bytes");
        }
        if value == MAIN_WORKTREE_ALIAS {
            return invalid_alias(value, "main is reserved for the control atlas");
        }
        let mut bytes = value.bytes();
        let first = bytes.next().ok_or_else(|| DbError::InvalidWorktreeAlias {
            alias: value.to_string(),
            reason: "alias is empty",
        })?;
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return invalid_alias(value, "alias must start with a lowercase letter or digit");
        }
        if !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        }) {
            return invalid_alias(value, "alias contains unsupported characters");
        }
        Ok(Self(value.to_string()))
    }

    /// Borrow the normalized serialized alias.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorktreeAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Durable registration lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeRegistrationState {
    /// The alias may resolve source operations.
    Active,
    /// The alias no longer resolves, while historical aggregate state remains.
    Retired,
}

impl WorktreeRegistrationState {
    /// Return the stable `SQLite` representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
        }
    }

    /// Parse a stable `SQLite` representation.
    fn parse(value: &str) -> DbResult<Self> {
        match value {
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            _ => Err(DbError::WorktreeRegistrationRow {
                reason: "unknown registration state",
            }),
        }
    }
}

/// One active or retired `ProjectAtlas` worktree registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRegistration {
    /// Stable control-database row identity.
    pub registration_id: i64,
    /// Human/agent-facing short selector.
    pub alias: WorktreeAlias,
    /// Whether the selector remains active.
    pub state: WorktreeRegistrationState,
    /// UTF-8 compatibility projection of the Git common directory. The native
    /// identity field beside it is authoritative for routing and persistence.
    pub git_common_directory: String,
    /// Lossless native identity authority for the Git common directory.
    pub git_common_directory_identity: CanonicalProjectRoot,
    /// UTF-8 compatibility projection of the administrative directory. The
    /// native identity field beside it is authoritative.
    pub git_administrative_directory: String,
    /// Lossless native identity authority for the administrative directory.
    pub git_administrative_directory_identity: CanonicalProjectRoot,
    /// Opaque identity for the current administrative-directory lifecycle.
    pub git_administrative_identity: String,
    /// UTF-8 compatibility projection of the last source root. The native
    /// identity field beside it is authoritative.
    pub last_root: String,
    /// Lossless native identity authority for the source root.
    pub last_root_identity: CanonicalProjectRoot,
    /// Exact worktree atlas identity after initialization.
    pub project_instance_id: Option<ProjectInstanceId>,
    /// Last local aggregate revision accepted by the control atlas.
    pub accepted_telemetry_revision: u64,
    /// Creation time as Unix epoch seconds.
    pub created_at_epoch: u64,
    /// Retirement time as Unix epoch seconds.
    pub retired_at_epoch: Option<u64>,
}

/// Transaction-owned capability for one exact active worktree registration.
///
/// Construction is restricted to [`AtlasStore::with_active_worktree_registration`]
/// so lifecycle-sensitive callers can validate external state and publish one
/// bind or retirement under the same control-catalog writer exclusion.
pub struct ActiveWorktreeRegistrationGuard<'transaction> {
    /// Connection currently owned by the outer validated write transaction.
    connection: &'transaction Connection,
    /// Exact active row reloaded after writer exclusion was acquired.
    registration: WorktreeRegistration,
}

impl ActiveWorktreeRegistrationGuard<'_> {
    /// Borrow the exact active row reloaded by the transaction.
    #[must_use]
    pub const fn registration(&self) -> &WorktreeRegistration {
        &self.registration
    }

    /// Bind the exact initialized project inside this transaction.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, conflicting project identity,
    /// malformed persisted state, or any `SQLite` failure.
    pub fn bind_project(
        &mut self,
        root: &Path,
        project_instance_id: ProjectInstanceId,
    ) -> DbResult<WorktreeRegistration> {
        let root = worktree_identity("root", root)?;
        let bound = bind_registration_project(
            self.connection,
            &self.registration,
            &root,
            project_instance_id,
        )?;
        self.registration = bound.clone();
        Ok(bound)
    }

    /// Bind this project and accept its initial usage snapshot atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, conflicting project or telemetry
    /// identity, malformed or excessive snapshot state, or any `SQLite` failure.
    pub fn bind_project_with_usage_snapshot(
        &mut self,
        root: &Path,
        project_instance_id: ProjectInstanceId,
        snapshot: &WorktreeUsageSnapshot,
    ) -> DbResult<(WorktreeRegistration, WorktreeUsageSyncState)> {
        let root = worktree_identity("root", root)?;
        let bound = bind_registration_project(
            self.connection,
            &self.registration,
            &root,
            project_instance_id,
        )?;
        let synchronized = telemetry::synchronize_worktree_usage_snapshot(
            self.connection,
            bound.registration_id,
            snapshot,
        )?;
        let bound = load_by_id(self.connection, bound.registration_id)?;
        self.registration = bound.clone();
        Ok((bound, synchronized))
    }

    /// Accept one usage snapshot for this already-bound active registration.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or mismatched project identity, malformed
    /// or excessive snapshot state, or any `SQLite` failure.
    pub fn synchronize_usage_snapshot(
        &mut self,
        snapshot: &WorktreeUsageSnapshot,
    ) -> DbResult<WorktreeUsageSyncState> {
        let synchronized = telemetry::synchronize_worktree_usage_snapshot(
            self.connection,
            self.registration.registration_id,
            snapshot,
        )?;
        self.registration = load_by_id(self.connection, self.registration.registration_id)?;
        Ok(synchronized)
    }

    /// Retire this active registration without importing another local snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid time, malformed persisted state, or any
    /// `SQLite` failure.
    pub fn retire(&mut self, retired_at_epoch: u64) -> DbResult<WorktreeRegistration> {
        let retired = retire_registration(
            self.connection,
            &self.registration,
            epoch_to_sqlite(retired_at_epoch)?,
        )?;
        self.registration = retired.clone();
        Ok(retired)
    }

    /// Bind, synchronize one writer-excluded local snapshot, and retire atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths or times, a mismatched project or
    /// telemetry identity, malformed persisted state, or any `SQLite` failure.
    pub fn retire_with_usage_snapshot(
        &mut self,
        root: &Path,
        project_instance_id: ProjectInstanceId,
        snapshot: &WorktreeUsageSnapshot,
        retired_at_epoch: u64,
    ) -> DbResult<(WorktreeRegistration, WorktreeUsageSyncState)> {
        let retired_at_epoch = epoch_to_sqlite(retired_at_epoch)?;
        let (bound, synchronized) =
            self.bind_project_with_usage_snapshot(root, project_instance_id, snapshot)?;
        let retired = retire_registration(self.connection, &bound, retired_at_epoch)?;
        self.registration = retired.clone();
        Ok((retired, synchronized))
    }
}

/// Raw row retained until all typed conversions succeed.
struct PersistedWorktreeRegistration {
    /// Stable row identity.
    registration_id: i64,
    /// Persisted alias text.
    alias: String,
    /// Persisted lifecycle state.
    state: String,
    /// Persisted normalized common-directory path.
    git_common_directory: String,
    /// Persisted lossless common-directory codec bytes.
    git_common_directory_identity: Vec<u8>,
    /// Persisted normalized administrative-directory path.
    git_administrative_directory: String,
    /// Persisted lossless administrative-directory codec bytes.
    git_administrative_directory_identity: Vec<u8>,
    /// Persisted opaque administrative-directory lifecycle identity.
    git_administrative_identity: String,
    /// Persisted last structurally validated root.
    last_root: String,
    /// Persisted lossless source-root codec bytes.
    last_root_identity: Vec<u8>,
    /// Optional exact initialized atlas identity bytes.
    project_instance_id: Option<Vec<u8>>,
    /// Last accepted local aggregate revision.
    accepted_telemetry_revision: i64,
    /// Creation epoch seconds.
    created_at_epoch: i64,
    /// Optional retirement epoch seconds.
    retired_at_epoch: Option<i64>,
}

impl AtlasStore {
    /// Register one structurally validated non-control Git worktree.
    ///
    /// A matching retired row is reactivated only when its administrative and
    /// project identities still agree. Historical rows for a replaced atlas
    /// remain retired so their aggregate telemetry cannot be relabelled.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths/times, active alias or identity
    /// conflicts, malformed persisted state, or any transactional `SQLite`
    /// failure.
    #[allow(clippy::too_many_arguments)]
    pub fn register_worktree(
        &self,
        alias: &WorktreeAlias,
        git_common_directory: &Path,
        git_administrative_directory: &Path,
        git_administrative_identity: &str,
        root: &Path,
        project_instance_id: Option<ProjectInstanceId>,
        created_at_epoch: u64,
    ) -> DbResult<WorktreeRegistration> {
        let git_common_directory_identity =
            worktree_identity("git_common_directory", git_common_directory)?;
        let git_administrative_directory_identity =
            worktree_identity("git_administrative_directory", git_administrative_directory)?;
        let git_administrative_identity =
            validated_administrative_identity(git_administrative_identity)?;
        let root_identity = worktree_identity("root", root)?;
        let git_common_directory = native_path_projection(&git_common_directory_identity)?;
        let git_administrative_directory =
            native_path_projection(&git_administrative_directory_identity)?;
        let root = native_path_projection(&root_identity)?;
        let created_at_epoch = epoch_to_sqlite(created_at_epoch)?;
        let project_bytes = project_instance_id.map(ProjectInstanceId::as_bytes);
        let common_identity_bytes = git_common_directory_identity.encode()?;
        let administrative_identity_bytes = git_administrative_directory_identity.encode()?;
        let root_identity_bytes = root_identity.encode()?;

        self.with_validated_write(|transaction| {
            if let Some(existing) = load_active_by_alias(transaction, alias.as_str())? {
                if existing.git_administrative_directory_identity
                    != git_administrative_directory_identity
                {
                    return Err(DbError::WorktreeRegistrationConflict {
                        field: "alias",
                        value: alias.to_string(),
                    });
                }
                if existing.git_administrative_identity != git_administrative_identity {
                    return Err(DbError::WorktreeRegistrationConflict {
                        field: "git_administrative_identity",
                        value: git_administrative_identity,
                    });
                }
                if identities_conflict(existing.project_instance_id, project_instance_id) {
                    return Err(DbError::WorktreeRegistrationConflict {
                        field: "project_instance_id",
                        value: project_instance_id
                            .map_or_else(String::new, |value| value.to_string()),
                    });
                }
                if let Some(project_bytes) = project_bytes.as_ref()
                    && project_identity_exists_for_other(
                        transaction,
                        Some(existing.registration_id),
                        project_bytes.as_slice(),
                    )?
                {
                    return Err(DbError::WorktreeRegistrationConflict {
                        field: "project_instance_id",
                        value: project_instance_id
                            .map_or_else(String::new, |value| value.to_string()),
                    });
                }
                if native_root_identity_exists_for_other(
                    transaction,
                    Some(existing.registration_id),
                    root_identity_bytes.as_slice(),
                )? {
                    return Err(DbError::WorktreeRegistrationConflict {
                        field: "root",
                        value: root,
                    });
                }
                transaction.execute(
                    "UPDATE worktree_registrations
                 SET git_common_directory = ?1, git_common_directory_identity = ?2,
                     last_root = ?3, last_root_identity = ?4,
                     project_instance_id = COALESCE(project_instance_id, ?5)
                 WHERE registration_id = ?6",
                    params![
                        git_common_directory,
                        common_identity_bytes.as_slice(),
                        root,
                        root_identity_bytes.as_slice(),
                        project_bytes.as_ref().map(<[u8; 16]>::as_slice),
                        existing.registration_id,
                    ],
                )?;
                return load_by_id(transaction, existing.registration_id);
            }

            let retired_id = load_matching_retired_id(
                transaction,
                &administrative_identity_bytes,
                &git_administrative_identity,
                project_bytes.as_ref(),
            )?;
            if active_git_identity_exists(
                transaction,
                &administrative_identity_bytes,
                &git_administrative_identity,
            )? {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "git_or_project_identity",
                    value: git_administrative_directory,
                });
            }
            if let Some(project_bytes) = project_bytes.as_ref()
                && project_identity_exists_for_other(
                    transaction,
                    retired_id,
                    project_bytes.as_slice(),
                )?
            {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    value: project_instance_id.map_or_else(String::new, |value| value.to_string()),
                });
            }
            if native_root_identity_exists_for_other(
                transaction,
                retired_id,
                root_identity_bytes.as_slice(),
            )? {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "root",
                    value: root,
                });
            }
            let registration_id = if let Some(registration_id) = retired_id {
                transaction.execute(
                    "UPDATE worktree_registrations
                 SET alias = ?1, state = 'active', git_common_directory = ?2,
                     git_common_directory_identity = ?3,
                     git_administrative_directory = ?4,
                     git_administrative_directory_identity = ?5,
                     git_administrative_identity = ?6, last_root = ?7,
                     last_root_identity = ?8, project_instance_id = ?9,
                     retired_at_epoch = NULL
                 WHERE registration_id = ?10",
                    params![
                        alias.as_str(),
                        git_common_directory,
                        common_identity_bytes.as_slice(),
                        git_administrative_directory,
                        administrative_identity_bytes.as_slice(),
                        git_administrative_identity,
                        root,
                        root_identity_bytes.as_slice(),
                        project_bytes.as_ref().map(<[u8; 16]>::as_slice),
                        registration_id,
                    ],
                )?;
                registration_id
            } else {
                let registration_count = transaction.query_row(
                    "SELECT COUNT(*) FROM worktree_registrations",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                if usize::try_from(registration_count).map_err(|_source| {
                    DbError::WorktreeRegistrationRow {
                        reason: "negative registration count",
                    }
                })? >= MAX_GIT_WORKTREE_REGISTRATIONS
                {
                    return Err(DbError::WorktreeRegistrationCapacity {
                        limit: MAX_GIT_WORKTREE_REGISTRATIONS,
                    });
                }
                transaction.execute(
                    "INSERT INTO worktree_registrations(
                    alias, state, git_common_directory, git_common_directory_identity,
                    git_administrative_directory, git_administrative_directory_identity,
                    git_administrative_identity, last_root, last_root_identity,
                    project_instance_id, created_at_epoch
                 ) VALUES(?1, 'active', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        alias.as_str(),
                        git_common_directory,
                        common_identity_bytes.as_slice(),
                        git_administrative_directory,
                        administrative_identity_bytes.as_slice(),
                        git_administrative_identity,
                        root,
                        root_identity_bytes.as_slice(),
                        project_bytes.as_ref().map(<[u8; 16]>::as_slice),
                        created_at_epoch,
                    ],
                )?;
                transaction.last_insert_rowid()
            };
            load_by_id(transaction, registration_id)
        })
    }

    /// Register one initialized worktree and import its captured usage atomically.
    ///
    /// # Errors
    ///
    /// Returns any registration or snapshot synchronization error without making
    /// the registration visible.
    #[allow(clippy::too_many_arguments)]
    pub fn register_worktree_with_usage_snapshot(
        &self,
        alias: &WorktreeAlias,
        git_common_directory: &Path,
        git_administrative_directory: &Path,
        git_administrative_identity: &str,
        root: &Path,
        project_instance_id: ProjectInstanceId,
        snapshot: &WorktreeUsageSnapshot,
        created_at_epoch: u64,
    ) -> DbResult<(WorktreeRegistration, WorktreeUsageSyncState)> {
        self.with_validated_write(|transaction| {
            let registration = self.register_worktree(
                alias,
                git_common_directory,
                git_administrative_directory,
                git_administrative_identity,
                root,
                Some(project_instance_id),
                created_at_epoch,
            )?;
            let synchronization = telemetry::synchronize_worktree_usage_snapshot(
                transaction,
                registration.registration_id,
                snapshot,
            )?;
            Ok((
                load_by_id(transaction, registration.registration_id)?,
                synchronization,
            ))
        })
    }

    /// Refresh the canonical root of one captured active registration.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured registration is no longer active,
    /// the root is invalid, persisted state is malformed, or `SQLite` fails.
    pub fn refresh_worktree_root(
        &self,
        registration: &WorktreeRegistration,
        root: &Path,
    ) -> DbResult<WorktreeRegistration> {
        let root_identity = worktree_identity("root", root)?;
        let root = native_path_projection(&root_identity)?;
        let root_identity_bytes = root_identity.encode()?;
        self.with_validated_write(|transaction| {
            let updated = transaction.execute(
                "UPDATE worktree_registrations
                 SET last_root = ?1, last_root_identity = ?2
                 WHERE registration_id = ?3 AND alias = ?4 AND state = 'active'",
                params![
                    root,
                    root_identity_bytes.as_slice(),
                    registration.registration_id,
                    registration.alias.as_str()
                ],
            )?;
            if updated != 1 {
                return Err(DbError::WorktreeRegistrationNotFound {
                    alias: registration.alias.to_string(),
                });
            }
            load_by_id(transaction, registration.registration_id)
        })
    }

    /// Return one active registration by alias.
    ///
    /// # Errors
    ///
    /// Returns an error when the alias is absent, a persisted row is malformed,
    /// or `SQLite` cannot complete the read.
    pub fn worktree_registration(&self, alias: &WorktreeAlias) -> DbResult<WorktreeRegistration> {
        load_active_by_alias(&self.connection, alias.as_str())?.ok_or_else(|| {
            DbError::WorktreeRegistrationNotFound {
                alias: alias.to_string(),
            }
        })
    }

    /// List active registrations and optionally retained retired history.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed persisted rows or `SQLite` read failures.
    pub fn worktree_registrations(
        &self,
        include_retired: bool,
    ) -> DbResult<Vec<WorktreeRegistration>> {
        let mut statement = self.connection.prepare(
            "SELECT registration_id, alias, state, git_common_directory,
                    git_common_directory_identity,
                    git_administrative_directory, git_administrative_directory_identity,
                    git_administrative_identity, last_root, last_root_identity,
                    project_instance_id,
                    accepted_telemetry_revision, created_at_epoch, retired_at_epoch
             FROM worktree_registrations
             WHERE state = 'active' OR ?1
             ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END, alias, registration_id
             LIMIT ?2",
        )?;
        let limit = i64::try_from(MAX_GIT_WORKTREE_REGISTRATIONS + 1).map_err(|_source| {
            DbError::WorktreeRegistrationRow {
                reason: "registration bound exceeds SQLite integer range",
            }
        })?;
        let rows = statement.query_map(params![include_retired, limit], persisted_registration)?;
        let registrations = rows
            .map(|row| row.map_err(DbError::from).and_then(try_registration))
            .collect::<DbResult<Vec<_>>>()?;
        if registrations.len() > MAX_GIT_WORKTREE_REGISTRATIONS {
            return Err(DbError::WorktreeRegistrationCapacity {
                limit: MAX_GIT_WORKTREE_REGISTRATIONS,
            });
        }
        Ok(registrations)
    }

    /// Run one short operation when active catalog identities still match.
    ///
    /// # Errors
    ///
    /// Returns `None` when the active row count, order, aliases, or project
    /// bindings changed. Returns an error for malformed persisted rows, a
    /// changed database binding, an operation failure, or any `SQLite`
    /// transaction failure.
    pub fn with_matching_active_worktree_catalog<T>(
        &self,
        expected: &[WorktreeRegistration],
        operation: impl FnOnce() -> DbResult<T>,
    ) -> DbResult<Option<T>> {
        self.with_validated_write(|_| {
            let current = self.worktree_registrations(false)?;
            if current.len() != expected.len()
                || current.iter().zip(expected).any(|(current, expected)| {
                    current.registration_id != expected.registration_id
                        || current.alias != expected.alias
                        || current.project_instance_id != expected.project_instance_id
                })
            {
                return Ok(None);
            }
            operation().map(Some)
        })
    }

    /// Run one short operation while an exact active registration owns control-writer exclusion.
    ///
    /// Callers that coordinate another local atlas must acquire this scope first,
    /// then open or lock the local atlas, and finally publish through this guard
    /// before returning.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured registration is no longer active under
    /// the same alias, the control database binding changed, the callback fails,
    /// or `SQLite` cannot commit or roll back the transaction.
    pub fn with_active_worktree_registration<T>(
        &self,
        registration_id: i64,
        alias: &WorktreeAlias,
        operation: impl FnOnce(&mut ActiveWorktreeRegistrationGuard<'_>) -> DbResult<T>,
    ) -> DbResult<T> {
        self.with_validated_write(|transaction| {
            let registration = load_by_id(transaction, registration_id)?;
            if registration.state != WorktreeRegistrationState::Active
                || registration.alias != *alias
            {
                return Err(DbError::WorktreeRegistrationNotFound {
                    alias: alias.to_string(),
                });
            }
            operation(&mut ActiveWorktreeRegistrationGuard {
                connection: transaction,
                registration,
            })
        })
    }

    /// Run one external reset operation while an exact registration remains unbound.
    ///
    /// The callback must not mutate the control catalog. Its nested result keeps
    /// caller-owned filesystem errors typed while the outer result owns `SQLite`
    /// validation and transaction failures.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured registration is no longer active under
    /// the same alias, became bound, the control binding changed, or `SQLite`
    /// cannot complete the writer-exclusion transaction.
    pub fn with_unbound_worktree_registration<T, E>(
        &self,
        registration_id: i64,
        alias: &WorktreeAlias,
        operation: impl FnOnce(&WorktreeRegistration) -> Result<T, E>,
    ) -> DbResult<Result<T, E>> {
        self.with_active_worktree_registration(registration_id, alias, |guard| {
            if guard.registration().project_instance_id.is_some() {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    value: guard
                        .registration()
                        .project_instance_id
                        .map_or_else(String::new, |value| value.to_string()),
                });
            }
            Ok(operation(guard.registration()))
        })
    }

    /// Export this exact atlas's bounded local aggregate snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected database binding is invalid, stored
    /// aggregate state is corrupt, a bound is exceeded, or `SQLite` cannot read
    /// one complete snapshot.
    pub fn export_worktree_usage_snapshot(&self) -> DbResult<WorktreeUsageSnapshot> {
        telemetry::export_worktree_usage_snapshot(&self.connection)
    }

    /// Hold local writer exclusion while a caller consumes one exact usage snapshot.
    ///
    /// The callback must remain short: its lifetime is the final-synchronization
    /// boundary that prevents a local usage commit from landing between export
    /// and control-atlas retirement.
    ///
    /// # Errors
    ///
    /// Returns an error when the exact database binding changed, writer exclusion
    /// cannot be acquired, snapshot export fails, or the callback fails.
    pub fn with_exclusive_worktree_usage_snapshot<T>(
        &self,
        operation: impl FnOnce(&WorktreeUsageSnapshot) -> DbResult<T>,
    ) -> DbResult<T> {
        let binding = self.captured_project_binding()?;
        self.with_telemetry_connection(|connection| {
            crate::with_validated_native_write_transaction(
                connection,
                Some(&binding.project_root_identity),
                Some(binding.project_instance_id),
                |transaction| {
                    let snapshot = telemetry::export_worktree_usage_snapshot(transaction)?;
                    operation(&snapshot)
                },
            )
        })
    }

    /// Accept a strictly newer local aggregate snapshot for one active alias.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent alias, mismatched project identity,
    /// malformed or excessive snapshot state, changed control binding, or any
    /// transactional `SQLite` failure. The last accepted snapshot remains intact
    /// on every error.
    pub fn synchronize_worktree_usage(
        &self,
        alias: &WorktreeAlias,
        snapshot: &WorktreeUsageSnapshot,
    ) -> DbResult<WorktreeUsageSyncState> {
        let registration = self.worktree_registration(alias)?;
        self.with_validated_write(|transaction| {
            telemetry::synchronize_worktree_usage_snapshot(
                transaction,
                registration.registration_id,
                snapshot,
            )
        })
    }

    /// Bind an initialized worktree identity to one captured active registration.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured registration is no longer active under
    /// the same alias, its project identity conflicts, the path is invalid, stored
    /// state is malformed, or `SQLite` fails.
    pub fn bind_worktree_project(
        &self,
        registration_id: i64,
        alias: &WorktreeAlias,
        root: &Path,
        project_instance_id: ProjectInstanceId,
    ) -> DbResult<WorktreeRegistration> {
        self.with_active_worktree_registration(registration_id, alias, |guard| {
            guard.bind_project(root, project_instance_id)
        })
    }

    /// Retire one active alias without deleting its aggregate history.
    ///
    /// The caller owns required final telemetry synchronization before invoking
    /// this storage transition.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured registration is no longer active under
    /// the same alias, the time or stored state is invalid, or `SQLite` fails.
    pub fn retire_worktree(
        &self,
        registration_id: i64,
        alias: &WorktreeAlias,
        retired_at_epoch: u64,
    ) -> DbResult<WorktreeRegistration> {
        let retired_at_epoch = epoch_to_sqlite(retired_at_epoch)?;
        self.with_validated_write(|transaction| {
            let existing = load_by_id(transaction, registration_id)?;
            if existing.state != WorktreeRegistrationState::Active || existing.alias != *alias {
                return Err(DbError::WorktreeRegistrationNotFound {
                    alias: alias.to_string(),
                });
            }
            retire_registration(transaction, &existing, retired_at_epoch)
        })
    }

    /// Bind, synchronize one writer-excluded local snapshot, and retire its alias atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent alias, mismatched snapshot identity, invalid
    /// time or aggregate state, changed control binding, or transactional `SQLite`
    /// failure. Binding, synchronization, and retirement roll back together.
    pub fn retire_worktree_with_usage_snapshot(
        &self,
        registration_id: i64,
        alias: &WorktreeAlias,
        root: &Path,
        project_instance_id: ProjectInstanceId,
        snapshot: &WorktreeUsageSnapshot,
        retired_at_epoch: u64,
    ) -> DbResult<(WorktreeRegistration, WorktreeUsageSyncState)> {
        self.with_active_worktree_registration(registration_id, alias, |guard| {
            guard.retire_with_usage_snapshot(root, project_instance_id, snapshot, retired_at_epoch)
        })
    }
}

/// Bind one already-loaded active registration inside its caller-owned transaction.
fn bind_registration_project(
    connection: &Connection,
    registration: &WorktreeRegistration,
    root: &CanonicalProjectRoot,
    project_instance_id: ProjectInstanceId,
) -> DbResult<WorktreeRegistration> {
    if identities_conflict(registration.project_instance_id, Some(project_instance_id)) {
        return Err(DbError::WorktreeRegistrationConflict {
            field: "project_instance_id",
            value: project_instance_id.to_string(),
        });
    }
    let project_bytes = project_instance_id.as_bytes();
    if project_identity_exists_for_other(
        connection,
        Some(registration.registration_id),
        project_bytes.as_slice(),
    )? {
        return Err(DbError::WorktreeRegistrationConflict {
            field: "project_instance_id",
            value: project_instance_id.to_string(),
        });
    }
    let root_display = native_path_projection(root)?;
    let root_identity = root.encode()?;
    let updated = connection.execute(
        "UPDATE worktree_registrations
         SET last_root = ?1, last_root_identity = ?2, project_instance_id = ?3
         WHERE registration_id = ?4 AND alias = ?5 AND state = 'active'",
        params![
            root_display,
            root_identity.as_slice(),
            project_bytes.as_slice(),
            registration.registration_id,
            registration.alias.as_str()
        ],
    )?;
    if updated != 1 {
        return Err(DbError::WorktreeRegistrationNotFound {
            alias: registration.alias.to_string(),
        });
    }
    load_by_id(connection, registration.registration_id)
}

/// Retire one already-loaded active registration inside its caller-owned transaction.
fn retire_registration(
    connection: &Connection,
    registration: &WorktreeRegistration,
    retired_at_epoch: i64,
) -> DbResult<WorktreeRegistration> {
    if retired_at_epoch < epoch_to_sqlite(registration.created_at_epoch)? {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "retirement time precedes creation time",
        });
    }
    let updated = connection.execute(
        "UPDATE worktree_registrations
         SET state = 'retired', retired_at_epoch = ?1
         WHERE registration_id = ?2 AND alias = ?3 AND state = 'active'",
        params![
            retired_at_epoch,
            registration.registration_id,
            registration.alias.as_str()
        ],
    )?;
    if updated != 1 {
        return Err(DbError::WorktreeRegistrationNotFound {
            alias: registration.alias.to_string(),
        });
    }
    load_by_id(connection, registration.registration_id)
}

/// Build one typed public alias-validation error.
fn invalid_alias<T>(value: &str, reason: &'static str) -> DbResult<T> {
    Err(DbError::InvalidWorktreeAlias {
        alias: value.to_string(),
        reason,
    })
}

/// Admit one caller-validated absolute path through the shared native identity.
fn worktree_identity(field: &'static str, path: &Path) -> DbResult<CanonicalProjectRoot> {
    let byte_len = path.as_os_str().as_encoded_bytes().len();
    if !path.is_absolute() || byte_len > MAX_WORKTREE_REGISTRATION_PATH_BYTES {
        return Err(DbError::InvalidWorktreeRegistrationPath {
            field,
            path: path.to_string_lossy().into_owned(),
        });
    }
    // Existing worktree paths are canonicalized against the filesystem. The
    // persisted-path fallback keeps registration/recovery usable for a
    // retired or moved worktree whose old directory is absent, while still
    // rejecting an existing regular file at an active boundary.
    if path.exists() {
        CanonicalProjectRoot::from_path(path).map_err(DbError::from)
    } else {
        CanonicalProjectRoot::from_persisted_path(path.to_path_buf()).map_err(DbError::from)
    }
}

/// Return the UTF-8 projection retained for compatibility metadata.
fn native_path_projection(identity: &CanonicalProjectRoot) -> DbResult<String> {
    identity.display_string().or_else(|_| {
        let encoded = identity.encode()?;
        Ok(format!(
            "native-path-unavailable:{}",
            blake3::hash(&encoded).to_hex()
        ))
    })
}

/// Validate one filesystem-derived opaque administrative identity.
fn validated_administrative_identity(value: &str) -> DbResult<String> {
    if value.len() != GIT_ADMINISTRATIVE_IDENTITY_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "invalid Git administrative identity",
        });
    }
    Ok(value.to_string())
}

/// Narrow one public epoch into the exact `SQLite` integer domain.
fn epoch_to_sqlite(value: u64) -> DbResult<i64> {
    i64::try_from(value).map_err(|_source| DbError::WorktreeRegistrationRow {
        reason: "epoch exceeds SQLite integer range",
    })
}

/// Detect only incompatible initialized project identities.
fn identities_conflict(
    existing: Option<ProjectInstanceId>,
    requested: Option<ProjectInstanceId>,
) -> bool {
    matches!((existing, requested), (Some(left), Some(right)) if left != right)
}

/// Read one raw registration row before typed validation.
fn persisted_registration(row: &Row<'_>) -> rusqlite::Result<PersistedWorktreeRegistration> {
    Ok(PersistedWorktreeRegistration {
        registration_id: row.get(0)?,
        alias: row.get(1)?,
        state: row.get(2)?,
        git_common_directory: row.get(3)?,
        git_common_directory_identity: row.get(4)?,
        git_administrative_directory: row.get(5)?,
        git_administrative_directory_identity: row.get(6)?,
        git_administrative_identity: row.get(7)?,
        last_root: row.get(8)?,
        last_root_identity: row.get(9)?,
        project_instance_id: row.get(10)?,
        accepted_telemetry_revision: row.get(11)?,
        created_at_epoch: row.get(12)?,
        retired_at_epoch: row.get(13)?,
    })
}

/// Validate and convert one persisted registration row.
fn try_registration(row: PersistedWorktreeRegistration) -> DbResult<WorktreeRegistration> {
    let git_common_directory_identity =
        CanonicalProjectRoot::decode(&row.git_common_directory_identity)?;
    let git_administrative_directory_identity =
        CanonicalProjectRoot::decode(&row.git_administrative_directory_identity)?;
    let last_root_identity = CanonicalProjectRoot::decode(&row.last_root_identity)?;
    let project_instance_id = row
        .project_instance_id
        .map(|value| {
            let bytes: [u8; 16] =
                value
                    .try_into()
                    .map_err(|value: Vec<u8>| DbError::InvalidBlobLength {
                        field: "worktree_registrations.project_instance_id",
                        expected: 16,
                        found: value.len(),
                    })?;
            ProjectInstanceId::from_bytes(bytes).map_err(DbError::from)
        })
        .transpose()?;
    let accepted_telemetry_revision =
        u64::try_from(row.accepted_telemetry_revision).map_err(|_source| {
            DbError::WorktreeRegistrationRow {
                reason: "negative accepted telemetry revision",
            }
        })?;
    let created_at_epoch = u64::try_from(row.created_at_epoch).map_err(|_source| {
        DbError::WorktreeRegistrationRow {
            reason: "negative creation time",
        }
    })?;
    let retired_at_epoch = row
        .retired_at_epoch
        .map(|value| {
            u64::try_from(value).map_err(|_source| DbError::WorktreeRegistrationRow {
                reason: "negative retirement time",
            })
        })
        .transpose()?;
    Ok(WorktreeRegistration {
        registration_id: row.registration_id,
        alias: WorktreeAlias::parse(&row.alias)?,
        state: WorktreeRegistrationState::parse(&row.state)?,
        git_common_directory: row.git_common_directory,
        git_common_directory_identity,
        git_administrative_directory: row.git_administrative_directory,
        git_administrative_directory_identity,
        git_administrative_identity: validated_administrative_identity(
            &row.git_administrative_identity,
        )?,
        last_root: row.last_root,
        last_root_identity,
        project_instance_id,
        accepted_telemetry_revision,
        created_at_epoch,
        retired_at_epoch,
    })
}

/// Common typed registration projection shared by bounded lookups.
const REGISTRATION_SELECT: &str = "SELECT registration_id, alias, state, git_common_directory,
            git_common_directory_identity, git_administrative_directory,
            git_administrative_directory_identity, git_administrative_identity,
            last_root, last_root_identity, project_instance_id,
            accepted_telemetry_revision, created_at_epoch, retired_at_epoch
     FROM worktree_registrations";

/// Load one active alias through its required partial unique index.
fn load_active_by_alias(
    connection: &Connection,
    alias: &str,
) -> DbResult<Option<WorktreeRegistration>> {
    let sql = format!(
        "{REGISTRATION_SELECT} INDEXED BY idx_worktree_registrations_active_alias
         WHERE state = 'active' AND alias = ?1"
    );
    let row = connection
        .query_row(&sql, [alias], persisted_registration)
        .optional()?;
    row.map(try_registration).transpose()
}

/// Load one active or retired row by stable primary key.
fn load_by_id(connection: &Connection, registration_id: i64) -> DbResult<WorktreeRegistration> {
    let sql = format!("{REGISTRATION_SELECT} WHERE registration_id = ?1");
    let row = connection.query_row(&sql, [registration_id], persisted_registration)?;
    try_registration(row)
}

/// Check active Git identity conflicts through owned indexes.
fn active_git_identity_exists(
    connection: &Connection,
    administrative_directory: &[u8],
    administrative_identity: &str,
) -> DbResult<bool> {
    let administrative_identity_exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worktree_registrations
                 INDEXED BY idx_worktree_registrations_active_native_administrative_directory
            WHERE state = 'active' AND git_administrative_directory_identity = ?1
         )",
        [administrative_directory],
        |row| row.get::<_, bool>(0),
    )?;
    if administrative_identity_exists {
        return Ok(true);
    }
    let lifecycle_identity_exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worktree_registrations
                 INDEXED BY idx_worktree_registrations_active_administrative_identity
            WHERE state = 'active' AND git_administrative_identity = ?1
         )",
        [administrative_identity],
        |row| row.get::<_, bool>(0),
    )?;
    if lifecycle_identity_exists {
        return Ok(true);
    }
    Ok(false)
}

/// Check whether another active or retired row owns one initialized project identity.
fn project_identity_exists_for_other(
    connection: &Connection,
    registration_id: Option<i64>,
    project_instance_id: &[u8],
) -> DbResult<bool> {
    let found = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worktree_registrations
            WHERE registration_id IS NOT ?1 AND project_instance_id = ?2
         )",
        params![registration_id, project_instance_id],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(found)
}

/// Check whether another active registration owns the native source root.
fn native_root_identity_exists_for_other(
    connection: &Connection,
    registration_id: Option<i64>,
    root_identity: &[u8],
) -> DbResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM worktree_registrations
                INDEXED BY idx_worktree_registrations_active_native_root
                WHERE state = 'active'
                  AND registration_id IS NOT ?1
                  AND last_root_identity = ?2
             )",
            params![registration_id, root_identity],
            |row| row.get::<_, bool>(0),
        )
        .map_err(DbError::from)
}

/// Find the newest retired history row with the exact same stable identities.
fn load_matching_retired_id(
    connection: &Connection,
    administrative_directory: &[u8],
    administrative_identity: &str,
    project_instance_id: Option<&[u8; 16]>,
) -> DbResult<Option<i64>> {
    connection
        .query_row(
            "SELECT registration_id
             FROM worktree_registrations
             WHERE state = 'retired'
               AND git_administrative_directory_identity = ?1
               AND git_administrative_identity = ?2
               AND project_instance_id IS ?3
             ORDER BY registration_id DESC
             LIMIT 1",
            params![
                administrative_directory,
                administrative_identity,
                project_instance_id.map(<[u8; 16]>::as_slice),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Return a test error instead of panicking inside a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Return whether an independent writer reached `SQLite`'s held writer lock.
    fn sqlite_writer_busy(error: &DbError) -> bool {
        matches!(
            error,
            DbError::Sqlite(rusqlite::Error::SqliteFailure(code, _))
                if matches!(
                    code.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        )
    }

    /// Compare test values without panicking inside a fallible test.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, found {actual:?}"
            ))
            .into())
        }
    }

    fn identity(byte: u8) -> Result<ProjectInstanceId, Box<dyn Error>> {
        Ok(ProjectInstanceId::from_bytes([byte; 16])?)
    }

    fn administrative_identity(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    #[test]
    fn aliases_reject_reserved_or_ambiguous_shapes() -> Result<(), Box<dyn Error>> {
        for invalid in ["", "main", "Issue-430", "issue 430", "-issue", "ä"] {
            if WorktreeAlias::parse(invalid).is_ok() {
                return Err(format!("invalid alias was accepted: {invalid:?}").into());
            }
        }
        let alias = WorktreeAlias::parse("issue-430.fix")?;
        require_eq(&alias.as_str(), &"issue-430.fix", "valid alias")?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn registry_round_trips_non_utf8_identity_paths_without_lossy_keys()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        let common = temp.path().join("common.git");
        let administrative = common.join("worktrees").join("linked");
        let root = temp.path().join("linked");
        let invalid_common = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"common-\xff".to_vec()));
        let invalid_administrative = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"admin-\xff".to_vec()));
        let invalid_root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"root-\xff".to_vec()));
        let invalid_administrative_two = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"admin-\xfe".to_vec()));
        fs::create_dir_all(&control)?;
        for path in [
            &common,
            &administrative,
            &root,
            &invalid_common,
            &invalid_administrative,
            &invalid_root,
            &invalid_administrative_two,
        ] {
            fs::create_dir_all(path)?;
        }
        let store = AtlasStore::open_for_project(&control.join("projectatlas.db"), &control)?;
        // Keep the temporary paths in owned variables while registration uses
        // them; this also makes each active root identity distinct.
        let second_root = temp.path().join("second");
        let third_administrative = common.join("worktrees/third");
        fs::create_dir_all(&second_root)?;
        fs::create_dir_all(&third_administrative)?;
        let mut rows = Vec::new();
        for (alias, common_path, administrative_path, root_path, identity) in [
            (
                "nonutf-common",
                invalid_common.as_path(),
                administrative.as_path(),
                root.as_path(),
                1,
            ),
            (
                "nonutf-admin",
                common.as_path(),
                invalid_administrative.as_path(),
                second_root.as_path(),
                2,
            ),
            (
                "nonutf-root",
                common.as_path(),
                third_administrative.as_path(),
                invalid_root.as_path(),
                3,
            ),
        ] {
            rows.push(store.register_worktree(
                &WorktreeAlias::parse(alias)?,
                common_path,
                administrative_path,
                &administrative_identity(identity),
                root_path,
                None,
                1,
            )?);
        }
        for row in &rows {
            require(
                row.git_common_directory_identity.encode()?.len() >= 3
                    && row.git_administrative_directory_identity.encode()?.len() >= 3
                    && row.last_root_identity.encode()?.len() >= 3,
                "native identity codec bytes were not persisted",
            )?;
            require(
                row.git_common_directory.contains("native-path-unavailable")
                    || row
                        .git_administrative_directory
                        .contains("native-path-unavailable")
                    || row.last_root.contains("native-path-unavailable"),
                "non-UTF-8 display projection was not typed as unavailable",
            )?;
        }
        require(
            matches!(
                store.register_worktree(
                    &WorktreeAlias::parse("duplicate-native-admin")?,
                    &common,
                    &invalid_administrative,
                    &administrative_identity(4),
                    &temp.path().join("duplicate-root"),
                    None,
                    1,
                ),
                Err(DbError::WorktreeRegistrationConflict { .. })
            ),
            "duplicate native administrative identity was accepted",
        )?;
        fs::create_dir_all(temp.path().join("distinct-root"))?;
        store.register_worktree(
            &WorktreeAlias::parse("distinct-native-admin")?,
            &common,
            &invalid_administrative_two,
            &administrative_identity(5),
            &temp.path().join("distinct-root"),
            None,
            1,
        )?;
        let reopened = AtlasStore::open_for_project(&control.join("projectatlas.db"), &control)?;
        require_eq(
            &reopened.worktree_registrations(false)?.len(),
            &4,
            "native rows",
        )?;
        require(
            reopened
                .worktree_registrations(false)?
                .iter()
                .any(|row| row.last_root_identity.display_string().is_err()),
            "native non-UTF-8 root did not remain lossless after reopen",
        )
    }

    #[test]
    fn registry_reuses_matching_history_and_keeps_retired_alias_history()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        let common = temp.path().join("common.git");
        let first_admin = common.join("worktrees/first");
        let second_admin = common.join("worktrees/second");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        for path in [
            &control,
            &first_admin,
            &second_admin,
            &first_root,
            &second_root,
        ] {
            fs::create_dir_all(path)?;
        }
        let database = control.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &control)?;
        let alias = WorktreeAlias::parse("issue-430")?;
        let first = store.register_worktree(
            &alias,
            &common,
            &first_admin,
            &administrative_identity(1),
            &first_root,
            Some(identity(1)?),
            10,
        )?;
        let idempotent = store.register_worktree(
            &alias,
            &common,
            &first_admin,
            &administrative_identity(1),
            &first_root,
            Some(identity(1)?),
            11,
        )?;
        require_eq(
            &first.registration_id,
            &idempotent.registration_id,
            "idempotent registration identity",
        )?;
        require(
            matches!(
                store.register_worktree(
                    &alias,
                    &common,
                    &second_admin,
                    &administrative_identity(2),
                    &second_root,
                    Some(identity(2)?),
                    12,
                ),
                Err(DbError::WorktreeRegistrationConflict { field: "alias", .. })
            ),
            "active alias conflict was not rejected",
        )?;

        let retired = store.retire_worktree(first.registration_id, &alias, 20)?;
        require_eq(
            &retired.state,
            &WorktreeRegistrationState::Retired,
            "retired state",
        )?;
        require(
            matches!(
                store.refresh_worktree_root(&first, &second_root),
                Err(DbError::WorktreeRegistrationNotFound { .. })
            ),
            "stale root refresh reactivated a retired registration",
        )?;
        require_eq(
            &store
                .worktree_registrations(true)?
                .into_iter()
                .find(|registration| registration.registration_id == first.registration_id)
                .ok_or_else(|| io::Error::other("retired registration history is missing"))?
                .state,
            &WorktreeRegistrationState::Retired,
            "state after stale root refresh",
        )?;
        let replacement = store.register_worktree(
            &alias,
            &common,
            &first_admin,
            &administrative_identity(2),
            &second_root,
            None,
            21,
        )?;
        require(
            replacement.registration_id != first.registration_id,
            "replacement reused unrelated retired history",
        )?;
        require(
            matches!(
                store.bind_worktree_project(
                    first.registration_id,
                    &alias,
                    &first_root,
                    identity(1)?
                ),
                Err(DbError::WorktreeRegistrationNotFound { .. })
            ),
            "stale bind targeted a replacement registration after alias reuse",
        )?;
        require(
            matches!(
                store.retire_worktree(first.registration_id, &alias, 22),
                Err(DbError::WorktreeRegistrationNotFound { .. })
            ),
            "stale retirement targeted a replacement registration after alias reuse",
        )?;
        let replacement = store.worktree_registration(&alias)?;
        require(
            replacement.project_instance_id.is_none()
                && replacement.last_root
                    == native_path_projection(&worktree_identity("root", &second_root)?)?,
            "stale bind changed the replacement registration",
        )?;
        let all = store.worktree_registrations(true)?;
        require_eq(&all.len(), &2, "retained registration count")?;
        require_eq(
            &all.first()
                .ok_or_else(|| io::Error::other("active registration is missing"))?
                .state,
            &WorktreeRegistrationState::Active,
            "active row ordering",
        )?;
        require_eq(
            &all.get(1)
                .ok_or_else(|| io::Error::other("retired registration is missing"))?
                .state,
            &WorktreeRegistrationState::Retired,
            "retired row ordering",
        )?;
        require(
            store
                .worktree_registrations(false)?
                .iter()
                .all(|row| row.state == WorktreeRegistrationState::Active),
            "active-only list returned retired history",
        )?;
        Ok(())
    }

    #[test]
    fn active_registration_guard_rolls_back_nested_binding_and_rechecks_unbound_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let common = temp.path().join("common.git");
        let admin = common.join("worktrees/guarded");
        let root = temp.path().join("guarded");
        for path in [&control_root, &admin, &root] {
            fs::create_dir_all(path)?;
        }
        let store =
            AtlasStore::open_for_project(&control_root.join("projectatlas.db"), &control_root)?;
        let alias = WorktreeAlias::parse("guarded")?;
        let project = identity(1)?;
        let registration = store.register_worktree(
            &alias,
            &common,
            &admin,
            &administrative_identity(1),
            &root,
            None,
            1,
        )?;

        let rejected = store.with_active_worktree_registration(
            registration.registration_id,
            &alias,
            |guard| {
                guard.bind_project(&root, project)?;
                Err::<(), _>(DbError::WorktreeRegistrationConflict {
                    field: "test_operation",
                    value: "rollback".to_string(),
                })
            },
        );
        require(
            matches!(
                rejected,
                Err(DbError::WorktreeRegistrationConflict {
                    field: "test_operation",
                    ..
                })
            ),
            "guarded callback failure was not returned",
        )?;
        require(
            store
                .worktree_registration(&alias)?
                .project_instance_id
                .is_none(),
            "failed guarded callback committed its nested binding",
        )?;

        store.with_active_worktree_registration(registration.registration_id, &alias, |guard| {
            guard.bind_project(&root, project).map(|_| ())
        })?;
        let reset_callback_ran = std::cell::Cell::new(false);
        require(
            matches!(
                store.with_unbound_worktree_registration(
                    registration.registration_id,
                    &alias,
                    |_registration| {
                        reset_callback_ran.set(true);
                        Ok::<(), io::Error>(())
                    }
                ),
                Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    ..
                })
            ) && !reset_callback_ran.get(),
            "bound registration entered the guarded reset callback",
        )
    }

    #[test]
    fn active_registration_guard_serializes_bind_and_reset_across_connections()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let common = temp.path().join("common.git");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        for path in [&control_root, &first_root, &second_root] {
            fs::create_dir_all(path)?;
        }
        let database = control_root.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &control_root)?;
        let contender = AtlasStore::open_for_project(&database, &control_root)?;
        contender.connection.busy_timeout(Duration::ZERO)?;
        let bind_wins = WorktreeAlias::parse("bind-wins")?;
        let bind_registration = store.register_worktree(
            &bind_wins,
            &common,
            &common.join("worktrees/bind-wins"),
            &administrative_identity(1),
            &first_root,
            None,
            1,
        )?;
        let bind_project = identity(1)?;
        store.with_active_worktree_registration(
            bind_registration.registration_id,
            &bind_wins,
            |guard| {
                let blocked = contender.with_unbound_worktree_registration(
                    bind_registration.registration_id,
                    &bind_wins,
                    |_registration| Ok::<(), io::Error>(()),
                );
                if !blocked.as_ref().is_err_and(sqlite_writer_busy) {
                    return Err(DbError::WorktreeRegistrationRow {
                        reason: "reset contender did not reach the held SQLite writer lock",
                    });
                }
                guard.bind_project(&first_root, bind_project).map(|_| ())
            },
        )?;
        let reset_callback_ran = std::cell::Cell::new(false);
        require(
            matches!(
                contender.with_unbound_worktree_registration(
                    bind_registration.registration_id,
                    &bind_wins,
                    |_registration| {
                        reset_callback_ran.set(true);
                        Ok::<(), io::Error>(())
                    },
                ),
                Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    ..
                })
            ) && !reset_callback_ran.get(),
            "reset did not reload the binding committed by the winning writer",
        )?;

        let reset_wins = WorktreeAlias::parse("reset-wins")?;
        let reset_registration = store.register_worktree(
            &reset_wins,
            &common,
            &common.join("worktrees/reset-wins"),
            &administrative_identity(2),
            &second_root,
            None,
            2,
        )?;
        let reset_project = identity(2)?;
        let target_database = second_root.join("projectatlas.db");
        fs::write(&target_database, b"captured atlas")?;
        store.with_unbound_worktree_registration(
            reset_registration.registration_id,
            &reset_wins,
            |_registration| {
                let blocked = contender.with_active_worktree_registration(
                    reset_registration.registration_id,
                    &reset_wins,
                    |guard| guard.bind_project(&second_root, reset_project).map(|_| ()),
                );
                if !blocked.as_ref().is_err_and(sqlite_writer_busy) {
                    return Err(io::Error::other(
                        "bind contender did not reach the held SQLite writer lock",
                    ));
                }
                fs::remove_file(&target_database)
            },
        )??;
        let late_bind_result = contender.with_active_worktree_registration(
            reset_registration.registration_id,
            &reset_wins,
            |guard| {
                if !target_database.is_file() {
                    return Ok(Err("target atlas is missing"));
                }
                guard.bind_project(&second_root, reset_project)?;
                Ok(Ok(()))
            },
        )?;
        require(
            matches!(late_bind_result, Err("target atlas is missing")),
            "late bind did not recheck the reset target after writer exclusion",
        )?;
        require(
            store
                .worktree_registration(&reset_wins)?
                .project_instance_id
                .is_none()
                && !target_database.exists(),
            "reset-wins interleaving bound or recreated the deleted target atlas",
        )
    }

    #[test]
    fn failed_final_sync_rolls_back_project_binding_and_retirement() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let common = temp.path().join("common.git");
        let admin = common.join("worktrees/issue-430");
        let original_root = temp.path().join("original");
        let moved_root = temp.path().join("moved");
        let other_root = temp.path().join("other");
        for path in [
            &control_root,
            &admin,
            &original_root,
            &moved_root,
            &other_root,
        ] {
            fs::create_dir_all(path)?;
        }

        let control =
            AtlasStore::open_for_project(&control_root.join("projectatlas.db"), &control_root)?;
        let other = AtlasStore::open_for_project(&other_root.join("projectatlas.db"), &other_root)?;
        let mismatched_snapshot = other.export_worktree_usage_snapshot()?;
        let target_project = if mismatched_snapshot.project_instance_id() == identity(7)? {
            identity(8)?
        } else {
            identity(7)?
        };
        let alias = WorktreeAlias::parse("issue-430")?;
        let before = control.register_worktree(
            &alias,
            &common,
            &admin,
            &administrative_identity(1),
            &original_root,
            None,
            10,
        )?;

        require(
            matches!(
                control.retire_worktree_with_usage_snapshot(
                    before.registration_id,
                    &alias,
                    &moved_root,
                    target_project,
                    &mismatched_snapshot,
                    20,
                ),
                Err(DbError::WorktreeTelemetryProjectMismatch { .. })
            ),
            "mismatched final snapshot was not rejected",
        )?;
        require_eq(
            &control.worktree_registration(&alias)?,
            &before,
            "active registration after failed final synchronization",
        )?;
        Ok(())
    }

    #[test]
    fn registration_and_initial_usage_snapshot_commit_atomically() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let local_root = temp.path().join("local");
        let common = temp.path().join("common.git");
        let admin = common.join("worktrees/local");
        for path in [&control_root, &local_root, &admin] {
            fs::create_dir_all(path)?;
        }
        let control =
            AtlasStore::open_for_project(&control_root.join("projectatlas.db"), &control_root)?;
        let local = AtlasStore::open_for_project(&local_root.join("projectatlas.db"), &local_root)?;
        local.record_usage(&projectatlas_core::telemetry::usage_from_estimates(
            "atomic-registration",
            "atlas_overview",
            None,
            None,
            100,
            20,
        ))?;
        let snapshot = local.export_worktree_usage_snapshot()?;
        let project = snapshot.project_instance_id();
        let mismatched_project = if project == identity(7)? {
            identity(8)?
        } else {
            identity(7)?
        };
        let alias = WorktreeAlias::parse("local")?;

        require(
            matches!(
                control.register_worktree_with_usage_snapshot(
                    &alias,
                    &common,
                    &admin,
                    &administrative_identity(1),
                    &local_root,
                    mismatched_project,
                    &snapshot,
                    1,
                ),
                Err(DbError::WorktreeTelemetryProjectMismatch { .. })
            ),
            "failed initial snapshot synchronization made a registration visible",
        )?;
        require(
            matches!(
                control.worktree_registration(&alias),
                Err(DbError::WorktreeRegistrationNotFound { .. })
            ) && control.repository_token_overview()?.calls == 0,
            "failed initial snapshot synchronization changed registration or aggregate state",
        )?;

        let (registration, synchronization) = control.register_worktree_with_usage_snapshot(
            &alias,
            &common,
            &admin,
            &administrative_identity(1),
            &local_root,
            project,
            &snapshot,
            1,
        )?;
        require(
            registration.project_instance_id == Some(project)
                && registration.accepted_telemetry_revision == snapshot.revision()
                && synchronization == WorktreeUsageSyncState::Synchronized
                && control.repository_token_overview()?.calls == 1,
            "successful initial registration exposed incomplete aggregate state",
        )
    }

    #[test]
    fn deferred_binding_and_initial_usage_snapshot_commit_atomically() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let local_root = temp.path().join("local");
        let common = temp.path().join("common.git");
        let admin = common.join("worktrees/local");
        for path in [&control_root, &local_root, &admin] {
            fs::create_dir_all(path)?;
        }
        let control =
            AtlasStore::open_for_project(&control_root.join("projectatlas.db"), &control_root)?;
        let local = AtlasStore::open_for_project(&local_root.join("projectatlas.db"), &local_root)?;
        local.record_usage(&projectatlas_core::telemetry::usage_from_estimates(
            "deferred-binding",
            "atlas_overview",
            None,
            None,
            100,
            20,
        ))?;
        let snapshot = local.export_worktree_usage_snapshot()?;
        let project = snapshot.project_instance_id();
        let mismatched_project = if project == identity(7)? {
            identity(8)?
        } else {
            identity(7)?
        };
        let alias = WorktreeAlias::parse("local")?;
        let registration = control.register_worktree(
            &alias,
            &common,
            &admin,
            &administrative_identity(1),
            &local_root,
            None,
            1,
        )?;

        let rejected = control.with_active_worktree_registration(
            registration.registration_id,
            &alias,
            |guard| {
                guard.bind_project_with_usage_snapshot(&local_root, mismatched_project, &snapshot)
            },
        );
        require(
            matches!(
                rejected,
                Err(DbError::WorktreeTelemetryProjectMismatch { .. })
            ) && control
                .worktree_registration(&alias)?
                .project_instance_id
                .is_none()
                && control.repository_token_overview()?.calls == 0,
            "failed deferred synchronization committed a project binding or aggregate",
        )?;

        let (bound, synchronization) = control.with_active_worktree_registration(
            registration.registration_id,
            &alias,
            |guard| guard.bind_project_with_usage_snapshot(&local_root, project, &snapshot),
        )?;
        require(
            bound.project_instance_id == Some(project)
                && bound.accepted_telemetry_revision == snapshot.revision()
                && synchronization == WorktreeUsageSyncState::Synchronized
                && control.repository_token_overview()?.calls == 1,
            "successful deferred binding exposed incomplete aggregate state",
        )
    }

    #[test]
    fn retired_project_identity_cannot_bind_another_registration() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        let common = temp.path().join("common.git");
        fs::create_dir_all(&control)?;
        let store = AtlasStore::open_for_project(&control.join("projectatlas.db"), &control)?;
        let original = WorktreeAlias::parse("original")?;
        let original_registration = store.register_worktree(
            &original,
            &common,
            &common.join("worktrees/original"),
            &administrative_identity(1),
            &temp.path().join("original"),
            Some(identity(1)?),
            1,
        )?;
        store.retire_worktree(original_registration.registration_id, &original, 2)?;

        let unbound = WorktreeAlias::parse("unbound")?;
        let unbound_registration = store.register_worktree(
            &unbound,
            &common,
            &common.join("worktrees/unbound"),
            &administrative_identity(2),
            &temp.path().join("unbound"),
            None,
            3,
        )?;
        for result in [
            store
                .bind_worktree_project(
                    unbound_registration.registration_id,
                    &unbound,
                    &temp.path().join("unbound"),
                    identity(1)?,
                )
                .map(|_| ()),
            store
                .register_worktree(
                    &unbound,
                    &common,
                    &common.join("worktrees/unbound"),
                    &administrative_identity(2),
                    &temp.path().join("unbound"),
                    Some(identity(1)?),
                    4,
                )
                .map(|_| ()),
            store
                .register_worktree(
                    &WorktreeAlias::parse("direct")?,
                    &common,
                    &common.join("worktrees/direct"),
                    &administrative_identity(3),
                    &temp.path().join("direct"),
                    Some(identity(1)?),
                    4,
                )
                .map(|_| ()),
        ] {
            require(
                matches!(
                    result,
                    Err(DbError::WorktreeRegistrationConflict {
                        field: "project_instance_id",
                        ..
                    })
                ),
                "retired project identity was rebound to another registration",
            )?;
        }
        require(
            store
                .worktree_registration(&unbound)?
                .project_instance_id
                .is_none(),
            "failed retired-identity binding changed the active registration",
        )
    }

    #[test]
    fn registry_capacity_rejects_new_history_without_changing_existing_rows()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        fs::create_dir_all(&control)?;
        let store = AtlasStore::open_for_project(&control.join("projectatlas.db"), &control)?;
        store.connection.execute_batch(
            "WITH RECURSIVE registrations(value) AS (
                 SELECT 0
                 UNION ALL
                 SELECT value + 1 FROM registrations WHERE value + 1 < 1024
             )
             INSERT INTO worktree_registrations(
                 alias, state, git_common_directory, git_administrative_directory,
                 git_administrative_identity, last_root, created_at_epoch, retired_at_epoch
             )
             SELECT printf('retired-%04d', value), 'retired', '/common',
                    printf('/common/worktrees/%04d', value),
                    printf('%064x', value + 1), printf('/worktrees/%04d', value), 0, 0
             FROM registrations;",
        )?;
        let alias = WorktreeAlias::parse("overflow")?;
        require(
            matches!(
                store.register_worktree(
                    &alias,
                    &temp.path().join("common"),
                    &temp.path().join("common/worktrees/overflow"),
                    &administrative_identity(3),
                    &temp.path().join("overflow"),
                    None,
                    1,
                ),
                Err(DbError::WorktreeRegistrationCapacity {
                    limit: MAX_GIT_WORKTREE_REGISTRATIONS
                })
            ),
            "registration capacity did not reject new history",
        )?;
        require_eq(
            &store.worktree_registrations(true)?.len(),
            &MAX_GIT_WORKTREE_REGISTRATIONS,
            "catalog after capacity rejection",
        )?;
        Ok(())
    }

    #[test]
    fn schema_twentytwo_worktree_identity_migration_backfills_and_retries_atomically()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        let common = temp.path().join("common.git");
        let administrative = common.join("worktrees/legacy");
        let root = temp.path().join("legacy");
        for path in [&control, &common, &administrative, &root] {
            fs::create_dir_all(path)?;
        }
        let database = control.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &control)?;
        let common_display = projectatlas_core::normalize_native_path_display(&common);
        let administrative_display =
            projectatlas_core::normalize_native_path_display(&administrative);
        let root_display = projectatlas_core::normalize_native_path_display(&root);
        store.connection.execute(
            "INSERT INTO worktree_registrations(
                alias, state, git_common_directory, git_administrative_directory,
                git_administrative_identity, last_root, created_at_epoch
             ) VALUES('legacy', 'active', ?1, ?2, ?3, ?4, 1)",
            params![
                common_display,
                administrative_display,
                administrative_identity(9),
                root_display,
            ],
        )?;
        crate::schema::drop_worktree_native_identity_schema(&store.connection)?;
        store.connection.execute(
            "UPDATE metadata SET value = '22' WHERE key = 'schema_version'",
            [],
        )?;
        drop(store);

        let migrated = AtlasStore::open_for_project(&database, &control)?;
        let registration = migrated.worktree_registration(&WorktreeAlias::parse("legacy")?)?;
        require_eq(
            &registration.git_common_directory_identity,
            &CanonicalProjectRoot::from_persisted_path(PathBuf::from(common_display.clone()))?,
            "migrated common identity",
        )?;
        require_eq(
            &registration.git_administrative_directory_identity,
            &CanonicalProjectRoot::from_persisted_path(PathBuf::from(
                administrative_display.clone(),
            ))?,
            "migrated administrative identity",
        )?;
        require_eq(
            &registration.last_root_identity,
            &CanonicalProjectRoot::from_persisted_path(PathBuf::from(root_display.clone()))?,
            "migrated root identity",
        )?;
        drop(migrated);

        let failed_database = control.join("failed-projectatlas.db");
        let failed = AtlasStore::open_for_project(&failed_database, &control)?;
        failed.connection.execute(
            "INSERT INTO worktree_registrations(
                alias, state, git_common_directory, git_administrative_directory,
                git_administrative_identity, last_root, created_at_epoch
             ) VALUES('legacy', 'active', ?1, ?2, ?3, ?4, 1)",
            params![
                common_display,
                administrative_display,
                administrative_identity(10),
                root_display,
            ],
        )?;
        crate::schema::drop_worktree_native_identity_schema(&failed.connection)?;
        failed.connection.execute_batch(
            "UPDATE metadata SET value = '22' WHERE key = 'schema_version';
             UPDATE worktree_registrations SET last_root = 'relative';",
        )?;
        let failed_database_path = failed_database;
        drop(failed);
        let migration_result = AtlasStore::open_for_project(&failed_database_path, &control);
        require(
            matches!(&migration_result, Err(DbError::ProjectRootIdentity(_))),
            &format!(
                "injected native identity migration failure was not returned: {:?}",
                migration_result.as_ref().err().map(ToString::to_string)
            ),
        )?;
        let inspect = Connection::open(&failed_database_path)?;
        let marker = inspect.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(&marker, &"22".to_string(), "failed migration marker")?;
        require_eq(
            &inspect.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('worktree_registrations')
                 WHERE name = 'last_root_identity'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &0,
            "failed migration native columns",
        )?;
        inspect.execute(
            "UPDATE worktree_registrations SET last_root = ?1",
            [root_display.as_str()],
        )?;
        let retried = AtlasStore::open_for_project(&failed_database_path, &control)?;
        require_eq(
            &retried.worktree_registrations(false)?.len(),
            &1,
            "retried migration registration",
        )?;
        Ok(())
    }

    #[test]
    fn schema_twentytwo_worktree_identity_migration_rejects_legacy_collisions_atomically()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control = temp.path().join("control");
        let common = temp.path().join("common.git");
        let administrative_a = common.join("worktrees/legacy-a");
        let administrative_b = common.join("worktrees/legacy-b");
        let administrative_distinct = common.join("worktrees/distinct");
        let collision_root = temp.path().join("legacy-\u{fffd}-root");
        let repaired_root = temp.path().join("repaired-root");
        let unaffected_root = temp.path().join("unaffected-root");
        for path in [
            &control,
            &common,
            &administrative_a,
            &administrative_b,
            &administrative_distinct,
            &collision_root,
            &repaired_root,
            &unaffected_root,
        ] {
            fs::create_dir_all(path)?;
        }

        let database = control.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &control)?;
        let common_display = projectatlas_core::normalize_native_path_display(&common);
        let collision_display = projectatlas_core::normalize_native_path_display(&collision_root);
        let repaired_display = projectatlas_core::normalize_native_path_display(&repaired_root);
        let unaffected_display = projectatlas_core::normalize_native_path_display(&unaffected_root);
        crate::schema::drop_worktree_native_identity_schema(&store.connection)?;
        store.connection.execute(
            "UPDATE metadata SET value = '22' WHERE key = 'schema_version'",
            [],
        )?;
        for (alias, administrative, root, identity) in [
            (
                "legacy-a",
                &administrative_a,
                &collision_display,
                administrative_identity(11),
            ),
            (
                "legacy-b",
                &administrative_b,
                &collision_display,
                administrative_identity(12),
            ),
            (
                "distinct",
                &administrative_distinct,
                &unaffected_display,
                administrative_identity(13),
            ),
        ] {
            let administrative_display =
                projectatlas_core::normalize_native_path_display(administrative);
            store.connection.execute(
                "INSERT INTO worktree_registrations(
                    alias, state, git_common_directory, git_administrative_directory,
                    git_administrative_identity, last_root, created_at_epoch
                 ) VALUES(?1, 'active', ?2, ?3, ?4, ?5, 1)",
                params![
                    alias,
                    common_display,
                    administrative_display,
                    identity,
                    root,
                ],
            )?;
        }
        drop(store);

        let migration_result = AtlasStore::open_for_project(&database, &control);
        require(
            matches!(
                &migration_result,
                Err(DbError::WorktreeRegistrationMigrationConflict {
                    field: "last_root_identity",
                    first_registration_id: 1,
                    second_registration_id: 2,
                })
            ),
            &format!(
                "legacy native identity collision was not rejected deterministically: {:?}",
                migration_result.as_ref().err().map(ToString::to_string)
            ),
        )?;

        let inspect = Connection::open(&database)?;
        let marker = inspect.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(&marker, &"22".to_string(), "collision migration marker")?;
        require_eq(
            &inspect.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('worktree_registrations')
                 WHERE name IN (
                     'git_common_directory_identity',
                     'git_administrative_directory_identity',
                     'last_root_identity'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &0,
            "collision migration native columns",
        )?;
        require_eq(
            &inspect.query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type = 'index' AND name IN (
                     'idx_worktree_registrations_active_native_administrative_directory',
                     'idx_worktree_registrations_active_native_root'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &0,
            "collision migration native indexes",
        )?;
        require_eq(
            &inspect.query_row(
                "SELECT COUNT(*) FROM worktree_registrations
                 WHERE state = 'active'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &3,
            "collision migration registered rows",
        )?;
        let legacy_rows = inspect
            .prepare(
                "SELECT alias, last_root FROM worktree_registrations
                 ORDER BY registration_id",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &legacy_rows,
            &vec![
                ("legacy-a".to_string(), collision_display.clone()),
                ("legacy-b".to_string(), collision_display.clone()),
                ("distinct".to_string(), unaffected_display.clone()),
            ],
            "collision migration preserved legacy rows",
        )?;
        inspect.execute(
            "UPDATE worktree_registrations SET last_root = ?1 WHERE alias = 'legacy-b'",
            [repaired_display.as_str()],
        )?;
        drop(inspect);

        let retried = AtlasStore::open_for_project(&database, &control)?;
        let legacy_a = retried.worktree_registration(&WorktreeAlias::parse("legacy-a")?)?;
        let legacy_b = retried.worktree_registration(&WorktreeAlias::parse("legacy-b")?)?;
        let distinct = retried.worktree_registration(&WorktreeAlias::parse("distinct")?)?;
        require_eq(
            &legacy_a.last_root,
            &collision_display,
            "collision migration first row",
        )?;
        require_eq(
            &legacy_b.last_root,
            &repaired_display,
            "collision migration repaired row",
        )?;
        require_eq(
            &distinct.last_root,
            &unaffected_display,
            "collision migration unaffected row",
        )?;
        require(
            legacy_a.last_root_identity != legacy_b.last_root_identity
                && legacy_b.last_root_identity != distinct.last_root_identity
                && legacy_a.last_root_identity != distinct.last_root_identity,
            "collision migration did not preserve distinct native identities",
        )?;
        require_eq(
            &retried.worktree_registrations(false)?.len(),
            &3,
            "collision migration retry registration count",
        )?;
        require_eq(
            &retried.connection.query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type = 'index' AND name IN (
                     'idx_worktree_registrations_active_native_administrative_directory',
                     'idx_worktree_registrations_active_native_root'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &2,
            "collision migration native indexes after retry",
        )?;
        for (identity, index, column) in [
            (
                legacy_a.git_administrative_directory_identity,
                "idx_worktree_registrations_active_native_administrative_directory",
                "git_administrative_directory_identity",
            ),
            (
                legacy_a.last_root_identity,
                "idx_worktree_registrations_active_native_root",
                "last_root_identity",
            ),
        ] {
            let query = format!(
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY {index}
                 WHERE state = 'active' AND {column} = ?1"
            );
            let plan = retried
                .connection
                .prepare(&query)?
                .query_map(params![identity.encode()?], |row| row.get::<_, String>(3))?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                plan.iter().any(|detail| detail.contains(index)),
                &format!("retry query plan omitted {index}: {plan:?}"),
            )?;
        }
        Ok(())
    }

    #[test]
    fn hot_registry_and_aggregate_lookups_use_owning_indexes() -> Result<(), Box<dyn Error>> {
        let connection = Connection::open_in_memory()?;
        crate::schema::initialize(&connection, None)?;
        for (sql, index) in [
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_alias
                 WHERE state = 'active' AND alias = 'issue-430'",
                "idx_worktree_registrations_active_alias",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_administrative_directory
                 WHERE state = 'active' AND git_administrative_directory = 'admin'",
                "idx_worktree_registrations_active_administrative_directory",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_administrative_identity
                 WHERE state = 'active' AND git_administrative_identity = 'identity'",
                "idx_worktree_registrations_active_administrative_identity",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_native_administrative_directory
                 WHERE state = 'active' AND git_administrative_directory_identity = zeroblob(3)",
                "idx_worktree_registrations_active_native_administrative_directory",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_native_root
                 WHERE state = 'active' AND last_root_identity = zeroblob(3)",
                "idx_worktree_registrations_active_native_root",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_registrations
                       INDEXED BY idx_worktree_registrations_active_project
                 WHERE state = 'active' AND project_instance_id = zeroblob(16)",
                "idx_worktree_registrations_active_project",
            ),
            (
                "EXPLAIN QUERY PLAN
                 SELECT registration_id FROM worktree_usage_aggregates
                       INDEXED BY idx_worktree_usage_aggregates_day_registration
                 WHERE day_epoch = -1 AND source_kind = 'synchronized'",
                "idx_worktree_usage_aggregates_day_registration",
            ),
        ] {
            let plan = connection
                .prepare(sql)?
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                plan.iter().any(|detail| detail.contains(index)),
                &format!("query plan omitted {index}: {plan:?}"),
            )?;
        }
        Ok(())
    }
}
