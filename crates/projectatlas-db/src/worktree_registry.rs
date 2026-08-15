//! Durable `ProjectAtlas` worktree registrations owned by one control atlas.

use crate::{
    AtlasStore, DbError, DbResult, WorktreeUsageSnapshot, WorktreeUsageSyncState,
    normalize_metadata_path, telemetry,
};
use projectatlas_core::{MAX_GIT_WORKTREE_REGISTRATIONS, graph::ProjectInstanceId};
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
    /// Structurally validated Git common directory.
    pub git_common_directory: String,
    /// Stable linked-worktree administrative identity.
    pub git_administrative_directory: String,
    /// Opaque identity for the current administrative-directory lifecycle.
    pub git_administrative_identity: String,
    /// Last structurally validated canonical source root.
    pub last_root: String,
    /// Exact worktree atlas identity after initialization.
    pub project_instance_id: Option<ProjectInstanceId>,
    /// Last local aggregate revision accepted by the control atlas.
    pub accepted_telemetry_revision: u64,
    /// Creation time as Unix epoch seconds.
    pub created_at_epoch: u64,
    /// Retirement time as Unix epoch seconds.
    pub retired_at_epoch: Option<u64>,
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
    /// Persisted normalized administrative-directory path.
    git_administrative_directory: String,
    /// Persisted opaque administrative-directory lifecycle identity.
    git_administrative_identity: String,
    /// Persisted last structurally validated root.
    last_root: String,
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
        let git_common_directory =
            normalized_absolute_path("git_common_directory", git_common_directory)?;
        let git_administrative_directory =
            normalized_absolute_path("git_administrative_directory", git_administrative_directory)?;
        let git_administrative_identity =
            validated_administrative_identity(git_administrative_identity)?;
        let root = normalized_absolute_path("root", root)?;
        let created_at_epoch = epoch_to_sqlite(created_at_epoch)?;
        let project_bytes = project_instance_id.map(ProjectInstanceId::as_bytes);

        self.with_validated_write(|transaction| {
            if let Some(existing) = load_active_by_alias(transaction, alias.as_str())? {
                if existing.git_administrative_directory != git_administrative_directory {
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
                transaction.execute(
                    "UPDATE worktree_registrations
                 SET git_common_directory = ?1, last_root = ?2,
                     project_instance_id = COALESCE(project_instance_id, ?3)
                 WHERE registration_id = ?4",
                    params![
                        git_common_directory,
                        root,
                        project_bytes.as_ref().map(<[u8; 16]>::as_slice),
                        existing.registration_id,
                    ],
                )?;
                return load_by_id(transaction, existing.registration_id);
            }

            if active_identity_exists(
                transaction,
                &git_administrative_directory,
                &git_administrative_identity,
                project_bytes.as_ref(),
            )? {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "git_or_project_identity",
                    value: git_administrative_directory,
                });
            }

            let retired_id = load_matching_retired_id(
                transaction,
                &git_administrative_directory,
                &git_administrative_identity,
                project_bytes.as_ref(),
            )?;
            let registration_id = if let Some(registration_id) = retired_id {
                transaction.execute(
                    "UPDATE worktree_registrations
                 SET alias = ?1, state = 'active', git_common_directory = ?2,
                     git_administrative_identity = ?3, last_root = ?4,
                     project_instance_id = ?5, retired_at_epoch = NULL
                 WHERE registration_id = ?6",
                    params![
                        alias.as_str(),
                        git_common_directory,
                        git_administrative_identity,
                        root,
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
                    alias, state, git_common_directory, git_administrative_directory,
                    git_administrative_identity, last_root, project_instance_id,
                    created_at_epoch
                 ) VALUES(?1, 'active', ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        alias.as_str(),
                        git_common_directory,
                        git_administrative_directory,
                        git_administrative_identity,
                        root,
                        project_bytes.as_ref().map(<[u8; 16]>::as_slice),
                        created_at_epoch,
                    ],
                )?;
                transaction.last_insert_rowid()
            };
            load_by_id(transaction, registration_id)
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
                    git_administrative_directory, git_administrative_identity,
                    last_root, project_instance_id,
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
            crate::with_validated_write_transaction(
                connection,
                Some(&binding.project_root),
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

    /// Bind an initialized worktree identity and its latest validated root.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent alias, conflicting active project
    /// identity, invalid path, malformed state, or transactional `SQLite` failure.
    pub fn bind_worktree_project(
        &self,
        alias: &WorktreeAlias,
        root: &Path,
        project_instance_id: ProjectInstanceId,
    ) -> DbResult<WorktreeRegistration> {
        let root = normalized_absolute_path("root", root)?;
        let project_bytes = project_instance_id.as_bytes();
        self.with_validated_write(|transaction| {
            let existing = load_active_by_alias(transaction, alias.as_str())?.ok_or_else(|| {
                DbError::WorktreeRegistrationNotFound {
                    alias: alias.to_string(),
                }
            })?;
            if identities_conflict(existing.project_instance_id, Some(project_instance_id)) {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    value: project_instance_id.to_string(),
                });
            }
            if active_project_exists_for_other(
                transaction,
                existing.registration_id,
                project_bytes.as_slice(),
            )? {
                return Err(DbError::WorktreeRegistrationConflict {
                    field: "project_instance_id",
                    value: project_instance_id.to_string(),
                });
            }
            transaction.execute(
                "UPDATE worktree_registrations
             SET last_root = ?1, project_instance_id = ?2
             WHERE registration_id = ?3",
                params![root, project_bytes.as_slice(), existing.registration_id],
            )?;
            load_by_id(transaction, existing.registration_id)
        })
    }

    /// Retire one active alias without deleting its aggregate history.
    ///
    /// The caller owns required final telemetry synchronization before invoking
    /// this storage transition.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent alias, invalid time, malformed state, or
    /// transactional `SQLite` failure.
    pub fn retire_worktree(
        &self,
        alias: &WorktreeAlias,
        retired_at_epoch: u64,
    ) -> DbResult<WorktreeRegistration> {
        let retired_at_epoch = epoch_to_sqlite(retired_at_epoch)?;
        self.with_validated_write(|transaction| {
            let existing = load_active_by_alias(transaction, alias.as_str())?.ok_or_else(|| {
                DbError::WorktreeRegistrationNotFound {
                    alias: alias.to_string(),
                }
            })?;
            retire_registration(transaction, &existing, retired_at_epoch)
        })
    }

    /// Synchronize one writer-excluded local snapshot and retire its alias atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent alias, mismatched snapshot identity, invalid
    /// time or aggregate state, changed control binding, or transactional `SQLite`
    /// failure. Synchronization and retirement roll back together.
    pub fn retire_worktree_with_usage_snapshot(
        &self,
        alias: &WorktreeAlias,
        snapshot: &WorktreeUsageSnapshot,
        retired_at_epoch: u64,
    ) -> DbResult<(WorktreeRegistration, WorktreeUsageSyncState)> {
        let retired_at_epoch = epoch_to_sqlite(retired_at_epoch)?;
        self.with_validated_write(|transaction| {
            let existing = load_active_by_alias(transaction, alias.as_str())?.ok_or_else(|| {
                DbError::WorktreeRegistrationNotFound {
                    alias: alias.to_string(),
                }
            })?;
            let synchronized = telemetry::synchronize_worktree_usage_snapshot(
                transaction,
                existing.registration_id,
                snapshot,
            )?;
            let retired = retire_registration(transaction, &existing, retired_at_epoch)?;
            Ok((retired, synchronized))
        })
    }
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
    connection.execute(
        "UPDATE worktree_registrations
         SET state = 'retired', retired_at_epoch = ?1
         WHERE registration_id = ?2",
        params![retired_at_epoch, registration.registration_id],
    )?;
    load_by_id(connection, registration.registration_id)
}

