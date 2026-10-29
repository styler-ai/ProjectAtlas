//! Causal ownership, publication and compatibility controls for generated state.

use super::{DATABASE_FILE, FilePublication, HOST_FILES, ProjectStateGuard, STATE_DIRECTORY};
use std::fs;
use std::io;
use std::path::Path;

/// Return a typed test failure rather than panic in a fallible filesystem test.
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

/// Create a directory alias through the platform's ordinary filesystem API.
#[cfg(unix)]
fn directory_alias(target: &Path, path: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

/// Create a directory alias on the supported Windows test host.
#[cfg(windows)]
fn directory_alias(target: &Path, path: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, path)
}

#[test]
fn automatic_database_and_wal_aliases_refuse_before_creation() -> io::Result<()> {
    for suffix in super::WAL_SIDECAR_SUFFIXES {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let state = root.join(STATE_DIRECTORY);
        fs::create_dir(&state)?;
        let database = state.join(DATABASE_FILE);
        let outside = root.join("outside.bin");
        fs::write(&outside, b"preserve outside bytes")?;
        let mut name = database.as_os_str().to_os_string();
        name.push(suffix);
        fs::hard_link(&outside, Path::new(&name))?;
        require(
            ProjectStateGuard::preflight(&root, Some(&database), None).is_err(),
            "automatic database/WAL hard link was admitted",
        )?;
        require(
            fs::read(&outside)? == b"preserve outside bytes",
            "outside bytes changed",
        )?;
        require(
            !state.join(HOST_FILES[0]).exists(),
            "refusal created host files",
        )?;
    }
    Ok(())
}

#[test]
fn explicit_external_database_hardlinks_remain_operator_selected() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let outside = temp.path().join("external").join(STATE_DIRECTORY);
    fs::create_dir(&root)?;
    fs::create_dir_all(&outside)?;
    let external_database = outside.join(DATABASE_FILE);
    fs::write(&external_database, b"operator-selected database")?;
    let external_copy = outside.join("database-backup");
    fs::hard_link(&external_database, &external_copy)?;
    let root = root.canonicalize()?;
    require(
        ProjectStateGuard::preflight_database_path(&external_database, &root)?.is_none(),
        "conventional-looking external database was treated as selected managed state",
    )?;
    let mut guard = ProjectStateGuard::preflight(&root, Some(&external_database), None)?;
    guard.ensure_directory()?;
    guard.write_generated(
        &root.join(STATE_DIRECTORY).join(HOST_FILES[0]),
        b"generated",
    )?;
    require(
        fs::read(&external_database)? == b"operator-selected database",
        "external database was restricted or modified",
    )
}

#[test]
fn database_access_does_not_require_generated_host_outputs() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let state = root.join(STATE_DIRECTORY);
    fs::create_dir(&state)?;
    let database = state.join(DATABASE_FILE);
    fs::write(&database, b"regular database fixture")?;
    fs::create_dir(state.join(HOST_FILES[0]))?;
    require(
        ProjectStateGuard::preflight_database_path(&database, &root)?.is_some(),
        "managed database was not admitted",
    )?;
    require(
        ProjectStateGuard::preflight(&root, Some(&database), None).is_err(),
        "bootstrap admitted an invalid host destination",
    )
}

#[test]
#[cfg(any(unix, windows))]
fn external_database_is_not_inherited_through_a_state_directory_alias() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let outside = temp.path().join("external");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    directory_alias(&outside, &root.join(STATE_DIRECTORY))?;
    let external_database = outside.join(DATABASE_FILE);
    fs::write(&external_database, b"operator-selected external database")?;
    let root = root.canonicalize()?;
    require(
        !super::owned_named_path(
            &external_database,
            &root.join(STATE_DIRECTORY),
            DATABASE_FILE,
        ),
        "physical alias incorrectly granted automatic ownership",
    )?;
    require(
        ProjectStateGuard::preflight_database_path(&external_database, &root)?.is_none(),
        "external database was restricted by unrelated project state",
    )?;
    require(
        ProjectStateGuard::preflight_database_path(
            &root.join(STATE_DIRECTORY).join(DATABASE_FILE),
            &root,
        )
        .is_err(),
        "managed database inherited the aliased directory",
    )
}

