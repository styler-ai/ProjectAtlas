//! Causal ownership, publication and compatibility controls for generated state.

use super::{DATABASE_FILE, FilePublication, HOST_FILES, ProjectStateGuard, STATE_DIRECTORY};
use std::fs;
use std::io;
use std::path::Path;

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
    #[cfg(target_os = "macos")]
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
        name.replace('.', "\u{200d}."),
        name.replace('.', "\u{feff}."),
    ] {
        if variant != name && !variants.contains(&variant) {
            variants.push(variant);
        }
    }
    variants
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
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
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
        let parent_resolves = fs::canonicalize(super::containing_directory(&unresolved)).is_ok();
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
    )
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
            require(
                super::macos_access::get_acl(&held)? == source_acl,
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
                super::macos_access::get_acl(&held)? == source_acl,
                "descriptor ACL read followed the replaced name",
            )?;
            super::macos_access::set_acl(&held, &[])?;
            require(
                super::macos_access::get_acl(&held)?.is_empty()
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
    super::macos_access::set_acl(exposed.as_file(), &[])?;
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
            $a.SetAccessRuleProtection({},$true); Set-Acl -LiteralPath $p -AclObject $a; {READ}",
            if protected { "$true" } else { "$false" },
        )
    } else {
        format!("$ErrorActionPreference='Stop'; {READ}")
    };
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env("PROJECTATLAS_TEST_ACL_PATH", path)
        .output()?;
    require(
        result.status.success(),
        &format!("native ACL query failed: {result:?}"),
    )?;
    String::from_utf8(result.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(io::Error::other)
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
            let query = if snapshot != "absent" {
                Some(super::ReplacementSource::capture(
                    &output,
                    &fs::symlink_metadata(&output)?,
                )?)
            } else {
                None
            };
            let mut query = query;
            let mut stage = tempfile::Builder::new()
                .make_in(&root, |path| super::create_windows_staging_file(path, true))?;
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
    let mut stage = tempfile::Builder::new()
        .make_in(&root, |path| super::create_windows_staging_file(path, true))?;
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

/// Copying an equal owner does not require otherwise denied WRITE_OWNER access.
#[cfg(windows)]
#[test]
fn windows_same_owner_replacement_does_not_require_take_ownership() -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::constants::AccessRights;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let guard = ProjectStateGuard::preflight(&root, None, None)?;
    guard.ensure_directory()?;
    let setup = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; $p=$env:PROJECTATLAS_TEST_ACL_PATH; \
            $a=Get-Acl -LiteralPath $p; \
            $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; \
            $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($me,'TakeOwnership','ContainerInherit,ObjectInherit','None','Deny')); \
            Set-Acl -LiteralPath $p -AclObject $a"])
        .env("PROJECTATLAS_TEST_ACL_PATH", &guard.directory)
        .output()?;
    require(
        setup.status.success(),
        "take-ownership denial fixture failed",
    )?;
    let excessive = fs::OpenOptions::new()
        .access_mode(
            (AccessRights::GenericRead
                | AccessRights::GenericWrite
                | AccessRights::ReadControl
                | AccessRights::WriteDac
                | AccessRights::WriteOwner)
                .bits(),
        )
        .create_new(true)
        .open(guard.directory.join("unneeded-ownership-right"));
    require(
        excessive
            .as_ref()
            .is_err_and(|source| source.raw_os_error() == Some(5)),
        "native fixture did not deny an unnecessary take-ownership request",
    )?;
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
                fs::read(&output)? == b"owned replacement" && windows_acl(&output, None)? == acl,
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
        let setup = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; $p=$env:PROJECTATLAS_TEST_ACL_PATH; $a=Get-Acl -LiteralPath $p; $a.SetSecurityDescriptorSddlForm($env:PROJECTATLAS_TEST_ACL_SDDL); Set-Acl -LiteralPath $p -AclObject $a",
            ])
            .env("PROJECTATLAS_TEST_ACL_PATH", &output)
            .env("PROJECTATLAS_TEST_ACL_SDDL", sddl)
            .output()?;
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
