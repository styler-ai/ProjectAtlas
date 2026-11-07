//! Causal ownership, publication and compatibility controls for generated state.

#[cfg(windows)]
use super::{CONFIG_FILE, JSON_MAP_FILE, LEGACY_CONFIG_FILE};
use super::{DATABASE_FILE, FilePublication, HOST_FILES, ProjectStateGuard, STATE_DIRECTORY};
use std::fs;
use std::io;
use std::path::Path;

/// A separate SQLite process must still be excluded by its parent's reserved lock.
#[cfg(unix)]
#[test]
fn managed_parent_sqlite_lock_peer() -> io::Result<()> {
    let Some(path) = std::env::var_os("PROJECTATLAS_TEST_MANAGED_PARENT_LOCK") else {
        return Ok(());
    };
    let connection = rusqlite::Connection::open(path).map_err(io::Error::other)?;
    connection
        .busy_timeout(std::time::Duration::ZERO)
        .map_err(io::Error::other)?;
    match connection.execute_batch("BEGIN IMMEDIATE") {
        Err(rusqlite::Error::SqliteFailure(source, _))
            if matches!(
                source.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            Ok(())
        }
        Err(source) => Err(io::Error::other(source)),
        Ok(()) => {
            connection
                .execute_batch("ROLLBACK")
                .map_err(io::Error::other)?;
            Err(io::Error::other(
                "managed parent SQLite reserved lock was lost",
            ))
        }
    }
}

/// Check the kernel-visible write exclusion without opening the file in this process.
#[cfg(unix)]
fn verify_managed_parent_sqlite_lock(path: &Path) -> io::Result<()> {
    let peer = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "project_state::tests::managed_parent_sqlite_lock_peer",
            "--nocapture",
        ])
        .env("PROJECTATLAS_TEST_MANAGED_PARENT_LOCK", path)
        .output()?;
    if !peer.status.success() {
        return Err(io::Error::other(format!(
            "managed parent lock peer failed: {} {}",
            String::from_utf8_lossy(&peer.stdout),
            String::from_utf8_lossy(&peer.stderr)
        )));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn refused_managed_database_parents_preserve_live_sqlite_locks() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let database = root.join(STATE_DIRECTORY);
    let connection = rusqlite::Connection::open(&database).map_err(io::Error::other)?;
    connection
        .execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE control(value); BEGIN IMMEDIATE;")
        .map_err(io::Error::other)?;
    verify_managed_parent_sqlite_lock(&database)?;
    for reset in [false, true] {
        let selected = database.join(DATABASE_FILE.to_ascii_uppercase());
        let result = if reset {
            ProjectStateGuard::preflight_database_reset_path(&selected, &root)
        } else {
            ProjectStateGuard::preflight_database_path(&selected, &root)
        };
        require(
            !matches!(result, Ok(Some(_))),
            "regular-file managed parent was admitted",
        )?;
        verify_managed_parent_sqlite_lock(&database)?;
    }
    connection
        .execute_batch("ROLLBACK")
        .map_err(io::Error::other)
}

#[test]
fn root_document_creation_preserves_existing_entries_and_race_status() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let path = root.join(super::GITIGNORE_FILE);
    let mut guard = ProjectStateGuard::preflight_document(&root, &path)?
        .ok_or_else(|| io::Error::other("root document was not admitted"))?;
    let written = guard.publish(&path, b"requested", FilePublication::Create, || {
        fs::write(&path, b"racing complete document")
    })?;
    require(!written, "racing document was reported as created")?;
    require(
        fs::read(&path)? == b"racing complete document",
        "racing document was clobbered",
    )?;
    require(
        !guard.create_document(b"replacement")?,
        "existing document was reported as created",
    )?;
    require(
        !root.join(STATE_DIRECTORY).exists(),
        "root document created unrelated private state",
    )
}

#[cfg(unix)]
#[test]
fn racing_gitignore_alias_is_neither_followed_nor_reported_created() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let path = root.join(super::GITIGNORE_FILE);
    let outside = root.join("outside");
    let guard = ProjectStateGuard::preflight_document(&root, &path)?
        .ok_or_else(|| io::Error::other("root document was not admitted"))?;
    let written = guard.publish(&path, b"requested", FilePublication::Create, || {
        std::os::unix::fs::symlink(&outside, &path)
    })?;
    require(
        !written && !outside.exists(),
        "racing alias was written or reported created",
    )?;
    require(
        fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "existing alias was replaced",
    )
}

#[cfg(unix)]
#[test]
fn document_creation_uses_ordinary_umask_and_replacement_preserves_permissions() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let control = root.join("ordinary-create");
    fs::write(&control, b"control")?;
    let path = root.join(super::GITIGNORE_FILE);
    let mut guard = ProjectStateGuard::preflight_document(&root, &path)?
        .ok_or_else(|| io::Error::other("root document was not admitted"))?;
    require(
        guard.create_document(b"first")?,
        "new document was not created",
    )?;
    require(
        fs::metadata(&path)?.permissions().mode() & 0o777
            == fs::metadata(&control)?.permissions().mode() & 0o777,
        "document creation widened or restricted the ordinary umask mode",
    )?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
    guard.write_document(b"second")?;
    require(
        fs::metadata(&path)?.permissions().mode() & 0o777 == 0o640,
        "replacement lost existing permissions",
    )
}

#[cfg(unix)]
#[test]
fn write_only_destination_remains_replaceable_without_payload_read_access() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    fs::write(&output, b"old output")?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o200))?;
    guard.write_generated(&output, b"write-only replacement")?;
    require(
        fs::metadata(&output)?.permissions().mode() & 0o777 == 0o200,
        "write-only destination permissions changed",
    )?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o600))?;
    require(
        fs::read(output)? == b"write-only replacement",
        "write-only replacement bytes were lost",
    )
}

#[cfg(unix)]
#[test]
fn replacement_refuses_destination_entry_and_access_changes() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for change in ["replace", "remove", "permissions", "appear"] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
        guard.ensure_directory()?;
        let output = guard.directory.join(HOST_FILES[0]);
        let retired = root.join("previous-destination");
        if change != "appear" {
            fs::write(&output, b"old output")?;
            fs::set_permissions(&output, fs::Permissions::from_mode(0o640))?;
        }
        let result = guard.publish(
            &output,
            b"candidate",
            FilePublication::Replace,
            || match change {
                "replace" => {
                    fs::rename(&output, &retired)?;
                    fs::write(&output, b"racing complete output")?;
                    fs::set_permissions(&output, fs::Permissions::from_mode(0o600))
                }
                "remove" => fs::rename(&output, &retired),
                "permissions" => fs::set_permissions(&output, fs::Permissions::from_mode(0o600)),
                "appear" => fs::write(&output, b"racing complete output"),
                _ => Err(io::Error::other("unknown destination change fixture")),
            },
        );
        require(
            result.is_err(),
            &format!("destination {change} was overwritten: {result:?}"),
        )?;
        if change == "remove" {
            require(!output.exists(), "removed destination was recreated")?;
        } else {
            let expected = if change == "permissions" {
                b"old output".as_slice()
            } else {
                b"racing complete output".as_slice()
            };
            require(
                fs::read(&output)? == expected,
                "changed output was overwritten",
            )?;
            if change != "appear" {
                require(
                    fs::metadata(&output)?.permissions().mode() & 0o777 == 0o600,
                    "changed output permissions were overwritten",
                )?;
            }
        }
        if retired.exists() {
            require(
                fs::read(retired)? == b"old output",
                "retained source changed",
            )?;
        }
        require(
            fs::read_dir(&guard.directory)?.count() == usize::from(change != "remove"),
            "destination refusal leaked owned staging",
        )?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn generated_creation_preserves_ordinary_modes_for_all_host_and_default_roles() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let control = root.join("ordinary-control");
    fs::write(&control, b"ordinary creation control")?;
    let mode = fs::metadata(&control)?.permissions().mode() & 0o777;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    for name in HOST_FILES {
        let path = root.join(STATE_DIRECTORY).join(name);
        guard.write_generated(&path, b"generated host")?;
        require(
            fs::metadata(path)?.permissions().mode() & 0o777 == mode,
            "generated host changed ordinary creation mode",
        )?;
    }
    let path = root.join(STATE_DIRECTORY).join(super::NONSOURCE_FILE);
    guard.write_new(&path, b"generated defaults")?;
    require(
        fs::metadata(path)?.permissions().mode() & 0o777 == mode,
        "generated defaults changed ordinary creation mode",
    )
}

/// Return a typed test failure rather than panic in a fallible filesystem test.
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

/// Read native codes through contextual errors without dropping the I/O cause.
fn native_io_error_code(error: &io::Error) -> Option<i32> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        if let Some(code) = cause
            .downcast_ref::<io::Error>()
            .and_then(io::Error::raw_os_error)
        {
            return Some(code);
        }
        current = cause.source();
    }
    None
}

#[test]
fn publication_context_preserves_native_failure_kind_and_code() -> io::Result<()> {
    let original = io::Error::from_raw_os_error(5);
    let kind = original.kind();
    let contextual = super::publication_failure("controlled native operation", original);
    require(
        contextual.kind() == kind
            && native_io_error_code(&contextual) == Some(5)
            && contextual
                .to_string()
                .contains("controlled native operation"),
        "publication context lost its operation or native I/O cause",
    )
}

/// Keep reductions literal even when Windows `PathBuf::push` would normalize a verbatim prefix.
fn unreduced_path(parent: &Path, components: &[&str]) -> io::Result<std::path::PathBuf> {
    let mut spelling = parent.as_os_str().to_os_string();
    for component in components {
        spelling.push(std::path::MAIN_SEPARATOR_STR);
        spelling.push(component);
    }
    let path = std::path::PathBuf::from(spelling);
    require(
        path.components()
            .any(|component| component == std::path::Component::ParentDir),
        "reduction fixture lost its literal parent component",
    )?;
    Ok(path)
}

/// Observe the fixture filesystem rather than assuming an OS-wide case policy.
fn fixture_names_are_case_sensitive(root: &Path) -> io::Result<bool> {
    const PROBE_NAME: &str = "Case-Probe";
    let probe = root.join(PROBE_NAME);
    fs::write(&probe, b"case behavior")?;
    let sensitive = !root.join(PROBE_NAME.to_ascii_lowercase()).try_exists()?;
    fs::remove_file(probe)?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Ok(expected) = std::env::var("PROJECTATLAS_TEST_CASE_SENSITIVE") {
        require(
            expected == sensitive.to_string(),
            "fixture volume does not have the requested case behavior",
        )?;
    }
    Ok(sensitive)
}

/// Measure each spelling pair, since filesystem Unicode rules differ by format.
fn fixture_names_alias(directory: &Path, fixed: &str, variant: &str) -> io::Result<bool> {
    let target = directory.join(fixed);
    require(
        !target.try_exists()?,
        "name probe would overwrite a fixture",
    )?;
    fs::write(&target, b"name comparison")?;
    let selected = directory.join(variant);
    let aliases = selected.try_exists()?;
    if aliases {
        require(
            same_file::is_same_file(&target, &selected)?,
            "name comparison found an unrelated fixture entry",
        )?;
    }
    fs::remove_file(target)?;
    Ok(aliases)
}

/// Exercise ASCII, simple-fold, expanding-fold and native uppercase spellings.
fn managed_name_case_variants(name: &str) -> Vec<String> {
    let mut variants = vec![name.to_ascii_uppercase()];
    for variant in [
        name.replace('s', "ſ"),
        name.replace("fi", "ﬁ"),
        name.replace('i', "ı"),
        format!("\u{200d}{name}"),
        format!("{name}\u{feff}"),
    ] {
        if variant != name && !variants.contains(&variant) {
            variants.push(variant);
        }
    }
    for character in [
        '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
        '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}', '\u{feff}',
    ] {
        variants.push(name.replace('.', &format!("{character}.")));
    }
    #[cfg(target_os = "linux")]
    for character in [
        '\u{00ad}',
        '\u{034f}',
        '\u{061c}',
        '\u{115f}',
        '\u{180e}',
        '\u{2060}',
        '\u{3164}',
        '\u{fe00}',
        '\u{ffa0}',
        '\u{1bca0}',
        '\u{1d173}',
        '\u{e0001}',
        '\u{e0020}',
        '\u{e0100}',
    ] {
        variants.push(name.replace('.', &format!("{character}.")));
    }
    variants
}

#[test]
fn hfs_fixed_names_ignore_only_the_frozen_format_characters() -> io::Result<()> {
    for name in HOST_FILES.into_iter().chain([
        STATE_DIRECTORY,
        DATABASE_FILE,
        super::CONFIG_FILE,
        super::LEGACY_CONFIG_FILE,
        super::JSON_MAP_FILE,
        super::GITIGNORE_FILE,
    ]) {
        require(
            super::hfs_managed_name_matches(&name.to_ascii_uppercase(), name),
            "HFS ASCII case was lost",
        )?;
        for character in [
            '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
            '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
            '\u{206f}', '\u{feff}',
        ] {
            for variant in [
                format!("{character}{name}"),
                format!("{name}{character}"),
                name.replace('.', &format!("{character}.")),
            ] {
                require(
                    super::hfs_managed_name_matches(&variant, name)
                        && super::fixed_name_candidate(Path::new(&variant), name)?,
                    "HFS ignorable spelling was dropped before volume lookup",
                )?;
            }
        }
        for character in ['ſ', 'ı', '\u{034f}', '\u{2060}', '\u{00ad}'] {
            let variant = format!("{character}{name}");
            require(
                !super::hfs_managed_name_matches(&variant, name),
                "HFS comparison ignored an unrelated character",
            )?;
        }
        require(
            !super::hfs_managed_name_matches(&name.replace('s', "ſ"), name) || !name.contains('s'),
            "HFS long-s became ASCII s",
        )?;
    }
    require(
        !super::hfs_managed_name_matches("conﬁg.toml", super::CONFIG_FILE),
        "HFS expanded a compatibility ligature",
    )
}

/// Linux's pinned model includes assigned ignorables, rather than modern folds.
#[cfg(target_os = "linux")]
#[test]
fn linux_fixed_names_follow_the_frozen_ascii_projection() -> io::Result<()> {
    let matches = super::linux_access::fixed_name_matches;
    for (variant, expected) in [
        ("CONFIG.TOML", "config.toml"),
        ("config.toml", "CONFIG.TOML"),
        ("conﬁg.toml", "config.toml"),
        ("projectatlaſ.db", "projectatlas.db"),
        ("con\u{034f}fig.toml", "config.toml"),
        ("config\u{e0100}.toml", "config.toml"),
        ("config\u{1bca0}.toml", "config.toml"),
    ] {
        require(
            matches(variant, expected),
            "Linux fixed-name alias was lost",
        )?;
        require(
            super::fixed_name_candidate(Path::new(variant), expected)?,
            "Linux alias was filtered before native classification",
        )?;
    }
    for variant in [
        "conﬃg.toml",
        "config\u{2065}.toml",
        "conéf ig.toml",
        "config\u{13430}.toml",
    ] {
        require(
            !matches(variant, super::CONFIG_FILE),
            "distinct custom Unicode name became a Linux alias",
        )?;
    }
    Ok(())
}

#[test]
fn apfs_full_fold_recognizes_expanding_aliases_without_compatibility_folding() {
    assert_eq!(unicode_casefold::UNICODE_VERSION, (9, 0, 0));
    assert!(super::unicode_name_matches(
        "conﬁg.toml",
        super::CONFIG_FILE
    ));
    assert!(super::unicode_name_matches(
        "projectatlaſ.db",
        DATABASE_FILE
    ));
    assert!(!super::unicode_name_matches(
        "conﬃg.toml",
        super::CONFIG_FILE
    ));
    assert!(!super::unicode_name_matches(
        "confıg.toml",
        super::CONFIG_FILE
    ));
    assert!(!super::unicode_name_matches(
        "projectatlaß.db",
        DATABASE_FILE
    ));
}

#[test]
fn generated_registry_outputs_preserve_stored_aliases_and_distinct_custom_entries() -> io::Result<()>
{
    for filename in HOST_FILES {
        for variant in managed_name_case_variants(filename) {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let directory = root.join(STATE_DIRECTORY);
            fs::create_dir(&directory)?;
            let aliases = fixture_names_alias(&directory, filename, &variant)?;
            let selected = directory.join(&variant);
            let outside = root.join("outside-owner");
            fs::write(&outside, b"stored outside bytes")?;
            fs::hard_link(&outside, &selected)?;
            let guard = ProjectStateGuard::preflight(&root, None, None)?;
            // Real host producers always choose the registry spelling. The
            // filesystem resolves an existing stored alias; this API's parent
            // guard does not classify arbitrary children as managed filenames.
            guard.write_generated(&directory.join(filename), b"generated bytes")?;
            require(
                fs::read(directory.join(filename))? == b"generated bytes"
                    && fs::read(&outside)? == b"stored outside bytes"
                    && fs::read(&selected)?
                        == if aliases {
                            b"generated bytes".as_slice()
                        } else {
                            b"stored outside bytes".as_slice()
                        },
                "registry publication changed an outside owner or a distinct stored name",
            )?;
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn explicit_external_parent_traversal_does_not_require_directory_read_access() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("selected");
    let outside = temp.path().join("operator");
    let external_state = outside.join(STATE_DIRECTORY);
    fs::create_dir(&root)?;
    fs::create_dir_all(&external_state)?;
    let root = root.canonicalize()?;
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o111))?;
    fs::set_permissions(&external_state, fs::Permissions::from_mode(0o111))?;
    let result =
        ProjectStateGuard::preflight_database_path(&external_state.join(DATABASE_FILE), &root);
    fs::set_permissions(&external_state, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o700))?;
    require(
        result?.is_none(),
        "external directory traversal was treated as managed ownership",
    )
}

#[test]
fn unicode_root_directory_aliases_keep_missing_and_existing_managed_ownership() -> io::Result<()> {
    for variant in [
        "owned-é-checkout",
        "owned-E\u{301}-checkout",
        "owned-É\u{200d}-checkout",
        "owned-É\u{feff}-checkout",
        "distinct-checkout",
    ] {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("owned-É-checkout");
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let alias = base.join(variant);
        let aliases = alias.try_exists()?;
        if aliases {
            require(
                same_file::is_same_file(&root, &alias)?,
                "root name probe found a different object",
            )?;
        } else {
            fs::create_dir(&alias)?;
        }
        for existing in [false, true] {
            if existing {
                fs::create_dir(root.join(STATE_DIRECTORY))?;
                if !aliases {
                    fs::create_dir(alias.join(STATE_DIRECTORY))?;
                }
            }
            for (private, filename, database) in [
                (true, DATABASE_FILE, true),
                (true, super::CONFIG_FILE, false),
                (false, super::LEGACY_CONFIG_FILE, false),
                (true, super::JSON_MAP_FILE, false),
                (false, super::GITIGNORE_FILE, false),
            ] {
                let parent = if private {
                    alias.join(STATE_DIRECTORY)
                } else {
                    alias.clone()
                };
                let selected = parent.join(filename);
                let result = if database {
                    ProjectStateGuard::preflight_database_path(&selected, &root)
                } else {
                    ProjectStateGuard::preflight_document(&root, &selected)
                };
                require(
                    result?.is_some() == aliases,
                    &format!(
                        "Unicode root alias lost {filename} ownership: alias={aliases}, existing={existing}, spelling={variant:?}"
                    ),
                )?;
            }
        }
        let outside = base.join("outside-owner");
        fs::write(&outside, b"outside bytes")?;
        let selected = alias.join(STATE_DIRECTORY).join(DATABASE_FILE);
        fs::hard_link(&outside, &selected)?;
        let result = ProjectStateGuard::preflight_database_path(&selected, &root);
        require(
            if aliases {
                result.is_err()
            } else {
                result?.is_none()
            },
            "root spelling bypassed a managed hardlink or restricted a different root",
        )?;
        require(
            fs::read(outside)? == b"outside bytes",
            "root admission changed an outside owner",
        )?;
    }
    Ok(())
}

#[test]
fn managed_case_aliases_preserve_volume_ownership_and_distinct_custom_entries() -> io::Result<()> {
    let mut failures = Vec::new();
    for (private, filename, database) in [
        (true, DATABASE_FILE, true),
        (true, super::CONFIG_FILE, false),
        (false, super::LEGACY_CONFIG_FILE, false),
        (true, super::JSON_MAP_FILE, false),
        (false, super::GITIGNORE_FILE, false),
    ] {
        for variant in managed_name_case_variants(filename) {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            fixture_names_are_case_sensitive(&root)?;
            let directory = if private {
                let state = root.join(STATE_DIRECTORY);
                fs::create_dir(&state)?;
                state
            } else {
                root.clone()
            };
            let target = directory.join(filename);
            let aliases = fixture_names_alias(&directory, filename, &variant)?;
            let selected = directory.join(&variant);
            let admit = || {
                if database {
                    ProjectStateGuard::preflight_database_path(&selected, &root)
                } else {
                    ProjectStateGuard::preflight_document(&root, &selected)
                }
            };
            let missing = admit();
            if !missing
                .as_ref()
                .is_ok_and(|guard| guard.is_some() == aliases)
            {
                failures.push(format!(
                    "missing {variant:?} lost native alias ownership ({aliases}); error={:?}",
                    missing.err(),
                ));
                continue;
            }
            let outside = root.join("outside-owner");
            fs::write(&outside, b"outside bytes")?;
            fs::hard_link(&outside, &target)?;
            if !aliases {
                // Distinct directory entries can share an inode on a sensitive FS.
                fs::hard_link(&target, &selected)?;
                require(
                    admit()?.is_none(),
                    "distinct custom hardlink became managed",
                )?;
            } else if database {
                require(
                    admit().is_err(),
                    "case-aliased DB hardlink bypassed admission",
                )?;
            } else {
                let mut guard =
                    admit()?.ok_or_else(|| io::Error::other("document alias was external"))?;
                guard.write_document(b"owned replacement")?;
                require(
                    fs::read(&selected)? == b"owned replacement",
                    "alias publication failed",
                )?;
            }
            require(
                fs::read(&outside)? == b"outside bytes",
                "outside owner was changed",
            )?;
        }
    }
    // Prove the filesystem distinction before using these as custom controls.
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let directory = root.join(STATE_DIRECTORY);
    fs::create_dir(&directory)?;
    for (fixed, custom) in [
        (super::CONFIG_FILE, "conﬃg.toml"),
        (DATABASE_FILE, "projectatlaß.db"),
        (super::CONFIG_FILE, "prefix-config.toml"),
    ] {
        let fixed = directory.join(fixed);
        fs::write(&fixed, b"fixed-name control")?;
        let selected = directory.join(custom);
        require(
            !selected.try_exists()?,
            "Unicode custom control aliases the fixed filesystem name",
        )?;
        require(
            ProjectStateGuard::preflight_document(&root, &selected)?.is_none()
                && ProjectStateGuard::preflight_database_path(&selected, &root)?.is_none(),
            "distinct Unicode or prefixed custom name became managed",
        )?;
        fs::remove_file(&fixed)?;
    }
    require(failures.is_empty(), &failures.join("\n"))
}

#[cfg(unix)]
#[test]
fn managed_case_aliases_admit_sidecars_and_refuse_indirect_namespaces() -> io::Result<()> {
    for suffix in super::WAL_SIDECAR_SUFFIXES {
        for variant in managed_name_case_variants(DATABASE_FILE) {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            fixture_names_are_case_sensitive(&root)?;
            let directory = root.join(STATE_DIRECTORY);
            fs::create_dir(&directory)?;
            let aliases = fixture_names_alias(&directory, DATABASE_FILE, &variant)?;
            let outside = root.join("outside-owner");
            fs::write(&outside, b"outside bytes")?;
            fs::hard_link(&outside, directory.join(format!("{DATABASE_FILE}{suffix}")))?;
            let selected = directory.join(variant);
            let admission = ProjectStateGuard::preflight_database_path(&selected, &root);
            require(
                if aliases {
                    admission.is_err()
                } else {
                    admission?.is_none()
                },
                "case variant bypassed main/sidecar ownership or blocked a distinct custom name",
            )?;
            require(
                fs::read(&outside)? == b"outside bytes",
                "outside sidecar owner changed",
            )?;
        }
    }
    for document in [
        super::ProjectDocument::Config,
        super::ProjectDocument::LegacyConfig,
        super::ProjectDocument::JsonMap,
        super::ProjectDocument::Gitignore,
    ] {
        for variant in managed_name_case_variants(document.filename()) {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            fixture_names_are_case_sensitive(&root)?;
            let directory = document.directory(&root);
            fs::create_dir_all(&directory)?;
            let aliases = fixture_names_alias(&directory, document.filename(), &variant)?;
            let outside = root.join("outside-owner");
            let target = directory.join(document.filename());
            let selected = directory.join(variant);
            std::os::unix::fs::symlink(&outside, &target)?;
            let admission = ProjectStateGuard::preflight_document(&root, &selected);
            if !aliases {
                require(
                    admission?.is_none(),
                    "distinct custom name became a managed dangling alias",
                )?;
            } else if matches!(document, super::ProjectDocument::Gitignore) {
                let mut guard =
                    admission?.ok_or_else(|| io::Error::other("Gitignore alias was external"))?;
                require(
                    !guard.create_document(b"requested")?,
                    "existing Gitignore alias was overwritten",
                )?;
            } else {
                require(
                    admission.is_err(),
                    "case-aliased dangling document bypassed admission",
                )?;
            }
            require(
                !outside.exists(),
                "outside dangling document target was created",
            )?;
        }
    }
    for dangling in [false, true] {
        for variant in managed_name_case_variants(STATE_DIRECTORY) {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("checkout");
            fs::create_dir(&root)?;
            let root = root.canonicalize()?;
            fixture_names_are_case_sensitive(&root)?;
            let aliases = fixture_names_alias(&root, STATE_DIRECTORY, &variant)?;
            let outside = temp.path().join("outside-state");
            if !dangling {
                fs::create_dir(&outside)?;
            }
            let upper_state = root.join(variant);
            std::os::unix::fs::symlink(&outside, &upper_state)?;
            let selected = upper_state.join(DATABASE_FILE);
            let admission = ProjectStateGuard::preflight_database_path(&selected, &root);
            require(
                if aliases {
                    admission.is_err()
                } else {
                    admission?.is_none()
                },
                "case-aliased state directory was followed or distinct custom directory refused",
            )?;
            require(
                !outside.join(DATABASE_FILE).exists(),
                "outside DB was created",
            )?;
            require(
                outside.exists() != dangling,
                "outside directory inventory changed",
            )?;
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(mount) = std::env::var_os("PROJECTATLAS_TEST_CASE_MOUNT") {
        let mount = std::path::PathBuf::from(mount).canonicalize()?;
        let root = mount
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("nested volume has no project root"))?;
        let sensitive = fixture_names_are_case_sensitive(root)?;
        require(
            whichdisk::resolve(&mount)?.case_sensitive() == Some(!sensitive),
            "nested volume must have opposite case behavior",
        )?;
        for (private, filename, database) in [
            (true, DATABASE_FILE, true),
            (true, super::CONFIG_FILE, false),
            (false, super::LEGACY_CONFIG_FILE, false),
            (true, super::JSON_MAP_FILE, false),
            (false, super::GITIGNORE_FILE, false),
        ] {
            for variant in managed_name_case_variants(filename) {
                let directory = if private {
                    root.join(STATE_DIRECTORY)
                } else {
                    root.to_path_buf()
                };
                let aliases = fixture_names_alias(&directory, filename, &variant)?;
                let mut parent = mount.join("missing").join("..").join("..");
                if !private {
                    parent.push("..");
                }
                let selected = parent.join(variant);
                let admission = if database {
                    ProjectStateGuard::preflight_database_path(&selected, root)
                } else {
                    ProjectStateGuard::preflight_document(root, &selected)
                };
                require(
                    if aliases {
                        admission.is_err()
                    } else {
                        admission?.is_none()
                    },
                    "unresolved reduction used the prefix volume instead of the destination volume",
                )?;
                require(
                    !mount.join("missing").exists(),
                    "case classification created a missing parent",
                )?;
            }
        }
    }

    Ok(())
}

#[test]
fn managed_parent_reductions_preserve_admission_and_outside_owners() -> io::Result<()> {
    let mut failures = Vec::new();
    for (private, filename, database) in [
        (true, DATABASE_FILE, true),
        (true, super::CONFIG_FILE, false),
        (false, super::LEGACY_CONFIG_FILE, false),
        (true, super::JSON_MAP_FILE, false),
        (false, super::GITIGNORE_FILE, false),
    ] {
        for existing in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("checkout");
            fs::create_dir(&root)?;
            let root = root.canonicalize()?;
            let directory = if private {
                let state = root.join(STATE_DIRECTORY);
                fs::create_dir(&state)?;
                state
            } else {
                root.clone()
            };
            let intermediate = directory.join("intermediate");
            if existing {
                fs::create_dir(&intermediate)?;
            }
            let outside = temp.path().join("outside");
            fs::write(&outside, b"preserve outside bytes")?;
            let target = directory.join(filename);
            if database {
                fs::write(&target, b"regular database fixture")?;
            } else {
                fs::hard_link(&outside, &target)?;
            }
            let selected = unreduced_path(&intermediate, &["..", filename])?;
            let parent_resolves = fs::canonicalize(super::containing_directory(&selected)).is_ok();
            let admission = if database {
                ProjectStateGuard::preflight_database_path(&selected, &root)
            } else {
                ProjectStateGuard::preflight_document(&root, &selected)
            };
            match admission {
                Ok(Some(mut guard)) if parent_resolves => {
                    if !database {
                        if filename == super::GITIGNORE_FILE {
                            require(
                                !guard.create_document(b"replacement")?,
                                "existing Gitignore was reported created",
                            )?;
                        } else {
                            guard.write_document(b"managed replacement")?;
                            require(
                                fs::read(&target)? == b"managed replacement",
                                "resolved managed document was not replaced",
                            )?;
                        }
                    }
                }
                Err(_) if !parent_resolves => {}
                _ => failures.push(format!("{filename}: existing parent={existing}")),
            }
            require(
                fs::read(&outside)? == b"preserve outside bytes"
                    && intermediate.exists() == existing,
                "admission followed an outside owner or created its unresolved parent",
            )?;
        }
    }
    require(
        failures.is_empty(),
        &format!("managed parent classification lost authority: {failures:?}"),
    )
}

