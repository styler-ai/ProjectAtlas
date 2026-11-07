//! Bounded direct Git configuration inputs for external source exclusions.

use super::{FsError, FsResult, HASH_BUFFER_BYTES, home_directory};
use projectatlas_core::{IndexWorkControl, IndexWorkFailure, IndexWorkResource, IndexWorkStage};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Maximum bytes parsed from one directly selected configuration file.
const MAX_CONFIG_BYTES: u64 = 1_024 * 1_024;
/// Maximum bytes parsed across all directly selected configuration files.
const MAX_CONFIG_TOTAL_BYTES: u64 = 4 * MAX_CONFIG_BYTES;
/// Canonical key without a subsection; other sections cannot authorize exclusions.
const EXCLUDES_KEY: &str = "core.excludesFile";
/// Environment selection of the ordinary global configuration files.
const GLOBAL_CONFIG_ENV: &str = "GIT_CONFIG_GLOBAL";
/// Environment selection of the system configuration file.
const SYSTEM_CONFIG_ENV: &str = "GIT_CONFIG_SYSTEM";
/// Canonical Git boolean disabling system configuration.
const NO_SYSTEM_CONFIG_ENV: &str = "GIT_CONFIG_NOSYSTEM";
/// Environment selection of the XDG configuration directory.
const XDG_CONFIG_ENV: &str = "XDG_CONFIG_HOME";

/// Captured native environment values; construction never changes process state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ConfigEnvironment {
    /// Existing host home-directory context.
    home: Option<PathBuf>,
    /// Nonempty XDG directory, or the home fallback when absent.
    xdg: Option<OsString>,
    /// An explicitly empty value disables the ordinary global config sources.
    global: Option<OsString>,
    /// An explicitly empty value disables the system config source.
    system: Option<OsString>,
    /// Optional canonical Git boolean, independent of mere variable presence.
    no_system: Option<OsString>,
}

impl ConfigEnvironment {
    /// Capture only the supported direct-source selectors.
    pub(super) fn capture() -> Self {
        Self {
            home: home_directory(),
            xdg: std::env::var_os(XDG_CONFIG_ENV).filter(|value| !value.is_empty()),
            global: std::env::var_os(GLOBAL_CONFIG_ENV),
            system: std::env::var_os(SYSTEM_CONFIG_ENV),
            no_system: std::env::var_os(NO_SYSTEM_CONFIG_ENV),
        }
    }
}

/// Exact selected paths, including absent sources needed by the policy witness.
#[derive(Clone, Debug)]
pub(super) struct ExternalGitConfig {
    /// Direct system/global files in increasing precedence.
    pub(super) sources: Vec<PathBuf>,
    /// Effective configured or default excludes file; empty values disable it.
    pub(super) excludes: Option<PathBuf>,
}

impl ExternalGitConfig {
    /// Parse direct sources once, without following includes or repository overrides.
    pub(super) fn discover(root: &Path, control: &IndexWorkControl) -> FsResult<Self> {
        Self::from_environment(root, &ConfigEnvironment::capture(), control)
    }

    /// Retain exactly the bytes supplied to parsing, without another config read.
    pub(super) fn discover_recorded(
        root: &Path,
        control: &IndexWorkControl,
        capture: Option<&super::policy_evidence::PolicyCapture>,
    ) -> FsResult<Self> {
        Self::from_environment_recorded(root, &ConfigEnvironment::capture(), control, capture)
    }

    /// Resolve a captured environment without adding mutable global test state.
    fn from_environment(
        root: &Path,
        environment: &ConfigEnvironment,
        control: &IndexWorkControl,
    ) -> FsResult<Self> {
        Self::from_environment_recorded(root, environment, control, None)
    }

