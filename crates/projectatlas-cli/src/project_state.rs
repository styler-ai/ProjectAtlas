//! Admission and publication of automatic project-owned initialization files.

use same_file::Handle;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Product-owned directory, independent of explicitly selected external storage.
const STATE_DIRECTORY: &str = ".projectatlas";
/// Automatic SQLite filename whose aliases must be checked before opening SQLite.
const DATABASE_FILE: &str = "projectatlas.db";
/// Default config whose creation remains an automatic project output.
const CONFIG_FILE: &str = "config.toml";
/// Default nonsource metadata created by every ordinary initializer.
const NONSOURCE_FILE: &str = "projectatlas-nonsource-files.toon";
/// Automatically generated host outputs, all preflighted before identity changes.
const HOST_FILES: [&str; 3] = [
    "projectatlas.mcp.json",
    "projectatlas.claude.mcp.json",
    "projectatlas.opencode.json",
];

/// The two sidecars opened by the supported SQLite WAL profile.
const WAL_SIDECAR_SUFFIXES: [&str; 3] = ["", "-wal", "-shm"];

/// Whether publication creates defaults or replaces an owned generated document.
#[derive(Clone, Copy)]
enum FilePublication {
    /// Preserve a default another initializer has already published.
    Create,
    /// Replace the directory entry without modifying a hard-linked target.
    Replace,
}

/// Preserve both the operation and ownership failure when cleanup is unsafe.
#[derive(Debug, thiserror::Error)]
#[error("{operation}; generated-file cleanup was withheld because {ownership}")]
struct RetainedStaging {
    /// Original write or publication failure.
    #[source]
    operation: io::Error,
    /// Why the old temporary pathname can no longer be safely unlinked.
    ownership: io::Error,
}

/// Keep database admission independent of optional generated host documents.
#[derive(Clone, Copy)]
enum StateAdmission {
    /// Inspect only the managed directory and SQLite database/sidecars.
    Database,
    /// Also admit bootstrap and generated-host destinations.
    Generated,
}

/// Retain the admitted root and existing state-directory objects during writes.
///
/// These checks are repeated at mutation boundaries. They do not lock arbitrary
/// ancestors against a hostile concurrent filesystem actor.
pub(crate) struct ProjectStateGuard {
    /// Canonical selected source root; source-root aliases are resolved by callers.
    root: PathBuf,
    /// Retained root object, preventing reuse of its old filesystem identity.
    root_identity: Handle,
    /// Product-owned direct directory under the selected root.
    directory: PathBuf,
    /// Existing state-directory object, when one was present at admission.
    directory_identity: Option<Handle>,
    /// Selected automatic database; external operator paths remain independent.
    automatic_database: Option<PathBuf>,
    /// Selected default configuration; explicit external paths are not inferred.
    automatic_config: Option<PathBuf>,
    /// The concrete operation boundary, never a process-global routing setting.
    admission: StateAdmission,
}

impl ProjectStateGuard {
    /// Inspect automatic destinations without creating files or SQLite sidecars.
    pub(crate) fn preflight(
        root: &Path,
        database: Option<&Path>,
        config: Option<&Path>,
    ) -> io::Result<Self> {
        Self::admit(root, database, config, StateAdmission::Generated)
    }

    /// Admit an exact managed database before even reading its stored root.
    ///
    /// Arbitrary external DB names/locations remain operator-selected, including
    /// conventional-looking names under a different root. Resolve only the known
    /// selected root; resolving `.projectatlas` here would trust its alias.
    pub(crate) fn preflight_database_path(
        database: &Path,
        selected_root: &Path,
    ) -> io::Result<Option<Self>> {
        let root = fs::canonicalize(selected_root)?;
        if !owned_named_path(database, &root.join(STATE_DIRECTORY), DATABASE_FILE) {
            return Ok(None);
        }
        Self::admit(&root, Some(database), None, StateAdmission::Database).map(Some)
    }

    /// Capture the exact filesystem owner for one operation role.
    fn admit(
        root: &Path,
        database: Option<&Path>,
        config: Option<&Path>,
        admission: StateAdmission,
    ) -> io::Result<Self> {
        let canonical = fs::canonicalize(root)?;
        if canonical != root || indirect(&fs::symlink_metadata(root)?) {
            return Err(refusal(root, "selected canonical root changed"));
        }
        let root_identity = Handle::from_path(root)?;
        let directory = root.join(STATE_DIRECTORY);
        let directory_identity = match optional_metadata(&directory)? {
            Some(metadata) => {
                require_directory(&directory, &metadata)?;
                Some(Handle::from_path(&directory)?)
            }
            None => None,
        };
        let automatic_database = database
            .filter(|path| owned_named_path(path, &directory, DATABASE_FILE))
            .map(Path::to_path_buf);
        let automatic_config = config
            .filter(|path| owned_named_path(path, &directory, CONFIG_FILE))
            .map(Path::to_path_buf);
        let guard = Self {
            root: root.to_path_buf(),
            root_identity,
            directory,
            directory_identity,
            automatic_database,
            automatic_config,
            admission,
        };
        guard.verify()?;
        Ok(guard)
    }