#[test]
fn generated_capture_propagates_unresolved_managed_inputs() -> io::Result<()> {
    let mut failures = Vec::new();
    for (private, filename, database) in [
        (true, DATABASE_FILE, true),
        (true, super::CONFIG_FILE, false),
        (false, super::LEGACY_CONFIG_FILE, false),
    ] {
        for existing in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().join("checkout");
            fs::create_dir(&root)?;
            let root = root.canonicalize()?;
            let state = root.join(STATE_DIRECTORY);
            fs::create_dir(&state)?;
            let directory = if private { state } else { root.clone() };
            let intermediate = directory.join("intermediate");
            if existing {
                fs::create_dir(&intermediate)?;
            }
            fs::write(directory.join(filename), b"owned regular entry")?;
            let selected = unreduced_path(&intermediate, &["..", filename])?;
            let parent_resolves = fs::canonicalize(super::containing_directory(&selected)).is_ok();
            let (db, config) = if database {
                (Some(selected.as_path()), None)
            } else {
                (None, Some(selected.as_path()))
            };
            match ProjectStateGuard::preflight(&root, db, config) {
                Ok(guard) if parent_resolves => {
                    let captured = if database {
                        guard.automatic_database.as_deref()
                    } else {
                        guard.automatic_document.as_deref()
                    };
                    if captured != Some(selected.as_path()) {
                        failures.push(format!("{filename}: managed input was not captured"));
                    }
                }
                Err(_) if !parent_resolves => {}
                _ => failures.push(format!("{filename}: existing parent={existing}")),
            }
            require(
                intermediate.exists() == existing
                    && fs::read(directory.join(filename))? == b"owned regular entry",
                "generated admission changed its input before establishing ownership",
            )?;
        }
    }
    require(
        failures.is_empty(),
        &format!("generated input classification lost authority: {failures:?}"),
    )
}

#[cfg(any(unix, windows))]
#[test]
fn managed_classification_preserves_root_aliases_and_physical_external_parents() -> io::Result<()> {
    for root_name in ["checkout", "checkout-É"] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join(root_name);
        let external = temp.path().join("external");
        fs::create_dir(&root)?;
        fs::create_dir_all(external.join("deep"))?;
        let root = root.canonicalize()?;
        let state = root.join(STATE_DIRECTORY);
        fs::create_dir(&state)?;
        let root_alias = root.with_file_name("root-alias");
        directory_alias(&root, &root_alias)?;
        directory_alias(&external.join("deep"), &root.join("shortcut"))?;
        for (private, filename, database) in [
            (true, DATABASE_FILE, true),
            (true, super::CONFIG_FILE, false),
            (false, super::LEGACY_CONFIG_FILE, false),
            (true, super::JSON_MAP_FILE, false),
            (false, super::GITIGNORE_FILE, false),
        ] {
            let alias_parent = if private {
                root_alias.join(STATE_DIRECTORY)
            } else {
                root_alias.clone()
            };
            let admit = |path: &Path| {
                if database {
                    ProjectStateGuard::preflight_database_path(path, &root)
                } else {
                    ProjectStateGuard::preflight_document(&root, path)
                }
            };
            require(
                admit(&alias_parent.join(filename))?.is_some(),
                "selected root alias lost managed ownership",
            )?;
            let unresolved = unreduced_path(&alias_parent, &["missing", "..", filename])?;
            let parent_resolves =
                fs::canonicalize(super::containing_directory(&unresolved)).is_ok();
            let admission = admit(&unresolved);
            require(
                if parent_resolves {
                    admission?.is_some()
                } else {
                    admission.is_err()
                },
                "root alias lost managed ownership under native parent resolution",
            )?;
            let mut components = vec!["shortcut", ".."];
            if private {
                components.push(STATE_DIRECTORY);
            }
            components.push(filename);
            let physical = unreduced_path(&root, &components)?;
            let managed_parent = if private {
                state.as_path()
            } else {
                root.as_path()
            };
            let resolves_managed = fs::canonicalize(super::containing_directory(&physical))
                .is_ok_and(|resolved| resolved == managed_parent);
            require(
                admit(&physical)?.is_some() == resolves_managed,
                "native physical parent resolution lost its actual owner",
            )?;
            let outside_missing =
                unreduced_path(&external.canonicalize()?, &["missing", "..", filename])?;
            require(
                admit(&outside_missing)?.is_none(),
                "ordinary missing external parent was restricted",
            )?;
        }
        require(
            !external.join("missing").exists() && !external.join(STATE_DIRECTORY).exists(),
            "classification created external parents",
        )?;
    }
    Ok(())
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

#[cfg(any(unix, windows))]
#[test]
fn logical_managed_reductions_cannot_inherit_an_aliased_state_owner() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let outside = temp.path().join("outside").join(STATE_DIRECTORY);
    fs::create_dir(&root)?;
    fs::create_dir_all(&outside)?;
    let root = root.canonicalize()?;
    directory_alias(&outside, &root.join(STATE_DIRECTORY))?;
    for existing in [false, true] {
        let intermediate = outside.join("intermediate");
        if existing {
            fs::create_dir(&intermediate)?;
        }
        for (filename, database) in [
            (DATABASE_FILE, true),
            (super::JSON_MAP_FILE, false),
            (super::CONFIG_FILE, false),
        ] {
            fs::write(outside.join(filename), b"outside selected bytes")?;
            let logical =
                unreduced_path(&root, &[STATE_DIRECTORY, "intermediate", "..", filename])?;
            let admission = if database {
                ProjectStateGuard::preflight_database_path(&logical, &root)
            } else {
                ProjectStateGuard::preflight_document(&root, &logical)
            };
            require(
                admission.is_err(),
                "logical managed reduction inherited an outside state owner",
            )?;
            require(
                fs::read(outside.join(filename))? == b"outside selected bytes"
                    && intermediate.exists() == existing,
                "logical admission changed outside state",
            )?;
            let physical =
                unreduced_path(&outside.canonicalize()?, &["intermediate", "..", filename])?;
            let external = if database {
                ProjectStateGuard::preflight_database_path(&physical, &root)
            } else {
                ProjectStateGuard::preflight_document(&root, &physical)
            };
            require(
                external?.is_none(),
                "explicit outside spelling inherited logical ownership",
            )?;
        }
    }
    Ok(())
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
        )?,
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
    let sensitive = fixture_names_are_case_sensitive(&root)?;
    let case_path = root.join(".PROJECTATLAS").join("PROJECTATLAS.DB");
    let entries = fs::read_dir(&root)?.count();
    let case_admission = ProjectStateGuard::preflight_database_path(&case_path, &root);
    require(
        if sensitive {
            case_admission?.is_none()
        } else {
            case_admission.is_err()
        },
        "private-directory case alias bypassed ownership or blocked a distinct name",
    )?;
    require(
        fs::read(&outside)? == b"preserve outside database"
            && fs::read_dir(&root)?.count() == entries,
        "private-directory alias admission changed outside bytes or created an entry",
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
fn staging_entry_replacement_never_publishes_or_unlinks_the_foreign_file() -> io::Result<()> {
    let mut failures = Vec::new();
    for fail_before_publication in [true, false] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
        guard.ensure_directory()?;
        let state = root.join(STATE_DIRECTORY);
        let output = state.join(HOST_FILES[0]);
        fs::write(&output, b"last valid output")?;
        let retained_stage = root.join("retained-owned-stage");
        let stage = std::cell::OnceCell::new();
        let mut foreign = None;
        let result = guard.publish_with_stage_check(
            &output,
            b"candidate",
            FilePublication::Replace,
            |temporary| {
                stage
                    .set(temporary.path().to_path_buf())
                    .map_err(|duplicate| {
                        io::Error::other(format!("stage observed twice: {duplicate:?}"))
                    })
            },
            || {
                let path = stage
                    .get()
                    .ok_or_else(|| io::Error::other("missing owned stage"))?;
                fs::rename(path, &retained_stage)?;
                fs::write(path, b"foreign sentinel")?;
                foreign = Some(path.clone());
                if fail_before_publication {
                    Err(io::Error::other("controlled pre-publication failure"))
                } else {
                    Ok(())
                }
            },
        );
        let foreign = foreign.ok_or_else(|| io::Error::other("stage replacement did not run"))?;
        if result.is_ok()
            || fs::read(&output)? != b"last valid output"
            || !fs::read(&foreign).is_ok_and(|bytes| bytes == b"foreign sentinel")
            || fs::read(&retained_stage)? != b"candidate"
        {
            failures.push(format!(
                "failure={fail_before_publication}, result={result:?}"
            ));
        }
    }
    require(
        failures.is_empty(),
        &format!("foreign stage ownership lost: {failures:?}"),
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
    let mut callback_started = false;
    let mut renamed = false;
    let mut aliased = false;
    let result = guard.publish(&output, b"candidate", FilePublication::Replace, || {
        callback_started = true;
        let name = fs::read_dir(&state)?
            .next()
            .ok_or_else(|| io::Error::other("missing owned stage"))??
            .file_name();
        stage_name = Some(name.clone());
        fs::write(outside.join(&name), b"foreign sentinel")?;
        fs::rename(&state, &parked)?;
        renamed = true;
        directory_alias(&outside, &state)?;
        aliased = true;
        Ok(())
    });
    require(result.is_err(), "replaced owner was admitted")?;
    require(
        callback_started,
        &format!("publication failed before callback: {result:?}"),
    )?;
    let name = stage_name
        .ok_or_else(|| io::Error::other(format!("staging enumeration failed: {result:?}")))?;
    require(
        fs::read(outside.join(&name))? == b"foreign sentinel",
        "cleanup unlinked foreign state",
    )?;
    #[cfg(windows)]
    if !renamed {
        require(
            result.as_ref().err().is_some_and(|error| {
                error.raw_os_error() == Some(5) && error.kind() == io::ErrorKind::PermissionDenied
            }),
            &format!("directory rename failed without native access denial: {result:?}"),
        )?;
        require(!aliased, "blocked rename still replaced the directory")?;
        guard.verify()?;
        let metadata = fs::symlink_metadata(&state)?;
        require(
            metadata.is_dir() && !super::indirect(&metadata),
            "blocked rename changed the direct owner",
        )?;
        require(
            !parked.try_exists()? && !output.try_exists()? && !state.join(&name).try_exists()?,
            "blocked rename published output or retained owned staging",
        )?;
        require(
            fs::read_dir(&state)?.next().is_none(),
            "blocked rename left an unexpected directory entry",
        )?;
        // The native retained handle prevented replacement; this is not swap coverage.
        guard.write_generated(&output, b"recovered output")?;
        return require(
            fs::read(&output)? == b"recovered output"
                && fs::read(outside.join(&name))? == b"foreign sentinel",
            "original owner could not retry without changing foreign state",
        );
    }
    require(
        renamed,
        &format!("native directory rename failed: {result:?}"),
    )?;
    require(
        aliased,
        &format!("replacement alias creation failed: {result:?}"),
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

/// Derive replacement coverage from the host registry and exact document roles.
fn replacement_targets() -> impl Iterator<Item = (bool, &'static str, bool)> {
    HOST_FILES
        .into_iter()
        .map(|name| (true, name, true))
        .chain([
            (true, super::CONFIG_FILE, false),
            (false, super::LEGACY_CONFIG_FILE, false),
            (true, super::JSON_MAP_FILE, false),
        ])
}

/// Independently enable and measure native verity using the public Linux UAPI.
#[cfg(target_os = "linux")]
fn fixture_verity(path: &Path, enable: bool) -> io::Result<Vec<u8>> {
    const SCRIPT: &str = r"
import fcntl, os, struct, sys
with open(sys.argv[1], 'rb') as source:
    if sys.argv[2] == 'enable':
        argument = struct.pack('=IIIIQIIQ11Q', 1, 1, 4096, 0, 0, 0, 0, 0, *([0]*11))
        assert len(argument) == 128
        fcntl.ioctl(source, 0x40806685, argument)
    digest = bytearray(struct.pack('=HH', 0, 64) + bytes(64))
    fcntl.ioctl(source, 0xc0046686, digest, True)
    algorithm, size = struct.unpack('=HH', digest[:4])
    assert algorithm == 1 and size == 32
    print(digest[4:4+size].hex())
";
    let result = std::process::Command::new("python3")
        .args(["-c", SCRIPT])
        .arg(path)
        .arg(if enable { "enable" } else { "measure" })
        .output()?;
    require(
        result.status.success(),
        &format!("native fs-verity fixture failed: {result:?}"),
    )?;
    Ok(result.stdout)
}

/// A rename can discard native sealing; actual managed publication must refuse it.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an fs-verity-enabled native filesystem; selected explicitly in Linux CI"]
fn replacement_refuses_native_verity_and_late_sealing_without_leaking_stages() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    for (private, filename, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let directory = if private {
            let state = root.join(STATE_DIRECTORY);
            fs::create_dir(&state)?;
            state
        } else {
            root.clone()
        };
        let output = directory.join(filename);
        let outside = root.join("outside-owner");
        fs::write(&output, b"sealed last valid output")?;
        fs::hard_link(&output, &outside)?;
        let digest = fixture_verity(&output, true)?;
        let identity = fs::metadata(&output)?;
        require(
            fs::OpenOptions::new().write(true).open(&output).is_err(),
            "native sealing did not prohibit data writes",
        )?;
        let guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("sealed document was external"))?
        };
        let entries = fs::read_dir(&directory)?.count();
        let reached_stage = std::cell::Cell::new(false);
        let created = guard.publish_with_stage_check(
            &output,
            b"unpublished no-clobber payload",
            FilePublication::Create,
            |_| {
                reached_stage.set(true);
                Ok(())
            },
            || Ok(()),
        )?;
        require(
            !created && !reached_stage.get() && fixture_verity(&output, false)? == digest,
            "create-only sealed collision invoked replacement policy or staging",
        )?;
        let result = guard.publish(
            &output,
            b"unsealed candidate",
            FilePublication::Replace,
            || Ok(()),
        );
        require(
            result
                .as_ref()
                .is_err_and(|source| source.to_string().contains("fs-verity")),
            "sealed output replacement was not refused causally",
        )?;
        require(
            fs::metadata(&output)?.ino() == identity.ino()
                && fs::read(&output)? == b"sealed last valid output"
                && fs::read(&outside)? == b"sealed last valid output"
                && fixture_verity(&output, false)? == digest
                && fixture_verity(&outside, false)? == digest
                && fs::read_dir(&directory)?.count() == entries,
            "sealed refusal changed native state or leaked staging",
        )?;

        // Reproduce the faulty mechanism on the same filesystem. The outside
        // hardlink stays sealed while pathname replacement silently loses it.
        let raw = root.join("raw-rename-stage");
        fs::write(&raw, b"unsealed raw replacement")?;
        fs::rename(&raw, &output)?;
        let flags = rustix::fs::ioctl_getflags(&fs::File::open(&output)?)?;
        require(
            flags.bits() & 0x0010_0000 == 0 && fixture_verity(&outside, false)? == digest,
            "raw rename did not isolate loss of native sealing",
        )?;

        // The source can become sealed after capture without changing inode,
        // mode, owner or access xattrs; final retained-source admission must see it.
        let late = guard.publish(
            &output,
            b"must remain unpublished",
            FilePublication::Replace,
            || fixture_verity(&output, true).map(|_| ()),
        );
        require(
            late.as_ref()
                .is_err_and(|source| source.to_string().contains("fs-verity"))
                && fs::read(&output)? == b"unsealed raw replacement"
                && fixture_verity(&output, false)?.len() == digest.len()
                && fs::read_dir(&directory)?.count() == entries,
            "late sealing escaped final admission or leaked staging",
        )?;
        fs::remove_file(&output)?;
        fs::write(&output, b"ordinary output")?;
        guard.publish(
            &output,
            b"ordinary replacement",
            FilePublication::Replace,
            || Ok(()),
        )?;
        require(
            fs::read(&output)? == b"ordinary replacement"
                && fixture_verity(&outside, false)? == digest,
            "ordinary recovery or outside sealed owner was lost",
        )?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn replacement_preserves_posix_access_acl_and_outside_owner() -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // Native setfacl fixtures: owner/group rw, other none, mask rw, uid65534
    // denied on the destination but allowed by the parent's default ACL.
    const RESTRICTED: &[u8] = &[
        2, 0, 0, 0, 1, 0, 6, 0, 255, 255, 255, 255, 2, 0, 0, 0, 254, 255, 0, 0, 4, 0, 6, 0, 255,
        255, 255, 255, 16, 0, 6, 0, 255, 255, 255, 255, 32, 0, 0, 0, 255, 255, 255, 255,
    ];
    const INHERITED: &[u8] = &[
        2, 0, 0, 0, 1, 0, 6, 0, 255, 255, 255, 255, 2, 0, 6, 0, 254, 255, 0, 0, 4, 0, 6, 0, 255,
        255, 255, 255, 16, 0, 6, 0, 255, 255, 255, 255, 32, 0, 0, 0, 255, 255, 255, 255,
    ];
    const ACCESS: &str = "system.posix_acl_access";
    const DEFAULT: &str = "system.posix_acl_default";
    for (private, filename, host) in replacement_targets() {
        for explicit in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let directory = if private {
                let directory = root.join(STATE_DIRECTORY);
                fs::create_dir(&directory)?;
                directory
            } else {
                root.clone()
            };
            xattr::set(&directory, DEFAULT, INHERITED)?;
            let output = directory.join(filename);
            fs::write(&output, b"last valid output")?;
            if explicit {
                xattr::set(&output, ACCESS, RESTRICTED)?;
            } else {
                xattr::remove(&output, ACCESS)?;
            }
            fs::set_permissions(&output, fs::Permissions::from_mode(0o660))?;
            let acl = xattr::get(&output, ACCESS)?;
            let owner = fs::metadata(&output)?;
            let ownership = (owner.uid(), owner.gid());
            require(
                acl.as_deref() == explicit.then_some(RESTRICTED),
                "native ACL fixture changed",
            )?;
            let outside = root.join("outside-owner");
            fs::hard_link(&output, &outside)?;
            let mut guard = if host {
                ProjectStateGuard::preflight(&root, None, None)?
            } else {
                ProjectStateGuard::preflight_document(&root, &output)?
                    .ok_or_else(|| io::Error::other("ACL document was not admitted"))?
            };
            let before_entries = fs::read_dir(&directory)?.count();
            let failed = guard.publish(
                &output,
                b"unpublished bytes",
                FilePublication::Replace,
                || Err(io::Error::other("controlled ACL publication failure")),
            );
            require(
                failed.is_err()
                    && fs::read(&output)? == b"last valid output"
                    && xattr::get(&output, ACCESS)? == acl
                    && fs::read_dir(&directory)?.count() == before_entries,
                "ACL publication failure changed the destination or leaked staging",
            )?;
            if host {
                guard.write_generated(&output, b"owned replacement")?;
            } else {
                guard.write_document(b"owned replacement")?;
            }
            require(
                xattr::get(&output, ACCESS)? == acl,
                &format!("replacement lost explicit ACL for {filename}"),
            )?;
            require(
                fs::read(&output)? == b"owned replacement"
                    && fs::metadata(&output)?.permissions().mode() & 0o777 == 0o660
                    && (fs::metadata(&output)?.uid(), fs::metadata(&output)?.gid()) == ownership,
                "ACL preservation changed output or mode",
            )?;
            require(
                fs::read(&outside)? == b"last valid output" && xattr::get(&outside, ACCESS)? == acl,
                "ACL replacement changed the outside owner",
            )?;
        }
    }
    Ok(())
}

/// Kernel attribute-storage proof, distinct from an enforced SELinux/Smack policy test.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires root security-xattr access and setpriv; selected explicitly in Linux CI"]
fn replacement_preserves_linux_security_labels_without_optional_relabel_privilege() -> io::Result<()>
{
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use xattr::FileExt;
    const CHILD_ROOT: &str = "PROJECTATLAS_TEST_SECURITY_LABEL_ROOT";
    const LABELS: [(&str, &[u8]); 4] = [
        ("security.selinux", b"system_u:object_r:tmp_t:s0\0"),
        ("security.SMACK64", b"projectatlas-fixture"),
        ("security.SMACK64EXEC", b"projectatlas-exec"),
        ("security.SMACK64MMAP", b"projectatlas-mmap"),
    ];
    require(
        fs::metadata("/proc/self")?.uid() == 0,
        "native security-label fixture requires root",
    )?;
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = std::path::PathBuf::from(root).canonicalize()?;
        let output = root.join(STATE_DIRECTORY).join(HOST_FILES[0]);
        let probe = root.join("equivalent-policy-probe");
        let policy = super::linux_access::ReplacementPolicy::capture(
            &output,
            &fs::symlink_metadata(&output)?,
        )?;
        let probe = fs::OpenOptions::new().read(true).write(true).open(probe)?;
        require(policy.matches(&probe)?, "equivalent-policy fixture differs")?;
        let prohibited = probe.set_xattr(LABELS[0].0, LABELS[0].1);
        require(
            prohibited
                .as_ref()
                .is_err_and(|source| matches!(source.raw_os_error(), Some(1 | 13))),
            &format!("label setter was not privilege-restricted: {prohibited:?}"),
        )?;
        policy.apply(&probe)?;
        require(
            policy.matches(&probe)?,
            "equal-policy preservation requested setters",
        )?;
        let guard = ProjectStateGuard::preflight(&root, None, None)?;
        let result = guard.write_generated(&output, b"must remain unpublished");
        require(
            result
                .as_ref()
                .is_err_and(|source| matches!(source.raw_os_error(), Some(1 | 13))),
            &format!("mismatched policy failed without a relabel refusal: {result:?}"),
        )?;
        require(
            fs::read(&output)? == b"last valid output",
            "refusal replaced source",
        )?;
        for (name, value) in LABELS {
            require(
                xattr::get(&output, name)?.as_deref() == Some(value),
                "refusal changed source security label",
            )?;
        }
        return require(
            fs::read_dir(root.join(STATE_DIRECTORY))?.count() == 1,
            "relabel refusal leaked named staging",
        );
    }
    for (private, filename, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let directory = if private {
            let directory = root.join(STATE_DIRECTORY);
            fs::create_dir(&directory)?;
            directory
        } else {
            root.clone()
        };
        let output = directory.join(filename);
        fs::write(&output, b"last valid output")?;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o640))?;
        // The privileged fixture also exercises a real ownership change on
        // the anonymous replacement, rather than only equal-owner metadata.
        std::os::unix::fs::chown(&output, Some(65534), Some(65534))?;
        for (name, value) in LABELS {
            xattr::set(&output, name, value)?;
            require(
                xattr::get(&output, name)?.as_deref() == Some(value),
                "kernel did not retain native label fixture",
            )?;
        }
        let outside = root.join("outside-owner");
        fs::hard_link(&output, &outside)?;
        let owner = fs::metadata(&output)?;
        let mut guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("native-label document was not admitted"))?
        };
        let entries = fs::read_dir(&directory)?.count();
        let failed = guard.publish(&output, b"unpublished", FilePublication::Replace, || {
            Err(io::Error::other("controlled labeled publication failure"))
        });
        require(
            failed.is_err()
                && fs::read(&output)? == b"last valid output"
                && fs::read_dir(&directory)?.count() == entries,
            "failed labeled replacement changed source or leaked staging",
        )?;
        if host {
            guard.write_generated(&output, b"owned replacement")?;
        } else {
            guard.write_document(b"owned replacement")?;
        }
        let after = fs::metadata(&output)?;
        require(
            fs::read(&output)? == b"owned replacement"
                && fs::read(&outside)? == b"last valid output"
                && (after.uid(), after.gid(), after.mode())
                    == (owner.uid(), owner.gid(), owner.mode())
                && fs::read_dir(&directory)?.count() == entries,
            "native-label replacement changed ownership, outside bytes or cleanup",
        )?;
        for path in [&output, &outside] {
            for (name, value) in LABELS {
                require(
                    xattr::get(path, name)?.as_deref() == Some(value),
                    "replacement lost a native file access label",
                )?;
            }
        }
    }
    // Prove equivalent policy needs no optional relabel capability, while a
    // real mismatched replacement refuses without changing its old output.
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let state = root.join(STATE_DIRECTORY);
    fs::create_dir(&state)?;
    for path in [
        state.join(HOST_FILES[0]),
        root.join("equivalent-policy-probe"),
    ] {
        fs::write(&path, b"last valid output")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
        for (name, value) in LABELS {
            xattr::set(&path, name, value)?;
        }
    }
    let child = std::process::Command::new("setpriv")
        .args(["--bounding-set=-sys_admin,-mac_admin", "--inh-caps=-all", "--ambient-caps=-all"])
        .arg(std::env::current_exe()?)
        .args([
            "--exact",
            "project_state::tests::replacement_preserves_linux_security_labels_without_optional_relabel_privilege",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_ROOT, &root)
        .output()?;
    require(
        child.status.success(),
        &format!("capability control failed: {child:?}"),
    )
}