/// Build one typed public alias-validation error.
fn invalid_alias<T>(value: &str, reason: &'static str) -> DbResult<T> {
    Err(DbError::InvalidWorktreeAlias {
        alias: value.to_string(),
        reason,
    })
}

/// Normalize one caller-validated absolute path into bounded metadata text.
fn normalized_absolute_path(field: &'static str, path: &Path) -> DbResult<String> {
    let normalized = normalize_metadata_path(path);
    if !path.is_absolute() || normalized.len() > MAX_WORKTREE_REGISTRATION_PATH_BYTES {
        return Err(DbError::InvalidWorktreeRegistrationPath {
            field,
            path: normalized,
        });
    }
    Ok(normalized)
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
        git_administrative_directory: row.get(4)?,
        git_administrative_identity: row.get(5)?,
        last_root: row.get(6)?,
        project_instance_id: row.get(7)?,
        accepted_telemetry_revision: row.get(8)?,
        created_at_epoch: row.get(9)?,
        retired_at_epoch: row.get(10)?,
    })
}

/// Validate and convert one persisted registration row.
fn try_registration(row: PersistedWorktreeRegistration) -> DbResult<WorktreeRegistration> {
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
        git_administrative_directory: row.git_administrative_directory,
        git_administrative_identity: validated_administrative_identity(
            &row.git_administrative_identity,
        )?,
        last_root: row.last_root,
        project_instance_id,
        accepted_telemetry_revision,
        created_at_epoch,
        retired_at_epoch,
    })
}

