//! Durable project identity and explicit root-binding transitions.

use super::{AtlasStore, DbError, DbResult, normalize_metadata_path, set_metadata};
use crate::schema::{self, PROJECT_ROOT_KEY, SchemaState};
use projectatlas_core::graph::ProjectInstanceId;
use projectatlas_core::{CanonicalProjectRoot, IndexGeneration};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::Path;

/// Maximum attempts to obtain a nonzero identity distinct from an existing one.
const PROJECT_IDENTITY_GENERATION_ATTEMPTS: usize = 8;

/// Explicit root-binding behavior selected by a caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectRootTransition {
    /// Initialize a missing binding or verify an identical existing binding.
    Bind,
    /// Preserve identity while moving a database whose previous root is absent.
    Move,
    /// Rotate identity for an independent copy, clone, or worktree.
    Detach,
}

/// Result of one completed root transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRootTransitionResult {
    /// Transition selected by the caller.
    pub transition: ProjectRootTransition,
    /// Lossless UTF-8 display of the native root stored before the transition,
    /// when one existed and had a display projection.
    pub previous_root: Option<String>,
    /// Lossless UTF-8 display of the canonical root stored after the
    /// transition, when one exists.
    ///
    /// `None` is the typed unavailable state for a native non-UTF-8 root.
    pub project_root: Option<String>,
    /// Durable identity owned by the destination after the transition.
    pub project_instance_id: ProjectInstanceId,
    /// Whether this operation created or replaced the project identity.
    pub identity_changed: bool,
    /// Whether derived publication trust was invalidated.
    pub publication_invalidated: bool,
}

impl AtlasStore {
    /// Apply an explicit root-binding transition to one database path.
    ///
    /// `destination` must be an absolute existing project directory and is
    /// canonicalized before database preflight. `Bind` preserves the old compatible behavior. `Move` preserves identity
    /// only after the recorded root is proven absent. `Detach` rotates identity
    /// and discards project-qualified graph rows while preserving authored data.
    ///
    /// # Errors
    ///
    /// Returns an error for incompatible storage, an implicit rebind, an
    /// unproven move, a concurrent transition, identity corruption, or any
    /// transactional `SQLite` failure.
    pub fn transition_project_root(
        database_path: &Path,
        destination: &Path,
        transition: ProjectRootTransition,
    ) -> DbResult<ProjectRootTransitionResult> {
        let destination_identity = validate_project_root_destination(destination)?;
        let destination = destination_identity.display_string().ok();
        let (preflight, _) = schema::preflight(database_path, None)?;
        let previous_root = preflight.project_root.clone();
        let previous_identity = preflight.project_instance_id;
        let previous_root_identity = if preflight.state == SchemaState::Current {
            read_current_project_root_identity(database_path)?
        } else if (transition == ProjectRootTransition::Bind
            || transition == ProjectRootTransition::Detach)
            && preflight.state == SchemaState::UpgradeRequired
        {
            previous_root
                .as_deref()
                .map(Path::new)
                .map(CanonicalProjectRoot::from_path)
                .transpose()?
        } else {
            None
        };
        match transition {
            ProjectRootTransition::Bind => {
                if let Some(found) = previous_root_identity.as_ref() {
                    prove_existing_root_equivalence(
                        destination_identity.as_path(),
                        found.as_path(),
                    )?;
                } else if let Some(found) = previous_root.as_deref() {
                    prove_existing_root_equivalence(
                        destination_identity.as_path(),
                        Path::new(found),
                    )?;
                }
                let store = Self::open_for_project(database_path, destination_identity.as_path())?;
                let project_instance_id = store
                    .project_instance_id()?
                    .ok_or(DbError::ProjectInstanceIdentityMissing)?;
                Ok(ProjectRootTransitionResult {
                    transition,
                    previous_root: previous_root_identity
                        .as_ref()
                        .and_then(|root| root.display_string().ok()),
                    project_root: destination,
                    project_instance_id,
                    identity_changed: previous_identity != Some(project_instance_id),
                    publication_invalidated: false,
                })
            }
            ProjectRootTransition::Move | ProjectRootTransition::Detach => {
                let previous_root_identity = match previous_root_identity.as_ref() {
                    Some(identity) => identity,
                    None if preflight.state == SchemaState::UpgradeRequired => {
                        return Err(DbError::ProjectRootIdentityMissing);
                    }
                    None => return Err(DbError::ProjectRootTransitionRequiresExistingRoot),
                };
                if transition == ProjectRootTransition::Move {
                    if previous_root_identity == &destination_identity {
                        return Err(DbError::ProjectRootTransitionRequiresDifferentRoot {
                            root: destination_identity.display_string_lossy(),
                        });
                    }
                    verify_root_absent(previous_root_identity)?;
                }

                let mut store = Self::open_for_root_transition(database_path)?;
                let opened_identity = store.project_instance_id()?;
                if previous_identity.is_some() && opened_identity != previous_identity {
                    return Err(project_transition_changed(
                        previous_root_identity.display_string().ok(),
                        store.project_root()?,
                        previous_identity,
                        opened_identity,
                    ));
                }
                let mut result = apply_root_transition(
                    &mut store,
                    transition,
                    Some(previous_root_identity),
                    opened_identity,
                    &destination_identity,
                )?;
                result.identity_changed = previous_identity != Some(result.project_instance_id);
                Ok(result)
            }
        }
    }

    /// Return the durable project instance identity, when initialized.
    ///
    /// # Errors
    ///
    /// Returns an error when the singleton row is malformed or cannot be read.
    pub fn project_instance_id(&self) -> DbResult<Option<ProjectInstanceId>> {
        load_project_identity(&self.connection)
    }
}

/// Validate and canonicalize a transition destination before touching its database.
fn validate_project_root_destination(destination: &Path) -> DbResult<CanonicalProjectRoot> {
    let root = normalize_metadata_path(destination);
    if !destination.is_absolute() {
        return Err(DbError::ProjectRootDestinationInvalid {
            root,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project root destination is not absolute",
            ),
        });
    }
    CanonicalProjectRoot::from_path(destination).map_err(|error| match error {
        projectatlas_core::CoreError::CanonicalProjectRootIo { source, .. } => {
            DbError::ProjectRootDestinationInvalid { root, source }
        }
        projectatlas_core::CoreError::InvalidCanonicalProjectRoot { reason, .. } => {
            DbError::ProjectRootDestinationInvalid {
                root,
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, reason),
            }
        }
        other => DbError::from(other),
    })
}

/// Apply move or detach after non-mutating preflight has captured expected state.
fn apply_root_transition(
    store: &mut AtlasStore,
    transition: ProjectRootTransition,
    expected_root_identity: Option<&CanonicalProjectRoot>,
    expected_identity: Option<ProjectInstanceId>,
    destination: &CanonicalProjectRoot,
) -> DbResult<ProjectRootTransitionResult> {
    store.connection.execute_batch("BEGIN IMMEDIATE")?;
    let operation = (|| {
        let found_root = store.project_root()?;
        let found_root_identity = load_project_root_identity(&store.connection)?;
        let found_identity = load_project_identity(&store.connection)?;
        if found_root_identity.as_ref() != expected_root_identity
            || found_identity != expected_identity
        {
            return Err(project_transition_changed(
                expected_root_identity.map(CanonicalProjectRoot::display_string_lossy),
                found_root,
                expected_identity,
                found_identity,
            ));
        }

        if transition == ProjectRootTransition::Detach
            && let Some(previous_identity) = found_identity
        {
            crate::telemetry::seal_project_usage_instances(&store.connection, previous_identity)?;
        }
        set_project_root_metadata(&store.connection, destination)?;
        set_project_root_identity(&store.connection, destination)?;
        schema::invalidate_derived_publication(&store.connection)?;
        let (project_instance_id, identity_changed) = match transition {
            ProjectRootTransition::Bind => unreachable!("bind does not use transition mutation"),
            ProjectRootTransition::Move => {
                let (identity, identity_changed) = ensure_project_identity(&store.connection)?;
                set_graph_generation(&store.connection, IndexGeneration::ZERO)?;
                (identity, identity_changed)
            }
            ProjectRootTransition::Detach => {
                store
                    .connection
                    .execute("DELETE FROM graph_resolution_keys", [])?;
                store.connection.execute("DELETE FROM graph_coverage", [])?;
                store
                    .connection
                    .execute("DELETE FROM graph_relations", [])?;
                store.connection.execute("DELETE FROM graph_entities", [])?;
                let identity = generate_project_identity(&store.connection, found_identity)?;
                set_project_identity(&store.connection, identity)?;
                (identity, true)
            }
        };

        Ok(ProjectRootTransitionResult {
            transition,
            previous_root: expected_root_identity.and_then(|root| root.display_string().ok()),
            project_root: destination.display_string().ok(),
            project_instance_id,
            identity_changed,
            publication_invalidated: true,
        })
    })();
    match operation {
        Ok(result) => {
            if let Err(source) = store.connection.execute_batch("COMMIT") {
                return Err(schema::rollback_after_error(
                    &store.connection,
                    DbError::Sqlite(source),
                ));
            }
            store.validated_project_root = destination.display_string().ok();
            store.validated_project_root_identity = Some(destination.clone());
            store.validated_project_instance_id = Some(result.project_instance_id);
            if result.identity_changed {
                store.library_usage_instances.get_mut().clear();
            }
            Ok(result)
        }
        Err(error) => Err(schema::rollback_after_error(&store.connection, error)),
    }
}

/// Prove the recorded old root path entry is absent without following links.
fn verify_root_absent(root: &CanonicalProjectRoot) -> DbResult<()> {
    classify_root_absence(
        &root.display_string_lossy(),
        fs::symlink_metadata(root.as_path()).map(|_| ()),
    )
}