#[cfg(target_os = "macos")]
#[test]
fn replacement_preserves_ordered_extended_acl_and_outside_owner() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    for (private, filename, host) in replacement_targets() {
        for explicit in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let directory = if private {
                let directory = root.join(STATE_DIRECTORY);
                fs::create_dir(&directory)?;
                directory
            } else {
                root.clone()
            };
            let output = directory.join(filename);
            fs::write(&output, b"last valid output")?;
            if explicit {
                let changed = std::process::Command::new("chmod")
                    .args(["+a", "group:everyone deny write"])
                    .arg(&output)
                    .output()?;
                require(
                    changed.status.success(),
                    &format!("native ACL fixture failed: {changed:?}"),
                )?;
                let inherited = std::process::Command::new("chmod")
                    .args(["+a", "group:everyone allow read,write,file_inherit,directory_inherit,only_inherit"])
                    .arg(&directory)
                    .output()?;
                require(
                    inherited.status.success(),
                    &format!("native inherited ACL failed: {inherited:?}"),
                )?;
            }
            let acl = exacl::getfacl(&output, None)?;
            let owner = fs::metadata(&output)?;
            let ownership = (owner.uid(), owner.gid());
            require(
                acl.is_empty() != explicit,
                "native ACL fixture has unexpected entries",
            )?;
            let outside = root.join("outside-owner");
            fs::hard_link(&output, &outside)?;
            let mut guard = if host {
                ProjectStateGuard::preflight(&root, None, None)?
            } else {
                ProjectStateGuard::preflight_document(&root, &output)?
                    .ok_or_else(|| io::Error::other("ACL document was not admitted"))?
            };
            let before_entries = fs::read_dir(&directory)?.count();
            let failed = guard.publish(
                &output,
                b"unpublished bytes",
                FilePublication::Replace,
                || Err(io::Error::other("controlled ACL publication failure")),
            );
            require(
                failed.is_err()
                    && fs::read(&output)? == b"last valid output"
                    && exacl::getfacl(&output, None)? == acl
                    && fs::read_dir(&directory)?.count() == before_entries,
                "ACL failure changed the destination or leaked staging",
            )?;
            if host {
                guard.write_generated(&output, b"owned replacement")?;
            } else {
                guard.write_document(b"owned replacement")?;
            }
            require(
                fs::read(&output)? == b"owned replacement"
                    && exacl::getfacl(&output, None)? == acl
                    && (fs::metadata(&output)?.uid(), fs::metadata(&output)?.gid()) == ownership,
                "replacement lost ordered extended ACL",
            )?;
            require(
                fs::read(&outside)? == b"last valid output"
                    && exacl::getfacl(&outside, None)? == acl,
                "ACL replacement changed outside owner",
            )?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn descriptor_acl_empty_readback_and_unsupported_objects_preserve_handles() -> io::Result<()> {
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let file = tempfile::NamedTempFile::new()?;
    super::macos_access::clear_acl(file.as_file())?;
    let captured = super::macos_access::get_acl(file.as_file())?;
    let allocated_empty = super::macos_acl::AccessAcl::empty()?;
    require(
        !allocated_empty.is_absent() && allocated_empty.is_empty()?,
        "native empty ACL allocation was classified as an absent snapshot",
    )?;
    if captured.is_absent() {
        require(
            captured != allocated_empty,
            "absent and allocated-empty snapshots compare equally",
        )?;
    }
    require(
        captured.is_empty()? && exacl::getfacl(file.path(), None)?.is_empty(),
        "native empty ACL readback differs from the independent path query",
    )?;
    let target = tempfile::NamedTempFile::new()?;
    super::macos_access::set_acl(target.as_file(), &captured)?;
    require(
        super::macos_access::get_acl(target.as_file())? == captured
            && exacl::getfacl(target.path(), None)?.is_empty(),
        "native absent or empty ACL changed during descriptor preservation",
    )?;
    super::macos_access::set_acl(target.as_file(), &allocated_empty)?;
    require(
        super::macos_access::get_acl(target.as_file())?.is_empty()?
            && exacl::getfacl(target.path(), None)?.is_empty(),
        "allocated-empty descriptor ACL did not preserve empty native policy",
    )?;

    let (socket, mut receiver) = UnixStream::pair()?;
    let mut unsupported = fs::File::from(OwnedFd::from(socket));
    let refused = super::macos_access::get_acl(&unsupported);
    require(
        refused
            .as_ref()
            .is_err_and(|source| source.kind() == io::ErrorKind::InvalidInput),
        "unsupported descriptor was silently treated as an absent ACL",
    )?;
    unsupported.write_all(b"live")?;
    let mut received = [0; 4];
    receiver.read_exact(&mut received)?;
    require(
        received == *b"live",
        "ACL refusal consumed its borrowed descriptor",
    )
}

#[cfg(target_os = "macos")]
#[test]
fn descriptor_acl_operations_preserve_foreign_objects_after_name_substitution() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    for directory in [false, true] {
        for symlink in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let named = root.join("admitted-object");
            let retained = root.join("retained-object");
            let victim = root.join("foreign-object");
            for path in [&named, &victim] {
                if directory {
                    fs::create_dir(path)?;
                    fs::write(path.join("payload"), b"preserved directory payload")?;
                } else {
                    fs::write(path, b"preserved file payload")?;
                }
                let setup = std::process::Command::new("chmod")
                    .args(["+a", "user:nobody deny write"])
                    .arg(path)
                    .output()?;
                require(
                    setup.status.success(),
                    "descriptor ACL fixture setup failed",
                )?;
            }
            let source_acl = exacl::getfacl(&named, None)?;
            let foreign_acl = exacl::getfacl(&victim, None)?;
            require(
                !source_acl.is_empty() && !foreign_acl.is_empty(),
                "descriptor ACL fixture has no restrictive ACL",
            )?;
            let held = fs::File::open(&named)?;
            let identity = held.metadata()?;
            let captured_acl = super::macos_access::get_acl(&held)?;
            let readback = tempfile::NamedTempFile::new_in(&root)?;
            super::macos_access::set_acl(readback.as_file(), &captured_acl)?;
            require(
                !captured_acl.is_empty()? && exacl::getfacl(readback.path(), None)? == source_acl,
                "descriptor ACL read differs before substitution",
            )?;
            fs::rename(&named, &retained)?;
            if symlink {
                std::os::unix::fs::symlink(&victim, &named)?;
            } else if directory {
                fs::rename(&victim, &named)?;
            } else {
                fs::hard_link(&victim, &named)?;
            }
            let foreign = if directory && !symlink {
                &named
            } else {
                &victim
            };
            // Reproduce the old path-ACL boundary after its identity check:
            // the same setter now mutates a different real object or alias target.
            exacl::setfacl(&[&named], &[], None)?;
            require(
                exacl::getfacl(foreign, None)?.is_empty(),
                "path ACL control did not mutate the foreign owner",
            )?;
            exacl::setfacl(&[foreign], &foreign_acl, None)?;
            require(
                super::macos_access::get_acl(&held)? == captured_acl,
                "descriptor ACL read followed the replaced name",
            )?;
            super::macos_access::clear_acl(&held)?;
            require(
                super::macos_access::get_acl(&held)?.is_empty()?
                    && exacl::getfacl(&retained, None)?.is_empty()
                    && exacl::getfacl(foreign, None)? == foreign_acl
                    && (held.metadata()?.dev(), held.metadata()?.ino())
                        == (identity.dev(), identity.ino()),
                "descriptor ACL mutation escaped the retained object",
            )?;
            let payload = |path: &Path| {
                if directory {
                    path.join("payload")
                } else {
                    path.to_path_buf()
                }
            };
            let expected = if directory {
                b"preserved directory payload".as_slice()
            } else {
                b"preserved file payload".as_slice()
            };
            require(
                fs::read(payload(&retained))? == expected
                    && fs::read(payload(foreign))? == expected,
                "descriptor ACL mutation changed source or foreign bytes",
            )?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires passwordless fixture-only sudo as nobody; selected explicitly in macOS CI"]
fn private_replacement_staging_excludes_inherited_acl_handles_and_retains_foreign_entries()
-> io::Result<()> {
    use std::io::{BufRead, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::process::{Command, Stdio};

    fn chmod_acl(path: &Path, entry: &str) -> io::Result<()> {
        let output = Command::new("chmod")
            .args(["+a", entry])
            .arg(path)
            .output()?;
        require(
            output.status.success(),
            &format!("ACL fixture failed: {output:?}"),
        )
    }
    fn nobody_open(path: &Path, write: bool) -> io::Result<std::process::Output> {
        Command::new("sudo")
            .args([
                "-n",
                "-u",
                "nobody",
                "/usr/bin/python3",
                "-c",
                "import os,sys; fd=os.open(sys.argv[1], os.O_WRONLY if sys.argv[2]=='write' else os.O_RDONLY); os.close(fd)",
            ])
            .arg(path)
            .arg(if write { "write" } else { "read" })
            .output()
    }
    fn check_private(stage: &tempfile::NamedTempFile, writer: u32) -> io::Result<()> {
        let parent = stage
            .path()
            .parent()
            .ok_or_else(|| io::Error::other("stage has no parent"))?;
        let metadata = fs::metadata(parent)?;
        require(
            parent
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".publication-"))
                && metadata.uid() == writer
                && metadata.permissions().mode() & 0o777 == 0o700
                && exacl::getfacl(parent, None)?.is_empty(),
            "replacement workspace is not writer-owned, private and ACL-empty",
        )?;
        for write in [false, true] {
            let output = nobody_open(stage.path(), write)?;
            require(
                !output.status.success()
                    && String::from_utf8_lossy(&output.stderr).contains("Permission denied"),
                &format!("private stage did not deny native data open: {output:?}"),
            )?;
        }
        Ok(())
    }

    let sudo = Command::new("sudo")
        .args(["-n", "-u", "nobody", "/usr/bin/true"])
        .output()?;
    require(
        sudo.status.success(),
        "native ACL fixture requires passwordless sudo as nobody",
    )?;
    // A private runner TMPDIR would make every nobody open fail at an unrelated
    // ancestor. CI supplies a traversable disposable volume; ordinary proof uses
    // the public temporary directory and hardens only this fixture's own roots.
    let base = std::env::var_os("PROJECTATLAS_TEST_NATIVE_ACCESS_ROOT").map_or_else(
        || std::path::PathBuf::from("/private/tmp"),
        std::path::PathBuf::from,
    );
    let temp = tempfile::tempdir_in(&base)?;
    let root = temp.path().canonicalize()?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
    let writer = fs::metadata(&root)?.uid();
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    fs::set_permissions(&guard.directory, fs::Permissions::from_mode(0o755))?;
    let traversal = Command::new("sudo")
        .args(["-n", "-u", "nobody", "/bin/test", "-x"])
        .arg(&guard.directory)
        .output()?;
    require(
        traversal.status.success(),
        &format!("native ACL fixture ancestors block nobody traversal: {traversal:?}"),
    )?;
    let output = guard.directory.join(HOST_FILES[0]);
    fs::write(&output, b"last valid output")?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o640))?;
    chmod_acl(&output, "user:nobody deny read,write")?;
    let original_acl = exacl::getfacl(&output, None)?;
    chmod_acl(
        &guard.directory,
        "user:nobody allow read,write,file_inherit,only_inherit",
    )?;
    let parent_acl = exacl::getfacl(&guard.directory, None)?;
    require(
        !parent_acl.is_empty(),
        "native inheritance fixture has no parent ACL",
    )?;

    let baseline = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o600))
        .tempfile_in(&guard.directory)?;
    let mut child = Command::new("sudo")
        .args(["-n", "-u", "nobody", "/bin/sh", "-c",
            "exec 3>\"$1\" || exit; printf 'ready\\n'; IFS= read -r signal || exit; printf retained-fd >&3",
            "projectatlas-held-fd"])
        .arg(baseline.path())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn()?;
    let causal = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing fixture stdout"))?;
        let mut ready = String::new();
        std::io::BufReader::new(stdout).read_line(&mut ready)?;
        require(
            ready == "ready\n",
            "inherited ACL did not override baseline mode0600",
        )?;
        exacl::setfacl(&[baseline.path()], &[], None)?;
        baseline
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        require(
            !nobody_open(baseline.path(), true)?.status.success(),
            "narrowed ACL still permits new handles",
        )?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("missing fixture stdin"))?
            .write_all(b"continue\n")
    })();
    drop(child.stdin.take());
    let status = child.wait_with_output()?;
    causal.map_err(|source| {
        io::Error::new(
            source.kind(),
            format!(
                "{source}; baseline_uid={}, baseline_mode={:o}, baseline_acl_count={}, parent_acl_count={}, native_child={status:?}",
                baseline.as_file().metadata().map_or(u32::MAX, |metadata| metadata.uid()),
                baseline.as_file().metadata().map_or(0, |metadata| metadata.permissions().mode()),
                exacl::getfacl(baseline.path(), None).map_or(0, |entries| entries.len()),
                parent_acl.len(),
            ),
        )
    })?;
    require(
        status.status.success() && fs::read(baseline.path())? == b"retained-fd",
        &format!("baseline held descriptor did not survive ACL narrowing: {status:?}"),
    )?;
    baseline.close()?;
    for write in [false, true] {
        require(
            !nobody_open(&output, write)?.status.success(),
            "old target allows nobody access",
        )?;
    }
    guard.publish_with_stage_check(
        &output,
        b"owned replacement",
        FilePublication::Replace,
        |stage| check_private(stage, writer),
        || Ok(()),
    )?;
    require(
        fs::read(&output)? == b"owned replacement"
            && exacl::getfacl(&output, None)? == original_acl
            && !nobody_open(&output, true)?.status.success(),
        "protected replacement lost bytes or restrictive ACL",
    )?;
    require(
        exacl::getfacl(&guard.directory, None)? == parent_acl,
        "replacement changed its parent inheritance ACL",
    )?;
    let direct = guard.directory.join("ordinary-inheritance-control");
    fs::write(&direct, b"ordinary file")?;
    let direct_acl = exacl::getfacl(&direct, None)?;
    let direct_write = nobody_open(&direct, true)?.status.success();
    let append = Command::new("sudo")
        .args([
            "-n",
            "-u",
            "nobody",
            "/usr/bin/python3",
            "-c",
            "import os,sys; fd=os.open(sys.argv[1], os.O_WRONLY|os.O_APPEND); os.close(fd)",
        ])
        .arg(&direct)
        .output()?;
    require(
        !direct_acl.is_empty() && direct_write && !append.status.success(),
        &format!(
            "native fixture must distinguish granted write from ungranted append: parent_acl_count={}, direct_acl_count={}, write_allowed={direct_write}, append_allowed={}",
            parent_acl.len(),
            direct_acl.len(),
            append.status.success()
        ),
    )?;
    fs::remove_file(direct)?;
    let created = guard.directory.join(HOST_FILES[1]);
    let mut creation = None;
    guard.publish_with_stage_check(
        &created,
        b"fresh output",
        FilePublication::Create,
        |stage| {
            creation = Some((
                stage.as_file().metadata()?.ino(),
                exacl::getfacl(stage.path(), None)?,
                nobody_open(stage.path(), true)?.status.success(),
            ));
            Ok(())
        },
        || Ok(()),
    )?;
    let (staged_inode, staged_acl, staged_write) =
        creation.ok_or_else(|| io::Error::other("new-file stage was not observed"))?;
    require(
        !staged_acl.is_empty() && staged_write,
        &format!(
            "new-file stage lost inherited policy: stage_acl_count={}, write_allowed={staged_write}",
            staged_acl.len()
        ),
    )?;
    let final_acl = exacl::getfacl(&created, None)?;
    let final_write = nobody_open(&created, true)?.status.success();
    let same_inode = fs::metadata(&created)?.ino() == staged_inode;
    require(
        final_acl == staged_acl
            && final_write
            && same_inode
            && fs::read(&created)? == b"fresh output",
        &format!(
            "new output lost legitimate direct-parent ACL inheritance: stage_acl_count={}, final_acl_count={}, write_allowed={final_write}, same_inode={same_inode}",
            staged_acl.len(),
            final_acl.len()
        ),
    )?;

    for substitute_directory in [false, true] {
        let parked = root.join("retained-private-workspace");
        let mut workspace = None;
        let result = guard.publish_with_stage_check(
            &output,
            b"unpublished",
            FilePublication::Replace,
            |stage| {
                check_private(stage, writer)?;
                workspace = Some(
                    stage
                        .path()
                        .parent()
                        .ok_or_else(|| io::Error::other("missing parent"))?
                        .to_path_buf(),
                );
                Ok(())
            },
            || {
                // Obtain the exact active workspace independently of the stage
                // observer borrow; no process-global test hook is needed.
                let private = fs::read_dir(&guard.directory)?
                    .collect::<io::Result<Vec<_>>>()?
                    .into_iter()
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".publication-")
                    })
                    .ok_or_else(|| io::Error::other("missing private fixture directory"))?
                    .path();
                if substitute_directory {
                    fs::rename(&private, &parked)?;
                    fs::create_dir(&private)?;
                }
                fs::write(private.join("foreign-sentinel"), b"foreign owner")?;
                Err(io::Error::other("controlled private cleanup failure"))
            },
        );
        require(
            result
                .as_ref()
                .is_err_and(|error| error.to_string().contains("cleanup was withheld"))
                && fs::read(&output)? == b"owned replacement",
            &format!("private cleanup failure was concealed: {result:?}"),
        )?;
        let private = workspace.ok_or_else(|| io::Error::other("stage observer did not run"))?;
        require(
            fs::read(private.join("foreign-sentinel"))? == b"foreign owner",
            "foreign workspace entry was removed",
        )?;
        if substitute_directory {
            require(
                fs::read_dir(&parked)?.count() == 1,
                "retained payload stage was removed",
            )?;
            fs::remove_dir_all(&parked)?;
        } else {
            require(
                fs::read_dir(&private)?.count() == 1,
                "owned payload stage was not cleaned",
            )?;
        }
        fs::remove_file(private.join("foreign-sentinel"))?;
        fs::remove_dir(private)?;
    }

    // No inherited ACL and equal ownership do not protect a named stage from
    // directory-entry replacement by another user of a shared state directory.
    let shared_temp = tempfile::tempdir_in(&base)?;
    let shared_root = shared_temp.path().canonicalize()?;
    fs::set_permissions(&shared_root, fs::Permissions::from_mode(0o755))?;
    let mut shared = ProjectStateGuard::preflight(&shared_root, None, None)?;
    shared.ensure_directory()?;
    exacl::setfacl(&[&shared_root, &shared.directory], &[], None)?;
    fs::set_permissions(&shared.directory, fs::Permissions::from_mode(0o777))?;
    let selected = shared.directory.join(HOST_FILES[0]);
    fs::write(&selected, b"same-owner output")?;
    fs::set_permissions(&selected, fs::Permissions::from_mode(0o640))?;
    let victim = shared_root.join("outside-acl-owner");
    fs::write(&victim, b"outside ACL and bytes")?;
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o666))?;
    chmod_acl(&victim, "user:nobody deny write")?;
    let victim_acl = exacl::getfacl(&victim, None)?;
    let substitute = |path: &Path| {
        Command::new("sudo")
            .args([
                "-n",
                "-u",
                "nobody",
                "/usr/bin/python3",
                "-c",
                "import os,sys; os.unlink(sys.argv[1]); os.symlink(sys.argv[2],sys.argv[1])",
            ])
            .arg(path)
            .arg(&victim)
            .output()
    };
    let mut exposed = tempfile::NamedTempFile::new_in(&shared.directory)?;
    exposed.disable_cleanup(true);
    let changed = substitute(exposed.path())?;
    require(
        changed.status.success(),
        "shared-directory substitution control failed",
    )?;
    exacl::setfacl(&[exposed.path()], &[], None)?;
    require(
        exacl::getfacl(&victim, None)?.is_empty() && nobody_open(&victim, true)?.status.success(),
        "following ACL setter did not reproduce the outside-owner mutation",
    )?;
    exacl::setfacl(&[&victim], &victim_acl, None)?;
    // The held inode remains the target after its name becomes an alias.
    super::macos_access::clear_acl(exposed.as_file())?;
    require(
        exacl::getfacl(&victim, None)? == victim_acl,
        "descriptor ACL setter changed the outside owner",
    )?;
    fs::remove_file(exposed.path())?;
    drop(exposed);
    shared.publish_with_stage_check(
        &selected,
        b"protected same-owner replacement",
        FilePublication::Replace,
        |stage| {
            check_private(stage, writer)?;
            let changed = substitute(stage.path())?;
            require(
                !changed.status.success()
                    && String::from_utf8_lossy(&changed.stderr).contains("Permission denied"),
                "another user replaced a protected stage entry",
            )
        },
        || Ok(()),
    )?;
    require(
        fs::read(&selected)? == b"protected same-owner replacement"
            && fs::read(&victim)? == b"outside ACL and bytes"
            && exacl::getfacl(&victim, None)? == victim_acl,
        "shared-directory replacement changed its outside ACL owner",
    )?;

    let owner_temp = tempfile::tempdir_in(&base)?;
    let owner_root = owner_temp.path().canonicalize()?;
    fs::set_permissions(&owner_root, fs::Permissions::from_mode(0o755))?;
    let mut owner_guard = ProjectStateGuard::preflight(&owner_root, None, None)?;
    owner_guard.ensure_directory()?;
    exacl::setfacl(&[&owner_root, &owner_guard.directory], &[], None)?;
    require(
        exacl::getfacl(&owner_guard.directory, None)?.is_empty(),
        "owner-mismatch fixture still has an ACL inheritance trigger",
    )?;
    fs::set_permissions(&owner_guard.directory, fs::Permissions::from_mode(0o755))?;
    let owned = owner_guard.directory.join(HOST_FILES[0]);
    fs::write(&owned, b"other owner's output")?;
    fs::set_permissions(&owned, fs::Permissions::from_mode(0o644))?;
    let chown = Command::new("sudo")
        .args(["-n", "chown", "nobody"])
        .arg(&owned)
        .output()?;
    require(
        chown.status.success() && fs::metadata(&owned)?.uid() != writer,
        "owner-mismatch fixture failed",
    )?;
    let identity = fs::metadata(&owned)?.ino();
    let mut observed = false;
    let result = owner_guard.publish_with_stage_check(
        &owned,
        b"unpublished",
        FilePublication::Replace,
        |stage| {
            observed = true;
            check_private(stage, writer)?;
            Err(io::Error::other("controlled owner-mismatch stage failure"))
        },
        || Ok(()),
    );
    require(
        observed
            && result.is_err()
            && fs::metadata(&owned)?.ino() == identity
            && fs::read(&owned)? == b"other owner's output"
            && fs::read_dir(&owner_guard.directory)?.count() == 1,
        "owner-mismatch staging changed source or leaked a private workspace",
    )
}

/// Load only native fixture declarations, without its child-test entry point.
#[cfg(windows)]
fn windows_fixture_script(script: &str) -> io::Result<String> {
    let (declarations, _) = WINDOWS_SYSTEM_POLICY_FIXTURE
        .split_once("$mode=$env:PROJECTATLAS_TEST_SYSTEM_POLICY_MODE")
        .ok_or_else(|| io::Error::other("native security fixture entry point is missing"))?;
    Ok(format!("{declarations}\n{script}"))
}

/// Retain an owned UTF-8 script through child completion without command-line limits.
#[cfg(windows)]
fn windows_powershell_fixture(
    script: &str,
    configure: impl FnOnce(&mut std::process::Command),
) -> io::Result<std::process::Output> {
    use std::io::Write;
    let mut file = tempfile::Builder::new().suffix(".ps1").tempfile()?;
    // Windows PowerShell needs the BOM to distinguish UTF-8 from its ANSI default.
    file.write_all(b"\xef\xbb\xbf")?;
    file.write_all(script.as_bytes())?;
    file.flush()?;
    // The script reader may share READ only. Close the writable setup handle
    // before it opens the file; retain the owned path guard through completion.
    let path = file.into_temp_path();
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            // -Command did not apply script execution policy. Scope this
            // compatibility setting only to our owned test-script process.
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path);
    configure(&mut command);
    command.output()
}

/// Large Unicode fixtures execute from an owned file rather than an oversized argv.
#[cfg(windows)]
#[test]
fn native_policy_script_uses_file_transport_beyond_command_line_limits() -> io::Result<()> {
    let script = format!(
        "#{}\nif ('λ' -ne [string][char]0x3bb) {{ throw 'UTF-8 fixture changed' }}\n[Console]::Write('fixture passed')",
        "padding".repeat(5_000),
    );
    let output = windows_powershell_fixture(&script, |_| {})?;
    require(
        output.status.success() && output.stdout == b"fixture passed",
        &format!("file-backed native fixture failed: {output:?}"),
    )
}