    /// Parse direct sources while optionally recording their consumed contents.
    fn from_environment_recorded(
        root: &Path,
        environment: &ConfigEnvironment,
        control: &IndexWorkControl,
        capture: Option<&super::policy_evidence::PolicyCapture>,
    ) -> FsResult<Self> {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        if let Some(capture) = capture {
            capture
                .environment(environment)
                .map_err(|source| FsError::Io {
                    path: root.to_path_buf(),
                    source,
                })?;
        }
        let xdg = environment
            .xdg
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| environment.home.as_ref().map(|home| home.join(".config")));
        let mut sources = Vec::new();
        let no_system = environment
            .no_system
            .as_ref()
            .map(|value| {
                gix_config::Boolean::try_from(value.clone())
                    .map(bool::from)
                    .map_err(|source| FsError::Io {
                        path: PathBuf::from(NO_SYSTEM_CONFIG_ENV),
                        source: io::Error::other(source),
                    })
            })
            .transpose()?
            .unwrap_or(false);
        if !no_system {
            match environment.system.as_ref() {
                Some(path) if !path.is_empty() => sources.push(selected_path(root, path)),
                Some(_disabled) => {}
                #[cfg(unix)]
                None => sources.push(PathBuf::from("/etc/gitconfig")),
                #[cfg(not(unix))]
                None => {}
            }
        }
        match environment.global.as_ref() {
            Some(path) if !path.is_empty() => sources.push(selected_path(root, path)),
            Some(_disabled) => {}
            None => {
                if let Some(xdg) = &xdg {
                    sources.push(selected_path(root, xdg.join("git/config").as_os_str()));
                }
                if let Some(home) = &environment.home {
                    sources.push(selected_path(root, home.join(".gitconfig").as_os_str()));
                }
            }
        }
        let mut configured = None;
        let mut total = 0_u64;
        for source in &sources {
            let bytes = read_config(source, &mut total, control)?;
            if let Some(capture) = capture {
                capture
                    .config(source, bytes.as_deref())
                    .map_err(|error| FsError::Io {
                        path: source.clone(),
                        source: error,
                    })?;
            }
            let Some(bytes) = bytes else {
                continue;
            };
            let config = gix_config::File::from_bytes_no_includes(
                &bytes,
                gix_config::file::Metadata::default(),
                gix_config::file::init::Options::default(),
            )
            .map_err(|error| FsError::Io {
                path: source.clone(),
                source: io::Error::other(error),
            })?;
            control.check(IndexWorkStage::RepositoryTraversal)?;
            if let Some(value) = config.string_implicit(EXCLUDES_KEY) {
                configured = Some(match value {
                    None => None,
                    Some(value) if value.is_empty() => None,
                    Some(value) => Some(configured_path(
                        root,
                        environment.home.as_deref(),
                        value.as_ref(),
                        source,
                    )?),
                });
            }
        }
        let excludes = configured.unwrap_or_else(|| {
            xdg.map(|xdg| selected_path(root, xdg.join("git/ignore").as_os_str()))
        });
        Ok(Self { sources, excludes })
    }
}

/// Resolve relative operator selections against the exact captured project root.
fn selected_path(root: &Path, value: &std::ffi::OsStr) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Decode native path bytes and the supported leading-home form without lossy text.
fn configured_path(
    root: &Path,
    home: Option<&Path>,
    value: &[u8],
    source: &Path,
) -> FsResult<PathBuf> {
    let (base, value) = if let Some(relative) = value.strip_prefix(b"~/") {
        let home = home.ok_or_else(|| FsError::Io {
            path: source.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "Git excludes home path has no home context",
            ),
        })?;
        (home, relative)
    } else if value.starts_with(b"~") || value.starts_with(b"%(prefix)") {
        return Err(FsError::Io {
            path: source.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "Git excludes path uses unsupported user or installation-prefix interpolation",
            ),
        });
    } else {
        (root, value)
    };
    #[cfg(unix)]
    let native = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(value.to_vec())
    };
    #[cfg(not(unix))]
    let native =
        OsString::from(
            String::from_utf8(value.to_vec()).map_err(|error| FsError::Io {
                path: source.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData, error),
            })?,
        );
    Ok(selected_path(base, &native))
}

