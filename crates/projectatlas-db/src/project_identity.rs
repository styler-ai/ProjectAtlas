//! Durable project identity and explicit root-binding transitions.

use super::{AtlasStore, DbError, DbResult, normalize_metadata_path, set_metadata};
use crate::schema::{self, PROJECT_ROOT_KEY, SchemaState};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::ProjectInstanceId;
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
    /// Root stored before the transition, when one existed.
    pub previous_root: Option<String>,
    /// Canonical root stored after the transition.
    pub project_root: String,
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
        let destination = validate_project_root_destination(destination)?;
        let preflight = schema::preflight(database_path, None)?;
        let previous_root = preflight.project_root.clone();
        let previous_identity = if preflight.state == SchemaState::Current {
            read_current_project_identity(database_path)?
        } else {
            None
        };

        match transition {
            ProjectRootTransition::Bind => {
                if let Some(found) = previous_root.as_deref()
                    && found != destination
                {
                    return Err(DbError::ProjectRootMismatch {
                        expected: destination,
                        found: found.to_string(),
                    });
                }
                let store = Self::open_for_project(database_path, Path::new(&destination))?;
                let project_instance_id = store
                    .project_instance_id()?
                    .ok_or(DbError::ProjectInstanceIdentityMissing)?;
                Ok(ProjectRootTransitionResult {
                    transition,
                    previous_root,
                    project_root: destination,
                    project_instance_id,
                    identity_changed: previous_identity != Some(project_instance_id),
                    publication_invalidated: false,
                })
            }
            ProjectRootTransition::Move | ProjectRootTransition::Detach => {
                let previous_root =
                    previous_root.ok_or(DbError::ProjectRootTransitionRequiresExistingRoot)?;
                if transition == ProjectRootTransition::Move {
                    if previous_root == destination {
                        return Err(DbError::ProjectRootTransitionRequiresDifferentRoot {
                            root: destination,
                        });
                    }
                    verify_root_absent(&previous_root)?;
                }

                let mut store = Self::open(database_path)?;
                let opened_identity = store.project_instance_id()?;
                if previous_identity.is_some() && opened_identity != previous_identity {
                    return Err(project_transition_changed(
                        Some(previous_root),
                        store.project_root()?,
                        previous_identity,
                        opened_identity,
                    ));
                }
                let mut result = apply_root_transition(
                    &mut store,
                    transition,
                    &previous_root,
                    opened_identity,
                    &destination,
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
fn validate_project_root_destination(destination: &Path) -> DbResult<String> {
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
    let canonical =
        fs::canonicalize(destination).map_err(|source| DbError::ProjectRootDestinationInvalid {
            root: root.clone(),
            source,
        })?;
    let metadata =
        fs::metadata(&canonical).map_err(|source| DbError::ProjectRootDestinationInvalid {
            root: root.clone(),
            source,
        })?;
    if !metadata.is_dir() {
        return Err(DbError::ProjectRootDestinationInvalid {
            root,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project root destination is not a directory",
            ),
        });
    }
    Ok(normalize_metadata_path(&canonical))
}

/// Apply move or detach after non-mutating preflight has captured expected state.
fn apply_root_transition(
    store: &mut AtlasStore,
    transition: ProjectRootTransition,
    expected_root: &str,
    expected_identity: Option<ProjectInstanceId>,
    destination: &str,
) -> DbResult<ProjectRootTransitionResult> {
    store.connection.execute_batch("BEGIN IMMEDIATE")?;
    let operation = (|| {
        let found_root = store.project_root()?;
        let found_identity = load_project_identity(&store.connection)?;
        if found_root.as_deref() != Some(expected_root) || found_identity != expected_identity {
            return Err(project_transition_changed(
                Some(expected_root.to_string()),
                found_root,
                expected_identity,
                found_identity,
            ));
        }

        set_metadata(&store.connection, PROJECT_ROOT_KEY, destination)?;
        schema::invalidate_derived_publication(&store.connection)?;
        let (project_instance_id, identity_changed) = match transition {
            ProjectRootTransition::Bind => unreachable!("bind does not use transition mutation"),
            ProjectRootTransition::Move => {
                let identity = found_identity.ok_or(DbError::ProjectInstanceIdentityMissing)?;
                set_graph_generation(&store.connection, IndexGeneration::ZERO)?;
                (identity, false)
            }
            ProjectRootTransition::Detach => {
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
            previous_root: Some(expected_root.to_string()),
            project_root: destination.to_string(),
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
            store.validated_project_root = Some(destination.to_string());
            Ok(result)
        }
        Err(error) => Err(schema::rollback_after_error(&store.connection, error)),
    }
}

/// Prove the recorded old root path entry is absent without following links.
fn verify_root_absent(root: &str) -> DbResult<()> {
    let path = Path::new(root);
    if !path.is_absolute() {
        return Err(DbError::ProjectRootAbsenceUncertain {
            root: root.to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "stored project root is not absolute",
            ),
        });
    }
    classify_root_absence(root, fs::symlink_metadata(path).map(|_| ()))
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

/// Load identity through a validated current read-only snapshot.
fn read_current_project_identity(path: &Path) -> DbResult<Option<ProjectInstanceId>> {
    let (connection, _) = schema::open_current_read_only(path, None)?;
    load_project_identity(&connection)
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
fn set_project_identity(connection: &Connection, identity: ProjectInstanceId) -> DbResult<()> {
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
    use projectatlas_core::graph::{
        Completeness, ConfidenceClass, CoverageRecord, CoverageScope, CoverageState,
        EntitySelector, GraphEntity, GraphRelationKind, LogicalRelation, RelationOccurrence,
        RelationResolution, RepositoryFilePath, SourceSpan,
    };
    use projectatlas_core::symbols::RelationKind;
    use std::error::Error;
    use std::fmt::Debug;
    use std::io;

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
        assert_graph_counts(&rejected_store, (2, 1, 1, 1))?;
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
            matches!(relative_error, DbError::ProjectRootAbsenceUncertain { .. }),
            "non-absolute stored root returned the wrong error",
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
        let migrated_move = AtlasStore::transition_project_root(
            &legacy_db,
            &legacy_destination,
            ProjectRootTransition::Move,
        )?;
        require(
            migrated_move.identity_changed,
            "pre-graph-schema move did not report its initialized identity",
        )?;
        require(
            migrated_move.project_instance_id.as_bytes() != [0_u8; 16],
            "pre-graph-schema move initialized a zero identity",
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
        assert_graph_counts(&detached_store, (0, 0, 0, 0))?;
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
        assert_graph_counts(&source_store, (2, 1, 1, 1))?;
        drop(source_store);

        let mut rollback_store = AtlasStore::open(&rollback_db)?;
        rollback_store.connection.execute_batch(
            "CREATE TEMP TRIGGER fail_detach_graph
             BEFORE DELETE ON graph_entities
             BEGIN SELECT RAISE(ABORT, 'injected detach failure'); END;",
        )?;
        let rollback_error = require_error(
            apply_root_transition(
                &mut rollback_store,
                ProjectRootTransition::Detach,
                &normalize_metadata_path(&root_a),
                Some(source_identity),
                &normalize_metadata_path(&root_b),
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
        assert_graph_counts(&rollback_store, (2, 1, 1, 1))?;
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
        assert_graph_counts(&moved_store, (2, 1, 1, 1))?;
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
             INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                SELECT id, 'Own the local source entry point.', 'agent', 'approved', 'agent'
                  FROM nodes WHERE path = 'src/lib.rs';
             INSERT INTO metadata(key, value) VALUES('custom.identity-test-setting', 'retain-me');
             INSERT INTO health_resolutions(
                finding_id, category, path, related_path, rationale, resolved_by
             ) VALUES(
                'resolved-purpose', 'duplicate-purpose', 'src/lib.rs', 'src/main.rs',
                'The responsibilities are intentionally distinct.', 'agent'
             );
             INSERT INTO usage_events(
                session_id, command, path, query,
                estimated_tokens_without_projectatlas,
                estimated_tokens_with_projectatlas,
                estimated_tokens_saved
             ) VALUES(
                'identity-test', 'summary', 'src/lib.rs', 'identity transition', 120, 20, 100
             );",
        )?;
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
            &project_entity,
            GraphRelationKind::Legacy(RelationKind::Contains),
            RelationResolution::resolved(&file_entity)?,
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
        let mut publication = store.begin_index_publication("identity-transition")?;
        publication.replace_repository_graph(
            project,
            &[project_entity, file_entity],
            &[relation],
            &[occurrence],
            &[coverage],
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

        let telemetry = store.connection.query_row(
            "SELECT session_id, command, path, query,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved, token_savings_bucket, provider, model
               FROM usage_events",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )?;
        require_eq(
            &telemetry,
            &(
                "identity-test".to_string(),
                "summary".to_string(),
                Some("src/lib.rs".to_string()),
                Some("identity transition".to_string()),
                Some(120),
                Some(20),
                Some(100),
                "navigation_avoidance".to_string(),
                "heuristic".to_string(),
                "unknown".to_string(),
            ),
            "telemetry content",
        )?;
        Ok(())
    }

    /// Require that a rejected transition left the main database bytes unchanged.
    fn assert_database_unchanged(
        database_path: &Path,
        expected: &[u8],
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        require_eq(&fs::read(database_path)?, &expected.to_vec(), label)
    }

    fn assert_graph_counts(
        store: &AtlasStore,
        expected: (i64, i64, i64, i64),
    ) -> Result<(), Box<dyn Error>> {
        let mut counts = Vec::new();
        for table in [
            "graph_entities",
            "graph_relations",
            "graph_relation_occurrences",
            "graph_coverage",
        ] {
            counts.push(store.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |row| row.get::<_, i64>(0),
            )?);
        }
        let expected = vec![expected.0, expected.1, expected.2, expected.3];
        require_eq(&counts, &expected, "graph row counts")?;
        Ok(())
    }
}