/// Prepare a native Windows ACL independently of the Rust publication adapter.
#[cfg(windows)]
fn windows_acl(path: &Path, prepare: Option<bool>) -> io::Result<String> {
    const READ: &str = "(Get-Acl -LiteralPath $env:PROJECTATLAS_TEST_ACL_PATH).Sddl";
    let script = if let Some(protected) = prepare {
        format!(
            "$ErrorActionPreference='Stop'; $p=$env:PROJECTATLAS_TEST_ACL_PATH; \
            $a=Get-Acl -LiteralPath $p; $a.SetAccessRuleProtection($true,$false); \
            $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; \
            $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($me,'FullControl','Allow')); \
            $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($me,'Delete','Deny')); \
            $guest=[System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-546'); \
            $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($guest,'Write','Deny')); \
            $a.SetAccessRuleProtection({},$true); \
            [SystemPolicyFixture]::DiscretionaryPolicy($p,$a.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::Access)); {READ}",
            if protected { "$true" } else { "$false" },
        )
    } else {
        format!("$ErrorActionPreference='Stop'; {READ}")
    };
    let script = if prepare.is_some() {
        windows_fixture_script(&script)?
    } else {
        script
    };
    let result = windows_powershell_fixture(&script, |command| {
        command.env("PROJECTATLAS_TEST_ACL_PATH", path);
    })?;
    require(
        result.status.success(),
        &format!("native ACL query failed: {result:?}"),
    )?;
    String::from_utf8(result.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(io::Error::other)
}

/// Compare complete native file descriptors, permitting only DACL AI bookkeeping.
#[cfg(windows)]
fn windows_file_acl_matches(expected: &str, observed: &str) -> io::Result<bool> {
    use std::io::Write;
    let script = r"
$ErrorActionPreference='Stop'
function Normalize-FileAcl([string]$sddl) {
  $sd=[Security.AccessControl.RawSecurityDescriptor]::new($sddl)
  $before=[byte[]]::new($sd.BinaryLength); $sd.GetBinaryForm($before,0)
  $control=[int]$sd.ControlFlags
  $sd.SetFlags([Security.AccessControl.ControlFlags]($control -band (-bnot 0x400)))
  $after=[byte[]]::new($sd.BinaryLength); $sd.GetBinaryForm($after,0)
  if ($before.Length -ne $after.Length) { throw 'DACL AI normalization changed descriptor length' }
  # SECURITY_DESCRIPTOR_RELATIVE has its SDK control word at bytes 2..3.
  # Prove the typed operation changes exactly that one bit and no other byte.
  for ($i=0; $i -lt $before.Length; $i++) {
    $difference=$before[$i] -bxor $after[$i]
    $required=if ($i -eq 3) { ($control -band 0x400) -shr 8 } else { 0 }
    if ($difference -ne $required) { throw 'DACL AI normalization changed other native descriptor bytes' }
  }
  [Convert]::ToBase64String($after)
}
$expected=[Security.AccessControl.RawSecurityDescriptor]::new($env:PROJECTATLAS_TEST_EXPECTED_ACL)
$observed=[Security.AccessControl.RawSecurityDescriptor]::new($env:PROJECTATLAS_TEST_OBSERVED_ACL)
$equal=(Normalize-FileAcl $env:PROJECTATLAS_TEST_EXPECTED_ACL) -ceq (Normalize-FileAcl $env:PROJECTATLAS_TEST_OBSERVED_ACL)
[Console]::Error.WriteLine(('file ACL comparison expected_control={0:x4}; observed_control={1:x4}; equal_except_dacl_ai={2}' -f [int]$expected.ControlFlags,[int]$observed.ControlFlags,$equal))
$equal
";
    let result = windows_powershell_fixture(script, |command| {
        command
            .env("PROJECTATLAS_TEST_EXPECTED_ACL", expected)
            .env("PROJECTATLAS_TEST_OBSERVED_ACL", observed);
    })?;
    require(
        result.status.success(),
        &format!("native file ACL comparison failed: {result:?}"),
    )?;
    io::stderr().lock().write_all(&result.stderr)?;
    match result.stdout.trim_ascii() {
        b"True" => Ok(true),
        b"False" => Ok(false),
        _ => Err(io::Error::other(
            "native file ACL comparison returned no boolean",
        )),
    }
}

/// The independent oracle retains all controls/components except file DACL AI.
#[cfg(windows)]
#[test]
fn windows_file_acl_comparison_preserves_every_other_policy_component() -> io::Result<()> {
    const EXPECTED: &str = "O:SYG:BAD:P(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)";
    require(
        windows_file_acl_matches(
            EXPECTED,
            "O:SYG:BAD:PAI(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        )?,
        "file DACL AI alone changed policy equivalence",
    )?;
    for different in [
        "O:BAG:BAD:P(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BUD:P(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BAD:(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BAD:PAR(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BAD:P(A;;FW;;;WD)(D;;FW;;;AN)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BAD:P(D;;FW;;;AN)(A;;FR;;;WD)S:P(AU;SA;FR;;;WD)",
        "O:SYG:BAD:P(A;;FR;;;WD)(D;;FW;;;AN)S:PAI(AU;SA;FR;;;WD)",
        "O:SYG:BAD:P(A;;FR;;;WD)(D;;FW;;;AN)S:(AU;SA;FR;;;WD)",
        "O:SYG:BAD:P(A;;FR;;;WD)(D;;FW;;;AN)S:P(AU;SA;FW;;;WD)",
        "O:SYG:BAD:P",
        "O:SYG:BAD:NO_ACCESS_CONTROL",
    ] {
        require(
            !windows_file_acl_matches(EXPECTED, different)?,
            "file ACL oracle discarded a meaningful policy difference",
        )?;
    }
    require(
        !windows_file_acl_matches("O:SYG:BAD:", "O:SYG:BAD:NO_ACCESS_CONTROL")?,
        "file ACL oracle conflated empty and NULL DACLs",
    )
}

/// Set only a fixture's DACL before the publisher captures its native policy.
#[cfg(windows)]
fn windows_discretionary_policy(path: &Path, policy: &str) -> io::Result<()> {
    let script = windows_fixture_script(
        "[SystemPolicyFixture]::DiscretionaryPolicy($env:PROJECTATLAS_TEST_ACL_PATH,$env:PROJECTATLAS_TEST_DACL)",
    )?;
    let result = windows_powershell_fixture(&script, |command| {
        command
            .env("PROJECTATLAS_TEST_ACL_PATH", path)
            .env("PROJECTATLAS_TEST_DACL", policy);
    })?;
    require(
        result.status.success(),
        &format!("native discretionary policy setup failed: {result:?}"),
    )
}

/// Native fixture setup/oracle and privilege changes confined to child processes.
#[cfg(windows)]
const WINDOWS_SYSTEM_POLICY_FIXTURE: &str = r#"
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class SystemPolicyFixture {
  [StructLayout(LayoutKind.Sequential)] struct Luid { public uint Low; public int High; }
  [StructLayout(LayoutKind.Sequential)] struct Privilege { public uint Count; public Luid Id; public uint Attributes; }
  [StructLayout(LayoutKind.Sequential)] struct AclSize { public uint Count; public uint Used; public uint Free; }
  [StructLayout(LayoutKind.Sequential)] struct SecurityAttributes { public uint Length; public IntPtr Descriptor; public bool Inherit; }
  [StructLayout(LayoutKind.Sequential)] struct ObjectBasicInformation {
    public uint Attributes, GrantedAccess, HandleCount, PointerCount;
    [MarshalAs(UnmanagedType.ByValArray, SizeConst=10)] public uint[] Reserved;
  }
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool LookupPrivilegeValue(string system, string name, out Luid id);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr token, bool disableAll, ref Privilege state, uint length, IntPtr previous, IntPtr returned);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern uint GetNamedSecurityInfo(string path, uint kind, uint information, out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern uint SetNamedSecurityInfo(string path, uint kind, uint information, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string text, uint revision, out IntPtr descriptor, out uint length);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool ConvertSecurityDescriptorToStringSecurityDescriptor(IntPtr descriptor, uint revision, uint information, out IntPtr text, out uint length);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetSecurityDescriptorControl(IntPtr descriptor, out ushort control, out uint revision);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetSecurityDescriptorSacl(IntPtr descriptor, out bool present, out IntPtr sacl, out bool defaulted);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetAclInformation(IntPtr acl, out AclSize size, uint length, uint information);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetAce(IntPtr acl, uint index, out IntPtr ace);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool InitializeAcl(IntPtr acl, uint length, uint revision);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetSecurityDescriptorDacl(IntPtr descriptor, out bool present, out IntPtr dacl, out bool defaulted);
  [DllImport("advapi32.dll")] static extern uint GetSecurityDescriptorLength(IntPtr descriptor);
  [DllImport("advapi32.dll")] static extern uint GetSecurityInfo(IntPtr handle, uint kind, uint information, out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
  [DllImport("advapi32.dll")] static extern uint SetSecurityInfo(IntPtr handle, uint kind, uint information, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl);
  [DllImport("kernel32.dll", EntryPoint="CreateFileW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)] static extern IntPtr CreateFile(string path, uint access, uint share, ref SecurityAttributes security, uint disposition, uint flags, IntPtr template);
  [DllImport("kernel32.dll", EntryPoint="MoveFileExW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)] static extern bool MoveFileEx(string source, string destination, uint flags);
  [DllImport("kernel32.dll", EntryPoint="DeleteFileW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)] static extern bool DeleteFile(string path);
  [DllImport("advapi32.dll", EntryPoint="GetSecurityInfo")] static extern uint GetSecurityDescriptorOnly(IntPtr handle, uint kind, uint information, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl, out IntPtr descriptor);
  [DllImport("ntdll.dll")] static extern int NtQueryObject(IntPtr handle, uint information, out ObjectBasicInformation result, uint length, out uint returned);
  // Microsoft documents NtSetSecurityObject as the user-mode form of the
  // retained-handle DACL setter. Fixture-only: no product path calls this API.
  [DllImport("ntdll.dll")] static extern int NtSetSecurityObject(IntPtr handle,uint information,IntPtr descriptor);
  public static void Enable(bool enabled) {
    EnablePrivilege("SeSecurityPrivilege",enabled);
  }
  public static void EnableRestore() { EnablePrivilege("SeRestorePrivilege",true); }
  static void EnablePrivilege(string name,bool enabled) {
    IntPtr token;
    if (!OpenProcessToken(GetCurrentProcess(), 0x28, out token)) throw new Win32Exception();
    try {
      Luid id;
      if (!LookupPrivilegeValue(null, name, out id)) throw new Win32Exception();
      Privilege state=new Privilege { Count=1, Id=id, Attributes=enabled ? 2u : 0u };
      if (!AdjustTokenPrivileges(token, false, ref state, 0, IntPtr.Zero, IntPtr.Zero)) throw new Win32Exception();
      int error=Marshal.GetLastWin32Error();
      if (error != 0) throw new Win32Exception(error);
    } finally { CloseHandle(token); }
  }
  public static string Policy(string path, string replacement, uint query) {
    return Policy(path,replacement,query,false);
  }
  public static string Policy(string path, string replacement, uint query, bool retained) {
    IntPtr descriptor=IntPtr.Zero, text=IntPtr.Zero, empty=IntPtr.Zero;
    uint length, revision; ushort control;
    try {
      if (!String.IsNullOrEmpty(replacement)) {
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor(replacement, 1, out descriptor, out length)) throw new Win32Exception(Marshal.GetLastWin32Error(),"parse system-policy fixture SDDL");
        bool present, defaulted; IntPtr sacl;
        if (!GetSecurityDescriptorSacl(descriptor, out present, out sacl, out defaulted)) throw new Win32Exception();
        if (!GetSecurityDescriptorControl(descriptor, out control, out revision)) throw new Win32Exception();
        // Bare SDDL S: normalizes to a NULL pointer. Construct an actual empty
        // SDK ACL for the independent NULL-versus-allocated-empty file control.
        if (replacement == "S:" || replacement == "S:P") {
          empty=Marshal.AllocHGlobal(8);
          if (!InitializeAcl(empty,8,2)) throw new Win32Exception();
          sacl=empty;
        }
        uint flags=0x18u | ((control & 0x2000) != 0 ? 0x40000000u : 0x10000000u);
        // Extended fixture setup explicitly selects each requested ACE class;
        // production's combined SACL/LABEL query remains the subject of proof.
        if (replacement.Contains("(RA;")) flags |= 0x20u;
        if (replacement.Contains("(SP;")) flags |= 0x40u;
        if (replacement.Contains("(TL;")) flags |= 0x80u;
        if (replacement.Contains("(FL;")) flags |= 0x100u;
        uint error=SetNamedSecurityInfo(path, 1, flags, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, sacl);
        if (error != 0) throw new Win32Exception((int)error,"set system-policy fixture flags="+flags.ToString("x"));
        LocalFree(descriptor); descriptor=IntPtr.Zero;
      }
      IntPtr owner, group, dacl, system;
      uint result;
      if (retained) {
        SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
        uint access=0x20080u | ((query & 8) != 0 ? 0x01000000u : 0u);
        IntPtr handle=CreateFile(path,access,7,ref security,3,0x00200080,IntPtr.Zero);
        if (handle == new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error(),"Open retained policy handle");
        try { result=GetSecurityDescriptorOnly(handle,1,query,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out descriptor); }
        finally { CloseHandle(handle); }
      } else {
        result=GetNamedSecurityInfo(path, 1, query, out owner, out group, out dacl, out system, out descriptor);
      }
      if (result != 0) throw new Win32Exception((int)result,"query system-policy fixture mask="+query.ToString("x"));
      if (!GetSecurityDescriptorControl(descriptor, out control, out revision)) throw new Win32Exception();
      if (revision != 1) throw new Exception("Unexpected descriptor revision");
      bool exists, wasDefaulted; IntPtr acl;
      if (!GetSecurityDescriptorSacl(descriptor, out exists, out acl, out wasDefaulted)) throw new Win32Exception();
      string kind=!exists ? "absent" : acl == IntPtr.Zero ? "null" : "allocated";
      uint count=0;
      if (acl != IntPtr.Zero) {
        AclSize size;
        if (!GetAclInformation(acl, out size, (uint)Marshal.SizeOf(typeof(AclSize)), 2)) throw new Win32Exception();
        count=size.Count;
      }
      if (!ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor, 1, query, out text, out length)) throw new Win32Exception(Marshal.GetLastWin32Error(),"serialize system-policy fixture mask="+query.ToString("x"));
      return (control & 0x2a30).ToString("x") + "|" + kind + "|" + count + "|" + Marshal.PtrToStringUni(text);
    } finally {
      if (text != IntPtr.Zero) LocalFree(text);
      if (descriptor != IntPtr.Zero) LocalFree(descriptor);
      if (empty != IntPtr.Zero) Marshal.FreeHGlobal(empty);
    }
  }
  public static string EmptySystemPolicyControl(string path, bool retained, bool allocated, bool labels, bool writeOwner) {
    System.IO.File.WriteAllText(path,"disposable empty SACL setter control");
    string before=Policy(path,"S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)",24);
    IntPtr empty=IntPtr.Zero, handle=new IntPtr(-1);
    try {
      if (allocated) {
        empty=Marshal.AllocHGlobal(8);
        if (!InitializeAcl(empty,8,2)) throw new Win32Exception();
      }
      uint information=(labels ? 24u : 8u) | 0x10000000u;
      uint status;
      if (retained) {
        SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
        handle=CreateFile(path,0x01020080u | (writeOwner ? 0x00080000u : 0u),7,ref security,3,0x00200080,IntPtr.Zero);
        if (handle == new IntPtr(-1)) throw new Win32Exception();
        status=SetSecurityInfo(handle,1,information,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,empty);
      } else {
        status=SetNamedSecurityInfo(path,1,information,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,empty);
      }
      if (status != 0) throw new Win32Exception((int)status);
      return "before="+before+";after="+Policy(path,"",24,true);
    } finally {
      if (handle != new IntPtr(-1)) CloseHandle(handle);
      if (empty != IntPtr.Zero) Marshal.FreeHGlobal(empty);
    }
  }
  [StructLayout(LayoutKind.Sequential)] struct GenericMapping { public uint Read, Write, Execute, All; }
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool CreatePrivateObjectSecurityEx(IntPtr parent, IntPtr creator, out IntPtr result, IntPtr type, bool container, uint inheritance, IntPtr token, ref GenericMapping mapping);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool DestroyPrivateObjectSecurity(ref IntPtr descriptor);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool SetSecurityDescriptorControl(IntPtr descriptor, ushort interest, ushort value);
  [StructLayout(LayoutKind.Sequential)] struct FileDisposition { public byte Delete; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetFileInformationByHandle(IntPtr file, uint kind, ref FileDisposition information, uint size);
  [DllImport("kernel32.dll", EntryPoint="GetFileInformationByHandleEx", SetLastError=true)] static extern bool GetCaseInformation(IntPtr file, uint kind, out uint information, uint size);
  [DllImport("kernel32.dll", EntryPoint="SetFileInformationByHandle", SetLastError=true)] static extern bool SetCaseInformation(IntPtr file, uint kind, ref uint information, uint size);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern uint GetShortPathName(string path, System.Text.StringBuilder output, uint size);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern uint GetLongPathName(string path, System.Text.StringBuilder output, uint size);
  public static uint DirectoryCaseFlags(string path, uint? replacement) {
    SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
    IntPtr file=CreateFile(path,replacement.HasValue ? 0x180u : 0x80u,7,ref security,3,0x02200000,IntPtr.Zero);
    if (file == new IntPtr(-1)) throw new Win32Exception();
    try {
      uint flags;
      if (replacement.HasValue) {
        flags=replacement.Value;
        if (!SetCaseInformation(file,23,ref flags,4)) throw new Win32Exception();
      }
      if (!GetCaseInformation(file,23,out flags,4)) throw new Win32Exception();
      return flags;
    } finally { CloseHandle(file); }
  }
  public static string ExpandedName(string path, bool shortName) {
    uint length=shortName ? GetShortPathName(path,null,0) : GetLongPathName(path,null,0);
    if (length == 0) throw new Win32Exception();
    var output=new System.Text.StringBuilder(checked((int)length+1));
    uint result=shortName ? GetShortPathName(path,output,(uint)output.Capacity) : GetLongPathName(path,output,(uint)output.Capacity);
    if (result == 0 || result >= output.Capacity) throw new Win32Exception();
    return output.ToString();
  }
  static string NativeLookupControl(string path) {
    SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
    IntPtr file=CreateFile(path,0x80u,7,ref security,3,0x02200000,IntPtr.Zero);
    if (file == new IntPtr(-1)) return "open-error="+Marshal.GetLastWin32Error();
    if (!CloseHandle(file)) throw new Win32Exception();
    return "open=ok";
  }
  public static string CaseBehaviorControl(string root, bool sensitive) {
    System.IO.Directory.CreateDirectory(root);
    var rows=new System.Collections.Generic.List<string>();
    try {
      uint flags=DirectoryCaseFlags(root,sensitive ? 1u : 0u);
      if ((flags & 1) != (sensitive ? 1u : 0u)) throw new Exception("Native case flag did not retain requested state");
      string child=System.IO.Path.Combine(root,"child");System.IO.Directory.CreateDirectory(child);
      rows.Add("parent="+flags+";child="+DirectoryCaseFlags(child,null));
      foreach (string expected in new string[] { "config.toml", "projectatlas.db", "projectatlas.toml", "projectatlas.json", ".gitignore", ".projectatlas" }) {
        bool directory=expected == ".projectatlas";
        string original=System.IO.Path.Combine(root,expected);
        if (directory) System.IO.Directory.CreateDirectory(original); else System.IO.File.WriteAllText(original,"native alias fixture");
        var variants=new System.Collections.Generic.List<string> { expected.ToUpperInvariant(), expected.Replace("i","\u0131"), expected.Replace("s","\u017f"), expected+".", expected+" " };
        try {
          string shortName=System.IO.Path.GetFileName(ExpandedName(original,true));
          rows.Add("short|"+expected+"|"+shortName+"|"+(shortName != expected));
          variants.Add(shortName);
          variants.Add(shortName.ToLowerInvariant());
        } catch (Win32Exception error) {
          rows.Add("short-query-error|"+expected+"|"+error.NativeErrorCode);
        }
        if (expected == "config.toml") variants.Add("con\ufb01g.toml");
        foreach (string variant in variants) {
          if (variant == expected) continue;
          string candidate=System.IO.Path.Combine(root,variant);
          bool exists=directory ? System.IO.Directory.Exists(candidate) : System.IO.File.Exists(candidate);
          string expanded="absent";
          if (exists) {
            try { expanded=System.IO.Path.GetFileName(ExpandedName(candidate,false)); }
            catch (Win32Exception error) { expanded="long-query-error="+error.NativeErrorCode; }
          }
          rows.Add(expected+"|"+variant+"|"+exists+"|"+expanded);
          string verbatim=candidate.StartsWith(@"\\?\",StringComparison.Ordinal) ? candidate
            : candidate.StartsWith(@"\\",StringComparison.Ordinal) ? @"\\?\UNC\"+candidate.Substring(2) : @"\\?\"+candidate;
          rows.Add("native|"+expected+"|"+variant+"|win32="+NativeLookupControl(candidate)+"|verbatim="+NativeLookupControl(verbatim));
          if (candidate.Length > 2 && candidate[1] == ':')
            rows.Add("device|"+expected+"|"+variant+"|"+NativeLookupControl(@"\\.\"+candidate));
        }
      }
      return String.Join("\n",rows);
    } finally { System.IO.Directory.Delete(root,true); }
  }
  static string DescriptorImage(IntPtr descriptor, uint information, bool ignoreProtection, bool ignoreSystemInheritance=false) {
    ushort control; uint revision,length; IntPtr text=IntPtr.Zero;
    if (!GetSecurityDescriptorControl(descriptor,out control,out revision)) throw new Win32Exception();
    if (revision != 1) throw new Exception("Unexpected descriptor revision");
    try {
      if (!ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor,1,information,out text,out length)) throw new Win32Exception();
      string image=Marshal.PtrToStringUni(text);
      int start=image.IndexOf("D:",StringComparison.Ordinal);
      if (start >= 0) {
        start+=2;
        int end=image.IndexOf('(',start), system=image.IndexOf("S:",start,StringComparison.Ordinal);
        if (end < 0) end=image.Length;
        if (system >= 0 && system < end) end=system;
        string header=image.Substring(start,end-start).Replace("AI","");
        if (ignoreProtection) header=header.Replace("P","");
        image=image.Substring(0,start)+header+image.Substring(end);
      }
      // This diagnostic comparison may ignore only the SACL's inheritance
      // bookkeeping, never its ACEs, presence, NULL state or protection.
      if (ignoreSystemInheritance) {
        start=image.IndexOf("S:",StringComparison.Ordinal);
        if (start >= 0) {
          start+=2; int end=image.IndexOf('(',start);
          if (end < 0) end=image.Length;
          image=image.Substring(0,start)+image.Substring(start,end-start).Replace("AI","")+image.Substring(end);
        }
      }
      uint mask=0x3f3fu & ~0x400u & (ignoreProtection ? ~0x1000u : uint.MaxValue);
      if (ignoreSystemInheritance) mask &= ~0x800u;
      return (control & mask).ToString("x")+"|"+image;
    } finally { if (text != IntPtr.Zero) LocalFree(text); }
  }
  static IntPtr HeldDescriptor(IntPtr file, uint information) {
    IntPtr descriptor;
    uint result=GetSecurityDescriptorOnly(file,1,information,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out descriptor);
    if (result != 0) throw new Win32Exception((int)result);
    return descriptor;
  }
  static IntPtr NamedDescriptor(string path, uint information) {
    IntPtr owner,group,dacl,sacl,descriptor;
    uint result=GetNamedSecurityInfo(path,1,information,out owner,out group,out dacl,out sacl,out descriptor);
    if (result != 0) throw new Win32Exception((int)result);
    return descriptor;
  }
  static IntPtr FileDescriptor(string path, uint information) {
    SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
    uint access=0x20080u | ((information & 0x10008u) != 0 ? 0x01000000u : 0u);
    IntPtr file=CreateFile(path,access,7,ref security,3,0x00200080,IntPtr.Zero);
    if (file == new IntPtr(-1)) throw new Win32Exception();
    try { return HeldDescriptor(file,information); }
    finally { CloseHandle(file); }
  }
  public static string NativePolicy(string path, uint information) {
    IntPtr descriptor=FileDescriptor(path,information);
    try { return NativeSystemImage(descriptor); }
    finally { LocalFree(descriptor); }
  }
  public static string QueryMaskControl(string path, string policy) {
    System.IO.File.WriteAllText(path,"native mask fixture");
    if (!String.IsNullOrEmpty(policy)) Policy(path,policy,0x1f8u);
    var rows=new System.Collections.Generic.List<string>();
    SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
    IntPtr file=CreateFile(path,0x01020080u,7,ref security,3,0x00200080,IntPtr.Zero), full=IntPtr.Zero;
    if (file == new IntPtr(-1)) throw new Win32Exception();
    try {
      ObjectBasicInformation basic; uint returned;
      uint size=(uint)Marshal.SizeOf(typeof(ObjectBasicInformation));
      if (NtQueryObject(file,0,out basic,size,out returned) < 0 || returned != size) throw new Exception("Mask probe granted-access query failed");
      rows.Add("granted="+basic.GrantedAccess.ToString("x"));
      foreach (uint mask in new uint[] { 0x18u,0x78u,0x1f8u,0x10000u,0x10078u,0x1007fu,0x7fu }) {
        IntPtr descriptor=IntPtr.Zero,text=IntPtr.Zero; uint length;
        uint status=GetSecurityDescriptorOnly(file,1,mask,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out descriptor);
        try {
          if (status != 0) { rows.Add("query="+mask.ToString("x")+";status="+status);continue; }
          string value="query="+mask.ToString("x")+";status=0;native="+NativeSystemImage(descriptor);
          bool present,defaulted; IntPtr acl;
          if (!GetSecurityDescriptorSacl(descriptor,out present,out acl,out defaulted)) throw new Win32Exception();
          var types=new System.Collections.Generic.List<string>();
          if (present && acl != IntPtr.Zero) {
            AclSize aclSize;
            if (!GetAclInformation(acl,out aclSize,(uint)Marshal.SizeOf(typeof(AclSize)),2)) throw new Win32Exception();
            for (uint index=0;index<aclSize.Count;index++) {
              IntPtr ace;if (!GetAce(acl,index,out ace)) throw new Win32Exception();
              types.Add(Marshal.ReadByte(ace).ToString("x"));
            }
          }
          value+=";types="+String.Join(",",types);
          if (ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor,1,0x1f8u,out text,out length)) value+=";sddl="+Marshal.PtrToStringUni(text);
          else value+=";sddl-error="+Marshal.GetLastWin32Error();
          rows.Add(value);
          if (mask == 0x1f8u) { full=descriptor;descriptor=IntPtr.Zero; }
        } finally { if (text != IntPtr.Zero) LocalFree(text);if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
      }
      if (full != IntPtr.Zero) {
        bool present,defaulted;IntPtr acl;ushort control;uint revision;
        if (!GetSecurityDescriptorSacl(full,out present,out acl,out defaulted) || !GetSecurityDescriptorControl(full,out control,out revision)) throw new Win32Exception();
        if (present && acl != IntPtr.Zero) {
          foreach (uint mask in new uint[] { 0x78u,0x1f8u }) {
            string stage=path+"-setter-"+mask;System.IO.File.WriteAllText(stage,"empty setter control");
            IntPtr held=CreateFile(stage,0x010e0080u,7,ref security,3,0x00200080,IntPtr.Zero);
            if (held == new IntPtr(-1)) throw new Win32Exception();
            try {
              uint status=SetSecurityInfo(held,1,mask | ((control & 0x2000) != 0 ? 0x40000000u : 0x10000000u),IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,acl);
              string value="setter="+mask.ToString("x")+";status="+status;
              if (status == 0) {
                IntPtr observed=HeldDescriptor(held,0x1f8u);
                try { value+=";exact="+(NativeSystemImage(observed) == NativeSystemImage(full)); }
                finally { LocalFree(observed); }
              }
              rows.Add(value);
            } finally { CloseHandle(held);System.IO.File.Delete(stage); }
          }
        }
      }
      return String.Join("\n",rows);
    } finally { if (full != IntPtr.Zero) LocalFree(full);CloseHandle(file);System.IO.File.Delete(path); }
  }
  // Independent fixture oracle: compare the native opaque ACL bytes, including
  // resource-attribute payloads and scoped-policy SIDs, without an ACE parser.
  static string NativeSystemImage(IntPtr descriptor, bool ignoreAutoInherited=false) {
    ushort control; uint revision; bool present,defaulted; IntPtr acl;
    if (!GetSecurityDescriptorControl(descriptor,out control,out revision) || revision != 1) throw new Win32Exception();
    if (!GetSecurityDescriptorSacl(descriptor,out present,out acl,out defaulted)) throw new Win32Exception();
    string image=!present ? "absent" : acl == IntPtr.Zero ? "null" : "allocated";
    if (present && acl != IntPtr.Zero) {
      AclSize size;
      if (!GetAclInformation(acl,out size,(uint)Marshal.SizeOf(typeof(AclSize)),2)) throw new Win32Exception();
      byte[] bytes=new byte[checked((int)size.Used)];
      Marshal.Copy(acl,bytes,0,bytes.Length);
      // Both PowerShell 7 and Windows PowerShell/.NET Framework run this fixture.
      image+="|"+BitConverter.ToString(bytes).Replace("-","");
    }
    uint mask=ignoreAutoInherited ? 0x2230u : 0x2a30u;
    return (control & mask).ToString("x")+"|"+image;
  }
  static void RequireNativeAceClass(IntPtr descriptor, byte required) {
    bool present,defaulted; IntPtr acl;
    if (!GetSecurityDescriptorSacl(descriptor,out present,out acl,out defaulted)) throw new Win32Exception();
    if (!present || acl == IntPtr.Zero) throw new Exception("Extended fixture has no native SACL");
    AclSize size;
    if (!GetAclInformation(acl,out size,(uint)Marshal.SizeOf(typeof(AclSize)),2)) throw new Win32Exception();
    for (uint index=0; index<size.Count; index++) {
      IntPtr ace;
      if (!GetAce(acl,index,out ace)) throw new Win32Exception();
      // ACE_HEADER.AceType is the SDK-returned header's first byte. The
      // fixture inspects no variable ACE or claim layout.
      if (Marshal.ReadByte(ace) == required) return;
    }
    throw new Exception("Extended fixture did not install requested native ACE type="+required);
  }
  [StructLayout(LayoutKind.Sequential)] struct AceHeader { public byte Type, Flags; public ushort Size; }
  static string NativeAceContents(IntPtr descriptor) {
    bool present,defaulted; IntPtr acl;
    if (!GetSecurityDescriptorSacl(descriptor,out present,out acl,out defaulted)) throw new Win32Exception();
    if (!present || acl == IntPtr.Zero) return "";
    AclSize size;
    if (!GetAclInformation(acl,out size,(uint)Marshal.SizeOf(typeof(AclSize)),2)) throw new Win32Exception();
    var entries=new System.Collections.Generic.List<string>();
    for (uint index=0; index<size.Count; index++) {
      IntPtr ace;
      if (!GetAce(acl,index,out ace)) throw new Win32Exception();
      AceHeader header=(AceHeader)Marshal.PtrToStructure(ace,typeof(AceHeader));
      if (header.Size < Marshal.SizeOf(typeof(AceHeader)) || header.Size > size.Used) throw new Exception("Invalid native fixture ACE size");
      byte[] bytes=new byte[header.Size];Marshal.Copy(ace,bytes,0,bytes.Length);
      entries.Add(BitConverter.ToString(bytes));
    }
    // This comparison is only for initial fixture SET, which can canonicalize
    // cross-class order. Every later before/after image retains exact order.
    entries.Sort(StringComparer.Ordinal);return String.Join("|",entries);
  }
  static void VerifyExtendedPolicy(IntPtr descriptor, string path, uint information, string requested) {
    IntPtr complete=IntPtr.Zero,text=IntPtr.Zero,decoded=IntPtr.Zero,reference=IntPtr.Zero,filtered=IntPtr.Zero; uint length;
    try {
      complete=NamedDescriptor(path,0x1f8u);
      if (requested.Contains("(RA;")) RequireNativeAceClass(complete,0x12);
      if (requested.Contains("(SP;")) RequireNativeAceClass(complete,0x13);
      if (!ConvertStringSecurityDescriptorToSecurityDescriptor(requested,1,out reference,out length)) throw new Win32Exception();
      // Initial native SET may canonicalize order and set AI. Require each
      // exact requested ACE payload/SID, presence and other control first.
      string[] proposed=NativeSystemImage(reference,true).Split('|'), actual=NativeSystemImage(complete,true).Split('|');
      if (proposed[0] != actual[0] || proposed[1] != actual[1] || NativeAceContents(reference) != NativeAceContents(complete)) throw new Exception("Extended fixture native policy differs from requested payload");
      string expected=NativeSystemImage(complete);
      filtered=FileDescriptor(path,0x18u);
      if (NativeSystemImage(filtered) == expected) throw new Exception("Former retained SACL/LABEL query did not reproduce extended policy omission");
      if (NativeSystemImage(descriptor) != expected) throw new Exception("Complete retained query omitted native extended policy");
      if (!ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor,1,information,out text,out length)) throw new Win32Exception();
      if (!ConvertStringSecurityDescriptorToSecurityDescriptor(Marshal.PtrToStringUni(text),1,out decoded,out length)) throw new Win32Exception();
      if (NativeSystemImage(decoded) != expected) throw new Exception("Native extended policy changed through SDDL codec");
    } finally {
      if (decoded != IntPtr.Zero) LocalFree(decoded);
      if (reference != IntPtr.Zero) LocalFree(reference);
      if (filtered != IntPtr.Zero) LocalFree(filtered);
      if (text != IntPtr.Zero) LocalFree(text);
      if (complete != IntPtr.Zero) LocalFree(complete);
    }
  }
  [StructLayout(LayoutKind.Sequential)] struct SidAndAttributes { public IntPtr Sid; public uint Attributes; }
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool CreateRestrictedToken(IntPtr existing, uint flags, uint disabledCount, IntPtr disabled, uint deletedCount, IntPtr deleted, uint restrictedCount, [In] SidAndAttributes[] restricted, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool ImpersonateLoggedOnUser(IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool RevertToSelf();
  // Everyone must grant the second restricted-SID check, independently of the
  // normal administrator/owner check. No privilege or process token is changed.
  sealed class RestrictedActor : IDisposable {
    IntPtr token=IntPtr.Zero;
    public RestrictedActor() {
      IntPtr original=IntPtr.Zero,buffer=IntPtr.Zero;
      try {
        if (!OpenProcessToken(GetCurrentProcess(),0xeu,out original)) throw new Win32Exception();
        var sid=new System.Security.Principal.SecurityIdentifier("S-1-1-0");
        byte[] bytes=new byte[sid.BinaryLength];sid.GetBinaryForm(bytes,0);
        buffer=Marshal.AllocHGlobal(bytes.Length);Marshal.Copy(bytes,0,buffer,bytes.Length);
        var restricted=new SidAndAttributes[] { new SidAndAttributes { Sid=buffer, Attributes=0 } };
        if (!CreateRestrictedToken(original,1u,0,IntPtr.Zero,0,IntPtr.Zero,1,restricted,out token)) throw new Win32Exception();
      } finally { if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);if (original != IntPtr.Zero) CloseHandle(original); }
    }
    public T Run<T>(Func<T> operation) {
      if (!ImpersonateLoggedOnUser(token)) throw new Win32Exception();
      try { return operation(); }
      finally { if (!RevertToSelf()) throw new Win32Exception(); }
    }
    public Tuple<IntPtr,int> Open(string path,uint rights) {
      return Run(() => {
        SecurityAttributes attributes=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
        IntPtr file=CreateFile(path,rights,7,ref attributes,3,0x02200080,IntPtr.Zero);
        return Tuple.Create(file,file == new IntPtr(-1) ? Marshal.GetLastWin32Error() : 0);
      });
    }
    public Tuple<IntPtr,int> Create(string path) {
      return Run(() => {
        SecurityAttributes attributes=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
        IntPtr file=CreateFile(path,0x10002u,7,ref attributes,1,0x00200080,IntPtr.Zero);
        return Tuple.Create(file,file == new IntPtr(-1) ? Marshal.GetLastWin32Error() : 0);
      });
    }
    public void Denied(string path,uint rights) {
      var opened=Open(path,rights);
      if (opened.Item1 != new IntPtr(-1)) { CloseHandle(opened.Item1);throw new Exception("Restricted actor acquired forbidden rights="+rights.ToString("x")+" path="+path); }
      if (opened.Item2 != 5) throw new Win32Exception(opened.Item2,"Actor control did not establish DACL denial, rights="+rights.ToString("x"));
    }
    public IntPtr Allowed(string path,uint rights) {
      var opened=Open(path,rights);
      if (opened.Item1 == new IntPtr(-1)) throw new Win32Exception(opened.Item2,"Restricted actor positive open failed, rights="+rights.ToString("x"));
      return opened.Item1;
    }
    public void Dispose() { if (token != IntPtr.Zero) { CloseHandle(token);token=IntPtr.Zero; } }
  }
  static void ActorPremises(RestrictedActor actor,string parent,string target) {
    IntPtr directory=actor.Allowed(parent,2u);CloseHandle(directory); // FILE_ADD_FILE
    string path=System.IO.Path.Combine(parent,"actor-create-control-"+Guid.NewGuid());
    var created=actor.Create(path);
    if (created.Item1 == new IntPtr(-1)) throw new Win32Exception(created.Item2,"Restricted actor could not create an owned child");
    try {
      if (!actor.Run(() => { FileDisposition disposition=new FileDisposition { Delete=1 };return SetFileInformationByHandle(created.Item1,4,ref disposition,1); })) throw new Exception("Restricted actor could not clean its owned create control");
    } finally { CloseHandle(created.Item1); }
    if (System.IO.File.Exists(path)) throw new Exception("Restricted actor create control leaked a child");
    actor.Denied(parent,0x40u); // FILE_DELETE_CHILD
    actor.Denied(parent,0x40000u); // WRITE_DAC, including implicit owner rights
    foreach (uint rights in new uint[] { 2u,0x10000u,0x40000u }) actor.Denied(target,rights);
  }
  public static string ActorSetup(string parent,string target) {
    DiscretionaryPolicy(target,"D:P(D;;0x00050000;;;OW)(A;;FA;;;BA)(A;;FR;;;WD)");
    // Synchronous CreateFile reopens also request SYNCHRONIZE. Its omission
    // would deny the baseline before any policy tightening is exercised.
    DiscretionaryPolicy(parent,"D:P(D;;0x00040000;;;OW)(A;OICI;FA;;;BA)(A;;0x001200a3;;;WD)(A;OIIO;0x00170082;;;WD)");
    using (var actor=new RestrictedActor()) { ActorPremises(actor,parent,target); }
    return "restricted actor parent/target premises PASS";
  }
  public static string ActorStageProbe(string parent,string target,string stage) {
    using (var actor=new RestrictedActor()) {
      ActorPremises(actor,parent,target);
      actor.Denied(stage,0x10000u);actor.Denied(stage,0x40000u);
      // The actual publisher intentionally denies competing data opens by sharing.
      // A source-policy clone with all-share independently proves its DACL denial.
      IntPtr descriptor=FileDescriptor(stage,7u),raw=new IntPtr(-1);
      string control=stage+"-data-control";
      try {
        SecurityAttributes attributes=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=descriptor,Inherit=false };
        raw=CreateFile(control,0xc0030000u,7,ref attributes,1,0x00200080,IntPtr.Zero);
        if (raw == new IntPtr(-1)) throw new Win32Exception();
        actor.Denied(control,2u);
      } finally {
        if (raw != new IntPtr(-1)) {
          FileDisposition disposition=new FileDisposition { Delete=1 };
          if (!SetFileInformationByHandle(raw,4,ref disposition,1)) throw new Win32Exception();
          CloseHandle(raw);
        }
        LocalFree(descriptor);
      }
    }
    return "actual stage actor DELETE/WRITE_DAC and share-independent data denial PASS";
  }
  public static string ActorInheritedBaseline(string parent,string target) {
    string stage=System.IO.Path.Combine(parent,"inherited-actor-"+Guid.NewGuid()),published=stage+"-published",dataControl=stage+"-data";
    IntPtr raw=new IntPtr(-1),data=new IntPtr(-1),deletion=new IntPtr(-1),policy=new IntPtr(-1),queryPolicy=new IntPtr(-1),descriptor=IntPtr.Zero,changed=IntPtr.Zero;
    try {
      using (var actor=new RestrictedActor()) {
        ActorPremises(actor,parent,target);
        SecurityAttributes ordinary=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
        raw=CreateFile(stage,0xc2070000u,4,ref ordinary,1,0x00200080,IntPtr.Zero);
        if (raw == new IntPtr(-1)) throw new Win32Exception();
        IntPtr inherited=FileDescriptor(stage,4u);
        try { Console.WriteLine("actor baseline inherited DACL="+DescriptorImage(inherited,4u,false)); }
        finally { LocalFree(inherited); }
        deletion=actor.Allowed(stage,0x10080u);policy=actor.Allowed(stage,0x40000u);queryPolicy=actor.Allowed(stage,0x60080u);
        foreach (IntPtr retained in new IntPtr[] { policy,queryPolicy,deletion }) {
          ObjectBasicInformation basic;uint returned;uint size=(uint)Marshal.SizeOf(typeof(ObjectBasicInformation));
          int queryStatus=NtQueryObject(retained,0,out basic,size,out returned);
          if (queryStatus < 0 || returned != size) throw new Exception("Actor retained-rights query failed="+queryStatus.ToString("x"));
          Console.WriteLine("actor retained granted="+basic.GrantedAccess.ToString("x"));
        }
        data=CreateFile(dataControl,0xc0030000u,7,ref ordinary,1,0x00200080,IntPtr.Zero);
        if (data == new IntPtr(-1)) throw new Win32Exception();
        IntPtr writer=actor.Allowed(dataControl,2u);CloseHandle(writer);
        descriptor=FileDescriptor(target,7u);
        bool present,defaulted;IntPtr dacl;
        if (!GetSecurityDescriptorDacl(descriptor,out present,out dacl,out defaulted) || !present || dacl == IntPtr.Zero) throw new Exception("Actor source DACL missing");
        uint status=SetSecurityInfo(raw,1,0x80000004u,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero);
        if (status != 0) throw new Win32Exception((int)status,"Creator failed source-policy tightening");
        actor.Denied(stage,0x10000u);actor.Denied(stage,0x40000u);
        if (!MoveFileEx(stage,published,0)) throw new Win32Exception();
        status=actor.Run(() => SetSecurityInfo(policy,1,0x80000004u,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero));
        Console.WriteLine("actor retained WRITE_DAC-only Win32 setter status="+status);
        uint length;
        // Change the actual ACL while keeping the actor's fresh mutation opens
        // denied. Metadata access is already allowed on the protected target.
        string queryChange="D:P(D;;0x00050000;;;OW)(A;;FA;;;BA)(A;;FR;;;WD)(A;;0x80;;;LS)";
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor(queryChange,1,out changed,out length) || !GetSecurityDescriptorDacl(changed,out present,out dacl,out defaulted)) throw new Win32Exception();
        status=actor.Run(() => SetSecurityInfo(queryPolicy,1,0x80000004u,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero));
        Console.WriteLine("actor retained WRITE_DAC+query Win32 setter status="+status);
        if (status != 0) throw new Win32Exception((int)status,"Retained product-equivalent Win32 setter failed, access=60080");
        IntPtr win32Observed=FileDescriptor(published,4u);
        try {
          if (DescriptorImage(win32Observed,4u,false,true) != DescriptorImage(changed,4u,false,true) || DescriptorImage(win32Observed,4u,false,true) == DescriptorImage(descriptor,4u,false,true)) throw new Exception("Retained Win32 setter did not change the actual ACL");
        } finally { LocalFree(win32Observed); }
        LocalFree(changed);changed=IntPtr.Zero;
        string kernelChange="D:P(D;;0x00050000;;;OW)(A;;FA;;;BA)(A;;FR;;;WD)(A;;0x80;;;AN)";
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor(kernelChange,1,out changed,out length)) throw new Win32Exception();
        int native=actor.Run(() => NtSetSecurityObject(policy,4u,changed));
        Console.WriteLine("actor retained WRITE_DAC NtSetSecurityObject status="+native.ToString("x"));
        if (native < 0) throw new Exception("Previously acquired WRITE_DAC native setter failed="+native.ToString("x"));
        IntPtr observed=FileDescriptor(published,4u);
        try {
          if (DescriptorImage(observed,4u,false,true) != DescriptorImage(changed,4u,false,true) || DescriptorImage(observed,4u,false,true) == DescriptorImage(descriptor,4u,false,true)) throw new Exception("Retained WRITE_DAC did not change the actual published ACL");
        } finally { LocalFree(observed); }
        actor.Denied(published,0x10000u);actor.Denied(published,0x40000u);
        var removed=actor.Run(() => {
          FileDisposition disposition=new FileDisposition { Delete=1 };
          bool success=SetFileInformationByHandle(deletion,4,ref disposition,1);
          return Tuple.Create(success,success ? 0 : Marshal.GetLastWin32Error());
        });
        if (!removed.Item1) throw new Win32Exception(removed.Item2,"Previously acquired DELETE did not survive tightening/publication");
      }
    } finally {
      foreach (IntPtr retained in new IntPtr[] { policy,queryPolicy,deletion }) if (retained != new IntPtr(-1)) CloseHandle(retained);
      foreach (IntPtr file in new IntPtr[] { raw,data }) if (file != new IntPtr(-1)) {
        FileDisposition disposition=new FileDisposition { Delete=1 };
        SetFileInformationByHandle(file,4,ref disposition,1);CloseHandle(file);
      }
      if (descriptor != IntPtr.Zero) LocalFree(descriptor);
      if (changed != IntPtr.Zero) LocalFree(changed);
    }
    if (System.IO.File.Exists(stage) || System.IO.File.Exists(published) || System.IO.File.Exists(dataControl)) throw new Exception("Actor baseline leaked an owned file");
    return "former stage actor acquisition persists after tightening and publication PASS";
  }
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetSecurityDescriptorOwner(IntPtr descriptor,out IntPtr owner,out bool defaulted);
  public static string DefaultedFileControl(string root) {
    string seed=System.IO.Path.Combine(root,"defaulted-seed");
    IntPtr source=IntPtr.Zero;
    var rows=new System.Collections.Generic.List<string>();
    try {
      System.IO.File.WriteAllText(seed,"defaulted metadata fixture");
      source=FileDescriptor(seed,7u);
      byte[] original=new byte[GetSecurityDescriptorLength(source)];Marshal.Copy(source,original,0,original.Length);
      var initial=new System.Security.AccessControl.RawSecurityDescriptor(original,0);
      foreach (ushort defaults in new ushort[] { 0,1,2,3,8,9,10,11 }) {
        var flags=(initial.ControlFlags & ~(System.Security.AccessControl.ControlFlags)0xbu) | (System.Security.AccessControl.ControlFlags)defaults;
        var policy=new System.Security.AccessControl.RawSecurityDescriptor(flags,initial.Owner,initial.Group,initial.SystemAcl,initial.DiscretionaryAcl);
        byte[] bytes=new byte[policy.BinaryLength];policy.GetBinaryForm(bytes,0);
        IntPtr input=Marshal.AllocHGlobal(bytes.Length),held=new IntPtr(-1),observed=IntPtr.Zero;
        try {
          Marshal.Copy(bytes,0,input,bytes.Length);
          ushort inputControl,stored;uint revision;
          if (!GetSecurityDescriptorControl(input,out inputControl,out revision) || (inputControl & 0xbu) != defaults) throw new Exception("Defaulted fixture input was not encoded");
          SecurityAttributes attributes=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=input,Inherit=false };
          held=CreateFile(System.IO.Path.Combine(root,"defaulted-"+defaults),0xc0030000u,7,ref attributes,1,0x00200080u,IntPtr.Zero);
          if (held == new IntPtr(-1)) throw new Win32Exception();
          observed=HeldDescriptor(held,7u);
          if (!GetSecurityDescriptorControl(observed,out stored,out revision)) throw new Win32Exception();
          rows.Add("requested-default="+defaults.ToString("x")+";stored-default="+(stored & 0xbu).ToString("x"));
          if ((stored & 0xbu) != 0) throw new Exception("Physical file exposes defaulted metadata requiring preservation: "+stored.ToString("x"));
        } finally {
          if (observed != IntPtr.Zero) LocalFree(observed);
          if (held != new IntPtr(-1)) { FileDisposition disposition=new FileDisposition { Delete=1 };SetFileInformationByHandle(held,4,ref disposition,1);CloseHandle(held); }
          Marshal.FreeHGlobal(input);
        }
      }
      return String.Join("\n",rows);
    } finally { if (source != IntPtr.Zero) LocalFree(source);System.IO.File.Delete(seed); }
  }
  public static void ForeignOwner(string path) {
    DiscretionaryPolicy(path,"D:P(A;;FA;;;BA)");
    IntPtr descriptor=IntPtr.Zero;
    try {
      uint length;IntPtr owner;bool defaulted;
      if (!ConvertStringSecurityDescriptorToSecurityDescriptor("O:SY",1,out descriptor,out length) || !GetSecurityDescriptorOwner(descriptor,out owner,out defaulted)) throw new Win32Exception();
      uint status=SetNamedSecurityInfo(path,1,1u,owner,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
      if (status != 0) throw new Win32Exception((int)status,"Authorized foreign-owner fixture setup failed");
    } finally { if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
  }
  public static string ForeignOwnerControl(string root) {
    System.IO.Directory.CreateDirectory(root);
    string source=System.IO.Path.Combine(root,"foreign-owner-source");
    IntPtr sourceDescriptor=IntPtr.Zero,parentDescriptor=IntPtr.Zero,token=IntPtr.Zero;
    var results=new System.Collections.Generic.List<string>();
    EnablePrivilege("SeRestorePrivilege",true);
    try {
      System.IO.File.WriteAllText(source,"foreign owner original");
      ForeignOwner(source);
      IntPtr owner;bool defaulted;uint status;
      sourceDescriptor=FileDescriptor(source,7u);parentDescriptor=NamedDescriptor(root,7u);
      if (!OpenProcessToken(GetCurrentProcess(),0xau,out token)) throw new Win32Exception();
      GenericMapping mapping=new GenericMapping { Read=0x120089u,Write=0x120116u,Execute=0x1200a0u,All=0x1f01ffu };
      foreach (uint flags in new uint[] { 3u,19u }) {
        IntPtr preview=IntPtr.Zero;
        try {
          bool success=CreatePrivateObjectSecurityEx(parentDescriptor,sourceDescriptor,out preview,IntPtr.Zero,false,flags,token,ref mapping);
          results.Add("owner-preview-flags="+flags+";status="+(success ? 0 : Marshal.GetLastWin32Error())+(success ? ";image="+DescriptorImage(preview,7u,false) : ""));
        } finally { if (preview != IntPtr.Zero) DestroyPrivateObjectSecurity(ref preview); }
      }
      foreach (bool supplied in new bool[] { false,true }) foreach (bool backup in new bool[] { false,true }) {
        string stage=System.IO.Path.Combine(root,"foreign-owner-stage-"+supplied+"-"+backup);
        SecurityAttributes attributes=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)),Descriptor=supplied ? sourceDescriptor : IntPtr.Zero,Inherit=false };
        IntPtr held=CreateFile(stage,0xc20f0000u,7,ref attributes,1,0x00200080u | (backup ? 0x02000000u : 0u),IntPtr.Zero);
        if (held == new IntPtr(-1)) { results.Add("owner-create="+supplied+";backup="+backup+";error="+Marshal.GetLastWin32Error());continue; }
        try {
          if (!supplied) {
            bool present;IntPtr dacl;
            if (!GetSecurityDescriptorDacl(sourceDescriptor,out present,out dacl,out defaulted) || !GetSecurityDescriptorOwner(sourceDescriptor,out owner,out defaulted)) throw new Win32Exception();
            status=SetSecurityInfo(held,1,0x80000005u,owner,IntPtr.Zero,dacl,IntPtr.Zero);
            if (status != 0) throw new Win32Exception((int)status,"Former authorized ownership setter failed");
          }
          IntPtr observed=HeldDescriptor(held,7u);
          try { results.Add("owner-create="+supplied+";backup="+backup+";exact="+(DescriptorImage(observed,7u,false)==DescriptorImage(sourceDescriptor,7u,false))); }
          finally { LocalFree(observed); }
        } finally {
          FileDisposition disposition=new FileDisposition { Delete=1 };
          if (!SetFileInformationByHandle(held,4,ref disposition,1)) Console.WriteLine("Foreign owner control cleanup failed="+Marshal.GetLastWin32Error());
          CloseHandle(held);
        }
      }
      return String.Join("\n",results);
    } finally {
      if (token != IntPtr.Zero) CloseHandle(token);
      foreach (IntPtr descriptor in new IntPtr[] { sourceDescriptor,parentDescriptor }) if (descriptor != IntPtr.Zero) LocalFree(descriptor);
      EnablePrivilege("SeRestorePrivilege",false);
      System.IO.Directory.Delete(root,true);
    }
  }
  public static string SourceDescriptorControl(string root, bool protectedSource, bool widerParent, bool movedSource, bool systemAccess, string systemPolicy) {
    return SourceDescriptorControl(root,protectedSource,widerParent,movedSource,systemAccess,systemPolicy,"");
  }
  public static string SourceDescriptorControl(string root, bool protectedSource, bool widerParent, bool movedSource, bool systemAccess, string systemPolicy, string sourceDacl) {
    return SourceDescriptorControl(root,protectedSource,widerParent,movedSource,systemAccess,systemPolicy,sourceDacl,true,0x78u);
  }
  public static string SourceDescriptorControl(string root, bool protectedSource, bool widerParent, bool movedSource, bool systemAccess, string systemPolicy, string sourceDacl, bool maximumAllowed, uint systemInformation) {
    string preparation=System.IO.Path.Combine(root,"preparation"), parent=System.IO.Path.Combine(root,"publication");
    System.IO.Directory.CreateDirectory(preparation); System.IO.Directory.CreateDirectory(parent);
    string source=System.IO.Path.Combine(movedSource ? preparation : parent,"source"), destination=System.IO.Path.Combine(parent,"output"), stage=System.IO.Path.Combine(parent,"stage");
    string phase="prepare source", observations="";
    IntPtr sourceDescriptor=IntPtr.Zero,parentDescriptor=IntPtr.Zero,preview=IntPtr.Zero,stageDescriptor=IntPtr.Zero,token=IntPtr.Zero,raw=new IntPtr(-1);
    bool published=false;
    try {
      DiscretionaryPolicy(preparation,"D:P(A;;FA;;;OW)");
      DiscretionaryPolicy(parent,"D:P(A;;FA;;;OW)"+(widerParent ? "(A;OIIO;0x00050000;;;WD)" : ""));
      System.IO.File.WriteAllText(source,"previous destination");
      // Set the fixture SACL while the setup handle still has its initial
      // owner rights; a later restrictive DACL must not mask constructor proof.
      DiscretionaryPolicy(source,"D:P(A;;FA;;;OW)");
      if (!String.IsNullOrEmpty(systemPolicy)) Policy(source,systemPolicy,24);
      DiscretionaryPolicy(source,String.IsNullOrEmpty(sourceDacl) ? (protectedSource ? "D:P(A;;FA;;;OW)" : "D:(A;;FA;;;OW)") : sourceDacl);
      if (!MoveFileEx(source,destination,0)) throw new Win32Exception();
      uint information=7u | (systemAccess ? 0x1f8u : 16u);
      sourceDescriptor=FileDescriptor(destination,information);
      bool extended=systemPolicy.Contains("(RA;") || systemPolicy.Contains("(SP;");
      if (extended) {
        phase="verify native extended-policy query and codec";
        VerifyExtendedPolicy(sourceDescriptor,destination,information,systemPolicy);
      }
      parentDescriptor=NamedDescriptor(parent,information);
      ushort sourceControl; uint revision;
      if (!GetSecurityDescriptorControl(sourceDescriptor,out sourceControl,out revision)) throw new Win32Exception();
      if (!systemAccess && (sourceControl & 0x10) != 0) return "refused hidden source SACL before creation";
      string expected=DescriptorImage(sourceDescriptor,information,false), initialExpected=DescriptorImage(sourceDescriptor,information,true,true);
      observations="moved-source="+movedSource+";source="+expected;
      phase="preview native inheritance";
      if (!OpenProcessToken(GetCurrentProcess(),0xau,out token)) throw new Win32Exception();
      GenericMapping mapping=new GenericMapping { Read=0x120089u, Write=0x120116u, Execute=0x1200a0u, All=0x1f01ffu };
      if (!CreatePrivateObjectSecurityEx(parentDescriptor,sourceDescriptor,out preview,IntPtr.Zero,false,3u,token,ref mapping)) throw new Win32Exception();
      string predicted=DescriptorImage(preview,information,false);
      observations+=";preview="+predicted;
      if (DescriptorImage(preview,information,false,true) != DescriptorImage(sourceDescriptor,information,false,true)) return "refused differing inheritance before creation;"+observations;
      // Protection at creation prevents parent ACEs from granting a capability
      // before readback. Restore it only after the independent preview matches.
      phase="protect creation descriptor";
      if (!SetSecurityDescriptorControl(sourceDescriptor,0x1000,0x1000)) throw new Win32Exception();
      SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)), Descriptor=sourceDescriptor, Inherit=false };
      phase="create original data/security/delete handle";
      if (!maximumAllowed && systemInformation == 8u) {
        foreach (uint requested in new uint[] { 0x40000u,0x80000u,0x10000u,2u,0x20000u,0xc0070000u,0xc00f0000u }) {
          string probe=stage+"-exact-"+requested;
          IntPtr held=CreateFile(probe,requested | (systemAccess ? 0x01000000u : 0u),4,ref security,1,0x00200080,IntPtr.Zero);
          if (held == new IntPtr(-1)) { observations+=";exact-mask="+requested.ToString("x")+":error="+Marshal.GetLastWin32Error();continue; }
          try {
            ObjectBasicInformation rights;uint resultSize;
            uint nativeSize=(uint)Marshal.SizeOf(typeof(ObjectBasicInformation));
            if (NtQueryObject(held,0,out rights,nativeSize,out resultSize) < 0 || resultSize != nativeSize) throw new Exception("Exact creator granted-access query failed");
            observations+=";exact-mask="+requested.ToString("x")+":granted="+rights.GrantedAccess.ToString("x");
          } finally { CloseHandle(held);System.IO.File.Delete(probe); }
        }
      }
      uint desired=0xc0030000u | (!protectedSource ? 0x40000u : 0u)
        | (maximumAllowed ? 0x02000000u : 0u)
        | (systemAccess ? 0x01000000u : 0u)
        | (!maximumAllowed && systemAccess && (systemInformation & 0x10) != 0 ? 0x80000u : 0u);
      observations+=";requested="+desired.ToString("x")+";system-set="+systemInformation.ToString("x");
      raw=CreateFile(stage,desired,4,ref security,1,0x00200080,IntPtr.Zero);
      if (raw == new IntPtr(-1)) throw new Win32Exception();
      ObjectBasicInformation basic; uint returned;
      uint size=(uint)Marshal.SizeOf(typeof(ObjectBasicInformation));
      if (NtQueryObject(raw,0,out basic,size,out returned) < 0 || returned != size) throw new Exception("Creator granted-access query failed");
      observations+=";granted="+basic.GrantedAccess.ToString("x");
      uint required=0x00030003u | (!protectedSource ? 0x40000u : 0u);
      if ((basic.GrantedAccess & required) != required) throw new Exception("Creator lacks actually required data/security/delete rights");
      stageDescriptor=HeldDescriptor(raw,information);
      ushort initialControl; uint initialRevision;
      if (!GetSecurityDescriptorControl(stageDescriptor,out initialControl,out initialRevision) || initialRevision != 1) throw new Win32Exception();
      if ((initialControl & 0x1000) == 0) throw new Exception("Creation did not retain protected DACL");
      string initial=DescriptorImage(stageDescriptor,information,true,true);
      observations+=";initial="+DescriptorImage(stageDescriptor,information,false);
      if (initial != initialExpected) return "refused initial policy before payload;"+observations;
      LocalFree(stageDescriptor);stageDescriptor=IntPtr.Zero;
      if (!protectedSource) {
        bool present,defaulted; IntPtr acl;
        if (!GetSecurityDescriptorDacl(sourceDescriptor,out present,out acl,out defaulted)) throw new Win32Exception();
        phase="restore previewed inheritance through original handle";
        uint result=SetSecurityInfo(raw,1,0x20000004u,IntPtr.Zero,IntPtr.Zero,acl,IntPtr.Zero);
        if (result != 0) throw new Win32Exception((int)result);
      }
      if (systemAccess && (sourceControl & 0x10) != 0) {
        bool present,defaulted; IntPtr acl,empty=IntPtr.Zero;
        if (!GetSecurityDescriptorSacl(sourceDescriptor,out present,out acl,out defaulted) || !present) throw new Win32Exception();
        try {
          if (acl == IntPtr.Zero && (sourceControl & 0x2000) == 0) {
            empty=Marshal.AllocHGlobal(8);
            if (!InitializeAcl(empty,8,2)) throw new Win32Exception();
            acl=empty;
          }
          phase="restore exact system inheritance through original handle";
          uint result=SetSecurityInfo(raw,1,systemInformation | ((sourceControl & 0x2000) != 0 ? 0x40000000u : 0x10000000u),IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,acl);
          if (result != 0) throw new Win32Exception((int)result);
        } finally { if (empty != IntPtr.Zero) Marshal.FreeHGlobal(empty); }
      }
      stageDescriptor=HeldDescriptor(raw,information);
      string restored=DescriptorImage(stageDescriptor,information,false);
      observations+=";restored="+restored;
      if (restored != expected) return "refused final policy before payload;"+observations;
      if (extended && NativeSystemImage(stageDescriptor) != NativeSystemImage(sourceDescriptor)) throw new Exception("Extended policy setter changed opaque native ACL bytes");
      LocalFree(stageDescriptor);stageDescriptor=IntPtr.Zero;
      phase="creator payload and publication";
      using (var borrowed=new Microsoft.Win32.SafeHandles.SafeFileHandle(raw,false))
      using (var stream=new System.IO.FileStream(borrowed,System.IO.FileAccess.ReadWrite)) {
        byte[] payload=System.Text.Encoding.ASCII.GetBytes("verified source-descriptor payload");
        stream.Write(payload,0,payload.Length);stream.Flush(true);
      }
      if (!MoveFileEx(stage,destination,1)) throw new Win32Exception();
      published=true;
      // File.ReadAllText does not share a writer. Release our successful
      // publication handle before independently opening the resulting name.
      CloseHandle(raw);raw=new IntPtr(-1);
      if (System.IO.File.ReadAllText(destination) != "verified source-descriptor payload") throw new Exception("Published payload differs");
      if (extended) {
        stageDescriptor=NamedDescriptor(destination,0x1f8u);
        if (NativeSystemImage(stageDescriptor) != NativeSystemImage(sourceDescriptor)) throw new Exception("Publication changed opaque native extended policy");
      }
      return "source descriptor creation/publication PASS;"+observations;
    } catch (Exception error) { throw new Exception("Source descriptor phase: "+phase+";"+observations,error); }
    finally {
      if (raw != new IntPtr(-1)) {
        if (!published) {
          FileDisposition disposition=new FileDisposition { Delete=1 };
          if (!SetFileInformationByHandle(raw,4,ref disposition,1)) Console.WriteLine("original-handle cleanup failed native="+Marshal.GetLastWin32Error());
        }
        CloseHandle(raw);
      }
      if (stageDescriptor != IntPtr.Zero) LocalFree(stageDescriptor);
      if (preview != IntPtr.Zero && !DestroyPrivateObjectSecurity(ref preview)) Console.WriteLine("native descriptor cleanup failed="+Marshal.GetLastWin32Error());
      if (parentDescriptor != IntPtr.Zero) LocalFree(parentDescriptor);
      if (sourceDescriptor != IntPtr.Zero) LocalFree(sourceDescriptor);
      if (token != IntPtr.Zero) CloseHandle(token);
      DiscretionaryPolicy(parent,"D:P(A;;FA;;;OW)");
      if (System.IO.Directory.Exists(root)) System.IO.Directory.Delete(root,true);
    }
  }
  public static string CreatorControl(string path, bool systemAccess, bool maximumAllowed) {
    string directory=path+"-parent";
    System.IO.Directory.CreateDirectory(directory);
    try {
      DiscretionaryPolicy(directory,"D:P(A;;0x001f01bf;;;OW)");
      return CreatorFileControl(System.IO.Path.Combine(directory,"stage"),systemAccess,maximumAllowed);
    } finally {
      DiscretionaryPolicy(directory,"D:P(A;;FA;;;OW)");
      foreach (string file in System.IO.Directory.GetFiles(directory)) {
        if (!DeleteFile(file)) throw new Win32Exception(Marshal.GetLastWin32Error(),"Delete disposable native creator fixture");
      }
      System.IO.Directory.Delete(directory);
    }
  }
  public static void DiscretionaryPolicy(string path, string policy) {
    IntPtr descriptor=IntPtr.Zero;
    try {
      uint length;
      if (!ConvertStringSecurityDescriptorToSecurityDescriptor(policy,1,out descriptor,out length)) throw new Win32Exception();
      IntPtr dacl; bool present,defaulted;
      if (!GetSecurityDescriptorDacl(descriptor,out present,out dacl,out defaulted)) throw new Win32Exception();
      ushort control; uint revision;
      if (!GetSecurityDescriptorControl(descriptor,out control,out revision)) throw new Win32Exception();
      if (revision != 1) throw new Exception("Unexpected discretionary descriptor revision");
      uint information=4u | ((control & 0x1000) != 0 ? 0x80000000u : 0x20000000u);
      uint status=SetNamedSecurityInfo(path,1,information,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero);
      if (status != 0) throw new Win32Exception((int)status);
    } finally { if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
  }
  static string CreatorFileControl(string path, bool systemAccess, bool maximumAllowed) {
    string phase="convert creation descriptor";
    try { return CreatorFileControlCore(path,systemAccess,maximumAllowed,ref phase); }
    catch (Exception error) {
      var native=error as Win32Exception;
      if (native != null) Console.WriteLine("private creator failure native="+native.NativeErrorCode);
      throw new Exception("Private creator phase: "+phase,error);
    }
  }
  static string CreatorFileControlCore(string path, bool systemAccess, bool maximumAllowed, ref string phase) {
    IntPtr descriptor=IntPtr.Zero;
    uint length;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptor("D:P(A;;0x80;;;OW)", 1, out descriptor, out length)) throw new Win32Exception();
    IntPtr raw;
    try {
      SecurityAttributes security=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)), Descriptor=descriptor, Inherit=false };
      uint access=0xc0070000u | (maximumAllowed ? 0x02000000u : 0u) | (systemAccess ? 0x01000000u : 0u);
      phase="create original data/security/delete handle";
      // Share all rights so second-open denial measures the DACL, not sharing.
      raw=CreateFile(path, access, 7, ref security, 1, 0x00200080, IntPtr.Zero);
      if (raw == new IntPtr(-1)) throw new Win32Exception();
    } finally { LocalFree(descriptor); descriptor=IntPtr.Zero; }
    string destination=path+"-published";
    try { using (var handle=new Microsoft.Win32.SafeHandles.SafeFileHandle(raw, true)) {
      phase="read actual creator granted access";
      ObjectBasicInformation basic; uint returned;
      uint size=(uint)Marshal.SizeOf(typeof(ObjectBasicInformation));
      int query=NtQueryObject(raw,0,out basic,size,out returned);
      if (query < 0 || returned != size) throw new Exception("Creator access query failed: "+query.ToString("x"));
      Console.WriteLine("private creator granted access="+basic.GrantedAccess.ToString("x"));
      using (var stream=new System.IO.FileStream(handle, System.IO.FileAccess.ReadWrite)) {
        IntPtr owner,group,dacl,sacl;
        phase="query initial creator descriptor";
        uint status=GetSecurityInfo(raw,1,7,out owner,out group,out dacl,out sacl,out descriptor);
        if (status != 0) throw new Win32Exception((int)status);
        byte[] encoded=new byte[GetSecurityDescriptorLength(descriptor)];
        Marshal.Copy(descriptor,encoded,0,encoded.Length);
        var policy=new System.Security.AccessControl.RawSecurityDescriptor(encoded,0);
        var acl=policy.DiscretionaryAcl;
        var ace=acl != null && acl.Count == 1 ? acl[0] as System.Security.AccessControl.CommonAce : null;
        if ((policy.ControlFlags & System.Security.AccessControl.ControlFlags.DiscretionaryAclPresent) == 0 ||
            (policy.ControlFlags & System.Security.AccessControl.ControlFlags.DiscretionaryAclProtected) == 0 ||
            ace == null || ace.AccessMask != 0x80 || ace.SecurityIdentifier.Value != "S-1-3-4" ||
            ace.AceFlags != System.Security.AccessControl.AceFlags.None ||
            ace.AceQualifier != System.Security.AccessControl.AceQualifier.AccessAllowed)
          throw new Exception("Initial private creation DACL is not the intended protected Owner Rights policy");
        LocalFree(descriptor); descriptor=IntPtr.Zero;
        foreach (uint requested in new uint[] { 0x10000, 0x40000, 2 }) {
          phase="deny second opener for access "+requested;
          SecurityAttributes ordinary=new SecurityAttributes { Length=(uint)Marshal.SizeOf(typeof(SecurityAttributes)) };
          IntPtr second=CreateFile(path,requested,7,ref ordinary,3,0x00200080,IntPtr.Zero);
          if (second != new IntPtr(-1)) {
            CloseHandle(second);
            throw new Exception("Initial private creation DACL granted forbidden second-open access: "+requested);
          }
          int denied=Marshal.GetLastWin32Error();
          if (denied != 5) throw new Win32Exception(denied,"Second-open control did not establish access denial");
        }
        phase="creator payload write/flush/read";
        byte[] payload=System.Text.Encoding.ASCII.GetBytes("verified creator payload");
        stream.Write(payload,0,payload.Length); stream.Flush(true); stream.Position=0;
        byte[] read=new byte[payload.Length];
        if (stream.Read(read,0,read.Length) != payload.Length || System.Text.Encoding.ASCII.GetString(read) != "verified creator payload") throw new Exception("Creator data rights failed");
        phase="restore parent delete-child for publication control";
        DiscretionaryPolicy(System.IO.Path.GetDirectoryName(path),"D:P(A;;FA;;;OW)");
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor("D:P(A;;0x001e01ff;;;OW)",1,out descriptor,out length)) throw new Win32Exception();
        bool present,defaulted;
        if (!GetSecurityDescriptorDacl(descriptor,out present,out dacl,out defaulted)) throw new Win32Exception();
        phase="apply creator DACL through retained handle";
        status=SetSecurityInfo(raw,1,0x80000004u,IntPtr.Zero,IntPtr.Zero,dacl,IntPtr.Zero);
        if (status != 0) throw new Win32Exception((int)status);
        LocalFree(descriptor); descriptor=IntPtr.Zero;
        if (systemAccess) {
          phase="apply/query creator SACL through retained handle";
          if (!ConvertStringSecurityDescriptorToSecurityDescriptor("S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)",1,out descriptor,out length)) throw new Win32Exception();
          if (!GetSecurityDescriptorSacl(descriptor,out present,out sacl,out defaulted)) throw new Win32Exception();
          status=SetSecurityInfo(raw,1,0x40000018u,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,sacl);
          if (status != 0) throw new Win32Exception((int)status);
          LocalFree(descriptor); descriptor=IntPtr.Zero;
          status=GetSecurityInfo(raw,1,24,out owner,out group,out dacl,out sacl,out descriptor);
          if (status != 0) throw new Win32Exception((int)status);
          LocalFree(descriptor); descriptor=IntPtr.Zero;
        }
        phase="publish through MoveFileEx";
        System.IO.File.WriteAllText(destination,"previous destination");
        if (!MoveFileEx(path,destination,1)) throw new Win32Exception();
      }
    } } finally { if (descriptor != IntPtr.Zero) LocalFree(descriptor); }
    if (System.IO.File.ReadAllText(destination) != "verified creator payload") throw new Exception("Creator publication failed");
    return systemAccess ? "private creator data/DACL/SACL/publication PASS" : "private creator data/DACL/publication PASS";
  }
}
'@
$mode=$env:PROJECTATLAS_TEST_SYSTEM_POLICY_MODE
if ($mode -eq 'case-set') {
  [SystemPolicyFixture]::DirectoryCaseFlags($env:PROJECTATLAS_TEST_ACL_PATH,[uint32]$env:PROJECTATLAS_TEST_SYSTEM_POLICY_QUERY)
} elseif ($mode -eq 'short-name') {
  [SystemPolicyFixture]::ExpandedName($env:PROJECTATLAS_TEST_ACL_PATH,$true)
} elseif ($mode -eq 'query') {
  [SystemPolicyFixture]::Enable($true)
  [SystemPolicyFixture]::Policy($env:PROJECTATLAS_TEST_ACL_PATH,$env:PROJECTATLAS_TEST_SYSTEM_POLICY,[uint32]$env:PROJECTATLAS_TEST_SYSTEM_POLICY_QUERY)
} elseif ($mode -eq 'native-query') {
  [SystemPolicyFixture]::Enable($true)
  [SystemPolicyFixture]::NativePolicy($env:PROJECTATLAS_TEST_ACL_PATH,[uint32]$env:PROJECTATLAS_TEST_SYSTEM_POLICY_QUERY)
} elseif ($mode -eq 'creator') {
  [SystemPolicyFixture]::Enable($false)
  [SystemPolicyFixture]::CreatorControl($env:PROJECTATLAS_TEST_ACL_PATH,$false,$false)
  [SystemPolicyFixture]::Enable($true)
  [SystemPolicyFixture]::CreatorControl(($env:PROJECTATLAS_TEST_ACL_PATH+'-system'),$true,$false)
} elseif ($mode -eq 'efs-complete' -or $mode -eq 'efs-unprivileged') {
  [SystemPolicyFixture]::Enable($mode -eq 'efs-complete')
  & $env:PROJECTATLAS_TEST_SYSTEM_POLICY_EXE --exact project_state::tests::windows_encrypted_outputs_are_preserved_or_refused_before_publication --include-ignored --nocapture
  exit $LASTEXITCODE
} else {
  [SystemPolicyFixture]::Enable($mode -eq 'complete')
  & $env:PROJECTATLAS_TEST_SYSTEM_POLICY_EXE --exact project_state::tests::windows_system_policy_replacement_preserves_or_refuses_without_global_privilege_changes --nocapture
  exit $LASTEXITCODE
}
"#;