/// Classify a non-following filesystem probe without weakening uncertain failures.
fn classify_root_absence(root: &str, result: std::io::Result<()>) -> DbResult<()> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(()) => Err(DbError::ProjectRootStillPresent {
            root: root.to_string(),
        }),
        Err(source) => Err(DbError::ProjectRootAbsenceUncertain {
            root: root.to_string(),
            source,
        }),
    }
}

/// Load a current database's native project-root identity without mutation.
fn read_current_project_root_identity(path: &Path) -> DbResult<Option<CanonicalProjectRoot>> {
    let (connection, _) = schema::open_current_read_only(path, None)?;
    load_project_root_identity(&connection)
}

/// Read and validate the project singleton identity.
pub(crate) fn load_project_identity(
    connection: &Connection,
) -> DbResult<Option<ProjectInstanceId>> {
    let bytes = connection
        .query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    bytes.map(project_identity_from_blob).transpose()
}

/// Load the lossless native project-root identity owned by the database.
pub(crate) fn load_project_root_identity(
    connection: &Connection,
) -> DbResult<Option<CanonicalProjectRoot>> {
    let encoded = connection
        .query_row(
            "SELECT codec_version, root FROM project_root_identity WHERE singleton = 1",
            [],
            |row| {
                let version = row.get::<_, i64>(0)?;
                let root = row.get::<_, Vec<u8>>(1)?;
                Ok((version, root))
            },
        )
        .optional()?;
    encoded
        .map(|(version, root)| {
            if version
                != i64::from(projectatlas_core::project_root::CANONICAL_PROJECT_ROOT_CODEC_VERSION)
            {
                return Err(DbError::ProjectRootIdentity(
                    projectatlas_core::CoreError::CanonicalProjectRootCodec {
                        reason: "unsupported codec version",
                    },
                ));
            }
            CanonicalProjectRoot::decode(&root).map_err(DbError::ProjectRootIdentity)
        })
        .transpose()
}