    /// Recheck the admitted objects and every automatic host/database output.
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.verify_directories()?;
        if matches!(self.admission, StateAdmission::Generated) {
            for name in HOST_FILES {
                require_regular_or_absent(&self.directory.join(name))?;
            }
            let nonsource = self.directory.join(NONSOURCE_FILE);
            if !nonsource.exists() {
                require_regular_or_absent(&nonsource)?;
            }
            if let Some(config) = &self.automatic_config
                && !config.exists()
            {
                require_regular_or_absent(config)?;
            }
        }
        if let Some(database) = &self.automatic_database {
            for suffix in WAL_SIDECAR_SUFFIXES {
                let mut name = database.as_os_str().to_os_string();
                name.push(suffix);
                let path = PathBuf::from(name);
                if let Some(metadata) = optional_metadata(&path)? {
                    require_regular(&path, &metadata)?;
                    #[cfg(unix)]
                    let links = link_count(&path, &metadata);
                    #[cfg(not(unix))]
                    let links = link_count(&path, &metadata)?;
                    match links {
                        1 => {}
                        0 => {
                            return Err(refusal(
                                &path,
                                "automatic database link identity is unavailable",
                            ));
                        }
                        _ => {
                            return Err(refusal(
                                &path,
                                "automatic database or WAL sidecar has multiple hard links",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Check only retained directory objects, including before staging cleanup.
    fn verify_directories(&self) -> io::Result<()> {
        require_directory(&self.root, &fs::symlink_metadata(&self.root)?)?;
        if fs::canonicalize(&self.root)? != self.root
            || Handle::from_path(&self.root)? != self.root_identity
        {
            return Err(refusal(&self.root, "selected root object changed"));
        }
        match optional_metadata(&self.directory)? {
            Some(metadata) => {
                require_directory(&self.directory, &metadata)?;
                if let Some(identity) = &self.directory_identity
                    && Handle::from_path(&self.directory)? != *identity
                {
                    return Err(refusal(&self.directory, "state directory object changed"));
                }
            }
            None if self.directory_identity.is_some() => {
                return Err(refusal(&self.directory, "state directory disappeared"));
            }
            None => {}
        }
        Ok(())
    }

    /// Create only the admitted direct state directory and retain its object.
    pub(crate) fn ensure_directory(&mut self) -> io::Result<()> {
        if !matches!(self.admission, StateAdmission::Generated) {
            return Err(refusal(
                &self.directory,
                "database admission cannot create generated state",
            ));
        }
        self.verify()?;
        if self.directory_identity.is_none() {
            match fs::create_dir(&self.directory) {
                Ok(()) => {}
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(source),
            }
            require_directory(&self.directory, &fs::symlink_metadata(&self.directory)?)?;
            self.directory_identity = Some(Handle::from_path(&self.directory)?);
        }
        self.verify()
    }

    /// Publish a generated entry without writing through an existing file alias.
    pub(crate) fn write_generated(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.publish(path, bytes, FilePublication::Replace, || Ok(()))
    }

    /// Publish defaults without exposing partial bytes or replacing existing data.
    pub(crate) fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.publish(path, bytes, FilePublication::Create, || Ok(()))
    }

    /// Compare actual parent owners while preserving selected path spelling.
    pub(crate) fn owns_entry(&self, path: &Path) -> io::Result<bool> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Ok(fs::canonicalize(parent)? == fs::canonicalize(&self.directory)?)
    }

    /// Share atomic publication and a scoped failure boundary across file roles.
    fn publish(
        &self,
        path: &Path,
        bytes: &[u8],
        publication: FilePublication,
        before_publication: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        if !matches!(self.admission, StateAdmission::Generated) {
            return Err(refusal(
                path,
                "database admission cannot publish generated files",
            ));
        }
        if !self.owns_entry(path)? {
            return Err(refusal(path, "generated output is outside its state owner"));
        }
        self.verify()?;
        let permissions = optional_metadata(path)?
            .map(|metadata| {
                require_regular(path, &metadata)?;
                Ok::<_, io::Error>(metadata.permissions())
            })
            .transpose()?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        let prepared = (|| {
            temporary.write_all(bytes)?;
            if let Some(permissions) = permissions {
                temporary.as_file().set_permissions(permissions)?;
            }
            temporary.as_file().sync_all()?;
            before_publication()?;
            self.verify()
        })();
        if let Err(source) = prepared {
            return Err(self.staging_failure(&mut temporary, source));
        }
        let result = match publication {
            FilePublication::Create => temporary.persist_noclobber(path),
            FilePublication::Replace => temporary.persist(path),
        };
        match result {
            Ok(_) => Ok(()),
            Err(mut failure) => {
                if matches!(publication, FilePublication::Create)
                    && failure.error.kind() == io::ErrorKind::AlreadyExists
                {
                    let existing = self.verify().and_then(|()| optional_metadata(path));
                    match existing {
                        Ok(Some(metadata)) if !indirect(&metadata) && metadata.is_file() => {
                            return Ok(());
                        }
                        Err(source) => return Err(self.staging_failure(&mut failure.file, source)),
                        Ok(_) => {}
                    }
                }
                Err(self.staging_failure(&mut failure.file, failure.error))
            }
        }
    }

    /// Never unlink a temporary pathname through a changed directory owner.
    fn staging_failure(
        &self,
        temporary: &mut tempfile::NamedTempFile,
        operation: io::Error,
    ) -> io::Error {
        match self.verify_directories() {
            Ok(()) => operation,
            Err(ownership) => {
                // keep() itself performs path I/O on Windows. Only disable cleanup:
                // the original private staging file survives under its old owner.
                temporary.disable_cleanup(true);
                io::Error::new(
                    operation.kind(),
                    RetainedStaging {
                        operation,
                        ownership,
                    },
                )
            }
        }
    }
}

/// Recognize the selected default entry without restricting external DB choices.
fn owned_named_path(path: &Path, directory: &Path, expected_name: &str) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if !named(path.file_name(), expected_name) || !named(parent.file_name(), STATE_DIRECTORY) {
        return false;
    }
    let Some(root) = directory.parent() else {
        return false;
    };
    let prefix = parent
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent == directory || fs::canonicalize(prefix).is_ok_and(|resolved| resolved == root)
}

/// Match fixed product components using the supported platform's case behavior.
fn named(name: Option<&std::ffi::OsStr>, expected: &str) -> bool {
    let Some(name) = name.and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case(expected)
    }
    #[cfg(not(windows))]
    {
        name == expected
    }
}

/// Inspect a direct entry, preserving real I/O errors and dangling aliases.
fn optional_metadata(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source),
    }
}

/// Reject symbolic links and Windows junction/reparse entries.
fn indirect(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Admit a direct directory, matching the existing project-state contract.
fn require_directory(path: &Path, metadata: &Metadata) -> io::Result<()> {
    if indirect(metadata) || !metadata.is_dir() {
        return Err(refusal(path, "automatic state requires a direct directory"));
    }
    Ok(())
}

/// Admit a regular non-indirect output entry.
fn require_regular(path: &Path, metadata: &Metadata) -> io::Result<()> {
    if indirect(metadata) || !metadata.is_file() {
        return Err(refusal(
            path,
            "automatic output requires a direct regular file",
        ));
    }
    Ok(())
}

/// Admit a fresh output or its existing regular entry before publication.
fn require_regular_or_absent(path: &Path) -> io::Result<()> {
    if let Some(metadata) = optional_metadata(path)? {
        require_regular(path, &metadata)?;
    }
    Ok(())
}

/// Return the platform's hard-link count before any automatic SQLite open.
#[cfg(unix)]
fn link_count(_path: &Path, metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

/// Use the existing canonical Windows utility rather than unstable metadata APIs.
#[cfg(windows)]
fn link_count(path: &Path, _metadata: &Metadata) -> io::Result<u64> {
    let file = fs::File::open(path)?;
    Ok(winapi_util::file::information(&file)?.number_of_links())
}

/// Preserve a typed capability refusal on unsupported runtime platforms.
#[cfg(not(any(unix, windows)))]
fn link_count(path: &Path, _metadata: &Metadata) -> io::Result<u64> {
    Err(refusal(
        path,
        "automatic database link identity is unavailable",
    ))
}

/// Keep the owning path and actual refusal cause in the filesystem error chain.
fn refusal(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{}: {reason}", path.display()),
    )
}

#[cfg(test)]
#[path = "project_state_tests.rs"]
mod tests;