/// Configure native namespace fixtures through the independent SDK oracle.
#[cfg(windows)]
fn windows_namespace_fixture(path: &Path, mode: &str, sensitive: bool) -> io::Result<String> {
    let result = windows_powershell_fixture(WINDOWS_SYSTEM_POLICY_FIXTURE, |command| {
        command
            .env("PROJECTATLAS_TEST_SYSTEM_POLICY_MODE", mode)
            .env("PROJECTATLAS_TEST_ACL_PATH", path)
            .env(
                "PROJECTATLAS_TEST_SYSTEM_POLICY_QUERY",
                if sensitive { "1" } else { "0" },
            );
    })?;
    require(
        result.status.success(),
        &format!("native namespace fixture failed: {result:?}"),
    )?;
    Ok(String::from_utf8(result.stdout)
        .map_err(io::Error::other)?
        .trim()
        .to_owned())
}

/// Run one independent restricted-actor control in its owned child process.
#[cfg(windows)]
fn windows_actor_fixture(
    parent: &Path,
    target: &Path,
    stage: &Path,
    script: &str,
) -> io::Result<String> {
    let script = windows_fixture_script(script)?;
    let result = windows_powershell_fixture(&script, |command| {
        command
            .env("PROJECTATLAS_TEST_ACTOR_PARENT", parent)
            .env("PROJECTATLAS_TEST_ACTOR_TARGET", target)
            .env("PROJECTATLAS_TEST_ACTOR_STAGE", stage);
    })?;
    require(
        result.status.success(),
        &format!("restricted actor fixture failed: {result:?}"),
    )?;
    String::from_utf8(result.stdout).map_err(io::Error::other)
}