/// Insert or replace the native project-root identity in the caller's transaction.
pub(crate) fn set_project_root_identity(
    connection: &Connection,
    identity: &CanonicalProjectRoot,
) -> DbResult<()> {
    let encoded = identity.encode()?;
    connection.execute(
        "INSERT INTO project_root_identity(singleton, codec_version, root)
         VALUES(1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET
            codec_version = excluded.codec_version,
            root = excluded.root",
        rusqlite::params![
            i64::from(projectatlas_core::project_root::CANONICAL_PROJECT_ROOT_CODEC_VERSION),
            encoded,
        ],
    )?;
    Ok(())
}

/// Re-canonicalize two existing roots and return the fresh selected identity
/// only when their native paths are exactly equal.
///
/// This is intentionally an admission-only proof. It must not be used for a
/// move's recorded-root absence check: a move has a different contract and
/// requires the old native path to remain an exact, absent witness.
pub(crate) fn prove_existing_root_equivalence(
    selected: &Path,
    persisted: &Path,
) -> DbResult<CanonicalProjectRoot> {
    let selected = CanonicalProjectRoot::from_path(selected)?;
    let persisted = CanonicalProjectRoot::from_path(persisted)?;
    if selected.as_path() != persisted.as_path() {
        return Err(DbError::ProjectRootMismatch {
            expected: selected.display_string_lossy(),
            found: persisted.display_string_lossy(),
        });
    }
    Ok(selected)
}

/// Validate and atomically repair the root metadata and native identity.
pub(crate) fn ensure_project_root_identity(
    connection: &Connection,
    expected: &CanonicalProjectRoot,
) -> DbResult<()> {
    let metadata = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [PROJECT_ROOT_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(found) = load_project_root_identity(connection)? {
        let selected = prove_existing_root_equivalence(expected.as_path(), found.as_path())?;
        if metadata.as_deref() == selected.display_string().ok().as_deref() {
            return Ok(());
        }
    }
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = ensure_project_root_identity_in_transaction(connection, expected);
    match result {
        Ok(()) => connection.execute_batch("COMMIT").map_err(Into::into),
        Err(error) => Err(schema::rollback_after_error(connection, error)),
    }
}

/// Repair root identity while the caller already owns the transaction.
pub(crate) fn ensure_project_root_identity_in_transaction(
    connection: &Connection,
    expected: &CanonicalProjectRoot,
) -> DbResult<()> {
    let found_metadata = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [PROJECT_ROOT_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let found_identity = load_project_root_identity(connection)?;
    let selected = if let Some(found) = found_identity.as_ref() {
        prove_existing_root_equivalence(expected.as_path(), found.as_path())?
    } else {
        let Some(legacy) = found_metadata.as_deref() else {
            return Err(DbError::ProjectRootMissing);
        };
        prove_existing_root_equivalence(expected.as_path(), Path::new(legacy))?
    };
    set_project_root_identity(connection, &selected)?;
    set_project_root_metadata(connection, &selected)?;
    Ok(())
}

/// Keep the legacy text projection only when it is lossless UTF-8.
///
/// Native identity remains authoritative for every current binding. An
/// unrepresentable root clears the compatibility metadata rather than
/// persisting replacement characters that could name a different directory.
pub(crate) fn set_project_root_metadata(
    connection: &Connection,
    identity: &CanonicalProjectRoot,
) -> DbResult<()> {
    if let Ok(display) = identity.display_string() {
        set_metadata(connection, PROJECT_ROOT_KEY, &display)?;
    } else {
        connection.execute("DELETE FROM metadata WHERE key = ?1", [PROJECT_ROOT_KEY])?;
    }
    Ok(())
}

/// Read the typed graph generation owned by the project singleton.
pub(crate) fn load_graph_generation(connection: &Connection) -> DbResult<Option<IndexGeneration>> {
    let generation = connection
        .query_row(
            "SELECT active_generation FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    generation
        .map(|value| {
            let value = u64::try_from(value).map_err(|source| DbError::InvalidCount {
                field: "project_identity.active_generation",
                value,
                source,
            })?;
            Ok(IndexGeneration::new(value))
        })
        .transpose()
}

/// Return whether the project singleton exists and matches the selected identity.
pub(crate) fn verify_project_identity(
    connection: &Connection,
    expected: ProjectInstanceId,
) -> DbResult<bool> {
    let Some(found) = load_project_identity(connection)? else {
        return Ok(false);
    };
    require_project_identity(expected, found)?;
    Ok(true)
}

/// Require the already-bound destination identity for graph publication.
pub(crate) fn require_bound_project_identity(
    connection: &Connection,
    expected: ProjectInstanceId,
) -> DbResult<()> {
    let found =
        load_project_identity(connection)?.ok_or(DbError::ProjectInstanceIdentityMissing)?;
    require_project_identity(expected, found)
}

/// Create a project identity when a bound database has not initialized one yet.
pub(crate) fn ensure_project_identity(
    connection: &Connection,
) -> DbResult<(ProjectInstanceId, bool)> {
    if let Some(identity) = load_project_identity(connection)? {
        return Ok((identity, false));
    }
    let identity = generate_project_identity(connection, None)?;
    set_project_identity(connection, identity)?;
    Ok((identity, true))
}

/// Set the graph generation after a validated publication or invalidation.
pub(crate) fn set_graph_generation(
    connection: &Connection,
    generation: IndexGeneration,
) -> DbResult<()> {
    let generation =
        i64::try_from(generation.get()).map_err(|_source| DbError::GraphCountOverflow {
            field: "project_identity.active_generation",
            value: generation.get(),
        })?;
    connection.execute(
        "UPDATE project_identity SET active_generation = ?1 WHERE singleton = 1",
        [generation],
    )?;
    Ok(())
}

/// Generate a nonzero SQLite-owned identity distinct from an optional predecessor.
fn generate_project_identity(
    connection: &Connection,
    predecessor: Option<ProjectInstanceId>,
) -> DbResult<ProjectInstanceId> {
    for _ in 0..PROJECT_IDENTITY_GENERATION_ATTEMPTS {
        let bytes =
            connection.query_row("SELECT randomblob(16)", [], |row| row.get::<_, Vec<u8>>(0))?;
        if let Ok(identity) = project_identity_from_blob(bytes)
            && Some(identity) != predecessor
        {
            return Ok(identity);
        }
    }
    Err(DbError::ProjectInstanceIdentityGenerationFailed)
}

/// Insert or replace the singleton identity after dependent graph rows are removed.
pub(crate) fn set_project_identity(
    connection: &Connection,
    identity: ProjectInstanceId,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO project_identity(singleton, project_instance_id, active_generation)
         VALUES(1, ?1, 0)
         ON CONFLICT(singleton) DO UPDATE SET
            project_instance_id = excluded.project_instance_id,
            active_generation = 0",
        [&identity.as_bytes()[..]],
    )?;
    Ok(())
}

/// Convert the fixed persisted identity representation into its domain newtype.
fn project_identity_from_blob(value: Vec<u8>) -> DbResult<ProjectInstanceId> {
    let found = value.len();
    let bytes: [u8; 16] = value
        .try_into()
        .map_err(|_value| DbError::InvalidBlobLength {
            field: "project_identity.project_instance_id",
            expected: 16,
            found,
        })?;
    ProjectInstanceId::from_bytes(bytes).map_err(Into::into)
}

/// Require identical project ownership with a typed mismatch diagnostic.
fn require_project_identity(expected: ProjectInstanceId, found: ProjectInstanceId) -> DbResult<()> {
    if expected != found {
        return Err(DbError::GraphProjectIdentityMismatch {
            expected: expected.to_string(),
            found: found.to_string(),
        });
    }
    Ok(())
}

/// Build a typed concurrent-transition failure with both captured states.
fn project_transition_changed(
    expected_root: Option<String>,
    found_root: Option<String>,
    expected_identity: Option<ProjectInstanceId>,
    found_identity: Option<ProjectInstanceId>,
) -> DbError {
    DbError::ProjectRootTransitionChanged {
        expected_root,
        found_root,
        expected_identity: expected_identity.map(|identity| identity.to_string()),
        found_identity: found_identity.map(|identity| identity.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HealthResolution;
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
        CoverageState, EntityResolutionKey, EntitySelector, GraphEntity, GraphIdentityText,
        GraphRelationKind, LogicalRelation, RelationDependencyKey, RelationOccurrence,
        RelationResolution, RepositoryFilePath, ResolutionKeyDomain, SourceSpan,
    };
    use projectatlas_core::symbols::RelationKind;
    use projectatlas_core::telemetry::usage_from_estimates;
    use std::error::Error;
    use std::fmt::Debug;
    use std::io;

    #[cfg(unix)]
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn canonical_root_repairs_equivalent_alias_without_changing_project_identity()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let target = temp.path().join("private-var");
        let alias = temp.path().join("var");
        fs::create_dir(&target)?;
        symlink(&target, &alias)?;
        let database = temp.path().join("projectatlas.db");
        let initial = AtlasStore::open_for_project(&database, &target)?;
        let project = initial
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("fresh identity is missing"))?;
        drop(initial);

        let connection = Connection::open(&database)?;
        connection.execute(
            "UPDATE metadata SET value = ?1 WHERE key = ?2",
            rusqlite::params![normalize_metadata_path(&alias), PROJECT_ROOT_KEY],
        )?;
        connection.execute("DELETE FROM project_root_identity", [])?;
        drop(connection);

        let repaired = AtlasStore::open_for_project(&database, &alias)?;
        require_eq(
            &repaired.project_instance_id()?,
            &Some(project),
            "alias repair project identity",
        )?;
        require_eq(
            &repaired.project_root()?,
            &Some(normalize_metadata_path(&target)),
            "alias repair display metadata",
        )?;
        require_eq(
            &repaired.project_root_identity()?,
            &Some(CanonicalProjectRoot::from_path(&target)?),
            "alias repair native identity",
        )?;
        let plan = repaired.connection.query_row(
            "EXPLAIN QUERY PLAN
             SELECT root FROM project_root_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(3),
        )?;
        assert!(
            plan.contains("INTEGER PRIMARY KEY"),
            "unexpected root identity plan: {plan}"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn canonical_root_repair_rolls_back_when_identity_write_fails() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let target = temp.path().join("private-var");
        let alias = temp.path().join("var");
        fs::create_dir(&target)?;
        symlink(&target, &alias)?;
        let database = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&database, &target)?);
        let connection = Connection::open(&database)?;
        connection.execute(
            "UPDATE metadata SET value = ?1 WHERE key = ?2",
            rusqlite::params![normalize_metadata_path(&alias), PROJECT_ROOT_KEY],
        )?;
        connection.execute("DELETE FROM project_root_identity", [])?;
        connection.execute_batch(
            "CREATE TEMP TRIGGER fail_project_root_identity_insert
             BEFORE INSERT ON project_root_identity
             BEGIN SELECT RAISE(ABORT, 'injected root identity failure'); END;",
        )?;
        let expected = CanonicalProjectRoot::from_path(&alias)?;
        let error = ensure_project_root_identity(&connection, &expected)
            .err()
            .ok_or_else(|| io::Error::other("repair unexpectedly succeeded"))?;
        assert!(matches!(error, DbError::Sqlite(_)));
        let metadata = connection.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [PROJECT_ROOT_KEY],
            |row| row.get::<_, String>(0),
        )?;
        let identity_rows =
            connection.query_row("SELECT COUNT(*) FROM project_root_identity", [], |row| {
                row.get::<_, i64>(0)
            })?;
        assert_eq!(metadata, normalize_metadata_path(&alias));
        assert_eq!(identity_rows, 0);
        drop(connection);

        let connection = Connection::open(&database)?;
        let reopened_metadata = connection.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [PROJECT_ROOT_KEY],
            |row| row.get::<_, String>(0),
        )?;
        let reopened_identity_rows =
            connection.query_row("SELECT COUNT(*) FROM project_root_identity", [], |row| {
                row.get::<_, i64>(0)
            })?;
        assert_eq!(reopened_metadata, normalize_metadata_path(&alias));
        assert_eq!(reopened_identity_rows, 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn root_bind_accepts_equivalent_native_alias() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let target = temp.path().join("private-var");
        let alias = temp.path().join("var");
        fs::create_dir(&target)?;
        symlink(&target, &alias)?;
        let database = temp.path().join("projectatlas.db");
        let initial = AtlasStore::open_for_project(&database, &target)?;
        let project = initial
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("project identity missing"))?;
        drop(initial);
        let connection = Connection::open(&database)?;
        connection.execute(
            "UPDATE metadata SET value = ?1 WHERE key = ?2",
            rusqlite::params![normalize_metadata_path(&alias), PROJECT_ROOT_KEY],
        )?;
        drop(connection);

        let rebound =
            AtlasStore::transition_project_root(&database, &alias, ProjectRootTransition::Bind)?;
        assert_eq!(rebound.project_instance_id, project);
        assert!(!rebound.identity_changed);
        Ok(())
    }

    #[test]
    fn root_transitions_preserve_authored_state_and_isolate_copies() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root_a = temp.path().join("source-Δ");
        let root_b = temp.path().join("detached-copy");
        let root_c = temp.path().join("moved-source");
        fs::create_dir_all(&root_a)?;
        fs::create_dir_all(&root_b)?;
        fs::create_dir_all(&root_c)?;
        let source_db = temp.path().join("source.db");
        let copy_db = temp.path().join("copy.db");
        let rollback_db = temp.path().join("rollback.db");

        let bound =
            AtlasStore::transition_project_root(&source_db, &root_a, ProjectRootTransition::Bind)?;
        require(
            bound.previous_root.is_none(),
            "fresh bind had a previous root",
        )?;
        require(bound.identity_changed, "fresh bind did not create identity")?;
        require(
            !bound.publication_invalidated,
            "fresh bind invalidated publication",
        )?;
        let source_identity = bound.project_instance_id;
        let rebound =
            AtlasStore::transition_project_root(&source_db, &root_a, ProjectRootTransition::Bind)?;
        require_eq(
            &rebound.project_instance_id,
            &source_identity,
            "same-root bind identity",
        )?;
        require(!rebound.identity_changed, "same-root bind changed identity")?;

        let mut source = AtlasStore::open_for_project(&source_db, &root_a)?;
        seed_authored_and_graph_state(&mut source, source_identity)?;
        let foreign_identity = ProjectInstanceId::from_bytes([0x7a; 16])?;
        let foreign_project = GraphEntity::new(
            foreign_identity,
            EntitySelector::Project,
            IndexGeneration::new(2),
        )?;
        let mut rejected_publication = source.begin_index_publication("foreign-project")?;
        let foreign_error = require_error(
            rejected_publication.replace_repository_graph(
                foreign_identity,
                &[foreign_project],
                &[],
                &[],
                &[],
            ),
            "foreign graph publication replaced destination identity",
        )?;
        require(
            matches!(foreign_error, DbError::GraphProjectIdentityMismatch { .. }),
            "foreign graph publication returned the wrong error",
        )?;
        drop(rejected_publication);
        source
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(source);
        fs::copy(&source_db, &copy_db)?;
        fs::copy(&source_db, &rollback_db)?;
        let copied_bytes = fs::read(&copy_db)?;

        let invalid_file_root = temp.path().join("not-a-project-directory");
        let missing_root = temp.path().join("missing-project-root");
        fs::write(&invalid_file_root, "not a directory")?;
        for (destination, label) in [
            (Path::new("relative-project-root"), "relative destination"),
            (missing_root.as_path(), "missing destination"),
            (invalid_file_root.as_path(), "file destination"),
        ] {
            let invalid_error = require_error(
                AtlasStore::transition_project_root(
                    &copy_db,
                    destination,
                    ProjectRootTransition::Detach,
                ),
                "invalid transition destination was accepted",
            )?;
            require(
                matches!(invalid_error, DbError::ProjectRootDestinationInvalid { .. }),
                "invalid destination returned the wrong error",
            )?;
            assert_database_unchanged(&copy_db, &copied_bytes, label)?;
        }

        let bind_error = require_error(
            AtlasStore::transition_project_root(&copy_db, &root_b, ProjectRootTransition::Bind),
            "copied database accepted an implicit rebind",
        )?;
        require(
            matches!(bind_error, DbError::ProjectRootMismatch { .. }),
            "copied database bind returned the wrong error",
        )?;
        assert_database_unchanged(&copy_db, &copied_bytes, "rejected bind database")?;
        let move_error = require_error(
            AtlasStore::transition_project_root(&copy_db, &root_b, ProjectRootTransition::Move),
            "copy preserved identity while original root was accessible",
        )?;
        require(
            matches!(move_error, DbError::ProjectRootStillPresent { .. }),
            "accessible-root move returned the wrong error",
        )?;
        assert_database_unchanged(&copy_db, &copied_bytes, "rejected move database")?;
        let rejected_store = AtlasStore::open_read_only_for_project(&copy_db, &root_a)?;
        require_eq(
            &rejected_store.project_instance_id()?,
            &Some(source_identity),
            "rejected transition identity",
        )?;
        assert_authored_state(&rejected_store)?;
        assert_usage_report(&rejected_store, true)?;
        assert_runtime_scope(&rejected_store, source_identity, 1, 0, 1)?;
        assert_graph_counts(&rejected_store, [2, 1, 1, 1, 1, 1, 1])?;
        require(
            rejected_store.index_publication()?.is_some(),
            "rejected transition invalidated publication",
        )?;
        drop(rejected_store);

        let regular_root = temp.path().join("former-root-now-file");
        let regular_db = temp.path().join("regular-root.db");
        fs::create_dir(&regular_root)?;
        AtlasStore::transition_project_root(
            &regular_db,
            &regular_root,
            ProjectRootTransition::Bind,
        )?;
        fs::remove_dir(&regular_root)?;
        fs::write(&regular_root, "the old root path still exists")?;
        let regular_bytes = fs::read(&regular_db)?;
        let regular_error = require_error(
            AtlasStore::transition_project_root(&regular_db, &root_c, ProjectRootTransition::Move),
            "regular file at the old root was treated as absent",
        )?;
        require(
            matches!(regular_error, DbError::ProjectRootStillPresent { .. }),
            "regular-file move returned the wrong error",
        )?;
        assert_database_unchanged(&regular_db, &regular_bytes, "regular-file move database")?;

        let link_root = temp.path().join("former-root-now-link");
        let missing_link_target = temp.path().join("missing-link-target");
        let link_db = temp.path().join("linked-root.db");
        fs::create_dir(&link_root)?;
        AtlasStore::transition_project_root(&link_db, &link_root, ProjectRootTransition::Bind)?;
        fs::remove_dir(&link_root)?;
        if create_dangling_directory_link(&missing_link_target, &link_root)? {
            let link_bytes = fs::read(&link_db)?;
            let link_error = require_error(
                AtlasStore::transition_project_root(&link_db, &root_c, ProjectRootTransition::Move),
                "dangling root link was treated as absent",
            )?;
            require(
                matches!(link_error, DbError::ProjectRootStillPresent { .. }),
                "dangling-link move returned the wrong error",
            )?;
            assert_database_unchanged(&link_db, &link_bytes, "dangling-link move database")?;
            fs::remove_file(&link_root)?;
        }

        let relative_root = temp.path().join("relative-root-source");
        let relative_db = temp.path().join("relative-root.db");
        fs::create_dir(&relative_root)?;
        AtlasStore::transition_project_root(
            &relative_db,
            &relative_root,
            ProjectRootTransition::Bind,
        )?;
        {
            let relative_store = AtlasStore::open(&relative_db)?;
            set_metadata(
                &relative_store.connection,
                PROJECT_ROOT_KEY,
                "relative/stored/root",
            )?;
            relative_store
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        }
        let relative_bytes = fs::read(&relative_db)?;
        let relative_error = require_error(
            AtlasStore::transition_project_root(&relative_db, &root_c, ProjectRootTransition::Move),
            "non-absolute stored root was treated as a verified move",
        )?;
        require(
            matches!(relative_error, DbError::ProjectRootStillPresent { .. }),
            "native stored root returned the wrong error",
        )?;
        assert_database_unchanged(&relative_db, &relative_bytes, "relative-root move database")?;

        let uncertain_error = require_error(
            classify_root_absence(
                "C:/uncertain-project-root",
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected permission denial",
                )),
            ),
            "permission uncertainty was treated as absence",
        )?;
        require(
            matches!(uncertain_error, DbError::ProjectRootAbsenceUncertain { .. }),
            "permission uncertainty returned the wrong error",
        )?;

        let legacy_db = temp.path().join("legacy-root.db");
        let legacy_old_root = temp.path().join("legacy-old-root");
        let legacy_destination = temp.path().join("legacy-destination");
        fs::create_dir(&legacy_destination)?;
        {
            let legacy = Connection::open(&legacy_db)?;
            schema::create_released_schema_eight(&legacy)?;
            set_metadata(
                &legacy,
                PROJECT_ROOT_KEY,
                &normalize_metadata_path(&legacy_old_root),
            )?;
        }
        let legacy_bytes = fs::read(&legacy_db)?;
        let legacy_error = require_error(
            AtlasStore::transition_project_root(
                &legacy_db,
                &legacy_destination,
                ProjectRootTransition::Move,
            ),
            "legacy move repaired a missing root without native identity proof",
        )?;
        require(
            matches!(legacy_error, DbError::ProjectRootIdentityMissing),
            "legacy missing-root move returned the wrong error",
        )?;
        assert_database_unchanged(&legacy_db, &legacy_bytes, "legacy missing-root move")?;

        let current_missing_identity_db = temp.path().join("current-missing-identity.db");
        let current_missing_identity_root = temp.path().join("current-missing-identity-root");
        let current_missing_identity_destination =
            temp.path().join("current-missing-identity-destination");
        fs::create_dir(&current_missing_identity_root)?;
        fs::create_dir(&current_missing_identity_destination)?;
        AtlasStore::transition_project_root(
            &current_missing_identity_db,
            &current_missing_identity_root,
            ProjectRootTransition::Bind,
        )?;
        {
            let current = AtlasStore::open(&current_missing_identity_db)?;
            current
                .connection
                .execute("DELETE FROM project_identity", [])?;
            current
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        }
        fs::remove_dir(&current_missing_identity_root)?;
        let repaired_current_move = AtlasStore::transition_project_root(
            &current_missing_identity_db,
            &current_missing_identity_destination,
            ProjectRootTransition::Move,
        )?;
        require(
            repaired_current_move.identity_changed,
            "current bound database move did not report its repaired identity",
        )?;
        let repaired_current = AtlasStore::open_read_only_for_project(
            &current_missing_identity_db,
            &current_missing_identity_destination,
        )?;
        require_eq(
            &repaired_current.project_instance_id()?,
            &Some(repaired_current_move.project_instance_id),
            "current bound database repaired identity",
        )?;

        let detached =
            AtlasStore::transition_project_root(&copy_db, &root_b, ProjectRootTransition::Detach)?;
        require(
            detached.project_instance_id != source_identity,
            "detach preserved copied identity",
        )?;
        require(detached.identity_changed, "detach did not change identity")?;
        require(
            detached.publication_invalidated,
            "detach did not invalidate publication",
        )?;
        let detached_store = AtlasStore::open_read_only_for_project(&copy_db, &root_b)?;
        require_eq(
            &detached_store.project_instance_id()?,
            &Some(detached.project_instance_id),
            "detached identity",
        )?;
        assert_authored_state(&detached_store)?;
        assert_usage_report(&detached_store, false)?;
        assert_runtime_scope(&detached_store, source_identity, 0, 1, 0)?;
        assert_graph_counts(&detached_store, [0, 0, 0, 0, 0, 0, 0])?;
        require(
            detached_store.index_publication()?.is_none(),
            "detach retained publication",
        )?;

        let source_store = AtlasStore::open_read_only_for_project(&source_db, &root_a)?;
        require_eq(
            &source_store.project_instance_id()?,
            &Some(source_identity),
            "source identity after copy detach",
        )?;
        assert_authored_state(&source_store)?;
        assert_usage_report(&source_store, true)?;
        assert_runtime_scope(&source_store, source_identity, 1, 0, 1)?;
        assert_graph_counts(&source_store, [2, 1, 1, 1, 1, 1, 1])?;
        drop(source_store);

        let mut rollback_store = AtlasStore::open(&rollback_db)?;
        let rollback_source_root = CanonicalProjectRoot::from_path(&root_a)?;
        let rollback_destination_root = CanonicalProjectRoot::from_path(&root_b)?;
        rollback_store.connection.execute_batch(
            "CREATE TEMP TRIGGER fail_detach_graph
             BEFORE DELETE ON graph_entities
             BEGIN SELECT RAISE(ABORT, 'injected detach failure'); END;",
        )?;
        let rollback_error = require_error(
            apply_root_transition(
                &mut rollback_store,
                ProjectRootTransition::Detach,
                Some(&rollback_source_root),
                Some(source_identity),
                &rollback_destination_root,
            ),
            "late detach failure committed partial identity state",
        )?;
        require(
            matches!(rollback_error, DbError::Sqlite(_)),
            "late detach failure returned the wrong error",
        )?;
        require_eq(
            &rollback_store.project_root()?,
            &Some(normalize_metadata_path(&root_a)),
            "rollback project root",
        )?;
        require_eq(
            &rollback_store.project_instance_id()?,
            &Some(source_identity),
            "rollback project identity",
        )?;
        assert_authored_state(&rollback_store)?;
        assert_usage_report(&rollback_store, true)?;
        assert_runtime_scope(&rollback_store, source_identity, 1, 0, 1)?;
        assert_graph_counts(&rollback_store, [2, 1, 1, 1, 1, 1, 1])?;
        require(
            rollback_store.index_publication()?.is_some(),
            "rollback invalidated prior publication",
        )?;
        drop(rollback_store);

        fs::remove_dir(&root_a)?;
        let moved =
            AtlasStore::transition_project_root(&source_db, &root_c, ProjectRootTransition::Move)?;
        require_eq(
            &moved.project_instance_id,
            &source_identity,
            "moved identity",
        )?;
        require(!moved.identity_changed, "move changed identity")?;
        require(
            moved.publication_invalidated,
            "move did not invalidate publication",
        )?;
        let moved_store = AtlasStore::open_read_only_for_project(&source_db, &root_c)?;
        require_eq(
            &moved_store.project_instance_id()?,
            &Some(source_identity),
            "stored moved identity",
        )?;
        assert_authored_state(&moved_store)?;
        assert_usage_report(&moved_store, true)?;
        assert_runtime_scope(&moved_store, source_identity, 1, 0, 1)?;
        assert_graph_counts(&moved_store, [2, 1, 1, 1, 1, 1, 1])?;
        require(
            moved_store.index_publication()?.is_none(),
            "move retained publication",
        )?;
        let active_generation = moved_store.connection.query_row(
            "SELECT active_generation FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&active_generation, &0, "moved graph generation")?;
        Ok(())
    }

    #[test]
    fn schema_nineteen_detach_migrates_legacy_identity_before_transition()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let source_root = temp.path().join("schema-19-detach-source");
        let destination_root = temp.path().join("schema-19-detach-destination");
        fs::create_dir(&source_root)?;
        fs::create_dir(&destination_root)?;
        let database = temp.path().join("schema-19-detach.db");

        let mut store = AtlasStore::open_for_project(&database, &source_root)?;
        let previous_project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("schema-19 detach fixture identity is missing"))?;
        seed_authored_and_graph_state(&mut store, previous_project)?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(store);

        let detached = AtlasStore::transition_project_root(
            &database,
            &destination_root,
            ProjectRootTransition::Detach,
        )?;
        require(
            detached.identity_changed && detached.project_instance_id != previous_project,
            "schema-19 detach did not rotate the project identity",
        )?;
        require_eq(
            &detached.project_root,
            &Some(normalize_metadata_path(&destination_root)),
            "schema-19 detach destination",
        )?;

        let reopened = AtlasStore::open_read_only_for_project(&database, &destination_root)?;
        let schema_version = reopened.connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &schema_version,
            &"20".to_string(),
            "schema-19 detach schema",
        )?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(CanonicalProjectRoot::from_path(&destination_root)?),
            "schema-19 detach native destination identity",
        )?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(detached.project_instance_id),
            "schema-19 detach project identity",
        )?;
        assert_authored_state(&reopened)?;
        assert_usage_report(&reopened, false)?;
        assert_runtime_scope(&reopened, previous_project, 0, 1, 0)?;
        assert_graph_counts(&reopened, [0, 0, 0, 0, 0, 0, 0])?;
        require(
            reopened.index_publication()?.is_none(),
            "schema-19 detach retained derived publication",
        )?;
        drop(reopened);

        let missing_database = temp.path().join("schema-19-detach-missing.db");
        let missing_store = AtlasStore::open_for_project(&missing_database, &source_root)?;
        missing_store
            .connection
            .execute_batch("DROP TABLE project_root_identity; UPDATE metadata SET value = '19' WHERE key = 'schema_version';")?;
        drop(missing_store);
        let missing_before = fs::read(&missing_database)?;
        fs::remove_dir(&source_root)?;
        let missing_error = require_error(
            AtlasStore::transition_project_root(
                &missing_database,
                &destination_root,
                ProjectRootTransition::Detach,
            ),
            "schema-19 detach repaired a missing legacy root",
        )?;
        require(
            !matches!(missing_error, DbError::ProjectRootTransitionChanged { .. }),
            "schema-19 missing-root detach reached mutable transition validation",
        )?;
        assert_database_unchanged(
            &missing_database,
            &missing_before,
            "schema-19 missing-root detach",
        )?;
        Ok(())
    }

    #[test]
    fn schema_nineteen_same_root_bind_reports_preserved_identity_and_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("schema-19-bind-root");
        let database = temp.path().join("schema-19-bind.db");
        fs::create_dir(&root)?;

        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("schema-19 bind fixture identity is missing"))?;
        seed_authored_and_graph_state(&mut store, project)?;
        let publication = store.index_publication()?;
        assert_authored_state(&store)?;
        assert_usage_report(&store, true)?;
        assert_runtime_scope(&store, project, 1, 0, 1)?;
        assert_graph_counts(&store, [2, 1, 1, 1, 1, 1, 1])?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(store);

        let bound =
            AtlasStore::transition_project_root(&database, &root, ProjectRootTransition::Bind)?;
        let expected_display = normalize_metadata_path(&root);
        require_eq(
            &bound.previous_root,
            &Some(expected_display.clone()),
            "schema-19 bind previous root",
        )?;
        require_eq(
            &bound.project_root,
            &Some(expected_display),
            "schema-19 bind destination root",
        )?;
        require_eq(
            &bound.project_instance_id,
            &project,
            "schema-19 bind identity",
        )?;
        require(!bound.identity_changed, "schema-19 bind changed identity")?;
        require(
            !bound.publication_invalidated,
            "schema-19 bind invalidated publication",
        )?;

        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(CanonicalProjectRoot::from_path(&root)?),
            "schema-19 bind native root",
        )?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(project),
            "schema-19 bind reopened identity",
        )?;
        require_eq(
            &reopened.index_publication()?,
            &publication,
            "schema-19 bind publication",
        )?;
        assert_authored_state(&reopened)?;
        assert_usage_report(&reopened, true)?;
        assert_runtime_scope(&reopened, project, 1, 0, 1)?;
        assert_graph_counts(&reopened, [2, 1, 1, 1, 1, 1, 1])?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn case_only_root_rename_reopens_same_persisted_identity() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let original_path = temp.path().join("CaseOnlyRoot");
        let staging_path = temp.path().join("CaseOnlyRootStaging");
        let renamed_path = temp.path().join("caseonlyroot");
        fs::create_dir(&original_path)?;
        let database = temp.path().join("case-only-root.db");

        let initial = AtlasStore::open_for_project(&database, &original_path)?;
        let project = initial
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("case-only root fixture identity is missing"))?;
        let initial_root = initial
            .project_root_identity()?
            .ok_or_else(|| io::Error::other("case-only root fixture native identity is missing"))?;
        drop(initial);

        fs::rename(&original_path, &staging_path)?;
        fs::rename(&staging_path, &renamed_path)?;
        let renamed_root = CanonicalProjectRoot::from_path(&renamed_path)?;
        if initial_root.encode()? == renamed_root.encode()? {
            return Err("case-only root rename did not retain distinct native spelling".into());
        }
        // A case-sensitive Windows directory intentionally cannot resolve the
        // old spelling; the dedicated refusal test covers that namespace.
        let Ok(recanonicalized_root) = CanonicalProjectRoot::from_path(&original_path) else {
            return Ok(());
        };
        require_eq(
            &recanonicalized_root,
            &renamed_root,
            "re-canonicalized case-only root",
        )?;

        let reopened = AtlasStore::open_for_project(&database, &renamed_path)?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(project),
            "case-only root project identity",
        )?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(renamed_root),
            "case-only root native identity",
        )?;
        require_eq(
            &reopened.project_root()?,
            &Some(normalize_metadata_path(&renamed_path)),
            "case-only root display metadata",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_root_reopens_file_backed_store_without_identity_drift() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let base = temp
            .path()
            .to_str()
            .ok_or("temporary directory was not UTF-8")?;
        let long_component = "a".repeat(220);
        let root = std::path::PathBuf::from(format!(r"\\?\{base}\{long_component}"));
        fs::create_dir(&root)?;
        let database = root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or("verbatim root database has no parent")?,
        )?;

        let initial = AtlasStore::open_for_project(&database, &root)?;
        let project = initial
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("verbatim root project identity is missing"))?;
        let native_root = initial
            .project_root_identity()?
            .ok_or_else(|| io::Error::other("verbatim root native identity is missing"))?;
        let display = native_root.display_string()?;
        if !display.starts_with(r"\\?\") {
            return Err("verbatim root display lost its extended prefix".into());
        }
        drop(initial);

        let reopened = AtlasStore::open_for_project(&database, &root)?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(project),
            "verbatim root project identity",
        )?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(native_root),
            "verbatim root native identity",
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn case_sensitive_root_namespace_rejects_distinct_binding_without_mutation()
    -> Result<(), Box<dyn Error>> {
        use std::process::Command;

        let temp = tempfile::tempdir()?;
        let case_sensitive_parent = temp.path().join("case-sensitive-parent");
        fs::create_dir(&case_sensitive_parent)?;
        let enabled = Command::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&case_sensitive_parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            // Case-sensitive directory support is filesystem/host-policy
            // dependent; skip this negative proof when it cannot be enabled.
            return Ok(());
        }

        let upper_path = case_sensitive_parent.join("Repo");
        let lower_path = case_sensitive_parent.join("repo");
        if fs::create_dir(&upper_path).is_err() || fs::create_dir(&lower_path).is_err() {
            return Ok(());
        }
        let upper_identity = CanonicalProjectRoot::from_path(&upper_path)?;
        let lower_identity = CanonicalProjectRoot::from_path(&lower_path)?;
        if upper_identity == lower_identity {
            return Ok(());
        }

        let database = temp.path().join("case-sensitive.db");
        let mut store = AtlasStore::open_for_project(&database, &upper_path)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("case-sensitive root identity is missing"))?;
        seed_authored_and_graph_state(&mut store, project)?;
        drop(store);

        // Keep one validated read snapshot open so SQLite's WAL shared-memory
        // sidecar already exists before the rejected admission is attempted.
        // The refusal itself must not create or remove any sidecar.
        let read_guard = AtlasStore::open_read_only_for_project(&database, &upper_path)?;
        let database_before = fs::read(&database)?;
        let sidecars_before = ["-wal", "-shm", "-journal"].map(|suffix| {
            fs::read(database.with_file_name(format!("case-sensitive.db{suffix}"))).ok()
        });
        let mut inventory_before = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        inventory_before.sort();

        let Some(error) = AtlasStore::open_for_project(&database, &lower_path).err() else {
            return Err(io::Error::other(
                "case-sensitive sibling root was admitted as the persisted binding",
            )
            .into());
        };
        if !matches!(error, DbError::ProjectRootMismatch { .. }) {
            return Err(io::Error::other(format!(
                "case-sensitive sibling returned the wrong error: {error}"
            ))
            .into());
        }
        let database_unchanged = fs::read(&database)? == database_before;
        let sidecars_unchanged = ["-wal", "-shm", "-journal"].map(|suffix| {
            fs::read(database.with_file_name(format!("case-sensitive.db{suffix}"))).ok()
        }) == sidecars_before;
        if !database_unchanged || !sidecars_unchanged {
            return Err(io::Error::other(format!(
                "case-sensitive sibling refusal changed database or sidecar bytes (database_unchanged={database_unchanged}, sidecars_unchanged={sidecars_unchanged})"
            ))
            .into());
        }
        let mut inventory_after = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        inventory_after.sort();
        if inventory_after != inventory_before {
            return Err(io::Error::other(
                "case-sensitive sibling refusal changed sidecar inventory",
            )
            .into());
        }

        let reopened = AtlasStore::open_read_only_for_project(&database, &upper_path)?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(project),
            "case-sensitive root project identity",
        )?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(upper_identity),
            "case-sensitive root native identity",
        )?;
        assert_authored_state(&reopened)?;
        assert_usage_report(&reopened, true)?;
        assert_runtime_scope(&reopened, project, 1, 0, 1)?;
        assert_graph_counts(&reopened, [2, 1, 1, 1, 1, 1, 1])?;
        require(
            reopened.index_publication()?.is_some(),
            "case-sensitive sibling refusal changed publication",
        )?;
        drop(read_guard);
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn schema_nineteen_case_sensitive_sibling_refuses_before_migration_without_mutation()
    -> Result<(), Box<dyn Error>> {
        use rusqlite::OpenFlags;
        use std::process::Command;

        let temp = tempfile::tempdir()?;
        let case_sensitive_parent = temp.path().join("schema-19-case-sensitive-parent");
        fs::create_dir(&case_sensitive_parent)?;
        let enabled = Command::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&case_sensitive_parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            // Case-sensitive directory support is filesystem/host-policy
            // dependent; skip this negative proof when it cannot be enabled.
            return Ok(());
        }

        let upper_path = case_sensitive_parent.join("Repo");
        let lower_path = case_sensitive_parent.join("repo");
        if fs::create_dir(&upper_path).is_err() || fs::create_dir(&lower_path).is_err() {
            return Ok(());
        }
        let upper_identity = CanonicalProjectRoot::from_path(&upper_path)?;
        let lower_identity = CanonicalProjectRoot::from_path(&lower_path)?;
        if upper_identity == lower_identity {
            return Ok(());
        }

        let database = temp.path().join("schema-19-case-sensitive.db");
        let mut store = AtlasStore::open_for_project(&database, &upper_path)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("schema-19 root fixture identity is missing"))?;
        seed_authored_and_graph_state(&mut store, project)?;
        let publication_before = store.index_publication()?;
        let usage_before = store.usage_events(Some("identity-test"))?;
        let overview_before = store.token_overview(Some("identity-test"))?;
        assert_authored_state(&store)?;
        assert_usage_report(&store, true)?;
        assert_runtime_scope(&store, project, 1, 0, 1)?;
        assert_graph_counts(&store, [2, 1, 1, 1, 1, 1, 1])?;

        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';
             PRAGMA wal_checkpoint(TRUNCATE);",
        )?;

        let schema_before = store.connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let legacy_root_before = store
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'project_root'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let (project_bytes_before, generation_before) = store.connection.query_row(
            "SELECT project_instance_id, active_generation
             FROM project_identity WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let identity_table_before = store.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_root_identity'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &schema_before,
            &"19".to_string(),
            "schema-19 fixture marker",
        )?;
        require_eq(
            &identity_table_before,
            &0,
            "schema-19 fixture identity table absence",
        )?;

        // Keep a read-only connection open while taking the byte and inventory
        // snapshots. The rejected legacy admission must not create or remove
        // SQLite sidecars while it is still in read-only preflight.
        let read_guard = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let database_before = fs::read(&database)?;
        let sidecars_before = ["-wal", "-shm", "-journal"].map(|suffix| {
            fs::read(database.with_file_name(format!("schema-19-case-sensitive.db{suffix}"))).ok()
        });
        let mut inventory_before = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        inventory_before.sort();

        let Some(error) = AtlasStore::open_for_project(&database, &lower_path).err() else {
            return Err(io::Error::other(
                "schema-19 case-sensitive sibling reached migration or was admitted",
            )
            .into());
        };
        if !matches!(error, DbError::ProjectRootMismatch { .. }) {
            return Err(io::Error::other(format!(
                "schema-19 case-sensitive sibling returned the wrong error: {error}"
            ))
            .into());
        }

        require_eq(
            &fs::read(&database)?,
            &database_before,
            "schema-19 case-sensitive refusal database bytes",
        )?;
        let sidecars_after = ["-wal", "-shm", "-journal"].map(|suffix| {
            fs::read(database.with_file_name(format!("schema-19-case-sensitive.db{suffix}"))).ok()
        });
        require_eq(
            &sidecars_after,
            &sidecars_before,
            "schema-19 case-sensitive refusal sidecar bytes",
        )?;
        let mut inventory_after = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        inventory_after.sort();
        require_eq(
            &inventory_after,
            &inventory_before,
            "schema-19 case-sensitive refusal inventory",
        )?;

        require_eq(
            &store.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &schema_before,
            "schema-19 refusal schema marker",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'project_root'",
                [],
                |row| row.get::<_, Option<String>>(0),
            )?,
            &legacy_root_before,
            "schema-19 refusal legacy root metadata",
        )?;
        let (project_bytes_after, generation_after) = store.connection.query_row(
            "SELECT project_instance_id, active_generation
             FROM project_identity WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        require_eq(
            &project_bytes_after,
            &project_bytes_before,
            "schema-19 refusal project instance",
        )?;
        require_eq(
            &generation_after,
            &generation_before,
            "schema-19 refusal generation",
        )?;
        require_eq(
            &store.index_publication()?,
            &publication_before,
            "schema-19 refusal publication",
        )?;
        require_eq(
            &store.usage_events(Some("identity-test"))?,
            &usage_before,
            "schema-19 refusal usage events",
        )?;
        require_eq(
            &store.token_overview(Some("identity-test"))?,
            &overview_before,
            "schema-19 refusal usage overview",
        )?;
        let identity_table_after = store.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_root_identity'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &identity_table_after,
            &identity_table_before,
            "schema-19 refusal identity table",
        )?;
        assert_authored_state(&store)?;
        assert_usage_report(&store, true)?;
        assert_runtime_scope(&store, project, 1, 0, 1)?;
        assert_graph_counts(&store, [2, 1, 1, 1, 1, 1, 1])?;
        drop(read_guard);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_root_transitions_use_native_identity_not_display_projection()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let native_name = std::ffi::OsString::from_vec(vec![b'r', b'o', b'o', b't', 0x80]);
        let root = temp.path().join(&native_name);
        let display_collision = temp.path().join("root-�");
        let destination_collision = temp.path().join("dest-�");
        let destination_native_name =
            std::ffi::OsString::from_vec(vec![b'd', b'e', b's', b't', 0x81]);
        let destination_native = temp.path().join(&destination_native_name);
        fs::create_dir(&root)?;
        fs::create_dir(&display_collision)?;
        fs::create_dir(&destination_collision)?;
        fs::create_dir(&destination_native)?;

        let database = temp.path().join("non-utf8-root.db");
        let bound =
            AtlasStore::transition_project_root(&database, &root, ProjectRootTransition::Bind)?;
        require(
            bound.project_root.is_none(),
            "raw root exposed a lossy transition display",
        )?;
        let native_identity = CanonicalProjectRoot::from_path(&root)?;
        let opened = AtlasStore::open_read_only_for_project(&database, &root)?;
        require_eq(
            &opened.project_root_identity()?,
            &Some(native_identity),
            "non-UTF-8 bound native identity",
        )?;
        require_eq(
            &opened.captured_project_binding()?.project_root,
            &None,
            "non-UTF-8 typed display availability",
        )?;
        require_eq(
            &opened.project_root()?,
            &None,
            "non-UTF-8 compatibility metadata",
        )?;
        drop(opened);
        let before_collision = fs::read(&database)?;
        let collision_error = require_error(
            AtlasStore::transition_project_root(
                &database,
                &display_collision,
                ProjectRootTransition::Bind,
            ),
            "replacement-character root collision was accepted",
        )?;
        require(
            matches!(collision_error, DbError::ProjectRootMismatch { .. }),
            "non-UTF-8 root collision returned the wrong error",
        )?;
        assert_database_unchanged(&database, &before_collision, "non-UTF-8 root collision")?;
        crate::verify_project_database(&database, &root)?;

        let mut stale = AtlasStore::open_for_project(&database, &root)?;
        seed_authored_and_graph_state(&mut stale, bound.project_instance_id)?;
        stale
            .begin_index_publication("non-utf8-before-move")?
            .complete()?;
        fs::remove_dir(&root)?;
        let moved = AtlasStore::transition_project_root(
            &database,
            &display_collision,
            ProjectRootTransition::Move,
        )?;
        require_eq(
            &moved.project_instance_id,
            &bound.project_instance_id,
            "non-UTF-8 move identity",
        )?;
        require_eq(
            &moved.previous_root,
            &None,
            "non-UTF-8 move previous display availability",
        )?;
        let destination_identity = CanonicalProjectRoot::from_path(&display_collision)?;
        let moved_store = AtlasStore::open_read_only_for_project(&database, &display_collision)?;
        require_eq(
            &moved_store.project_root_identity()?,
            &Some(destination_identity),
            "non-UTF-8 moved native identity",
        )?;
        assert_authored_state(&moved_store)?;
        assert_usage_report(&moved_store, true)?;
        drop(moved_store);
        crate::verify_project_database(&database, &display_collision)?;

        let before_stale_publication = fs::read(&database)?;
        let publication_error = require_error(
            stale.begin_index_publication("stale-non-utf8-after-move"),
            "stale non-UTF-8 store entered publication after native root move",
        )?;
        require(
            matches!(
                publication_error,
                DbError::ProjectRootTransitionChanged { .. }
            ),
            "stale non-UTF-8 publication returned the wrong error",
        )?;
        assert_database_unchanged(
            &database,
            &before_stale_publication,
            "stale non-UTF-8 publication",
        )?;
        let destination_state =
            AtlasStore::open_read_only_for_project(&database, &display_collision)?;
        require(
            destination_state.index_publication()?.is_none(),
            "stale non-UTF-8 publication mutated derived state",
        )?;
        drop(destination_state);
        drop(stale);

        let before_destination_collision = fs::read(&database)?;
        let destination_collision_error = require_error(
            AtlasStore::transition_project_root(
                &database,
                &destination_collision,
                ProjectRootTransition::Bind,
            ),
            "replacement-character destination collision was accepted",
        )?;
        require(
            matches!(
                destination_collision_error,
                DbError::ProjectRootMismatch { .. }
            ),
            "non-UTF-8 destination collision returned the wrong error",
        )?;
        assert_database_unchanged(
            &database,
            &before_destination_collision,
            "non-UTF-8 destination collision",
        )?;

        let detached = AtlasStore::transition_project_root(
            &database,
            &destination_native,
            ProjectRootTransition::Detach,
        )?;
        require(
            detached.project_instance_id != bound.project_instance_id,
            "non-UTF-8 detach did not rotate project identity",
        )?;
        require_eq(
            &detached.previous_root,
            &Some(normalize_metadata_path(&display_collision)),
            "non-UTF-8 detach previous display",
        )?;
        require(
            detached.project_root.is_none(),
            "non-UTF-8 detach exposed a lossy destination display",
        )?;
        let detached_store =
            AtlasStore::open_read_only_for_project(&database, &destination_native)?;
        require_eq(
            &detached_store.project_root_identity()?,
            &Some(CanonicalProjectRoot::from_path(&destination_native)?),
            "non-UTF-8 detached native identity",
        )?;
        require_eq(
            &detached_store.captured_project_binding()?.project_root,
            &None,
            "non-UTF-8 detached display availability",
        )?;
        assert_authored_state(&detached_store)?;
        assert_usage_report(&detached_store, false)?;
        assert_graph_counts(&detached_store, [0, 0, 0, 0, 0, 0, 0])?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn set_project_root_preserves_non_utf8_binding_against_replacement_sibling()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let raw_root = temp.path().join(std::ffi::OsString::from_vec(vec![
            b'r', b'e', b'p', b'o', 0x80,
        ]));
        let replacement_root = temp.path().join("repo-�");
        fs::create_dir(&raw_root)?;
        fs::create_dir(&replacement_root)?;
        let database = temp.path().join("set-project-root-non-utf8.db");
        let bound =
            AtlasStore::transition_project_root(&database, &raw_root, ProjectRootTransition::Bind)?;
        let native_identity = CanonicalProjectRoot::from_path(&raw_root)?;
        let mut store = AtlasStore::open_for_project(&database, &raw_root)?;
        require_eq(
            &store.project_root_identity()?,
            &Some(native_identity.clone()),
            "non-UTF-8 set_project_root native identity",
        )?;
        require_eq(
            &store.project_root()?,
            &None,
            "non-UTF-8 set_project_root compatibility metadata",
        )?;
        seed_authored_and_graph_state(&mut store, bound.project_instance_id)?;
        let before_database = fs::read(&database)?;
        let same_store_error = require_error(
            store.set_project_root(&replacement_root),
            "set_project_root rebound a non-UTF-8 root to its replacement sibling",
        )?;
        require(
            matches!(same_store_error, DbError::ProjectRootMismatch { .. }),
            "same-store non-UTF-8 set_project_root returned the wrong error",
        )?;
        require_eq(
            &store.project_root_identity()?,
            &Some(native_identity.clone()),
            "same-store native identity after rejected set_project_root",
        )?;
        require_eq(
            &store.project_root()?,
            &None,
            "same-store metadata after rejected set_project_root",
        )?;
        require_eq(
            &store.project_instance_id()?,
            &Some(bound.project_instance_id),
            "same-store project identity after rejected set_project_root",
        )?;
        assert_authored_state(&store)?;
        assert_usage_report(&store, true)?;
        assert_graph_counts(&store, [2, 1, 1, 1, 1, 1, 1])?;
        assert_database_unchanged(
            &database,
            &before_database,
            "same-store rejected non-UTF-8 set_project_root",
        )?;
        drop(store);

        let mut reopened = AtlasStore::open_for_project(&database, &raw_root)?;
        let before_reopened_attempt = fs::read(&database)?;
        let reopened_error = require_error(
            reopened.set_project_root(&replacement_root),
            "reopened store rebound a non-UTF-8 root to its replacement sibling",
        )?;
        require(
            matches!(reopened_error, DbError::ProjectRootMismatch { .. }),
            "reopened non-UTF-8 set_project_root returned the wrong error",
        )?;
        require_eq(
            &reopened.project_root_identity()?,
            &Some(native_identity),
            "reopened native identity after rejected set_project_root",
        )?;
        require_eq(
            &reopened.project_root()?,
            &None,
            "reopened metadata after rejected set_project_root",
        )?;
        require_eq(
            &reopened.project_instance_id()?,
            &Some(bound.project_instance_id),
            "reopened project identity after rejected set_project_root",
        )?;
        assert_authored_state(&reopened)?;
        assert_usage_report(&reopened, true)?;
        assert_graph_counts(&reopened, [2, 1, 1, 1, 1, 1, 1])?;
        assert_database_unchanged(
            &database,
            &before_reopened_attempt,
            "reopened rejected non-UTF-8 set_project_root",
        )?;
        Ok(())
    }

    #[test]
    fn stale_stores_cannot_write_after_binding_transitions() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("same-root");
        fs::create_dir(&root)?;
        let database = temp.path().join("same-root.db");
        let initial =
            AtlasStore::transition_project_root(&database, &root, ProjectRootTransition::Bind)?;
        let mut stale = AtlasStore::open_for_project(&database, &root)?;
        seed_authored_and_graph_state(&mut stale, initial.project_instance_id)?;

        let detached =
            AtlasStore::transition_project_root(&database, &root, ProjectRootTransition::Detach)?;
        require(
            detached.project_instance_id != initial.project_instance_id,
            "same-root detach did not rotate identity",
        )?;
        let purpose_error = require_error(
            stale.set_purpose(
                "src/lib.rs",
                "Stale store must not replace this purpose.",
                projectatlas_core::PurposeSource::Agent,
            ),
            "stale store wrote purpose after same-root detach",
        )?;
        require(
            matches!(purpose_error, DbError::ProjectRootTransitionChanged { .. }),
            "same-root stale purpose returned the wrong error",
        )?;
        let scan_error = require_error(
            stale.replace_scan(&[]),
            "stale store replaced scan state after same-root detach",
        )?;
        require(
            matches!(scan_error, DbError::ProjectRootTransitionChanged { .. }),
            "same-root stale scan returned the wrong error",
        )?;
        let telemetry_error = require_error(
            stale.record_usage(&usage_from_estimates(
                "stale-after-detach",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                100,
                20,
            )),
            "stale store recorded telemetry after same-root detach",
        )?;
        require(
            matches!(
                telemetry_error,
                DbError::ProjectRootTransitionChanged { .. }
            ),
            "same-root stale telemetry returned the wrong error",
        )?;
        let health_error = require_error(
            stale.resolve_health_finding(&HealthResolution {
                finding_id: "stale-resolution".to_string(),
                category: "missing-purpose".to_string(),
                path: "src/lib.rs".to_string(),
                related_path: None,
                rationale: "A stale store must not persist this resolution.".to_string(),
            }),
            "stale store resolved health state after same-root detach",
        )?;
        require(
            matches!(health_error, DbError::ProjectRootTransitionChanged { .. }),
            "same-root stale health resolution returned the wrong error",
        )?;
        let publication_error = require_error(
            stale.begin_index_publication("stale-after-detach"),
            "stale store began publication after same-root detach",
        )?;
        require(
            matches!(
                publication_error,
                DbError::ProjectRootTransitionChanged { .. }
            ),
            "same-root stale publication returned the wrong error",
        )?;
        let detached_store = AtlasStore::open_read_only_for_project(&database, &root)?;
        assert_authored_state(&detached_store)?;
        assert_usage_report(&detached_store, false)?;
        assert_runtime_scope(&detached_store, initial.project_instance_id, 0, 1, 0)?;
        assert_graph_counts(&detached_store, [0, 0, 0, 0, 0, 0, 0])?;
        require(
            detached_store.index_publication()?.is_none(),
            "stale writes restored invalidated publication state",
        )?;
        let active_generation = detached_store.connection.query_row(
            "SELECT active_generation FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &active_generation,
            &0,
            "active generation after rejected stale writes",
        )?;
        drop(detached_store);
        drop(stale);

        let move_root = temp.path().join("move-source");
        let destination = temp.path().join("move-destination");
        fs::create_dir(&move_root)?;
        fs::create_dir(&destination)?;
        let move_database = temp.path().join("move.db");
        AtlasStore::transition_project_root(
            &move_database,
            &move_root,
            ProjectRootTransition::Bind,
        )?;
        let mut stale_move = AtlasStore::open_for_project(&move_database, &move_root)?;
        fs::remove_dir(&move_root)?;
        AtlasStore::transition_project_root(
            &move_database,
            &destination,
            ProjectRootTransition::Move,
        )?;
        let move_error = require_error(
            stale_move.begin_index_publication("stale-after-move"),
            "stale store began publication after root move",
        )?;
        require(
            matches!(
                move_error,
                DbError::ProjectRootMismatch { .. } | DbError::ProjectRootTransitionChanged { .. }
            ),
            "moved stale publication returned the wrong error",
        )?;
        drop(AtlasStore::open_read_only_for_project(
            &move_database,
            &destination,
        )?);
        Ok(())
    }

    /// Require a test condition without panicking in a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Require equality while retaining useful mismatch context.
    fn require_eq<T: Debug + PartialEq>(
        actual: &T,
        expected: &T,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }

    /// Require one database operation to fail without panic-based assertions.
    fn require_error<T>(result: DbResult<T>, message: &str) -> Result<DbError, Box<dyn Error>> {
        match result {
            Ok(_) => Err(io::Error::other(message).into()),
            Err(error) => Ok(error),
        }
    }

    /// Create a dangling directory link when the current platform permits it.
    #[cfg(unix)]
    fn create_dangling_directory_link(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }

    /// Create a dangling directory reparse point when Windows policy permits it.
    #[cfg(windows)]
    fn create_dangling_directory_link(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(true),
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Other targets have no standard-library directory-link fixture.
    #[cfg(not(any(unix, windows)))]
    fn create_dangling_directory_link(
        _target: &Path,
        _link: &Path,
    ) -> Result<bool, Box<dyn Error>> {
        Ok(false)
    }

    fn seed_authored_and_graph_state(
        store: &mut AtlasStore,
        project: ProjectInstanceId,
    ) -> Result<(), Box<dyn Error>> {
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind, parent_path) VALUES('.', 'folder', NULL);
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/lib.rs', 'file', '.');
             INSERT INTO file_content_classifications(path, classification)
                VALUES('src/lib.rs', 'source');
             INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                SELECT id, 'Own the local source entry point.', 'agent', 'approved', 'agent'
                  FROM nodes WHERE path = 'src/lib.rs';
             INSERT INTO metadata(key, value) VALUES('custom.identity-test-setting', 'retain-me');
             INSERT INTO health_resolutions(
                finding_id, category, path, related_path, rationale, resolved_by
             ) VALUES(
                'resolved-purpose', 'duplicate-purpose', 'src/lib.rs', 'src/main.rs',
                'The responsibilities are intentionally distinct.', 'agent'
             );",
        )?;
        store.record_usage(&identity_transition_usage_event())?;
        let generation = IndexGeneration::new(1);
        let project_entity = GraphEntity::new(project, EntitySelector::Project, generation)?;
        let file_entity = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
            },
            generation,
        )?;
        let relation = LogicalRelation::new(
            &file_entity,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new("src/lib.rs")?,
            },
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let occurrence = RelationOccurrence::new(
            &relation,
            RepositoryFilePath::new(Path::new("src/lib.rs"))?,
            SourceSpan::new(1, 0, 1, 1)?,
            generation,
        )?;
        let coverage = CoverageRecord::new(
            CoverageScope::Project,
            None,
            CoverageState::Complete,
            1,
            0,
            generation,
            None,
            None,
        )?;
        let resolution_key = CanonicalResolutionKey::new(
            project,
            ResolutionKeyDomain::Declaration,
            &GraphIdentityText::new("identity-transition")?,
            &GraphIdentityText::new("rust")?,
            None,
            None,
            Some(GraphRelationKind::Legacy(RelationKind::DependsOn)),
            &GraphIdentityText::new("src/lib.rs")?,
        );
        let entity_export =
            EntityResolutionKey::new(file_entity.key().clone(), resolution_key.clone())?;
        let relation_dependency =
            RelationDependencyKey::new(relation.key().clone(), resolution_key)?;
        let mut publication = store.begin_index_publication("identity-transition")?;
        publication.replace_repository_graph_with_resolution_keys(
            project,
            &[project_entity, file_entity],
            &[relation],
            &[occurrence],
            &[coverage],
            &[entity_export],
            &[relation_dependency],
        )?;
        publication.complete()?;
        Ok(())
    }

    fn assert_authored_state(store: &AtlasStore) -> Result<(), Box<dyn Error>> {
        let mut nodes = store
            .connection
            .prepare("SELECT path, kind, parent_path FROM nodes ORDER BY path")?;
        let nodes = nodes
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &nodes,
            &vec![
                (".".to_string(), "folder".to_string(), None),
                (
                    "src/lib.rs".to_string(),
                    "file".to_string(),
                    Some(".".to_string()),
                ),
            ],
            "node anchors",
        )?;

        let purpose = store.connection.query_row(
            "SELECT n.path, p.purpose, p.source, p.status, p.updated_by
               FROM purposes p JOIN nodes n ON n.id = p.node_id",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        require_eq(
            &purpose,
            &(
                "src/lib.rs".to_string(),
                Some("Own the local source entry point.".to_string()),
                "agent".to_string(),
                "approved".to_string(),
                Some("agent".to_string()),
            ),
            "purpose content and review ownership",
        )?;

        let custom_setting = store.connection.query_row(
            "SELECT value FROM metadata WHERE key = 'custom.identity-test-setting'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(&custom_setting, &"retain-me".to_string(), "custom metadata")?;

        let health_resolution = store.connection.query_row(
            "SELECT finding_id, category, path, related_path, rationale, resolved_by
               FROM health_resolutions",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?;
        require_eq(
            &health_resolution,
            &(
                "resolved-purpose".to_string(),
                "duplicate-purpose".to_string(),
                "src/lib.rs".to_string(),
                Some("src/main.rs".to_string()),
                "The responsibilities are intentionally distinct.".to_string(),
                "agent".to_string(),
            ),
            "health resolution content",
        )?;

        Ok(())
    }

    /// Build the modeled event used to prove project-scoped runtime lifecycle.
    fn identity_transition_usage_event() -> projectatlas_core::telemetry::UsageEvent {
        usage_from_estimates(
            "identity-test",
            "summary",
            Some("src/lib.rs".to_string()),
            Some("identity transition".to_string()),
            120,
            20,
        )
    }

    /// Assert the current project report without reading schema-private raw columns.
    fn assert_usage_report(store: &AtlasStore, retained: bool) -> Result<(), Box<dyn Error>> {
        let events = store.usage_events(Some("identity-test"))?;
        let overview = store.token_overview(Some("identity-test"))?;
        if retained {
            require_eq(
                &events,
                &vec![identity_transition_usage_event()],
                "project-scoped usage event",
            )?;
            require_eq(&overview.calls, &1, "project-scoped usage calls")?;
        } else {
            require(
                events.is_empty(),
                "detached project exposed prior raw usage",
            )?;
            require_eq(&overview.calls, &0, "detached project usage calls")?;
        }
        Ok(())
    }

    /// Assert active/sealed instance and baseline ownership for one project identity.
    fn assert_runtime_scope(
        store: &AtlasStore,
        project: ProjectInstanceId,
        expected_active: i64,
        expected_sealed: i64,
        expected_baselines: i64,
    ) -> Result<(), Box<dyn Error>> {
        const ACTIVE: &str = "active";
        const SEALED: &str = "sealed";
        let project_bytes = project.as_bytes();
        let (active, sealed) = store.connection.query_row(
            "SELECT
                 COALESCE(SUM(CASE WHEN state = ?2 THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN state = ?3 THEN 1 ELSE 0 END), 0)
             FROM usage_instances
             WHERE project_instance_id = ?1",
            rusqlite::params![project_bytes.as_slice(), ACTIVE, SEALED],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let baselines = store.connection.query_row(
            "SELECT COUNT(*)
             FROM usage_instance_baselines AS b
             JOIN usage_instances AS i USING(instance_row_id)
             WHERE i.project_instance_id = ?1",
            [project_bytes.as_slice()],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&active, &expected_active, "active runtime instances")?;
        require_eq(&sealed, &expected_sealed, "sealed runtime instances")?;
        require_eq(&baselines, &expected_baselines, "active baseline witnesses")
    }

    /// Require that a rejected transition left the main database bytes unchanged.
    fn assert_database_unchanged(
        database_path: &Path,
        expected: &[u8],
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        require_eq(&fs::read(database_path)?, &expected.to_vec(), label)
    }

    fn assert_graph_counts(store: &AtlasStore, expected: [i64; 7]) -> Result<(), Box<dyn Error>> {
        let mut counts = Vec::new();
        for table in [
            "graph_entities",
            "graph_relations",
            "graph_relation_occurrences",
            "graph_coverage",
            "graph_resolution_keys",
            "graph_entity_exports",
            "graph_relation_dependencies",
        ] {
            counts.push(store.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |row| row.get::<_, i64>(0),
            )?);
        }
        require_eq(&counts, &expected.to_vec(), "graph row counts")?;
        let quick_check = store
            .connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))?;
        require_eq(&quick_check, &"ok".to_string(), "database integrity")?;
        let foreign_key_failures = store.connection.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&foreign_key_failures, &0, "foreign key integrity")?;
        Ok(())
    }
}