/// Common typed registration projection shared by bounded lookups.
const REGISTRATION_SELECT: &str = "SELECT registration_id, alias, state, git_common_directory,
            git_administrative_directory, git_administrative_identity, last_root,
            project_instance_id,
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

/// Check active Git and initialized-project identity conflicts through owned indexes.
fn active_identity_exists(
    connection: &Connection,
    administrative_directory: &str,
    administrative_identity: &str,
    project_instance_id: Option<&[u8; 16]>,
) -> DbResult<bool> {
    let administrative_identity_exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worktree_registrations
                 INDEXED BY idx_worktree_registrations_active_administrative_directory
            WHERE state = 'active' AND git_administrative_directory = ?1
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
    let Some(project_instance_id) = project_instance_id else {
        return Ok(false);
    };
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM worktree_registrations
                     INDEXED BY idx_worktree_registrations_active_project
                WHERE state = 'active' AND project_instance_id = ?1
             )",
            [project_instance_id.as_slice()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(DbError::from)
}

/// Check whether another active row owns one initialized project identity.
fn active_project_exists_for_other(
    connection: &Connection,
    registration_id: i64,
    project_instance_id: &[u8],
) -> DbResult<bool> {
    let found = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worktree_registrations
            WHERE state = 'active' AND registration_id <> ?1
              AND project_instance_id = ?2
         )",
        params![registration_id, project_instance_id],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(found)
}

/// Find the newest retired history row with the exact same stable identities.
fn load_matching_retired_id(
    connection: &Connection,
    administrative_directory: &str,
    administrative_identity: &str,
    project_instance_id: Option<&[u8; 16]>,
) -> DbResult<Option<i64>> {
    connection
        .query_row(
            "SELECT registration_id
             FROM worktree_registrations
             WHERE state = 'retired'
               AND git_administrative_directory = ?1
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

    /// Return a test error instead of panicking inside a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
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

        let retired = store.retire_worktree(&alias, 20)?;
        require_eq(
            &retired.state,
            &WorktreeRegistrationState::Retired,
            "retired state",
        )?;
        let replacement = store.register_worktree(
            &alias,
            &common,
            &first_admin,
            &administrative_identity(2),
            &second_root,
            Some(identity(2)?),
            21,
        )?;
        require(
            replacement.registration_id != first.registration_id,
            "replacement reused unrelated retired history",
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