/// The actual protected constructor prevents capabilities that tightening cannot revoke.
#[cfg(windows)]
#[test]
fn windows_replacement_creation_denies_inherited_actor_capabilities() -> io::Result<()> {
    const SETUP: &str = "[SystemPolicyFixture]::ActorSetup($env:PROJECTATLAS_TEST_ACTOR_PARENT,$env:PROJECTATLAS_TEST_ACTOR_TARGET)";
    const BASELINE: &str = "[SystemPolicyFixture]::ActorInheritedBaseline($env:PROJECTATLAS_TEST_ACTOR_PARENT,$env:PROJECTATLAS_TEST_ACTOR_TARGET)";
    const PROBE: &str = "[SystemPolicyFixture]::ActorStageProbe($env:PROJECTATLAS_TEST_ACTOR_PARENT,$env:PROJECTATLAS_TEST_ACTOR_TARGET,$env:PROJECTATLAS_TEST_ACTOR_STAGE)";
    for (private, name, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let parent = if private {
            root.join(STATE_DIRECTORY)
        } else {
            root.clone()
        };
        fs::create_dir_all(&parent)?;
        let target = parent.join(name);
        let outside = root.join("actor-outside-owner");
        fs::write(&target, b"protected previous output")?;
        fs::hard_link(&target, &outside)?;
        windows_actor_fixture(&parent, &target, &target, SETUP)?;
        let expected = windows_acl(&target, None)?;
        require(
            windows_actor_fixture(&parent, &target, &target, BASELINE)?
                .contains("publication PASS"),
            "former inherited stage did not reproduce retained actor capabilities",
        )?;
        let guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &target)?
                .ok_or_else(|| io::Error::other("actor target was external"))?
        };
        let entries = fs::read_dir(&parent)?.count();
        let observed = std::cell::Cell::new(false);
        guard.publish_with_stage_check(
            &target,
            b"protected published output",
            FilePublication::Replace,
            |stage| {
                let proof = windows_actor_fixture(&parent, &target, stage.path(), PROBE)?;
                require(
                    proof.contains("data denial PASS"),
                    "actual stage did not deny actor capabilities",
                )?;
                observed.set(true);
                Ok(())
            },
            || Ok(()),
        )?;
        require(
            observed.get()
                && fs::read(&target)? == b"protected published output"
                && fs::read(&outside)? == b"protected previous output"
                && windows_file_acl_matches(&expected, &windows_acl(&target, None)?)?
                && windows_acl(&outside, None)? == expected
                && !same_file::is_same_file(&target, &outside)?
                && fs::read_dir(&parent)?.count() == entries,
            "protected creation lost payload, policy, outside owner or cleanup",
        )?;
        let failed = guard.publish_with_stage_check(
            &target,
            b"unpublished payload",
            FilePublication::Replace,
            |stage| {
                windows_actor_fixture(&parent, &target, stage.path(), PROBE)?;
                Ok(())
            },
            || Err(io::Error::other("injected actor publication failure")),
        );
        require(
            failed.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains("injected actor publication failure")
            }) && fs::read(&target)? == b"protected published output"
                && fs::read(&outside)? == b"protected previous output"
                && fs::read_dir(&parent)?.count() == entries,
            &format!("protected stage failed original-handle cleanup: {failed:?}"),
        )?;
    }
    Ok(())
}

/// Native creation-default metadata must not become an untested file limitation.
#[cfg(windows)]
#[test]
fn windows_file_snapshots_observe_defaulted_creation_metadata() -> io::Result<()> {
    use std::io::Write;
    let script = windows_fixture_script(
        "[SystemPolicyFixture]::DefaultedFileControl($env:PROJECTATLAS_TEST_ACL_PATH)",
    )?;
    let mut volumes = vec![std::env::temp_dir()];
    if let Some(runner) = std::env::var_os("RUNNER_TEMP") {
        let runner = std::path::PathBuf::from(runner);
        if !volumes.contains(&runner) {
            volumes.push(runner);
        }
    }
    for volume in volumes {
        let temp = tempfile::tempdir_in(volume)?;
        let result = windows_powershell_fixture(&script, |command| {
            command.env("PROJECTATLAS_TEST_ACL_PATH", temp.path());
        })?;
        require(
            result.status.success(),
            &format!("native defaulted-metadata observation failed: {result:?}"),
        )?;
        io::stderr().lock().write_all(&result.stdout)?;
        require(
            std::fs::read_dir(temp.path())?.next().is_none(),
            "defaulted fixture leaked owned files",
        )?;
    }
    Ok(())
}

/// An already authorized caller preserves a different owner's policy and cleanup.
#[cfg(windows)]
#[test]
fn windows_authorized_foreign_owner_replacement_preserves_policy() -> io::Result<()> {
    const MODE: &str = "PROJECTATLAS_TEST_SYSTEM_POLICY_MODE";
    if std::env::var(MODE).as_deref() != Ok("authorized-owner") {
        let executable = std::env::current_exe()?;
        let script = windows_fixture_script(
            "[SystemPolicyFixture]::Enable($false); [SystemPolicyFixture]::EnableRestore(); & $env:PROJECTATLAS_TEST_SYSTEM_POLICY_EXE --exact project_state::tests::windows_authorized_foreign_owner_replacement_preserves_policy --nocapture; exit $LASTEXITCODE",
        )?;
        let child = windows_powershell_fixture(&script, |command| {
            command
                .env(MODE, "authorized-owner")
                .env("PROJECTATLAS_TEST_SYSTEM_POLICY_EXE", &executable);
        })?;
        return require(
            child.status.success(),
            &format!("authorized-owner child failed: {child:?}"),
        );
    }
    let setup = windows_fixture_script(
        "[SystemPolicyFixture]::EnableRestore(); [SystemPolicyFixture]::ForeignOwner($env:PROJECTATLAS_TEST_ACL_PATH)",
    )?;
    for (private, name, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let directory = if private {
            root.join(STATE_DIRECTORY)
        } else {
            root.clone()
        };
        fs::create_dir_all(&directory)?;
        let output = directory.join(name);
        fs::write(&output, b"foreign owner original")?;
        let prepared = windows_powershell_fixture(&setup, |command| {
            command.env("PROJECTATLAS_TEST_ACL_PATH", &output);
        })?;
        require(
            prepared.status.success(),
            &format!("foreign-owner setup failed: {prepared:?}"),
        )?;
        let expected = windows_acl(&output, None)?;
        require(
            expected.contains("O:SY") || expected.contains("O:S-1-5-18"),
            "fixture did not establish SYSTEM ownership",
        )?;
        let outside = root.join("foreign-owner-outside");
        fs::hard_link(&output, &outside)?;
        let guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("foreign-owner target was external"))?
        };
        let entries = fs::read_dir(&directory)?.count();
        guard.publish_with_stage_check(
            &output,
            b"foreign owner replacement",
            FilePublication::Replace,
            |stage| {
                require(
                    windows_file_acl_matches(&expected, &windows_acl(stage.path(), None)?)?,
                    "initial stage lost authorized foreign-owner policy",
                )
            },
            || Ok(()),
        )?;
        require(
            fs::read(&output)? == b"foreign owner replacement"
                && fs::read(&outside)? == b"foreign owner original"
                && windows_file_acl_matches(&expected, &windows_acl(&output, None)?)?
                && windows_acl(&outside, None)? == expected
                && !same_file::is_same_file(&output, &outside)?
                && fs::read_dir(&directory)?.count() == entries,
            "authorized foreign-owner replacement lost payload, policy or outside owner",
        )?;
        let published_acl = windows_acl(&output, None)?;
        let failed = guard.publish_with_stage_check(
            &output,
            b"unpublished foreign-owner payload",
            FilePublication::Replace,
            |_| Ok(()),
            || {
                Err(io::Error::other(
                    "injected foreign-owner publication failure",
                ))
            },
        );
        require(
            failed.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains("injected foreign-owner publication failure")
            }) && fs::read(&output)? == b"foreign owner replacement"
                && fs::read(&outside)? == b"foreign owner original"
                && windows_acl(&output, None)? == published_acl
                && fs::read_dir(&directory)?.count() == entries,
            &format!("authorized foreign-owner cleanup failed: {failed:?}"),
        )?;
    }
    Ok(())
}

/// Directory-specific case and actual DOS aliases share the fixed-name owner.
#[cfg(windows)]
#[test]
fn windows_native_namespace_aliases_preserve_distinct_custom_entries() -> io::Result<()> {
    let mut volumes = vec![std::env::temp_dir()];
    if let Some(runner) = std::env::var_os("RUNNER_TEMP") {
        let runner = std::path::PathBuf::from(runner);
        if !volumes.contains(&runner) {
            volumes.push(runner);
        }
    }
    let mut actual_short_aliases = 0;
    for volume in volumes {
        for sensitive in [false, true] {
            let temp = tempfile::tempdir_in(&volume)?;
            let root = temp.path().canonicalize()?;
            windows_namespace_fixture(&root, "case-set", sensitive)?;
            require(
                fixture_names_are_case_sensitive(&root)? == sensitive,
                "native case setup did not affect actual name lookup",
            )?;
            let directory = root.join(STATE_DIRECTORY);
            fs::create_dir(&directory)?;
            require(
                super::windows_filesystem::case_sensitive(&directory)? == sensitive,
                "new private directory lost native case inheritance",
            )?;
            let roles = [
                (true, DATABASE_FILE, true),
                (true, CONFIG_FILE, false),
                (false, LEGACY_CONFIG_FILE, false),
                (true, JSON_MAP_FILE, false),
                (false, super::GITIGNORE_FILE, false),
            ];
            for (private, expected, database) in roles {
                let parent = if private { &directory } else { &root };
                let target = parent.join(expected);
                let upper = parent.join(expected.to_ascii_uppercase());
                let admit = |path: &Path| {
                    if database {
                        ProjectStateGuard::preflight_database_path(path, &root)
                    } else {
                        ProjectStateGuard::preflight_document(&root, path)
                    }
                };
                require(
                    admit(&upper)?.is_some() != sensitive,
                    "missing ASCII name used an OS-wide case assumption",
                )?;
                fs::write(&target, b"managed bytes")?;
                if sensitive {
                    // Equal inode identity cannot make a distinct namespace entry managed.
                    fs::hard_link(&target, &upper)?;
                    require(
                        admit(&upper)?.is_none(),
                        "distinct uppercase custom hardlink became managed",
                    )?;
                    fs::remove_file(&upper)?;
                }
                for suffix in [".", " "] {
                    let normal_parent = if private {
                        temp.path().join(STATE_DIRECTORY)
                    } else {
                        temp.path().to_path_buf()
                    };
                    let normal = normal_parent.join(format!("{expected}{suffix}"));
                    let mut device = std::ffi::OsString::from(r"\\.\");
                    device.push(normal.as_os_str());
                    for normal in [normal, std::path::PathBuf::from(device)] {
                        require(
                            normal.try_exists()?,
                            "ordinary Win32 suffix did not alias the existing fixture entry",
                        )?;
                        let outside = root.join("normal-alias-outside-owner");
                        let before = fs::read(&target)?;
                        fs::hard_link(&target, &outside)?;
                        if database {
                            require(
                                admit(&normal).is_err(),
                                "normal database suffix bypassed hardlink admission",
                            )?;
                        } else {
                            let mut admitted = admit(&normal)?.ok_or_else(|| {
                                io::Error::other("normal fixed-name suffix was external")
                            })?;
                            if expected != super::GITIGNORE_FILE {
                                admitted.write_document(b"normal alias publication")?;
                                require(
                                    fs::read(&outside)? == before
                                        && fs::read(&target)? == b"normal alias publication",
                                    "normal suffix publication changed the outside owner",
                                )?;
                            }
                        }
                        fs::remove_file(outside)?;
                    }
                    // The same suffix in an extended path is a literal custom
                    // entry, even when its inode also backs the managed name.
                    let literal = parent.join(format!("{expected}{suffix}"));
                    fs::hard_link(&target, &literal)?;
                    require(
                        admit(&literal)?.is_none(),
                        "verbatim literal suffix became managed",
                    )?;
                    fs::remove_file(literal)?;
                }
                let short = std::path::PathBuf::from(windows_namespace_fixture(
                    &target,
                    "short-name",
                    sensitive,
                )?);
                if short.file_name() != target.file_name() {
                    actual_short_aliases += 1;
                    for spelling in [
                        short.clone(),
                        short.with_file_name(
                            short
                                .file_name()
                                .ok_or_else(|| io::Error::other("short alias lacks filename"))?
                                .to_string_lossy()
                                .to_ascii_lowercase(),
                        ),
                    ] {
                        let aliases = spelling.try_exists()?;
                        let admitted = admit(&spelling)?;
                        require(
                            admitted.is_some() == aliases,
                            &format!(
                                "short alias classification differs from actual lookup: sensitive={sensitive}, selected={}, expected={expected}",
                                spelling.display()
                            ),
                        )?;
                    }
                    if database {
                        let outside = root.join("outside-database-owner");
                        fs::hard_link(&target, &outside)?;
                        require(
                            admit(&short).is_err(),
                            "short database alias bypassed hardlink admission",
                        )?;
                        fs::remove_file(outside)?;
                    }
                }
                fs::remove_file(&target)?;
                let custom = parent.join("CUSTOM~1.TXT");
                fs::write(&custom, b"custom bytes")?;
                require(
                    admit(&custom)?.is_none(),
                    "literal DOS-looking custom entry became managed",
                )?;
                fs::remove_file(custom)?;
            }
            let base = root.join(super::GITIGNORE_FILE);
            fs::write(&base, b"base namespace bytes")?;
            let stream = root.join(".gitignore:x");
            fs::write(&stream, b"selected custom stream")?;
            require(
                ProjectStateGuard::preflight_document(&root, &stream)?.is_none()
                    && fs::read(&base)? == b"base namespace bytes"
                    && fs::read(&stream)? == b"selected custom stream",
                "alternate data stream redirected into the managed base entry",
            )?;
            let outside = root.join("outside-reparse-owner");
            fs::write(&outside, b"outside bytes")?;
            let linked = directory.join(CONFIG_FILE);
            std::os::windows::fs::symlink_file(&outside, &linked)?;
            let short = std::path::PathBuf::from(windows_namespace_fixture(
                &linked,
                "short-name",
                sensitive,
            )?);
            require(
                ProjectStateGuard::preflight_document(&root, &short).is_err()
                    && fs::read(&outside)? == b"outside bytes",
                "short reparse entry followed its target or bypassed managed admission",
            )?;
            let custom = directory.join("CUSTOM~1.TXT");
            fs::hard_link(&outside, &custom)?;
            require(
                ProjectStateGuard::preflight_document(&root, &custom)?.is_none(),
                "custom short-looking hardlink became managed by target identity",
            )?;
            fs::remove_file(custom)?;
            fs::remove_file(linked)?;
            let short_directory = std::path::PathBuf::from(windows_namespace_fixture(
                &directory,
                "short-name",
                sensitive,
            )?);
            let database = directory.join(DATABASE_FILE);
            fs::write(&database, b"managed database")?;
            for suffix in [".", " "] {
                let alias = temp
                    .path()
                    .join(format!("{STATE_DIRECTORY}{suffix}"))
                    .join(DATABASE_FILE);
                require(
                    ProjectStateGuard::preflight_database_path(&alias, &root)?.is_some(),
                    "normal private-directory suffix bypassed database ownership",
                )?;
            }

            require(
                ProjectStateGuard::preflight_database_path(
                    &short_directory.join(DATABASE_FILE),
                    &root,
                )?
                .is_some(),
                "private directory short alias bypassed ownership",
            )?;
            fs::remove_file(database)?;
            let outside_directory = root.join("outside-state-owner");
            fs::rename(&directory, &outside_directory)?;
            std::os::windows::fs::symlink_dir(&outside_directory, &directory)?;
            for suffix in [".", " "] {
                let alias = temp
                    .path()
                    .join(format!("{STATE_DIRECTORY}{suffix}"))
                    .join(DATABASE_FILE);
                require(
                    ProjectStateGuard::preflight_database_path(&alias, &root).is_err(),
                    "normal private-directory suffix followed an indirect state owner",
                )?;
            }
            fs::remove_dir(&directory)?;
            fs::rename(&outside_directory, &directory)?;

            if sensitive {
                let upper_directory = root.join(STATE_DIRECTORY.to_ascii_uppercase());
                fs::create_dir(&upper_directory)?;
                require(
                    ProjectStateGuard::preflight_database_path(
                        &upper_directory.join(DATABASE_FILE),
                        &root,
                    )?
                    .is_none(),
                    "distinct uppercase private directory became managed",
                )?;
            }
            fs::remove_dir(&directory)?;
            require(
                ProjectStateGuard::preflight_document(
                    &root,
                    &directory.join(CONFIG_FILE.to_ascii_uppercase()),
                )?
                .is_some()
                    != sensitive,
                "missing private directory ignored inherited comparison mode",
            )?;
            let uppercase_config = CONFIG_FILE.to_ascii_uppercase();
            let reduced = unreduced_path(
                &root,
                &["missing", "..", STATE_DIRECTORY, &uppercase_config],
            )?;
            require(
                ProjectStateGuard::preflight_document(&root, &reduced).is_err() != sensitive
                    && !root.join("missing").exists(),
                "missing-parent reduction changed ownership or created state",
            )?;
        }
    }
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        require(
            actual_short_aliases > 0,
            "hosted namespace proof did not exercise any actual short aliases",
        )?;
    }
    Ok(())
}