/// Read a complete bounded file cooperatively; absence remains an observed input.
fn read_config(
    path: &Path,
    total: &mut u64,
    control: &IndexWorkControl,
) -> FsResult<Option<Vec<u8>>> {
    control.check(IndexWorkStage::RepositoryTraversal)?;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FsError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    require_regular_file(path, &metadata)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A path replaced by a FIFO after metadata inspection must not block open.
        options.custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY);
    }
    let file = options.open(path).map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    require_regular_file(
        path,
        &file.metadata().map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?,
    )?;
    read_config_file(path, file, total, control).map(Some)
}

/// Reject streams and devices that cannot supply a repeatable complete policy file.
pub(super) fn require_regular_file(path: &Path, metadata: &std::fs::Metadata) -> FsResult<()> {
    if !metadata.is_file() {
        return Err(FsError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "external Git policy input is not a regular file",
            ),
        });
    }
    Ok(())
}

/// Consume the already validated regular handle under both declared byte bounds.
fn read_config_file(
    path: &Path,
    file: File,
    total: &mut u64,
    control: &IndexWorkControl,
) -> FsResult<Vec<u8>> {
    let mut file = file.take(MAX_CONFIG_BYTES + 1);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; HASH_BUFFER_BYTES];
    loop {
        control.check(IndexWorkStage::RepositoryTraversal)?;
        let count = file.read(&mut chunk).map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            return Ok(bytes);
        }
        let size = bytes.len() as u64 + count as u64;
        let aggregate = total.saturating_add(count as u64);
        for (observed, limit) in [
            (size, MAX_CONFIG_BYTES),
            (aggregate, MAX_CONFIG_TOTAL_BYTES),
        ] {
            if observed > limit {
                return Err(IndexWorkFailure::resource_limit(
                    IndexWorkStage::RepositoryTraversal,
                    IndexWorkResource::SourceBytes,
                    limit,
                    observed,
                )
                .into());
            }
        }
        *total = aggregate;
        bytes.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::IndexCancellation;
    use std::error::Error;
    use std::fs;

    /// Make isolated native context without changing the process environment.
    fn environment(home: &Path) -> ConfigEnvironment {
        ConfigEnvironment {
            home: Some(home.to_path_buf()),
            system: Some(OsString::new()),
            ..ConfigEnvironment::default()
        }
    }

    /// Return a cooperative operation for deterministic local-file checks.
    fn control() -> IndexWorkControl {
        IndexWorkControl::new(IndexCancellation::new(), None)
    }

    /// Preserve test diagnostics without panicking in a fallible function.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if !condition {
            return Err(io::Error::other(message).into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_regular_configuration_refuses_and_regular_symlinks_remain_usable()
    -> Result<(), Box<dyn Error>> {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let fifo = temp.path().join("config-fifo");
        mkfifo(&fifo, Mode::S_IRUSR | Mode::S_IWUSR)?;
        require(
            matches!(read_config(&fifo, &mut 0, &control()), Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput),
            "non-regular configuration was not refused before open",
        )?;
        require(
            matches!(read_config(temp.path(), &mut 0, &control()), Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput),
            "directory configuration was not refused",
        )?;
        let regular = temp.path().join("regular");
        let contents = b"[core]\nexcludesFile = rules\n";
        fs::write(&regular, contents)?;
        let link = temp.path().join("config-link");
        symlink(&regular, &link)?;
        require(
            read_config(&link, &mut 0, &control())?.as_deref() == Some(contents.as_slice()),
            "regular configuration symlink lost compatibility",
        )?;
        Ok(())
    }

    #[test]
    fn canonical_values_preserve_paths_and_last_key_identity() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        fs::create_dir(&home)?;
        let environment = environment(&home);
        for (config, expected) in [
            (
                "[core]\nexcludesFile = \"rules with spaces\"\n",
                Some(root.join("rules with spaces")),
            ),
            (
                "[core]\nexcludesFile = rules with spaces\n",
                Some(root.join("rules with spaces")),
            ),
            (
                "[core]\nexcludesFile = \" π #; rules \" # comment\n",
                Some(root.join(" π #; rules ")),
            ),
            (
                "[core]\nexcludesFile = foo\\\nbar\n",
                Some(root.join("foobar")),
            ),
            (
                "[CORE]\nEXCLUDESFILE = first\n[core]\nexcludesFile = second\n",
                Some(root.join("second")),
            ),
            (
                "[other]\nexcludesFile = wrong\n[core \"other\"]\nexcludesFile = wrong\n",
                Some(home.join(".config/git/ignore")),
            ),
            ("[core]\nexcludesFile = first\nexcludesFile =\n", None),
            ("[core]\nexcludesFile = first\nexcludesFile\n", None),
            ("[core]\nexcludesFile = ~/rules\n", Some(home.join("rules"))),
            (
                "[core]\nexcludesFile = ../operator/rules\n",
                Some(root.join("../operator/rules")),
            ),
        ] {
            fs::write(home.join(".gitconfig"), config)?;
            let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
            require(
                selected.excludes == expected,
                &format!(
                    "config {config:?}: {:?}, expected {expected:?}",
                    selected.excludes
                ),
            )?;
        }
        Ok(())
    }

    #[test]
    fn absolute_configured_paths_retain_native_parent_components() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let home = root.join("home");
        fs::create_dir(&home)?;
        fs::create_dir(root.join("intermediate"))?;
        fs::write(root.join("rules"), "blocked.rs\n")?;
        #[cfg(windows)]
        let ordinary_root = PathBuf::from(projectatlas_core::lossless_native_path_display(&root)?);
        #[cfg(not(windows))]
        let ordinary_root = root.clone();
        #[cfg(windows)]
        require(
            ordinary_root.is_absolute()
                && matches!(ordinary_root.components().next(), Some(std::path::Component::Prefix(prefix))
                    if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(..))),
            "configured parent fixture requires an ordinary Win32 root",
        )?;
        let mut spelling = ordinary_root.as_os_str().to_os_string();
        spelling.push(std::path::MAIN_SEPARATOR_STR);
        spelling.push(["intermediate", "..", "rules"].join(std::path::MAIN_SEPARATOR_STR));
        let expected = PathBuf::from(spelling);
        let text = expected
            .to_str()
            .ok_or_else(|| io::Error::other("configured fixture path was not UTF-8"))?
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        fs::write(
            home.join(".gitconfig"),
            format!("[core]\nexcludesFile = \"{text}\"\n"),
        )?;
        let selected = ExternalGitConfig::from_environment(&root, &environment(&home), &control())?;
        require(
            selected.excludes.as_deref() == Some(expected.as_path())
                && expected
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
            "absolute configured selection lost native parent components",
        )?;
        let selected = selected
            .excludes
            .ok_or_else(|| io::Error::other("configured fixture did not select rules"))?;
        require(
            fs::read_to_string(selected)? == "blocked.rs\n",
            "configured native parent spelling did not read the selected rules",
        )
    }

    #[test]
    fn direct_sources_apply_system_xdg_home_and_environment_precedence()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        let xdg = root.join("xdg");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(xdg.join("git"))?;
        let system = root.join("system");
        fs::write(&system, "[core]\nexcludesFile=system-rules\n")?;
        fs::write(xdg.join("git/config"), "[core]\nexcludesFile=xdg-rules\n")?;
        fs::write(home.join(".gitconfig"), "[core]\nexcludesFile=home-rules\n")?;
        fs::write(
            root.join("override"),
            "[core]\nexcludesFile=override-rules\n",
        )?;
        let mut environment = environment(&home);
        environment.xdg = Some(xdg.clone().into_os_string());
        environment.system = Some(system.clone().into_os_string());
        let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
        require(
            selected.excludes == Some(root.join("home-rules")),
            "home did not win global precedence",
        )?;
        require(
            selected.sources
                == vec![
                    system.clone(),
                    xdg.join("git/config"),
                    home.join(".gitconfig"),
                ],
            "direct source order or absent input inventory changed",
        )?;
        environment.global = Some(OsString::from("override"));
        let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
        require(
            selected.excludes == Some(root.join("override-rules")),
            "explicit global override did not win",
        )?;
        require(
            selected.sources == vec![system, root.join("override")],
            "global override retained ordinary global files",
        )?;
        environment.global = Some(OsString::new());
        for (value, disabled) in [
            ("", false),
            ("0", false),
            ("false", false),
            ("off", false),
            ("no", false),
            ("1", true),
            ("2", true),
            ("true", true),
            ("yes", true),
        ] {
            environment.no_system = Some(OsString::from(value));
            let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
            let expected = if disabled {
                xdg.join("git/ignore")
            } else {
                root.join("system-rules")
            };
            require(
                selected.excludes == Some(expected),
                &format!("NOSYSTEM {value:?} had incorrect Git boolean semantics"),
            )?;
        }
        environment.no_system = None;
        environment.system = Some(OsString::new());
        require(
            ExternalGitConfig::from_environment(root, &environment, &control())?
                .sources
                .is_empty(),
            "empty direct selectors did not disable their sources",
        )?;
        Ok(())
    }

    #[test]
    fn absent_inputs_and_includes_do_not_invent_loaded_sources() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        fs::create_dir(&home)?;
        let environment = environment(&home);
        let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
        require(
            selected.sources == vec![home.join(".config/git/config"), home.join(".gitconfig")],
            "absent direct inputs were not retained",
        )?;
        fs::write(home.join("included"), "[core]\nexcludesFile=unread-rules\n")?;
        fs::write(
            home.join(".gitconfig"),
            "[include]\npath=included\n[includeIf \"gitdir:**\"]\npath=included\n",
        )?;
        let selected = ExternalGitConfig::from_environment(root, &environment, &control())?;
        require(
            selected.excludes == Some(home.join(".config/git/ignore")),
            "an unread include supplied configuration authority",
        )?;
        require(
            !selected.sources.contains(&home.join("included")),
            "an unsupported include was followed",
        )?;
        Ok(())
    }

    #[test]
    fn configuration_limits_cancellation_and_malformed_input_refuse() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let file = root.join("config");
        fs::write(&file, vec![b'#'; MAX_CONFIG_BYTES as usize + 1])?;
        require(
            matches!(
                read_config(&file, &mut 0, &control()),
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded { .. }
                ))
            ),
            "oversized config was not refused",
        )?;
        fs::write(&file, b"# bounded\n")?;
        let mut total = MAX_CONFIG_TOTAL_BYTES;
        require(
            matches!(
                read_config(&file, &mut total, &control()),
                Err(FsError::IndexWork(
                    IndexWorkFailure::ResourceLimitExceeded { .. }
                ))
            ),
            "aggregate config bytes were not refused",
        )?;
        let cancelled = control();
        cancelled.cancel();
        require(
            matches!(
                read_config(&file, &mut 0, &cancelled),
                Err(FsError::IndexWork(IndexWorkFailure::Cancelled { .. }))
            ),
            "cancelled config read was not refused",
        )?;
        let mut environment = environment(root);
        environment.global = Some(file.clone().into_os_string());
        fs::write(&file, b"[core\nexcludesFile=wrong\n")?;
        require(
            ExternalGitConfig::from_environment(root, &environment, &control()).is_err(),
            "malformed configuration supplied a path",
        )?;
        for unsupported in ["~another/rules", "%(prefix)/rules"] {
            fs::write(&file, format!("[core]\nexcludesFile={unsupported}\n"))?;
            require(
                ExternalGitConfig::from_environment(root, &environment, &control()).is_err(),
                "unsupported interpolation was implicitly followed",
            )?;
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn configured_unix_paths_preserve_non_utf8_bytes() -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStrExt;
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("config");
        let path = configured_path(temp.path(), None, b"native-\xff-ignore", &source)?;
        require(
            path.file_name()
                .is_some_and(|name| name.as_bytes() == b"native-\xff-ignore"),
            "native Unix path was converted through lossy text",
        )?;
        Ok(())
    }
}