#[test]
#[cfg(any(unix, windows))]
fn managed_database_root_prefix_aliases_keep_their_ownership_checks() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    fs::create_dir(&root)?;
    let state = root.join(STATE_DIRECTORY);
    fs::create_dir(&state)?;
    let outside = temp.path().join("outside.db");
    fs::write(&outside, b"preserve outside database")?;
    fs::hard_link(&outside, state.join(DATABASE_FILE))?;
    let alias = temp.path().join("root-alias");
    directory_alias(&root, &alias)?;
    require(
        ProjectStateGuard::preflight_database_path(
            &alias.join(STATE_DIRECTORY).join(DATABASE_FILE),
            &root,
        )
        .is_err(),
        "root-prefix alias bypassed database admission",
    )?;
    #[cfg(windows)]
    require(
        ProjectStateGuard::preflight_database_path(
            &root.join(".PROJECTATLAS").join("PROJECTATLAS.DB"),
            &root,
        )
        .is_err(),
        "case alias bypassed database admission",
    )?;
    Ok(())
}

#[test]
fn generated_replacement_preserves_an_external_hard_link() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let state = root.join(STATE_DIRECTORY);
    fs::create_dir(&state)?;
    let outside = root.join("external-host.json");
    fs::write(&outside, b"preserve external configuration")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o640))?;
    }
    let output = state.join(HOST_FILES[0]);
    fs::hard_link(&outside, &output)?;
    let guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.write_generated(&output, b"replacement")?;
    require(
        fs::read(&output)? == b"replacement",
        "generated replacement missing",
    )?;
    require(
        fs::read(&outside)? == b"preserve external configuration",
        "replacement wrote through a hard link",
    )?;
    require(
        fs::metadata(&output)?.permissions() == fs::metadata(&outside)?.permissions(),
        "replacement changed existing file permissions",
    )
}

#[test]
fn creation_race_preserves_the_other_complete_default() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = root.join(STATE_DIRECTORY).join(super::CONFIG_FILE);
    guard.publish(&output, b"our default", FilePublication::Create, || {
        fs::write(&output, b"other initializer's complete config")
    })?;
    require(
        fs::read(&output)? == b"other initializer's complete config",
        "racing default was overwritten",
    )?;
    require(
        fs::read_dir(
            output
                .parent()
                .ok_or_else(|| io::Error::other("missing parent"))?,
        )?
        .count()
            == 1,
        "racing creation leaked temporary staging",
    )
}

#[test]
fn publication_failure_preserves_existing_output_and_cleans_owned_staging() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = root.join(STATE_DIRECTORY).join(HOST_FILES[0]);
    fs::write(&output, b"last valid output")?;
    let result = guard.publish(&output, b"candidate", FilePublication::Replace, || {
        Err(io::Error::other("controlled pre-publication failure"))
    });
    require(result.is_err(), "controlled failure unexpectedly published")?;
    require(
        fs::read(&output)? == b"last valid output",
        "failure changed the last valid output",
    )?;
    require(
        fs::read_dir(
            output
                .parent()
                .ok_or_else(|| io::Error::other("missing parent"))?,
        )?
        .count()
            == 1,
        "failure leaked owned staging",
    )
}

#[test]
#[cfg(any(unix, windows))]
fn directory_replacement_never_unlinks_a_foreign_staging_name() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let outside = temp.path().join("outside");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    let root = root.canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let state = root.join(STATE_DIRECTORY);
    let parked = root.join("parked-state");
    let output = state.join(HOST_FILES[0]);
    let mut stage_name = None;
    let result = guard.publish(&output, b"candidate", FilePublication::Replace, || {
        let name = fs::read_dir(&state)?
            .next()
            .ok_or_else(|| io::Error::other("missing owned stage"))??
            .file_name();
        fs::write(outside.join(&name), b"foreign sentinel")?;
        fs::rename(&state, &parked)?;
        directory_alias(&outside, &state)?;
        stage_name = Some(name);
        Ok(())
    });
    require(result.is_err(), "replaced owner was admitted")?;
    let name = stage_name.ok_or_else(|| io::Error::other("replacement callback did not run"))?;
    require(
        fs::read(outside.join(&name))? == b"foreign sentinel",
        "cleanup unlinked foreign state",
    )?;
    require(
        parked.join(&name).is_file(),
        "uncertain cleanup removed original staging",
    )?;
    #[cfg(unix)]
    fs::remove_file(&state)?;
    #[cfg(windows)]
    fs::remove_dir(&state)?;
    fs::rename(&parked, &state)?;
    guard.write_generated(&output, b"recovered output")?;
    require(
        fs::read(&output)? == b"recovered output",
        "restored owner could not recover",
    )
}