/// The restrictive Owner Rights experiment cannot supply later policy rights.
#[cfg(windows)]
#[test]
fn windows_restrictive_creation_does_not_grant_unavailable_policy_rights() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let sentinel = temp.path().join("outside-fixture-owner");
    fs::write(&sentinel, b"unchanged fixture owner")?;
    let result = windows_powershell_fixture(WINDOWS_SYSTEM_POLICY_FIXTURE, |command| {
        command
            .env("PROJECTATLAS_TEST_SYSTEM_POLICY_MODE", "creator")
            .env(
                "PROJECTATLAS_TEST_ACL_PATH",
                temp.path().join("private-creation-control"),
            );
    })?;
    let stdout = String::from_utf8(result.stdout).map_err(io::Error::other)?;
    let stderr = String::from_utf8(result.stderr).map_err(io::Error::other)?;
    let mask = stdout
        .lines()
        .find_map(|line| line.strip_prefix("private creator granted access="))
        .ok_or_else(|| io::Error::other("native creator did not report granted rights"))?;
    let mask = u32::from_str_radix(mask.trim(), 16).map_err(io::Error::other)?;
    require(
        !result.status.success()
            && stdout.contains("private creator failure native=5")
            && stderr.contains("Private creator phase: apply creator DACL through retained")
            && mask & 0x0004_0000 == 0
            && mask & 0x0001_0003 == 0x0001_0003
            && fs::read(&sentinel)? == b"unchanged fixture owner"
            && fs::read_dir(temp.path())?.count() == 1,
        "rejected private-descriptor design lost its exact rights/refusal/cleanup control",
    )
}

/// Retained EFS admission refuses unsupported replacement while native creation
/// and explicit operator-selected in-place writes keep their encryption behavior.
#[cfg(windows)]
#[test]
#[ignore = "requires EFS-capable NTFS and a fixture encryption key; selected explicitly in Windows CI"]
fn windows_encrypted_outputs_are_preserved_or_refused_before_publication() -> io::Result<()> {
    use same_file::Handle;
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ENCRYPTED as ENCRYPTED;
    const MODE: &str = "PROJECTATLAS_TEST_SYSTEM_POLICY_MODE";
    if !matches!(
        std::env::var(MODE).as_deref(),
        Ok("efs-complete" | "efs-unprivileged")
    ) {
        // A hidden-SACL refusal must not mask the encrypted-file path for
        // callers whose full policy rights are already available.
        let executable = std::env::current_exe()?;
        for mode in ["efs-complete", "efs-unprivileged"] {
            let child = windows_powershell_fixture(WINDOWS_SYSTEM_POLICY_FIXTURE, |command| {
                command
                    .env(MODE, mode)
                    .env("PROJECTATLAS_TEST_SYSTEM_POLICY_EXE", &executable);
            })?;
            require(
                child.status.success(),
                &format!("native EFS {mode} control failed: {child:?}"),
            )?;
            io::stderr().lock().write_all(&child.stdout)?;
            io::stderr().lock().write_all(&child.stderr)?;
        }
        return Ok(());
    }
    let encrypt = |path: &Path| -> io::Result<()> {
        let result = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[System.IO.File]::Encrypt($env:PROJECTATLAS_TEST_ACL_PATH)",
            ])
            .env("PROJECTATLAS_TEST_ACL_PATH", path)
            .output()?;
        require(
            result.status.success() && fs::metadata(path)?.file_attributes() & ENCRYPTED != 0,
            &format!("native EFS setup failed: {result:?}"),
        )
    };
    // Counterfactual: the former full-attribute permissions copy succeeds but
    // cannot reproduce file-level encryption in an ordinary parent directory.
    let former = tempfile::tempdir()?;
    let output = former.path().join("former-encrypted-output");
    let outside = former.path().join("former-encrypted-owner");
    fs::write(&output, b"previous encrypted bytes")?;
    encrypt(&output)?;
    fs::hard_link(&output, &outside)?;
    let permissions = fs::metadata(&output)?.permissions();
    let mut stage = tempfile::NamedTempFile::new_in(former.path())?;
    stage.as_file().set_permissions(permissions)?;
    stage.write_all(b"former plaintext replacement")?;
    stage.as_file().sync_all()?;
    stage.persist(&output).map_err(|failure| failure.error)?;
    require(
        fs::read(&output)? == b"former plaintext replacement"
            && fs::metadata(&output)?.file_attributes() & ENCRYPTED == 0
            && fs::read(&outside)? == b"previous encrypted bytes"
            && fs::metadata(&outside)?.file_attributes() & ENCRYPTED != 0,
        "former permissions-only publisher did not establish EFS loss",
    )?;
    for (private, filename, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let directory = if private {
            let state = root.join(STATE_DIRECTORY);
            fs::create_dir(&state)?;
            state
        } else {
            root.clone()
        };
        require(
            fs::metadata(&directory)?.file_attributes() & ENCRYPTED == 0,
            "file-level encryption probe needs an ordinary parent",
        )?;
        let output = directory.join(filename);
        fs::write(&output, b"last valid encrypted output")?;
        encrypt(&output)?;
        let outside = root.join("outside-encrypted-owner");
        fs::hard_link(&output, &outside)?;
        let guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("EFS document was external"))?
        };
        let entries = fs::read_dir(&directory)?.count();
        let identity = Handle::from_path(&outside)?;
        let reached_stage = std::cell::Cell::new(false);
        let replaced = guard.publish_with_stage_check(
            &output,
            b"encrypted replacement",
            FilePublication::Replace,
            |_| {
                reached_stage.set(true);
                Ok(())
            },
            || Ok(()),
        );
        writeln!(
            io::stderr().lock(),
            "native EFS mode={:?} role={filename} publication={replaced:?}",
            std::env::var(MODE),
        )?;
        require(
            replaced
                .as_ref()
                .is_err_and(|source| source.to_string().contains("EFS"))
                && !reached_stage.get()
                && fs::read(&output)? == b"last valid encrypted output"
                && fs::metadata(&output)?.file_attributes() & ENCRYPTED != 0
                && fs::read(&outside)? == b"last valid encrypted output"
                && fs::metadata(&outside)?.file_attributes() & ENCRYPTED != 0
                && Handle::from_path(&output)? == identity
                && Handle::from_path(&outside)? == identity
                && fs::read_dir(&directory)?.count() == entries,
            &format!("EFS admission lost refusal, bytes, identity or cleanup: {replaced:?}"),
        )?;
        let created = guard.publish_with_stage_check(
            &output,
            b"unpublished no-clobber bytes",
            FilePublication::Create,
            |_| {
                reached_stage.set(true);
                Ok(())
            },
            || Ok(()),
        )?;
        require(
            !created
                && !reached_stage.get()
                && fs::read(&output)? == b"last valid encrypted output"
                && fs::metadata(&output)?.file_attributes() & ENCRYPTED != 0
                && Handle::from_path(&output)? == identity
                && Handle::from_path(&outside)? == identity
                && fs::read_dir(&directory)?.count() == entries,
            "create-only EFS collision changed existing output or staged a replacement",
        )?;
    }
    for (private, filename, host) in replacement_targets() {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let directory = if private {
            root.join(STATE_DIRECTORY)
        } else {
            root.clone()
        };
        fs::create_dir_all(&directory)?;
        let output = directory.join(filename);
        let outside = root.join("late-encrypted-owner");
        fs::write(&output, b"previous ordinary output")?;
        fs::hard_link(&output, &outside)?;
        let guard = if host {
            ProjectStateGuard::preflight(&root, None, None)?
        } else {
            ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("late EFS document was external"))?
        };
        let entries = fs::read_dir(&directory)?.count();
        let encrypted = std::cell::Cell::new(false);
        let reached_callback = std::cell::Cell::new(false);
        let replaced = guard.publish(
            &output,
            b"unpublished plaintext",
            FilePublication::Replace,
            || {
                reached_callback.set(true);
                if let Err(source) = encrypt(&output) {
                    writeln!(io::stderr().lock(),
                        "native EFS late mutation unavailable with retained snapshot: role={filename}, {source}")?;
                    return Err(source);
                }
                encrypted.set(true);
                Ok(())
            },
        );
        writeln!(
            io::stderr().lock(),
            "native EFS late role={filename} encrypted={} publication={replaced:?}",
            encrypted.get()
        )?;
        require(
            reached_callback.get()
                && replaced.is_err()
                && (!encrypted.get()
                    || replaced
                        .as_ref()
                        .is_err_and(|source| source.to_string().contains("EFS")))
                && fs::read(&output)? == b"previous ordinary output"
                && fs::read(&outside)? == b"previous ordinary output"
                && (fs::metadata(&output)?.file_attributes() & ENCRYPTED != 0) == encrypted.get()
                && (fs::metadata(&outside)?.file_attributes() & ENCRYPTED != 0) == encrypted.get()
                && same_file::is_same_file(&output, &outside)?
                && fs::read_dir(&directory)?.count() == entries,
            &format!("late EFS policy change escaped publication or cleanup: {replaced:?}"),
        )?;
    }
    // Genuinely new generated outputs retain ordinary directory EFS inheritance.
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    // File.Encrypt rejects directories; the OS cipher tool supports this
    // directory attribute and does not recurse without /s.
    let encrypted_directory = std::process::Command::new("cipher.exe")
        .arg("/e")
        // cipher's drive parser rejects canonical \\?\ paths. Select this
        // disposable directory as cwd instead of rewriting filesystem paths.
        .current_dir(&guard.directory)
        .arg(".")
        .output()?;
    require(
        encrypted_directory.status.success()
            && fs::metadata(&guard.directory)?.file_attributes() & ENCRYPTED != 0,
        &format!("native EFS directory setup failed: {encrypted_directory:?}"),
    )?;
    for filename in [HOST_FILES[0], CONFIG_FILE] {
        let output = guard.directory.join(filename);
        if filename == HOST_FILES[0] {
            guard.write_generated(&output, b"new encrypted output")?;
        } else {
            let document = ProjectStateGuard::preflight_document(&root, &output)?
                .ok_or_else(|| io::Error::other("new EFS document was external"))?;
            document.publish(
                &output,
                b"new encrypted output",
                FilePublication::Create,
                || Ok(()),
            )?;
        }
        require(
            fs::read(&output)? == b"new encrypted output"
                && fs::metadata(output)?.file_attributes() & ENCRYPTED != 0,
            "new output lost EFS directory inheritance",
        )?;
    }
    for filename in [HOST_FILES[1], JSON_MAP_FILE] {
        let output = guard.directory.join(filename);
        let outside = root.join(format!("encrypted-create-winner-{filename}"));
        let entries = fs::read_dir(&guard.directory)?.count();
        let document = if filename == JSON_MAP_FILE {
            Some(
                ProjectStateGuard::preflight_document(&root, &output)?
                    .ok_or_else(|| io::Error::other("racing EFS document was external"))?,
            )
        } else {
            None
        };
        let publisher = document.as_ref().unwrap_or(&guard);
        let created = publisher.publish(
            &output,
            b"unpublished create payload",
            FilePublication::Create,
            || {
                fs::write(&output, b"winning encrypted output")?;
                require(
                    fs::metadata(&output)?.file_attributes() & ENCRYPTED != 0,
                    "create-race winner did not inherit EFS",
                )?;
                fs::hard_link(&output, &outside)
            },
        )?;
        require(
            !created
                && fs::read(&output)? == b"winning encrypted output"
                && fs::read(&outside)? == b"winning encrypted output"
                && fs::metadata(&output)?.file_attributes() & ENCRYPTED != 0
                && fs::metadata(&outside)?.file_attributes() & ENCRYPTED != 0
                && same_file::is_same_file(&output, &outside)?
                && fs::read_dir(&guard.directory)?.count() == entries + 1,
            "create-only EFS race replaced the winner or leaked staging",
        )?;
    }
    let custom = guard.directory.join("operator-config.toml");
    fs::write(&custom, b"encrypted custom output")?;
    require(
        ProjectStateGuard::preflight_document(&root, &custom)?.is_none()
            && fs::metadata(&custom)?.file_attributes() & ENCRYPTED != 0,
        "EFS restriction captured an explicit custom output",
    )?;
    fs::write(&custom, b"changed explicit encrypted output")?;
    require(
        fs::read(&custom)? == b"changed explicit encrypted output"
            && fs::metadata(custom)?.file_attributes() & ENCRYPTED != 0,
        "explicit in-place output lost native EFS support",
    )
}

/// Independently query or prepare SACL policy through native named SDK calls.
#[cfg(windows)]
fn windows_system_policy(path: &Path, policy: Option<&str>) -> io::Result<String> {
    windows_system_policy_query(path, policy, 0x1f8)
}

/// Compare full versus filtered native queries without sharing the product adapter.
#[cfg(windows)]
fn windows_system_policy_query(
    path: &Path,
    policy: Option<&str>,
    information: u32,
) -> io::Result<String> {
    windows_policy_fixture(path, policy, information, false)
}

/// Independently retain and compare the complete native opaque system ACL.
#[cfg(windows)]
fn windows_native_system_policy(path: &Path, information: u32) -> io::Result<String> {
    windows_policy_fixture(path, None, information, true)
}

/// Run the shared native fixture with an explicit query class and output oracle.
#[cfg(windows)]
fn windows_policy_fixture(
    path: &Path,
    policy: Option<&str>,
    information: u32,
    native: bool,
) -> io::Result<String> {
    let result = windows_powershell_fixture(WINDOWS_SYSTEM_POLICY_FIXTURE, |command| {
        command
            .env(
                "PROJECTATLAS_TEST_SYSTEM_POLICY_MODE",
                if native { "native-query" } else { "query" },
            )
            .env(
                "PROJECTATLAS_TEST_SYSTEM_POLICY_QUERY",
                information.to_string(),
            )
            .env("PROJECTATLAS_TEST_ACL_PATH", path)
            .env("PROJECTATLAS_TEST_SYSTEM_POLICY", policy.unwrap_or(""));
    })?;
    require(
        result.status.success(),
        &format!("native SACL fixture failed: {result:?}"),
    )?;
    String::from_utf8(result.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(io::Error::other)
}

/// Ordinary replacement needs no SACL privilege; nonempty policy is never dropped.
#[cfg(windows)]
#[test]
fn windows_system_policy_replacement_preserves_or_refuses_without_global_privilege_changes()
-> io::Result<()> {
    use std::io::Write;
    const MODE: &str = "PROJECTATLAS_TEST_SYSTEM_POLICY_MODE";
    let mode = std::env::var(MODE).ok();
    if !matches!(mode.as_deref(), Some("complete" | "unprivileged")) {
        let executable = std::env::current_exe()?;
        for mode in ["complete", "unprivileged"] {
            let child = windows_powershell_fixture(WINDOWS_SYSTEM_POLICY_FIXTURE, |command| {
                command
                    .env(MODE, mode)
                    .env("PROJECTATLAS_TEST_SYSTEM_POLICY_EXE", &executable);
            })?;
            require(
                child.status.success(),
                &format!("native SACL {mode} child failed: {child:?}"),
            )?;
        }
        return Ok(());
    }
    let complete = mode.as_deref() == Some("complete");
    // The unprivileged child has SeSecurityPrivilege explicitly disabled, so
    // this positive cannot be satisfied accidentally by the runner's privileges.
    let ordinary = tempfile::tempdir()?;
    let root = ordinary.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    fs::write(&output, b"ordinary previous output")?;
    let ordinary_policy = windows_system_policy(&output, None)?;
    let ordinary_fields: Vec<_> = ordinary_policy.splitn(4, '|').collect();
    require(
        ordinary_fields.len() == 4
            && u16::from_str_radix(ordinary_fields[0], 16).map_err(io::Error::other)? & 0x10 == 0
            && ordinary_fields[1] == "absent"
            && ordinary_fields[2] == "0",
        &format!("ordinary fixture did not prove absent system policy: {ordinary_policy}"),
    )?;
    let captured = super::ReplacementSource::capture(&output, &fs::symlink_metadata(&output)?)?;
    require(
        captured.security.system.needs_system_access() == complete,
        "native child did not establish the requested SACL privilege state",
    )?;
    drop(captured);
    guard.write_generated(&output, b"ordinary replacement")?;
    require(
        fs::read(&output)? == b"ordinary replacement",
        "ordinary regeneration required SACL privilege",
    )?;
    if complete {
        let control = guard.directory.join("former-system-policy-publication");
        fs::write(&control, b"original control")?;
        let previous = windows_system_policy(&control, Some("S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)"))?;
        let mut stage = tempfile::Builder::new().make_in(&guard.directory, |path| {
            windows_policy_test_file(path, false)
        })?;
        {
            let source = fs::File::open(&control)?;
            // The former adapter preserved DACLs but omitted system policy.
            super::preserve_windows_access(&control, &source, &stage)?;
        }
        stage.write_all(b"former replacement")?;
        stage.persist(&control).map_err(|failure| failure.error)?;
        require(
            windows_system_policy(&control, None)? != previous,
            "former DACL-only publisher did not reproduce SACL policy loss",
        )?;
        fs::remove_file(control)?;

        let expected = windows_system_policy(&output, Some("S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)"))?;
        let outside = root.join("outside-integrity-policy-owner");
        fs::hard_link(&output, &outside)?;
        guard.publish_with_stage_check(
            &output,
            b"restored high-integrity replacement",
            FilePublication::Replace,
            |stage| {
                let lower = windows_system_policy(
                    stage.path(),
                    Some("S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;ME)"),
                )?;
                require(
                    expected.contains("(ML;;NW;;;HI)")
                        && lower.contains("(ML;;NW;;;ME)")
                        && expected != lower
                        && expected.split('|').next() == lower.split('|').next(),
                    "integrity-only copy control did not establish distinct labels with equal flags",
                )?;
                // Read the original creator handle before policy copy; the
                // label setter above is only independent fixture preparation.
                let observed = super::windows_security::SystemPolicy::capture(stage.as_file(), true)?;
                let source = super::ReplacementSource::capture(
                    &output,
                    &fs::symlink_metadata(&output)?,
                )?;
                require(
                    observed != source.security.system,
                    "held-handle comparison omitted the only integrity-policy difference",
                )
            },
            || Ok(()),
        ).map_err(|error| io::Error::new(error.kind(), format!(
            "protected integrity restoration control: {error}"
        )))?;
        require(
            windows_system_policy(&output, None)? == expected
                && windows_system_policy(&outside, None)? == expected
                && fs::read(&output)? == b"restored high-integrity replacement"
                && fs::read(&outside)? == b"ordinary replacement",
            "integrity-only stage difference escaped copy/readback or changed the outside owner",
        )?;
    }

    let roles = HOST_FILES.iter().map(|name| (true, true, *name)).chain([
        (false, true, CONFIG_FILE),
        (false, false, LEGACY_CONFIG_FILE),
        (false, true, JSON_MAP_FILE),
    ]);
    for (host, private, filename) in roles {
        let extra: &[&str] = if host && filename == HOST_FILES[0] {
            &[
                "S:",
                "S:P",
                "S:NO_ACCESS_CONTROL",
                "S:(AU;SAFA;FW;;;WD)",
                r#"S:P(RA;;;;;WD;("PublicationClaim",TS,0,"alpha","beta"))"#,
                "S:P(SP;;;;;S-1-17-1-2-3-4-5-6-7-8)",
                r#"S:P(AU;SAFA;FW;;;WD)(RA;;;;;WD;("PublicationClaim",TS,0,"alpha","beta"))(SP;;;;;S-1-17-1-2-3-4-5-6-7-8)(ML;;NW;;;HI)"#,
            ]
        } else {
            &[]
        };
        for policy in [
            "S:P(ML;;NW;;;HI)",
            "S:P(AU;SAFA;FW;;;WD)",
            "S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let directory = if private {
                root.join(STATE_DIRECTORY)
            } else {
                root.clone()
            };
            fs::create_dir_all(&directory)?;
            let output = directory.join(filename);
            fs::write(&output, b"previous complete output")?;
            let before = windows_system_policy(&output, Some(policy))?;
            let extended = policy.contains("(RA;") || policy.contains("(SP;");
            let opaque = if extended {
                let complete = windows_native_system_policy(&output, 0x1f8)?;
                require(
                    windows_native_system_policy(&output, 0x18)? != complete,
                    "former retained query did not reproduce extended-policy omission",
                )?;
                Some(complete)
            } else {
                None
            };
            let native: Vec<_> = before.splitn(4, '|').collect();
            require(
                native.len() == 4,
                "native system policy oracle is incomplete",
            )?;
            let control = u16::from_str_radix(native[0], 16).map_err(io::Error::other)?;
            if matches!(policy, "S:" | "S:P") {
                require(
                    control & 0x10 != 0
                        && (control & 0x2000 != 0) == (policy == "S:P")
                        && matches!(native[1], "allocated" | "null")
                        && native[2] == "0",
                    &format!("empty SACL request produced an unsupported native class: {before}"),
                )?;
                // NTFS on the supported hosted image normalizes an explicitly
                // initialized empty ACL to NULL. Preserve the actual snapshot
                // exactly below; this is not allocated-empty filesystem proof.
            } else if policy == "S:NO_ACCESS_CONTROL" {
                require(
                    control & 0x10 != 0 && native[1] == "null" && native[2] == "0",
                    &format!("NULL SACL fixture normalized to another native class: {before}"),
                )?;
            }
            require(
                (!policy.contains("(AU;") || before.contains("(AU;"))
                    && (!policy.contains("(ML;") || before.contains("(ML;"))
                    && (!policy.contains("(RA;")
                        || (before.contains("(RA;")
                            && before.contains("alpha")
                            && before.contains("beta")))
                    && (!policy.contains("(SP;") || before.contains("S-1-17-1-2-3-4-5-6-7-8")),
                "native SACL fixture did not retain its intended ACE classes",
            )?;
            let filtered = windows_system_policy_query(&output, None, 0x10)?;
            let flags = filtered
                .split('|')
                .next()
                .ok_or_else(|| io::Error::other("missing native SACL flags"))?;
            let flags = u16::from_str_radix(flags, 16).map_err(io::Error::other)?;
            if policy.contains('(') {
                require(flags & 0x10 != 0, "label query lost original SACL presence")?;
            }
            if policy == "S:P(AU;SAFA;FW;;;WD)" {
                require(
                    !filtered.contains("(AU;"),
                    "label query unexpectedly exposed audit ACEs",
                )?;
            }
            let outside = root.join("outside-system-policy-owner");
            fs::hard_link(&output, &outside)?;
            let mut guard = if host {
                ProjectStateGuard::preflight(&root, None, None)?
            } else {
                ProjectStateGuard::preflight_document(&root, &output)?
                    .ok_or_else(|| io::Error::other("system-policy document was not admitted"))?
            };
            let entries = fs::read_dir(&directory)?.count();
            let replaced = if let Some(expected) = &opaque
                && complete
            {
                let staged = std::cell::RefCell::new(None::<std::path::PathBuf>);
                guard
                    .publish_with_stage_check(
                        &output,
                        b"preserved system-policy replacement",
                        FilePublication::Replace,
                        |stage| {
                            *staged.borrow_mut() = Some(stage.path().to_owned());
                            Ok(())
                        },
                        || {
                            let path = staged.borrow();
                            let path = path
                                .as_ref()
                                .ok_or_else(|| io::Error::other("missing extended-policy stage"))?;
                            require(
                                windows_native_system_policy(path, 0x1f8)? == *expected,
                                "held-stage setter changed native extended-policy bytes or order",
                            )
                        },
                    )
                    .map(|_| ())
            } else if host {
                guard.write_generated(&output, b"preserved system-policy replacement")
            } else {
                guard.write_document(b"preserved system-policy replacement")
            };
            if complete || flags & 0x10 == 0 {
                replaced.map_err(|error| {
                    io::Error::new(error.kind(), format!(
                        "system-policy publication role={filename}, source_control={control:04x}, source_class={}, audit={}, label={}, resource={}, scope={}: {error}",
                        native[1], policy.contains("(AU;"), policy.contains("(ML;"),
                        policy.contains("(RA;"), policy.contains("(SP;")
                    ))
                })?;
                require(
                    fs::read(&output)? == b"preserved system-policy replacement",
                    "full-policy regeneration failed",
                )?;
            } else {
                require(
                    replaced.is_err() && fs::read(&output)? == b"previous complete output",
                    "unprivileged replacement dropped an unknown SACL",
                )?;
            }
            require(
                windows_system_policy(&output, None)? == before
                    && windows_system_policy(&outside, None)? == before
                    && fs::read(&outside)? == b"previous complete output"
                    && fs::read_dir(&directory)?.count() == entries,
                "replacement changed system policy, outside bytes or staging entries",
            )?;
            if let Some(expected) = opaque {
                require(
                    windows_native_system_policy(&output, 0x1f8)? == expected
                        && windows_native_system_policy(&outside, 0x1f8)? == expected,
                    "publication changed opaque native extended policy or the outside owner",
                )?;
            }
        }
    }
    if complete {
        for (dacl_case, dacl) in [
            "D:P(A;;0x0013019f;;;OW)",
            "D:P(D;;WD;;;OW)(A;;FA;;;OW)",
            "D:P(D;;0x000c0000;;;OW)(A;;FA;;;OW)",
        ]
        .into_iter()
        .enumerate()
        {
            for policy in [
                "S:P(AU;SAFA;FW;;;WD)",
                "S:P(ML;;NW;;;HI)",
                r#"S:P(RA;;;;;WD;("PublicationClaim",TS,0,"alpha","beta"))"#,
                "S:P(SP;;;;;S-1-17-1-2-3-4-5-6-7-8)",
                r#"S:(AU;SAFA;FW;;;WD)(RA;;;;;WD;("PublicationClaim",TS,0,"alpha","beta"))(SP;;;;;S-1-17-1-2-3-4-5-6-7-8)(ML;;NW;;;HI)"#,
            ] {
                let unprotected = policy.starts_with("S:(");
                let targets = if unprotected {
                    replacement_targets().collect::<Vec<_>>()
                } else {
                    vec![(true, HOST_FILES[0], true)]
                };
                for (private, name, host) in targets {
                    let temp = tempfile::tempdir()?;
                    let root = temp.path().canonicalize()?;
                    let directory = if private {
                        root.join(STATE_DIRECTORY)
                    } else {
                        root.clone()
                    };
                    fs::create_dir_all(&directory)?;
                    let output = directory.join(name);
                    let guard = if host {
                        ProjectStateGuard::preflight(&root, None, None)?
                    } else {
                        ProjectStateGuard::preflight_document(&root, &output)?.ok_or_else(|| {
                            io::Error::other("restricted policy target was external")
                        })?
                    };
                    let outside = root.join("restricted-policy-outside-owner");
                    fs::write(&output, b"restricted policy original")?;
                    windows_system_policy(&output, Some(policy))?;
                    windows_discretionary_policy(&output, dacl)?;
                    fs::hard_link(&output, &outside)?;
                    let expected = windows_native_system_policy(&output, 0x1f8)?;
                    let expected_dacl = windows_acl(&output, None)?;
                    let observed = std::cell::Cell::new(false);
                    guard.publish_with_stage_check(&output, b"restricted policy replacement", FilePublication::Replace,
                    |stage| {
                        if unprotected {
                            let initial = windows_native_system_policy(stage.path(), 0x1f8)?;
                            let flags = |image: &str| -> io::Result<u16> {
                                u16::from_str_radix(image.split('|').next().ok_or_else(|| io::Error::other("native policy has no controls"))?,16).map_err(io::Error::other)
                            };
                            require(flags(&expected)? & 0x2000 == 0 && flags(&initial)? & 0x2000 != 0,
                                "unprotected mixed-policy case did not require actual SACL restoration")?;
                        }
                        observed.set(true);Ok(())
                    }, || Ok(())).map_err(|error| io::Error::new(error.kind(), format!(
                        "restricted system-policy publication role={name}, dacl_case={dacl_case}, unprotected={unprotected}, audit={}, label={}, resource={}, scope={}: {error}",
                        policy.contains("(AU;"), policy.contains("(ML;"), policy.contains("(RA;"), policy.contains("(SP;")
                    )))?;
                    require(
                        observed.get()
                            && fs::read(&output)? == b"restricted policy replacement"
                            && fs::read(&outside)? == b"restricted policy original"
                            && windows_native_system_policy(&output, 0x1f8)? == expected
                            && windows_native_system_policy(&outside, 0x1f8)? == expected
                            && windows_file_acl_matches(
                                &expected_dacl,
                                &windows_acl(&output, None)?,
                            )?,
                        "available system-policy preservation required unused discretionary rights",
                    )?;
                }
            }
        }
    }
    windows_system_policy_failure_controls(complete)?;
    Ok(())
}

/// Exercise hidden inherited stage policy, source changes and held-object setters.
#[cfg(windows)]
fn windows_system_policy_failure_controls(complete: bool) -> io::Result<()> {
    const ORIGINAL: &str = "S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)";
    const CHANGED_AUDIT: &str = "S:P(AU;SAFA;FR;;;WD)(ML;;NW;;;HI)";
    const CHANGED_LABEL: &str = "S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;ME)";
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    let outside = root.join("outside-stage-policy-owner");
    // Move an unlabeled file into an audited parent without changing its own
    // policy. Only the newly created stage must inherit that audit SACL.
    fs::write(&outside, b"previous complete output")?;
    let absent = windows_system_policy(&outside, None)?;
    windows_system_policy(&guard.directory, Some("S:P(AU;OICIIOSAFA;FW;;;WD)"))?;
    fs::rename(&outside, &output)?;
    require(
        windows_system_policy(&output, None)? == absent,
        "native move fixture did not retain absent source policy",
    )?;
    let probe = guard.directory.join("inherited-system-policy-control");
    fs::write(&probe, b"empty-policy probe")?;
    require(
        windows_system_policy(&probe, None)? != absent,
        "native stage parent did not supply inherited system policy",
    )?;
    fs::remove_file(probe)?;
    let entries = fs::read_dir(&guard.directory)?.count();
    let result = guard.write_generated(&output, b"replacement");
    if !complete {
        require(
            result.is_err(),
            "unprivileged stage inherited hidden audit policy",
        )?;
    }
    let bytes: &[u8] = if result.is_ok() {
        b"replacement"
    } else {
        b"previous complete output"
    };
    require(
        fs::read(&output)? == bytes
            && windows_system_policy(&output, None)? == absent
            && fs::read_dir(&guard.directory)?.count() == entries,
        "inherited stage policy changed valid output or leaked staging",
    )?;
    if !complete {
        return Ok(());
    }

    // A separate ordinary parent keeps source-change and substitution controls
    // independent of the deliberately incompatible stage-inheritance fixture.
    let isolated = tempfile::tempdir()?;
    let root = isolated.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    fs::write(&output, b"last valid output")?;
    windows_system_policy(&output, Some(ORIGINAL))?;
    let entries = fs::read_dir(&guard.directory)?.count();
    let mut changed = None;
    for policy in [CHANGED_AUDIT, CHANGED_LABEL] {
        windows_system_policy(&output, Some(ORIGINAL))?;
        let failed = guard.publish(
            &output,
            b"unpublished bytes",
            FilePublication::Replace,
            || {
                changed = Some(windows_system_policy(&output, Some(policy))?);
                Ok(())
            },
        );
        require(
            failed.is_err()
                && fs::read(&output)? == b"last valid output"
                && Some(windows_system_policy(&output, None)?) == changed
                && fs::read_dir(&guard.directory)?.count() == entries,
            "source SACL change was overwritten or leaked staging",
        )?;
    }
    let captured = super::ReplacementSource::capture(&output, &fs::symlink_metadata(&output)?)?;
    let victim = root.join("foreign-system-policy-owner");
    fs::write(&victim, b"foreign complete bytes")?;
    let victim_policy = windows_system_policy(&victim, Some(ORIGINAL))?;
    let staged = tempfile::Builder::new().make_in(&guard.directory, |path| {
        windows_policy_test_file(path, true)
    })?;
    let retained = guard.directory.join("retained-system-policy-stage");
    fs::rename(staged.path(), &retained)?;
    fs::hard_link(&victim, staged.path())?;
    captured.security.system.apply(staged.as_file())?;
    require(
        Some(windows_system_policy(&retained, None)?) == changed
            && windows_system_policy(&victim, None)? == victim_policy
            && fs::read(&victim)? == b"foreign complete bytes",
        "system ACL setter followed a substituted staging name",
    )?;
    fs::remove_file(staged.path())?;
    fs::rename(retained, staged.path())?;
    drop(staged);
    drop(captured);

    let stage_path = std::cell::RefCell::new(None);
    let failed = guard.publish_with_stage_check(
        &output,
        b"unpublished bytes",
        FilePublication::Replace,
        |stage| {
            *stage_path.borrow_mut() = Some(stage.path().to_path_buf());
            Ok(())
        },
        || {
            let path = stage_path.borrow();
            let path = path
                .as_ref()
                .ok_or_else(|| io::Error::other("stage was not observed"))?;
            windows_system_policy(path, Some(ORIGINAL))?;
            Ok(())
        },
    );
    require(
        failed.is_err()
            && fs::read(&output)? == b"last valid output"
            && Some(windows_system_policy(&output, None)?) == changed
            && fs::read_dir(&guard.directory)?.count() == entries,
        "changed staging SACL was published or leaked staging",
    )
}

/// The retained snapshot must survive checks and be released at the rename boundary.
#[cfg(windows)]
#[test]
fn source_snapshot_handles_are_released_before_windows_replacement() -> io::Result<()> {
    use std::io::Write;
    for linked in [false, true] {
        for snapshot in ["absent", "retained", "released"] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let output = root.join("destination");
            fs::write(&output, b"previous complete output")?;
            if linked {
                fs::hard_link(&output, root.join("outside-owner"))?;
            }
            let query = if snapshot == "absent" {
                None
            } else {
                Some(super::ReplacementSource::capture(
                    &output,
                    &fs::symlink_metadata(&output)?,
                )?)
            };
            let mut query = query;
            let mut stage = tempfile::Builder::new()
                .make_in(&root, |path| windows_policy_test_file(path, false))?;
            stage.write_all(b"complete replacement")?;
            if let Some(query) = &query {
                query.verify_destination(&output)?;
            }
            if snapshot == "released" {
                drop(query.take());
            }
            let result = stage.persist(&output);
            if snapshot == "retained" {
                require(
                    result
                        .as_ref()
                        .is_err_and(|failure| failure.error.raw_os_error() == Some(5))
                        && fs::read(&output)? == b"previous complete output",
                    "retained snapshot did not reproduce the native replacement refusal",
                )?;
            } else {
                result.map_err(|failure| failure.error)?;
                require(
                    fs::read(&output)? == b"complete replacement",
                    "releasing the verified snapshot did not allow atomic replacement",
                )?;
            }
            if linked {
                require(
                    fs::read(root.join("outside-owner"))? == b"previous complete output",
                    "native snapshot control changed the outside owner",
                )?;
            }
            drop(query);
        }
    }
    Ok(())
}

/// Create an inherited-policy control for isolated setter/handle experiments.
#[cfg(windows)]
fn windows_policy_test_file(path: &Path, complete: bool) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    let rights = 0xc207_0000 | if complete { 0x0100_0000 } else { 0 };
    fs::File::options()
        .write(true)
        .create_new(true)
        .access_mode(rights)
        .share_mode(4)
        .open(path)
}

/// A same-directory policy change cannot bypass the retained inheritance proof.
#[cfg(windows)]
#[test]
fn windows_parent_policy_change_refuses_before_restoration() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let target = guard.directory.join(HOST_FILES[0]);
    let outside = root.join("parent-policy-outside-owner");
    fs::write(&target, b"protected source")?;
    windows_acl(&target, Some(true))?;
    fs::hard_link(&target, &outside)?;
    let entries = fs::read_dir(&guard.directory)?.count();
    let result = guard.publish_with_stage_check(
        &target,
        b"unpublished payload",
        FilePublication::Replace,
        |_| windows_discretionary_policy(&guard.directory, "D:P(A;;FA;;;OW)(A;;FR;;;WD)"),
        || Ok(()),
    );
    require(
        result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("parent policy changed"))
            && fs::read(&target)? == b"protected source"
            && fs::read(&outside)? == b"protected source"
            && same_file::is_same_file(&target, &outside)?
            && fs::read_dir(&guard.directory)?.count() == entries,
        &format!(
            "parent policy change bypassed creation-policy restoration or cleanup: {result:?}"
        ),
    )
}

/// Preparing a stage uses admitted policy even if a later source query is broader.
#[cfg(windows)]
#[test]
fn windows_stage_policy_uses_the_captured_descriptor() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    fs::write(&source, b"admitted source")?;
    let expected = windows_acl(&source, Some(true))?;
    let captured = super::ReplacementSource::capture(&source, &fs::symlink_metadata(&source)?)?;
    let identity = same_file::Handle::from_path(&root)?;
    let creation = super::windows_security::ReplacementCreation::capture(
        &captured.file,
        &root,
        &identity,
        &captured.security,
    )?;
    let stage = tempfile::Builder::new().make_in(&root, |path| {
        super::create_windows_staging_file(path, Some(&creation))
    })?;
    windows_discretionary_policy(&source, "D:P(A;;FA;;;WD)")?;
    let changed = windows_acl(&source, None)?;
    require(
        changed != expected && captured.verify_destination(&source).is_err(),
        "source policy change was not detected",
    )?;
    let former =
        tempfile::Builder::new().make_in(&root, |path| windows_policy_test_file(path, false))?;
    super::preserve_windows_access(&source, &captured.file, &former)?;
    require(
        windows_acl(former.path(), None)? == changed,
        "fresh source policy did not reproduce broader stage preparation",
    )?;
    captured.apply(&source, &stage, &creation)?;
    let stale_creation = super::windows_security::ReplacementCreation::capture(
        &captured.file,
        &root,
        &identity,
        &captured.security,
    );
    require(
        stale_creation.as_ref().is_err_and(|error| {
            error
                .to_string()
                .contains("source policy changed before replacement creation")
        }),
        "snapshot assembly accepted a changed retained source",
    )?;
    require(
        windows_file_acl_matches(&expected, &windows_acl(stage.path(), None)?)?
            && windows_acl(&source, None)? == changed,
        "stage preparation followed a fresh mutable source policy",
    )
}

/// Native ACL updates stay on the held stage after a hardlink replaces its name.
#[cfg(windows)]
#[test]
fn windows_acl_updates_do_not_reopen_a_substituted_stage_name() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    let victim = root.join("outside-owner");
    fs::write(&source, b"previous output")?;
    fs::write(&victim, b"outside bytes")?;
    let source_acl = windows_acl(&source, Some(true))?;
    let victim_acl = windows_acl(&victim, None)?;
    require(
        source_acl != victim_acl,
        "ACL substitution fixture has equal policies",
    )?;
    let snapshot = super::ReplacementSource::capture(&source, &fs::symlink_metadata(&source)?)?;
    let mut stage =
        tempfile::Builder::new().make_in(&root, |path| windows_policy_test_file(path, false))?;
    let stage_name = stage.path().to_path_buf();
    let retained = root.join("retained-stage");
    fs::rename(&stage_name, &retained)?;
    fs::hard_link(&victim, &stage_name)?;
    // Never let NamedTempFile remove the intentionally substituted foreign name.
    stage.disable_cleanup(true);
    super::preserve_windows_access(&source, &snapshot.file, &stage)?;
    let applied = windows_acl(&retained, None)?;
    require(
        applied == source_acl
            && windows_acl(&victim, None)? == victim_acl
            && windows_acl(&stage_name, None)? == victim_acl
            && fs::read(&victim)? == b"outside bytes"
            && stage.as_file().metadata()?.len() == 0,
        "stage ACL update mutated the substituted outside object",
    )?;
    drop(stage);
    fs::remove_file(stage_name)?;
    fs::remove_file(retained)?;
    Ok(())
}

/// Copying an equal owner does not require otherwise denied `WRITE_OWNER` access.
#[cfg(windows)]
#[test]
fn windows_same_owner_replacement_does_not_require_take_ownership() -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::constants::AccessRights;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let script = windows_fixture_script(
        "$p=$env:PROJECTATLAS_TEST_ACL_PATH; \
        $a=Get-Acl -LiteralPath $p; \
        $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; \
        $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($me,'TakeOwnership','ContainerInherit,ObjectInherit','None','Deny')); \
        [SystemPolicyFixture]::DiscretionaryPolicy($p,$a.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::Access))",
    )?;
    let setup = windows_powershell_fixture(&script, |command| {
        command.env("PROJECTATLAS_TEST_ACL_PATH", &guard.directory);
    })?;
    require(
        setup.status.success(),
        "take-ownership denial fixture failed",
    )?;
    // A creator's initial handle can receive requested rights despite the new
    // file's inherited DACL. Measure the denial on an existing object instead.
    let ownership_control = guard.directory.join("existing-ownership-control");
    fs::write(&ownership_control, b"ownership control")?;
    let excessive = fs::OpenOptions::new()
        .access_mode(
            (AccessRights::GenericRead
                | AccessRights::GenericWrite
                | AccessRights::ReadControl
                | AccessRights::WriteDac
                | AccessRights::WriteOwner)
                .bits(),
        )
        .open(&ownership_control);
    require(
        excessive
            .as_ref()
            .is_err_and(|source| source.raw_os_error() == Some(5)),
        &format!("existing-file fixture did not deny take-ownership access: {excessive:?}"),
    )?;
    let ordinary = fs::OpenOptions::new()
        .access_mode(
            (AccessRights::GenericRead
                | AccessRights::GenericWrite
                | AccessRights::ReadControl
                | AccessRights::WriteDac)
                .bits(),
        )
        .open(&ownership_control)?;
    drop(ordinary);
    let output = guard.directory.join(HOST_FILES[0]);
    fs::write(&output, b"previous output")?;
    let acl = windows_acl(&output, None)?;
    guard.write_generated(&output, b"replacement")?;
    require(
        fs::read(&output)? == b"replacement" && windows_acl(&output, None)? == acl,
        "same-owner regeneration required take-ownership rights or changed policy",
    )
}

#[cfg(windows)]
#[test]
fn readonly_destination_is_preserved_or_natively_refused() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    let outside = root.join("outside-owner");
    fs::write(&output, b"previous readonly output")?;
    fs::hard_link(&output, &outside)?;
    let mut permissions = fs::metadata(&output)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&output, permissions)?;
    let acl = windows_acl(&output, None)?;
    let result = guard.write_generated(&output, b"replacement");
    let replaced = match &result {
        Ok(()) => true,
        Err(source) => {
            require(
                source.kind() == io::ErrorKind::PermissionDenied
                    && native_io_error_code(source) == Some(5),
                &format!(
                    "readonly replacement failed outside the native access boundary: {result:?}"
                ),
            )?;
            false
        }
    };
    require(
        fs::read(&output)?
            == if replaced {
                b"replacement".as_slice()
            } else {
                b"previous readonly output".as_slice()
            }
            && fs::metadata(&output)?.permissions().readonly()
            && windows_acl(&output, None)? == acl
            && fs::read(&outside)? == b"previous readonly output"
            && fs::metadata(&outside)?.permissions().readonly()
            && fs::read_dir(&guard.directory)?.count() == 1,
        &format!("readonly publication changed protected policy or cleanup: {result:?}"),
    )?;
    for path in [&output, &outside] {
        let mut permissions = fs::metadata(path)?.permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "Restore only verified disposable readonly fixture objects"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn failed_readonly_replacement_cleans_only_its_owned_stage() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let output = guard.directory.join(HOST_FILES[0]);
    let outside = root.join("outside-owner");
    fs::write(&output, b"last valid output")?;
    fs::hard_link(&output, &outside)?;
    let mut permissions = fs::metadata(&output)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&output, permissions)?;
    let held = std::cell::RefCell::new(None::<fs::File>);
    let reached = std::cell::Cell::new(false);
    let result = guard.publish_with_stage_check(
        &output,
        b"",
        FilePublication::Replace,
        |stage| {
            *held.borrow_mut() = Some(stage.as_file().try_clone()?);
            Ok(())
        },
        || {
            let borrowed = held.borrow();
            let stage = borrowed
                .as_ref()
                .ok_or_else(|| io::Error::other("missing retained stage"))?;
            require(
                stage.metadata()?.permissions().readonly(),
                "native readonly policy was not applied before the failure",
            )?;
            reached.set(true);
            Err(io::Error::other("controlled readonly publication failure"))
        },
    );
    require(
        reached.get() && result.is_err(),
        "readonly fixture did not reach its controlled failure",
    )?;
    let stage = held
        .into_inner()
        .ok_or_else(|| io::Error::other("missing failed stage"))?;
    // IGNORE_READONLY_ATTRIBUTE deletion on NTFS can remove the old stage's
    // name without clearing its readonly flag. Assert the portable cleanup
    // invariant through the retained object, without claiming an NTFS leak.
    require(
        !stage.metadata()?.permissions().readonly(),
        "failed stage cleanup still depends on POSIX-delete readonly support",
    )?;
    drop(stage);
    require(
        fs::read_dir(&guard.directory)?.count() == 1,
        "failed readonly publication leaked a named stage",
    )?;
    for path in [&output, &outside] {
        require(
            fs::read(path)? == b"last valid output" && fs::metadata(path)?.permissions().readonly(),
            "readonly cleanup changed the destination or outside owner",
        )?;
    }
    // Restore only this fixture's original inode after checking preservation.
    let mut permissions = fs::metadata(&output)?.permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "This Windows fixture restores only its readonly attribute"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&output, permissions)
}

#[cfg(windows)]
#[test]
fn replacement_refuses_changed_windows_destination_identity_and_policy() -> io::Result<()> {
    for change in ["replace", "remove", "DACL", "readonly", "appear"] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
        guard.ensure_directory()?;
        let output = guard.directory.join(HOST_FILES[0]);
        let retired = root.join("retained-destination");
        if change != "appear" {
            fs::write(&output, b"last valid output")?;
        }
        let mut observed = false;
        let mut changed_acl = None;
        let result = guard.publish(&output, b"candidate", FilePublication::Replace, || {
            observed = true;
            match change {
                "replace" => {
                    fs::rename(&output, &retired)?;
                    fs::write(&output, b"racing complete output")
                }
                "remove" => fs::rename(&output, &retired),
                "DACL" => {
                    changed_acl = Some(windows_acl(&output, Some(true))?);
                    Ok(())
                }
                "readonly" => {
                    let mut permissions = fs::metadata(&output)?.permissions();
                    permissions.set_readonly(true);
                    fs::set_permissions(&output, permissions)
                }
                "appear" => fs::write(&output, b"racing complete output"),
                _ => Err(io::Error::other("unknown native destination change")),
            }
        });
        require(
            observed && result.is_err(),
            &format!("changed Windows destination was not refused: {change}, {result:?}"),
        )?;
        if change == "remove" {
            require(!output.exists(), "removed destination was recreated")?;
        } else {
            let expected = if matches!(change, "replace" | "appear") {
                b"racing complete output".as_slice()
            } else {
                b"last valid output".as_slice()
            };
            require(
                fs::read(&output)? == expected,
                "changed native output was overwritten",
            )?;
            if let Some(acl) = changed_acl {
                require(
                    windows_acl(&output, None)? == acl,
                    "changed DACL was overwritten",
                )?;
            }
            if change == "readonly" {
                let mut permissions = fs::metadata(&output)?.permissions();
                require(
                    permissions.readonly(),
                    "changed readonly policy was overwritten",
                )?;
                #[expect(
                    clippy::permissions_set_readonly_false,
                    reason = "This Windows fixture changes only its readonly attribute"
                )]
                permissions.set_readonly(false);
                fs::set_permissions(&output, permissions)?;
            }
        }
        require(
            fs::read_dir(&guard.directory)?.count() == usize::from(change != "remove"),
            "changed native destination leaked staging",
        )?;
        if retired.exists() {
            require(
                fs::read(retired)? == b"last valid output",
                "retained native source changed",
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn staged_file_identity_checks_keep_competing_data_handles_excluded() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let state = root.join(STATE_DIRECTORY);
    let output = state.join(HOST_FILES[0]);
    let mut observed = false;
    guard.publish(&output, b"owned output", FilePublication::Replace, || {
        observed = true;
        let stage = fs::read_dir(&state)?
            .next()
            .ok_or_else(|| io::Error::other("missing owned stage"))??
            .path();
        for read in [true, false] {
            let open = fs::OpenOptions::new().read(read).write(!read).open(&stage);
            require(
                open.as_ref()
                    .err()
                    .is_some_and(|error| error.raw_os_error() == Some(32)),
                &format!("stage allowed competing data access: read={read}, result={open:?}"),
            )?;
        }
        Ok(())
    })?;
    require(
        observed && fs::read(output)? == b"owned output",
        "metadata verification prevented valid publication",
    )
}

#[cfg(windows)]
#[test]
fn replacement_preserves_native_dacl_inheritance_and_outside_owner() -> io::Result<()> {
    for (private, filename, host) in replacement_targets() {
        for protected in [false, true] {
            let temp = tempfile::tempdir()?;
            let root = temp.path().canonicalize()?;
            let directory = if private {
                let directory = root.join(STATE_DIRECTORY);
                fs::create_dir(&directory)?;
                directory
            } else {
                root.clone()
            };
            let output = directory.join(filename);
            fs::write(&output, b"last valid output")?;
            let inherited = windows_acl(&output, None)?;
            let acl = windows_acl(&output, Some(protected))?;
            require(
                acl != inherited,
                "native DACL fixture did not differ from inheritance",
            )?;
            let outside = root.join("outside-owner");
            fs::hard_link(&output, &outside)?;
            let mut guard = if host {
                ProjectStateGuard::preflight(&root, None, None)?
            } else {
                ProjectStateGuard::preflight_document(&root, &output)?
                    .ok_or_else(|| io::Error::other("ACL document was not admitted"))?
            };
            let before_entries = fs::read_dir(&directory)?.count();
            let failed = guard.publish(
                &output,
                b"unpublished bytes",
                FilePublication::Replace,
                || Err(io::Error::other("controlled DACL publication failure")),
            );
            require(
                failed.is_err()
                    && fs::read(&output)? == b"last valid output"
                    && windows_acl(&output, None)? == acl
                    && fs::read_dir(&directory)?.count() == before_entries,
                "DACL failure changed the destination or leaked staging",
            )?;
            if host {
                guard.write_generated(&output, b"owned replacement")?;
            } else {
                guard.write_document(b"owned replacement")?;
            }
            require(
                fs::read(&output)? == b"owned replacement"
                    && windows_file_acl_matches(&acl, &windows_acl(&output, None)?)?,
                &format!("replacement lost native DACL for {filename}, protected={protected}"),
            )?;
            require(
                fs::read(&outside)? == b"last valid output" && windows_acl(&outside, None)? == acl,
                "DACL replacement changed outside owner",
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn replacement_handles_null_and_empty_dacl_without_losing_valid_output() -> io::Result<()> {
    let classify = |path: &Path| -> io::Result<String> {
        // .NET omits D: for NULL descriptors. Its binary form retains the
        // NULL-versus-allocated-empty distinction independently of SDDL text.
        let class = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; $a=Get-Acl -LiteralPath $env:PROJECTATLAS_TEST_ACL_PATH; $r=[System.Security.AccessControl.RawSecurityDescriptor]::new($a.GetSecurityDescriptorBinaryForm(),0); if ($null -eq $r.DiscretionaryAcl) { 'NULL' } elseif ($r.DiscretionaryAcl.Count -eq 0) { if (($r.ControlFlags -band [System.Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -ne 0) { 'EMPTY_PROTECTED' } else { 'EMPTY_INHERITED' } } else { 'NONEMPTY' }",
            ])
            .env("PROJECTATLAS_TEST_ACL_PATH", path)
            .output()?;
        require(
            class.status.success(),
            &format!("native DACL class query failed: {class:?}"),
        )?;
        String::from_utf8(class.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(io::Error::other)
    };
    for sddl in ["D:NO_ACCESS_CONTROL", "D:P", "D:"] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let mut guard = ProjectStateGuard::preflight(&root, None, None)?;
        guard.ensure_directory()?;
        let directory = root.join(STATE_DIRECTORY);
        if sddl == "D:" {
            // Keep directory deletion rights but remove inheritable ACEs, so
            // setting an unprotected empty file DACL stays genuinely empty.
            windows_acl(&directory, Some(true))?;
        }
        let output = directory.join(HOST_FILES[0]);
        fs::write(&output, b"last valid output")?;
        let original = same_file::Handle::from_path(&output)?;
        let script = windows_fixture_script(
            "[SystemPolicyFixture]::DiscretionaryPolicy($env:PROJECTATLAS_TEST_ACL_PATH,$env:PROJECTATLAS_TEST_ACL_SDDL)",
        )?;
        let setup = windows_powershell_fixture(&script, |command| {
            command
                .env("PROJECTATLAS_TEST_ACL_PATH", &output)
                .env("PROJECTATLAS_TEST_ACL_SDDL", sddl);
        })?;
        require(
            setup.status.success(),
            &format!("native special DACL setup failed: {setup:?}"),
        )?;
        let acl = windows_acl(&output, None)?;
        let class = classify(&output)?;
        require(
            class
                == match sddl {
                    "D:NO_ACCESS_CONTROL" => "NULL",
                    "D:P" => "EMPTY_PROTECTED",
                    _ => "EMPTY_INHERITED",
                },
            "native special DACL fixture did not retain its requested access class",
        )?;
        let result = guard.write_generated(&output, b"owned replacement");
        let applied = windows_acl(&output, None)?;
        let access_matches = if result.is_err() {
            applied == acl
        } else {
            match (acl.split_once("D:"), applied.split_once("D:")) {
                (Some((owner, dacl)), Some((applied_owner, applied_dacl))) => {
                    owner == applied_owner
                        && super::windows_file_dacl_matches(
                            std::ffi::OsStr::new(&format!("D:{dacl}")),
                            std::ffi::OsStr::new(&format!("D:{applied_dacl}")),
                        )?
                }
                (None, None) => applied == acl,
                _ => false,
            }
        };
        require(
            access_matches && classify(&output)? == class && fs::read_dir(&directory)?.count() == 1,
            "special DACL publication changed access or leaked staging",
        )?;
        if let Err(source) = &result {
            require(
                matches!(native_io_error_code(source), Some(5 | 1336))
                    || source.kind() == io::ErrorKind::InvalidInput,
                &format!("special DACL failed without a preservation/access cause: {source}"),
            )?;
        }
        // Empty DACLs prohibit fresh data handles. Restore test access only after
        // checking the native ACL, then inspect bytes and the retained old owner.
        windows_acl(&output, Some(true))?;
        require(
            fs::read(&output)?
                == if result.is_ok() {
                    b"owned replacement"
                } else {
                    b"last valid output"
                },
            "special DACL failure lost valid bytes",
        )?;
        if result.is_err() {
            require(
                same_file::Handle::from_path(&output)? == original,
                "special DACL refusal replaced the owner",
            )?;
        }
    }
    Ok(())
}

#[test]
fn file_dacl_readback_allows_only_auto_inherited_flag_normalization() -> io::Result<()> {
    use std::ffi::OsStr;
    let original = "D:(A;ID;FA;;;SY)(D;;GW;;;WD)";
    require(
        super::windows_file_dacl_matches(
            OsStr::new(original),
            OsStr::new("D:AI(A;ID;FA;;;SY)(D;;GW;;;WD)"),
        )?,
        "native AI normalization rejected unchanged regular-file policy",
    )?;
    for changed in [
        "D:P(A;ID;FA;;;SY)(D;;GW;;;WD)",
        "D:AR(A;ID;FA;;;SY)(D;;GW;;;WD)",
        "D:(A;;FA;;;SY)(D;;GW;;;WD)",
        "D:(A;ID;FA;;;WD)(D;;GW;;;WD)",
        "D:(D;;GW;;;WD)(A;ID;FA;;;SY)",
        "D:Z(A;ID;FA;;;SY)(D;;GW;;;WD)",
    ] {
        require(
            !super::windows_file_dacl_matches(OsStr::new(original), OsStr::new(changed))?,
            "DACL readback concealed an access-policy change",
        )?;
    }
    for (expected, observed) in [("D:", "D:NO_ACCESS_CONTROL"), ("", "D:AI")] {
        require(
            !super::windows_file_dacl_matches(OsStr::new(expected), OsStr::new(observed))?,
            "DACL readback conflated NULL, empty or absent ACLs",
        )?;
    }
    require(
        super::windows_file_dacl_matches(OsStr::new("D:AR"), OsStr::new("D:ARAI"))?,
        "AI normalization changed an otherwise preserved inheritance request",
    )
}

#[test]
fn dacl_diagnostics_separate_native_flags_from_ordered_aces_without_identity_text() -> io::Result<()>
{
    use std::ffi::OsStr;
    let expected = OsStr::new("D:(A;ID;FA;;;S-1-5-21-12345)");
    let inherited =
        super::windows_dacl_difference(expected, OsStr::new("D:AI(A;ID;FA;;;S-1-5-21-12345)"))?;
    require(
        inherited.contains("auto_inherited_matches=false")
            && inherited.contains("ordered_aces_match=true")
            && inherited.contains("protection_matches=true")
            && !inherited.contains("S-1-")
            && !inherited.contains("12345"),
        "DACL diagnostics conflated native flags or exposed principal text",
    )?;
    let changed =
        super::windows_dacl_difference(expected, OsStr::new("D:P(D;ID;FA;;;S-1-5-21-12345)"))?;
    require(
        changed.contains("ordered_aces_match=false")
            && changed.contains("protection_matches=false"),
        "DACL diagnostics concealed access-policy changes",
    )?;
    let null = super::windows_dacl_difference(OsStr::new("D:"), OsStr::new("D:NO_ACCESS_CONTROL"))?;
    require(
        null.contains("null_matches=false"),
        "NULL and empty DACL were conflated",
    )
}
