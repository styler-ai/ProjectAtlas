//! Admission and publication of automatic project-owned initialization files.

use same_file::Handle;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(target_os = "linux")]
#[path = "project_state_linux.rs"]
mod linux_access;

#[cfg(target_os = "macos")]
#[path = "project_state_macos.rs"]
mod macos_access;

#[cfg(target_os = "macos")]
#[path = "project_state_macos_acl.rs"]
mod macos_acl;

#[cfg(windows)]
#[path = "project_state_windows_security.rs"]
mod windows_security;

#[cfg(windows)]
#[path = "project_state_windows.rs"]
mod windows_filesystem;

/// Product-owned directory, independent of explicitly selected external storage.
const STATE_DIRECTORY: &str = ".projectatlas";
/// Automatic SQLite filename whose aliases must be checked before opening SQLite.
const DATABASE_FILE: &str = "projectatlas.db";
/// Default config whose creation remains an automatic project output.
const CONFIG_FILE: &str = "config.toml";
/// Legacy root configuration selected by the existing discovery fallback.
const LEGACY_CONFIG_FILE: &str = "projectatlas.toml";
/// Optional sidecar next to the default legacy map import location.
const JSON_MAP_FILE: &str = "projectatlas.json";
/// Git's fixed root-level exclusion document.
const GITIGNORE_FILE: &str = ".gitignore";
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

/// Preserve the operation and failed cleanup precondition without an unsafe unlink.
#[derive(Debug, thiserror::Error)]
#[error("{operation}; generated-file cleanup was withheld because {cleanup}")]
struct RetainedStaging {
    /// Original write or publication failure.
    #[source]
    operation: io::Error,
    /// Ownership or native metadata prevented safe cleanup of the old temporary name.
    cleanup: io::Error,
}

/// Keep a failed publication step and its native I/O cause together.
#[derive(Debug, thiserror::Error)]
#[error("{action}: {source}")]
struct PublicationFailure {
    /// Human-readable operation whose native cause remains available below.
    action: &'static str,
    /// Preserve the original native error and its code in the causal chain.
    #[source]
    source: io::Error,
}

/// Add causal operation context without changing the I/O failure category.
fn publication_failure(action: &'static str, source: io::Error) -> io::Error {
    io::Error::new(source.kind(), PublicationFailure { action, source })
}

/// Keep database admission independent of optional generated host documents.
#[derive(Clone, Copy)]
enum StateAdmission {
    /// Inspect only the managed directory and SQLite database/sidecars.
    Database,
    /// Inventory/unlink owned database entries without opening SQLite.
    Reset,
    /// Also admit bootstrap and generated-host destinations.
    Generated,
    /// Mutate only one exact managed document, independently of other outputs.
    Document(ProjectDocument),
}

/// Fixed project destinations whose implicit writers must not follow aliases.
#[derive(Clone, Copy)]
enum ProjectDocument {
    /// Configuration in the direct private state directory.
    Config,
    /// Legacy root-level configuration.
    LegacyConfig,
    /// Explicitly requested default JSON map sidecar.
    JsonMap,
    /// Create Git exclusions only when no directory entry already exists.
    Gitignore,
}

impl ProjectDocument {
    /// Return the fixed filename owned by this document kind.
    const fn filename(self) -> &'static str {
        match self {
            Self::Config => CONFIG_FILE,
            Self::LegacyConfig => LEGACY_CONFIG_FILE,
            Self::JsonMap => JSON_MAP_FILE,
            Self::Gitignore => GITIGNORE_FILE,
        }
    }

    /// Select the direct directory without following the private namespace.
    fn directory(self, root: &Path) -> PathBuf {
        match self {
            Self::Config | Self::JsonMap => root.join(STATE_DIRECTORY),
            Self::LegacyConfig | Self::Gitignore => root.to_path_buf(),
        }
    }
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
    /// Admitted publication directory: the root or its direct private directory.
    directory: PathBuf,
    /// Existing state-directory object, when one was present at admission.
    directory_identity: Option<Handle>,
    /// Selected automatic database; external operator paths remain independent.
    automatic_database: Option<PathBuf>,
    /// Exact automatic document; explicit custom/external paths are not inferred.
    automatic_document: Option<PathBuf>,
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
        if !owned_named_path(database, &root.join(STATE_DIRECTORY), DATABASE_FILE)? {
            return Ok(None);
        }
        Self::admit(&root, Some(database), None, StateAdmission::Database).map(Some)
    }

    /// Admit explicit reset without reading SQLite or refusing recoverable cohorts.
    pub(crate) fn preflight_database_reset_path(
        database: &Path,
        selected_root: &Path,
    ) -> io::Result<Option<Self>> {
        let root = fs::canonicalize(selected_root)?;
        if !owned_named_path(database, &root.join(STATE_DIRECTORY), DATABASE_FILE)? {
            return Ok(None);
        }
        Self::admit(&root, Some(database), None, StateAdmission::Reset).map(Some)
    }

    /// Admit one fixed document while preserving custom/external operator paths.
    pub(crate) fn preflight_document(root: &Path, path: &Path) -> io::Result<Option<Self>> {
        let root = fs::canonicalize(root)?;
        for document in [
            ProjectDocument::Config,
            ProjectDocument::LegacyConfig,
            ProjectDocument::JsonMap,
            ProjectDocument::Gitignore,
        ] {
            if !fixed_name_candidate(path, document.filename())? {
                continue;
            }
            let directory = document.directory(&root);
            let owned = owned_named_path(path, &directory, document.filename())?;
            if owned {
                let target = directory.join(document.filename());
                return Self::admit(
                    &root,
                    None,
                    Some(&target),
                    StateAdmission::Document(document),
                )
                .map(Some);
            }
        }
        Ok(None)
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
        let directory = match admission {
            StateAdmission::Document(document) => document.directory(root),
            StateAdmission::Database | StateAdmission::Generated | StateAdmission::Reset => {
                root.join(STATE_DIRECTORY)
            }
        };
        let directory_identity = match optional_metadata(&directory)? {
            Some(metadata) => {
                require_directory(&directory, &metadata)?;
                Some(Handle::from_path(&directory)?)
            }
            None => None,
        };
        let automatic_database = match database {
            Some(path) if owned_named_path(path, &directory, DATABASE_FILE)? => {
                #[cfg(windows)]
                {
                    Some(directory.join(DATABASE_FILE))
                }
                #[cfg(not(windows))]
                Some(path.to_path_buf())
            }
            _ => None,
        };
        let automatic_document = match admission {
            StateAdmission::Reset => None,
            StateAdmission::Document(_) => config.map(Path::to_path_buf),
            StateAdmission::Database | StateAdmission::Generated => match config {
                Some(path)
                    if owned_named_path(path, &directory, CONFIG_FILE)?
                        || owned_named_path(path, root, LEGACY_CONFIG_FILE)? =>
                {
                    Some(path.to_path_buf())
                }
                _ => None,
            },
        };
        let guard = Self {
            root: root.to_path_buf(),
            root_identity,
            directory,
            directory_identity,
            automatic_database,
            automatic_document,
            admission,
        };
        guard.verify()?;
        Ok(guard)
    }

    /// Return the admitted SQLite spelling; Windows managed aliases share one cohort.
    pub(crate) fn sqlite_path(&self) -> Option<&Path> {
        self.automatic_database.as_deref()
    }

    /// Enumerate only proven alternate recovery entries for an explicit reset.
    pub(crate) fn alternate_reset_paths(&self) -> io::Result<Vec<PathBuf>> {
        self.verify_directories()?;
        #[cfg(windows)]
        if let Some(database) = &self.automatic_database
            && let Some(short) = windows_filesystem::alternate_name(database)?
        {
            let alternate = self.directory.join(short);
            return Ok(["-wal", "-shm", "-journal"]
                .into_iter()
                .map(|suffix| {
                    let mut name = alternate.as_os_str().to_os_string();
                    name.push(suffix);
                    PathBuf::from(name)
                })
                .collect());
        }
        Ok(Vec::new())
    }

    /// Recheck the admitted objects and every automatic host/database output.
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.verify_directories()?;
        // Reset only unlinks/moves selected entries. Preserve recovery of corrupt
        // databases and leaf aliases without following them through SQLite.
        if matches!(self.admission, StateAdmission::Reset) {
            return Ok(());
        }
        if matches!(self.admission, StateAdmission::Generated) {
            for name in HOST_FILES {
                require_regular_or_absent(&self.directory.join(name))?;
            }
            let nonsource = self.directory.join(NONSOURCE_FILE);
            if !nonsource.exists() {
                require_regular_or_absent(&nonsource)?;
            }
            if let Some(config) = &self.automatic_document
                && !config.exists()
            {
                require_regular_or_absent(config)?;
            }
        }
        if let StateAdmission::Document(document) = self.admission
            && !matches!(document, ProjectDocument::Gitignore)
            && let Some(path) = &self.automatic_document
        {
            require_regular_or_absent(path)?;
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
            #[cfg(windows)]
            if let Some(short) = windows_filesystem::alternate_name(database)? {
                let alternate = self.directory.join(short);
                for suffix in ["-wal", "-shm", "-journal"] {
                    let mut name = alternate.as_os_str().to_os_string();
                    name.push(suffix);
                    let path = PathBuf::from(name);
                    if optional_metadata(&path)?.is_some() {
                        return Err(refusal(
                            &path,
                            "alternate 8.3 SQLite recovery cohort exists; close its users and use external SQLite recovery/checkpoint tooling with that original short spelling before reopening through ProjectAtlas; no cohort is retargeted or removed",
                        ));
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
        if matches!(
            self.admission,
            StateAdmission::Database | StateAdmission::Reset
        ) {
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
            .map(|_| ())
    }

    /// Publish defaults without exposing partial bytes or replacing existing data.
    pub(crate) fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.publish(path, bytes, FilePublication::Create, || Ok(()))
            .map(|_| ())
    }

    /// Replace only the exact admitted document, preserving existing permissions.
    pub(crate) fn write_document(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.ensure_directory()?;
        let path = self.document_path()?;
        self.publish(path, bytes, FilePublication::Replace, || Ok(()))
            .map(|_| ())
    }

    /// Atomically create an admitted document, reporting a racing existing entry.
    pub(crate) fn create_document(&mut self, bytes: &[u8]) -> io::Result<bool> {
        self.ensure_directory()?;
        self.publish(
            self.document_path()?,
            bytes,
            FilePublication::Create,
            || Ok(()),
        )
    }

    /// Keep document publication bound to its constructor-selected target.
    fn document_path(&self) -> io::Result<&Path> {
        if !matches!(self.admission, StateAdmission::Document(_)) {
            return Err(refusal(&self.root, "document admission is required"));
        }
        self.automatic_document
            .as_deref()
            .ok_or_else(|| refusal(&self.root, "document target is unavailable"))
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
    ) -> io::Result<bool> {
        self.publish_with_stage_check(path, bytes, publication, |_| Ok(()), before_publication)
    }

    /// Observe the real protected stage through a local causal-test seam.
    fn publish_with_stage_check(
        &self,
        path: &Path,
        bytes: &[u8],
        publication: FilePublication,
        stage_check: impl FnOnce(&tempfile::NamedTempFile) -> io::Result<()>,
        before_publication: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<bool> {
        if matches!(self.admission, StateAdmission::Database) {
            return Err(refusal(
                path,
                "database admission cannot publish generated files",
            ));
        }
        if matches!(self.admission, StateAdmission::Document(_)) && path != self.document_path()? {
            return Err(refusal(
                path,
                "document target differs from its admitted entry",
            ));
        }
        if !self.owns_entry(path)? {
            return Err(refusal(path, "generated output is outside its state owner"));
        }
        self.verify()?;
        let gitignore_creation = matches!(
            self.admission,
            StateAdmission::Document(ProjectDocument::Gitignore)
        ) && matches!(publication, FilePublication::Create);
        if gitignore_creation && optional_metadata(path)?.is_some() {
            return Ok(false);
        }
        let metadata = if gitignore_creation {
            None
        } else {
            optional_metadata(path)?
        };
        let metadata = metadata
            .map(|metadata| {
                require_regular(path, &metadata)?;
                Ok::<_, io::Error>(metadata)
            })
            .transpose()?;
        // A no-clobber collision never replaces the existing policy. Validate
        // its direct regular entry above without requiring copy/crypto rights.
        if matches!(publication, FilePublication::Create) && metadata.is_some() {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        let access = metadata
            .as_ref()
            .map(|metadata| linux_access::ReplacementPolicy::capture(path, metadata))
            .transpose()?;
        #[cfg(any(target_os = "macos", windows))]
        let access = metadata
            .as_ref()
            .map(|metadata| ReplacementSource::capture(path, metadata))
            .transpose()
            .map_err(|source| publication_failure("capture destination access policy", source))?;
        #[cfg(target_os = "macos")]
        let private = macos_access::ReplacementDirectory::create(self, metadata.is_some())?;
        #[cfg(target_os = "macos")]
        let staging_directory = private
            .as_ref()
            .map_or(self.directory.as_path(), |private| private.path());
        #[cfg(not(target_os = "macos"))]
        let staging_directory = self.directory.as_path();
        let result = (|| {
            let staging_identity = Handle::from_path(staging_directory)?;
            #[cfg(windows)]
            let creation = access
                .as_ref()
                .map(|access| {
                    windows_security::ReplacementCreation::capture(
                        &access.file,
                        staging_directory,
                        &staging_identity,
                        &access.security,
                    )
                })
                .transpose()?;
            #[cfg(target_os = "macos")]
            if let Some(private) = &private {
                private.verify()?;
            }
            #[cfg(unix)]
            let mut builder = tempfile::Builder::new();
            #[cfg(not(unix))]
            let builder = tempfile::Builder::new();
            #[cfg(unix)]
            if metadata.is_none() {
                use std::os::unix::fs::PermissionsExt;
                builder.permissions(fs::Permissions::from_mode(0o666));
            }
            #[cfg(target_os = "linux")]
            if let Some(access) = &access {
                builder.permissions(access.permissions());
            }
            #[cfg(not(windows))]
            let mut temporary = builder.tempfile_in(staging_directory)?;
            #[cfg(windows)]
            let mut temporary = builder.make_in(staging_directory, |path| {
                create_windows_staging_file(path, creation.as_ref())
            })?;
            #[cfg(target_os = "linux")]
            let content_prepared = if let Some(access) = &access {
                let equivalent = match self
                    .verify_staging_file(&temporary, &staging_identity)
                    .and_then(|()| access.matches(temporary.as_file()))
                {
                    Ok(equivalent) => equivalent,
                    Err(source) => {
                        return Err(self.staging_failure(
                            &mut temporary,
                            source,
                            &staging_identity,
                        ));
                    }
                };
                if equivalent {
                    false
                } else {
                    if let Err(source) = self.verify_staging_file(&temporary, &staging_identity) {
                        return Err(self.staging_failure(
                            &mut temporary,
                            source,
                            &staging_identity,
                        ));
                    }
                    // A mismatched named probe never receives generated bytes.
                    self.discard_staging_probe(temporary, &staging_identity)?;
                    temporary = access.anonymous_stage(&builder, self, bytes)?;
                    true
                }
            } else {
                false
            };
            #[cfg(not(target_os = "linux"))]
            let content_prepared = false;
            let prepared = (|| {
                self.verify_staging_file(&temporary, &staging_identity)?;
                #[cfg(windows)]
                if let Some(creation) = &creation {
                    creation.verify_initial(temporary.as_file())?;
                }
                stage_check(&temporary)?;
                self.verify_staging_file(&temporary, &staging_identity)?;
                #[cfg(any(target_os = "macos", windows))]
                if let Some(access) = &access {
                    access.verify_destination(path).map_err(|source| {
                        publication_failure("recheck destination before policy copy", source)
                    })?;
                    #[cfg(target_os = "macos")]
                    let prepared = access.apply(path, &temporary);
                    #[cfg(windows)]
                    let prepared = access.apply(
                        path,
                        &temporary,
                        creation.as_ref().ok_or_else(|| {
                            io::Error::other("replacement creation policy is absent")
                        })?,
                    );
                    prepared.map_err(|source| {
                        publication_failure("prepare staged native access policy", source)
                    })?;
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
                if let Some(metadata) = &metadata {
                    preserve_replacement_access(path, metadata, &temporary)?;
                }
                self.verify_staging_file(&temporary, &staging_identity)?;
                if !content_prepared {
                    temporary
                        .write_all(bytes)
                        .map_err(|source| publication_failure("write staged payload", source))?;
                    temporary
                        .as_file()
                        .sync_all()
                        .map_err(|source| publication_failure("sync staged payload", source))?;
                }
                before_publication()?;
                self.verify().map_err(|source| {
                    publication_failure("recheck project state before publication", source)
                })?;
                self.verify_staging_file(&temporary, &staging_identity)
                    .map_err(|source| {
                        publication_failure("recheck staged file before publication", source)
                    })?;
                #[cfg(windows)]
                if let Some(access) = &access {
                    access.security.system.verify(temporary.as_file())?;
                }
                #[cfg(target_os = "linux")]
                if let Some(access) = &access
                    && !access.matches(temporary.as_file())?
                {
                    return Err(refusal(
                        path,
                        "staged native access policy changed before publication",
                    ));
                }
                if matches!(publication, FilePublication::Replace) {
                    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
                    if let Some(access) = &access {
                        access.verify_destination(path).map_err(|source| {
                            publication_failure("recheck destination before replacement", source)
                        })?;
                    }
                    if metadata.is_none() && optional_metadata(path)?.is_some() {
                        return Err(refusal(
                            path,
                            "absent destination appeared before publication",
                        ));
                    }
                }
                Ok(())
            })();
            if let Err(source) = prepared {
                return Err(self.staging_failure(&mut temporary, source, &staging_identity));
            }
            // Windows replacement refuses a destination with our retained
            // READ_CONTROL snapshot open, even with delete sharing. Keep every
            // snapshot through the final identity/policy check, then release all
            // of its file/identity handles before the Windows rename boundary.
            #[cfg(windows)]
            drop(access);
            #[cfg(windows)]
            let result: Result<fs::File, tempfile::PersistError> = match windows_filesystem::publish(
                temporary.as_file(),
                path,
                matches!(publication, FilePublication::Replace),
            ) {
                Ok(()) => {
                    // The old temporary name is no longer ours. Never let its
                    // destructor remove a foreign entry substituted there.
                    temporary.disable_cleanup(true);
                    return Ok(true);
                }
                Err(error) => Err(tempfile::PersistError {
                    error,
                    file: temporary,
                }),
            };
            #[cfg(not(windows))]
            let result = match publication {
                FilePublication::Create => temporary.persist_noclobber(path),
                FilePublication::Replace => temporary.persist(path),
            };
            match result {
                Ok(_) => Ok(true),
                Err(mut failure) => {
                    if let Err(source) = self.verify_staging_file(&failure.file, &staging_identity)
                    {
                        return Err(self.staging_failure(
                            &mut failure.file,
                            source,
                            &staging_identity,
                        ));
                    }
                    if matches!(publication, FilePublication::Create)
                        && failure.error.kind() == io::ErrorKind::AlreadyExists
                    {
                        if gitignore_creation {
                            let existing = self
                                .verify_directories()
                                .and_then(|()| optional_metadata(path));
                            match existing {
                                Ok(Some(_)) => return Ok(false),
                                Err(source) => {
                                    return Err(self.staging_failure(
                                        &mut failure.file,
                                        source,
                                        &staging_identity,
                                    ));
                                }
                                Ok(None) => {}
                            }
                        }
                        let existing = self.verify().and_then(|()| optional_metadata(path));
                        match existing {
                            Ok(Some(metadata)) if !indirect(&metadata) && metadata.is_file() => {
                                return Ok(false);
                            }
                            Err(source) => {
                                return Err(self.staging_failure(
                                    &mut failure.file,
                                    source,
                                    &staging_identity,
                                ));
                            }
                            Ok(_) => {}
                        }
                    }
                    Err(self.staging_failure(
                        &mut failure.file,
                        publication_failure("persist generated output", failure.error),
                        &staging_identity,
                    ))
                }
            }
        })();
        #[cfg(target_os = "macos")]
        if let Some(private) = private {
            return private.finish(result);
        }
        result
    }

    /// Bind path-based metadata, publication and cleanup to the retained file.
    fn verify_staging_file(
        &self,
        temporary: &tempfile::NamedTempFile,
        directory_identity: &Handle,
    ) -> io::Result<()> {
        self.verify_staging_entry(temporary.path(), temporary.as_file(), directory_identity)
    }

    /// Retain file authority even when an explicit probe unlink fails.
    #[cfg(target_os = "linux")]
    fn discard_staging_probe(
        &self,
        mut temporary: tempfile::NamedTempFile,
        directory_identity: &Handle,
    ) -> io::Result<()> {
        if let Err(source) = self.verify_staging_file(&temporary, directory_identity) {
            return Err(self.staging_failure(&mut temporary, source, directory_identity));
        }
        let (file, path) = temporary.into_parts();
        let name = path.to_path_buf();
        if let Err(operation) = path.close() {
            return match self.verify_staging_entry(&name, &file, directory_identity) {
                Ok(()) => Err(operation),
                Err(ownership) => Err(io::Error::new(
                    operation.kind(),
                    RetainedStaging {
                        operation,
                        cleanup: ownership,
                    },
                )),
            };
        }
        Ok(())
    }

    /// Verify a direct stage name against both retained directory and file objects.
    fn verify_staging_entry(
        &self,
        path: &Path,
        file: &fs::File,
        directory_identity: &Handle,
    ) -> io::Result<()> {
        self.verify_directories()?;
        let directory = path
            .parent()
            .ok_or_else(|| refusal(path, "staging parent is unavailable"))?;
        require_directory(directory, &fs::symlink_metadata(directory)?)?;
        if Handle::from_path(directory)? != *directory_identity {
            return Err(refusal(directory, "staging directory object changed"));
        }
        let named = fs::symlink_metadata(path)?;
        require_regular(path, &named)?;
        #[cfg(unix)]
        let same = {
            use std::os::unix::fs::MetadataExt;
            let retained = file.metadata()?;
            named.dev() == retained.dev() && named.ino() == retained.ino()
        };
        #[cfg(windows)]
        let same = {
            use std::os::windows::fs::OpenOptionsExt;
            // Metadata access does not request competing file-data access.
            // Keep DELETE-only sharing on the original payload handle.
            const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
            let current = Handle::from_file(
                fs::OpenOptions::new()
                    .access_mode(FILE_READ_ATTRIBUTES)
                    .open(path)?,
            )?;
            current == Handle::from_file(file.try_clone()?)?
        };
        #[cfg(not(any(unix, windows)))]
        let same = Handle::from_path(path)? == Handle::from_file(file.try_clone()?)?;
        if !same {
            return Err(refusal(path, "temporary staging file object changed"));
        }
        Ok(())
    }

    /// Never unlink a temporary pathname through a changed directory or file owner.
    fn staging_failure(
        &self,
        temporary: &mut tempfile::NamedTempFile,
        operation: io::Error,
        directory_identity: &Handle,
    ) -> io::Error {
        let cleanup = self.verify_staging_file(temporary, directory_identity);
        #[cfg(windows)]
        let cleanup = cleanup.and_then(|()| {
            // Classic Windows deletion cannot remove a readonly file.
            // Normalize only our held, unpublished inode so cleanup does
            // not depend on a filesystem's optional POSIX-delete support.
            let mut permissions = temporary.as_file().metadata()?.permissions();
            if permissions.readonly() {
                #[expect(
                    clippy::permissions_set_readonly_false,
                    reason = "Windows clears only the readonly attribute on the verified held stage"
                )]
                permissions.set_readonly(false);
                temporary.as_file().set_permissions(permissions)?;
                self.verify_staging_file(temporary, directory_identity)?;
            }
            windows_filesystem::discard(temporary.as_file())?;
            temporary.disable_cleanup(true);
            Ok(())
        });
        match cleanup {
            Ok(()) => operation,
            Err(cleanup) => {
                // keep() itself performs path I/O on Windows. Only disable cleanup:
                // the original private staging file survives under its old owner.
                temporary.disable_cleanup(true);
                io::Error::new(operation.kind(), RetainedStaging { operation, cleanup })
            }
        }
    }
}

/// Retain the admitted native file and policy instead of reopening a mutable source name.
#[cfg(any(target_os = "macos", windows))]
struct ReplacementSource {
    /// Keep the captured destination alive until publication completes.
    file: fs::File,
    /// Compare the destination name with this retained object before replacing it.
    identity: Handle,
    /// Original permissions and, on Unix, ownership and inode identity.
    metadata: Metadata,
    /// Opaque macOS extended ACL, preserving ordered entries and native flags.
    #[cfg(target_os = "macos")]
    acl: macos_acl::AccessAcl,
    /// Windows discretionary policy and complete or proven-absent system policy.
    #[cfg(windows)]
    security: WindowsAccessSnapshot,
}

#[cfg(any(target_os = "macos", windows))]
impl ReplacementSource {
    /// Capture through a non-following descriptor, preserving write-only workflows.
    fn capture(path: &Path, expected: &Metadata) -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        let file = {
            use rustix::fs::{CWD, Mode, OFlags, openat};
            let flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            fs::File::from(
                match openat(CWD, path, flags | OFlags::RDONLY, Mode::empty()) {
                    Ok(file) => file,
                    Err(rustix::io::Errno::ACCESS) => {
                        openat(CWD, path, flags | OFlags::WRONLY, Mode::empty())?
                    }
                    Err(source) => return Err(source.into()),
                },
            )
        };
        #[cfg(windows)]
        let (file, complete_system_access) = {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_permissions::constants::AccessRights;
            const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            let open = |system| {
                fs::OpenOptions::new()
                    .access_mode(
                        AccessRights::ReadControl.bits()
                            | FILE_READ_ATTRIBUTES
                            | if system {
                                AccessRights::AccessSystemSecurity.bits()
                            } else {
                                0
                            },
                    )
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(path)
            };
            match open(true) {
                Ok(file) => (file, true),
                Err(source) if matches!(source.raw_os_error(), Some(5 | 1314)) => {
                    (open(false)?, false)
                }
                Err(source) => return Err(source),
            }
        };
        require_regular(path, &file.metadata()?)?;
        #[cfg(windows)]
        require_windows_unencrypted(&file, path)?;
        let identity = Handle::from_file(file.try_clone()?)?;
        #[cfg(target_os = "macos")]
        let acl = macos_access::get_acl(&file)?;
        #[cfg(windows)]
        let security = windows_access_snapshot(&file, complete_system_access)?;
        let source = Self {
            file,
            identity,
            metadata: expected.clone(),
            #[cfg(target_os = "macos")]
            acl,
            #[cfg(windows)]
            security,
        };
        source.verify_destination(path)?;
        Ok(source)
    }

    /// Refuse a changed destination name, ownership, permissions or native policy.
    fn verify_destination(&self, path: &Path) -> io::Result<()> {
        let named = fs::symlink_metadata(path)?;
        require_regular(path, &named)?;
        #[cfg(target_os = "macos")]
        let matches = {
            use std::os::unix::fs::MetadataExt;
            let held = self.file.metadata()?;
            named.dev() == self.metadata.dev()
                && named.ino() == self.metadata.ino()
                && held.dev() == self.metadata.dev()
                && held.ino() == self.metadata.ino()
                && held.uid() == self.metadata.uid()
                && held.gid() == self.metadata.gid()
                && held.mode() == self.metadata.mode()
                && macos_access::get_acl(&self.file)? == self.acl
                && Handle::from_file(self.file.try_clone()?)? == self.identity
        };
        #[cfg(windows)]
        let matches = {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            require_windows_unencrypted(&self.file, path)?;
            let current = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            require_regular(path, &current.metadata()?)?;
            Handle::from_file(current)? == self.identity
                && self.file.metadata()?.permissions() == self.metadata.permissions()
                && windows_access_snapshot(&self.file, self.security.system.needs_system_access())?
                    == self.security
        };
        if !matches {
            return Err(refusal(path, "destination object or access policy changed"));
        }
        Ok(())
    }

    /// Preserve the captured file policy before payload I/O and verify native readback.
    #[cfg(target_os = "macos")]
    fn apply(&self, path: &Path, staged: &tempfile::NamedTempFile) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::MetadataExt;
            let current = staged.as_file().metadata()?;
            if current.uid() != self.metadata.uid() || current.gid() != self.metadata.gid() {
                std::os::unix::fs::fchown(
                    staged.as_file(),
                    Some(self.metadata.uid()),
                    Some(self.metadata.gid()),
                )?;
            }
            macos_access::set_acl(staged.as_file(), &self.acl)?;
        }
        // Ownership changes can clear mode bits. Restore mode after ownership/ACL.
        staged
            .as_file()
            .set_permissions(self.metadata.permissions())?;
        #[cfg(target_os = "macos")]
        if macos_access::get_acl(staged.as_file())? != self.acl {
            return Err(refusal(
                path,
                "staged access ACL differs from the destination",
            ));
        }
        Ok(())
    }

    /// Apply only the captured creation policy, never a fresh mutable source policy.
    #[cfg(windows)]
    fn apply(
        &self,
        path: &Path,
        staged: &tempfile::NamedTempFile,
        creation: &windows_security::ReplacementCreation,
    ) -> io::Result<()> {
        creation.verify_parent()?;
        self.security.system.apply(staged.as_file())?;
        preserve_windows_descriptor(path, &creation.descriptor, &creation.dacl_image, staged)?;
        staged
            .as_file()
            .set_permissions(self.metadata.permissions())
    }
}

/// Keep access controls on the staged object before writing or publishing bytes.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn preserve_replacement_access(
    path: &Path,
    metadata: &Metadata,
    staged: &tempfile::NamedTempFile,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = staged.as_file().metadata()?;
        if current.uid() != metadata.uid() || current.gid() != metadata.gid() {
            std::os::unix::fs::fchown(
                staged.as_file(),
                Some(metadata.uid()),
                Some(metadata.gid()),
            )?;
        }
    }
    // chown can clear mode bits, and a POSIX ACL controls the group-class mask.
    // Restore the original mode only after restoring ownership and ACL entries.
    staged.as_file().set_permissions(metadata.permissions())?;
    let _ = path;
    Ok(())
}

/// Refuse EFS replacement until the complete native encryption policy is preserved.
#[cfg(windows)]
fn require_windows_unencrypted(file: &fs::File, path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ENCRYPTED;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_ENCRYPTED != 0 {
        return Err(refusal(
            path,
            "EFS-encrypted output cannot be replaced without preserving encryption policy",
        ));
    }
    Ok(())
}

/// Create an exclusive data handle with policy rights bound to the original stage.
#[cfg(windows)]
fn create_windows_staging_file(
    path: &Path,
    creation: Option<&windows_security::ReplacementCreation>,
) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::constants::AccessRights;
    if let Some(creation) = creation {
        return creation.create(path);
    }
    let rights =
        (AccessRights::GenericRead | AccessRights::GenericWrite | AccessRights::Delete).bits();
    // Deny competing data handles before native policy is copied; allow rename.
    fs::OpenOptions::new()
        // Rust validates creation flags independently of the custom access mask.
        .write(true)
        .access_mode(rights)
        .create_new(true)
        .share_mode(0x0000_0004)
        .open(path)
}

/// Discretionary access and a complete or proven-absent system-policy snapshot.
#[cfg(windows)]
#[derive(PartialEq, Eq)]
struct WindowsAccessSnapshot {
    /// Native owner/group/DACL comparison image.
    discretionary: std::ffi::OsString,
    /// Opaque system policy and native controls.
    system: windows_security::SystemPolicy,
}

/// Capture native policy without changing the caller's enabled privileges.
#[cfg(windows)]
fn windows_access_snapshot(
    file: &fs::File,
    complete_system_access: bool,
) -> io::Result<WindowsAccessSnapshot> {
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::wrappers::{
        ConvertSecurityDescriptorToStringSecurityDescriptor, GetSecurityInfo,
    };
    let information =
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
    let access = GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, information)?;
    Ok(WindowsAccessSnapshot {
        discretionary: ConvertSecurityDescriptorToStringSecurityDescriptor(&access, information)?,
        system: windows_security::SystemPolicy::capture(file, complete_system_access)?,
    })
}

/// Copy the destination DACL and inheritance policy using native file handles.
#[cfg(all(windows, test))]
fn preserve_windows_access(
    path: &Path,
    original: &fs::File,
    staged: &tempfile::NamedTempFile,
) -> io::Result<()> {
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::wrappers::{
        ConvertSecurityDescriptorToStringSecurityDescriptor, GetSecurityInfo,
    };
    let information =
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
    let descriptor = GetSecurityInfo(original, SeObjectType::SE_FILE_OBJECT, information)?;
    let dacl = ConvertSecurityDescriptorToStringSecurityDescriptor(
        &descriptor,
        SecurityInformation::Dacl,
    )?;
    preserve_windows_descriptor(path, &descriptor, &dacl, staged)
}

/// Preserve the original opaque descriptor after protected initial creation.
#[cfg(windows)]
fn preserve_windows_descriptor(
    path: &Path,
    access: &windows_permissions::SecurityDescriptor,
    sddl: &std::ffi::OsStr,
    staged: &tempfile::NamedTempFile,
) -> io::Result<()> {
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::wrappers::{
        ConvertSecurityDescriptorToStringSecurityDescriptor, GetSecurityInfo, SetSecurityInfo,
    };

    const DACL_PREFIX: &str = "D:";
    const NULL_DACL_FLAG: &str = "NO_ACCESS_CONTROL";
    const PROTECTED_FLAG: char = 'P';
    let information =
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
    let current = GetSecurityInfo(staged.as_file(), SeObjectType::SE_FILE_OBJECT, information)?;
    let owner_changed = access.owner() != current.owner();
    let group_changed = access.group() != current.group();
    // This safe wrapper exposes no control-bit getter. Read only the native
    // DACL SDDL header, never ACE text or a lossy filesystem path conversion.
    let current_dacl =
        ConvertSecurityDescriptorToStringSecurityDescriptor(&current, SecurityInformation::Dacl)?;
    if !owner_changed && !group_changed && windows_file_dacl_matches(sddl, &current_dacl)? {
        return Ok(());
    }
    let header = sddl
        .to_str()
        .ok_or_else(|| refusal(path, "native DACL header is not text"))?;
    let flags = if header.is_empty() {
        None
    } else {
        Some(
            header
                .strip_prefix(DACL_PREFIX)
                .ok_or_else(|| refusal(path, "native DACL header is invalid"))?
                .split('(')
                .next()
                .ok_or_else(|| refusal(path, "native DACL flags are unavailable"))?,
        )
    };
    let protected = flags.is_some_and(|flags| flags.contains(PROTECTED_FLAG));
    // The wrapper's accessor assumes a present ACL pointer is non-null. A
    // native NULL DACL is valid; keep it out of that accessor. Native systems
    // unable to serialize it return a causal error above before any mutation.
    let dacl = if flags.is_none_or(|flags| flags.contains(NULL_DACL_FLAG)) {
        None
    } else {
        access.dacl()
    };
    let mut information = SecurityInformation::Dacl
        | if protected {
            SecurityInformation::ProtectedDacl
        } else {
            SecurityInformation::UnprotectedDacl
        };
    if owner_changed {
        information |= SecurityInformation::Owner;
    }
    if group_changed {
        information |= SecurityInformation::Group;
    }
    // Duplicate only our already admitted stage handle. Reopening staged.path()
    // could mutate a foreign object substituted after the last identity check.
    let mut destination = staged.as_file().try_clone()?;
    SetSecurityInfo(
        &mut destination,
        SeObjectType::SE_FILE_OBJECT,
        information,
        access.owner(),
        access.group(),
        dacl,
        None,
    )?;
    let applied = GetSecurityInfo(
        &destination,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl,
    )?;
    let applied_sddl =
        ConvertSecurityDescriptorToStringSecurityDescriptor(&applied, SecurityInformation::Dacl)?;
    let owner_matches = applied.owner() == access.owner();
    let group_matches = applied.group() == access.group();
    if !owner_matches || !group_matches || !windows_file_dacl_matches(sddl, &applied_sddl)? {
        let difference = windows_dacl_difference(sddl, &applied_sddl)?;
        return Err(refusal(
            path,
            &format!(
                "staged DACL differs from the destination: owner_matches={owner_matches}, group_matches={group_matches}, {difference}"
            ),
        ));
    }
    Ok(())
}

/// Borrow the native DACL header and opaque ordered ACE suffix; never parse ACEs.
#[cfg(any(windows, test))]
fn windows_dacl_parts(value: &std::ffi::OsStr) -> io::Result<(Option<&str>, &str)> {
    const DACL_PREFIX: &str = "D:";
    let text = value.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "native DACL header is not text")
    })?;
    if text.is_empty() {
        return Ok((None, ""));
    }
    let dacl = text.strip_prefix(DACL_PREFIX).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "native DACL header is invalid")
    })?;
    let (flags, entries) = dacl.split_at(dacl.find('(').unwrap_or(dacl.len()));
    Ok((Some(flags), entries))
}

/// Native automatic-inheritance state may normalize when applying a file DACL.
#[cfg(any(windows, test))]
const WINDOWS_DACL_AUTO_INHERITED_FLAG: &str = "AI";

/// Preserve regular-file policy while allowing only native AI-flag normalization.
///
/// Both objects are admitted regular files, with no child ACLs to propagate. Keep
/// every ordered ACE (including inherited markers), protection, AR, NULL/empty
/// distinction and any unknown header text exact. Ownership is checked separately.
#[cfg(any(windows, test))]
fn windows_file_dacl_matches(
    expected: &std::ffi::OsStr,
    observed: &std::ffi::OsStr,
) -> io::Result<bool> {
    let (expected_flags, expected_entries) = windows_dacl_parts(expected)?;
    let (observed_flags, observed_entries) = windows_dacl_parts(observed)?;
    let file_flags = |flags: Option<&str>| {
        flags.map(|flags| flags.replace(WINDOWS_DACL_AUTO_INHERITED_FLAG, ""))
    };
    Ok(expected_entries == observed_entries
        && file_flags(expected_flags) == file_flags(observed_flags))
}

/// Describe native DACL differences without exposing ACEs or principal identities.
#[cfg(any(windows, test))]
fn windows_dacl_difference(
    expected: &std::ffi::OsStr,
    observed: &std::ffi::OsStr,
) -> io::Result<String> {
    const NULL_FLAG: &str = "NO_ACCESS_CONTROL";
    const PROTECTED_FLAG: &str = "P";
    const INHERIT_REQUEST_FLAG: &str = "AR";
    let (expected_flags, expected_entries) = windows_dacl_parts(expected)?;
    let (observed_flags, observed_entries) = windows_dacl_parts(observed)?;
    let flag_matches = |flag| {
        expected_flags.is_some_and(|flags| flags.contains(flag))
            == observed_flags.is_some_and(|flags| flags.contains(flag))
    };
    Ok(format!(
        "presence_matches={}, null_matches={}, protection_matches={}, inherit_request_matches={}, auto_inherited_matches={}, ordered_aces_match={}",
        expected_flags.is_some() == observed_flags.is_some(),
        flag_matches(NULL_FLAG),
        flag_matches(PROTECTED_FLAG),
        flag_matches(INHERIT_REQUEST_FLAG),
        flag_matches(WINDOWS_DACL_AUTO_INHERITED_FLAG),
        expected_entries == observed_entries,
    ))
}

/// Recognize a managed entry without treating unresolved ownership as external.
fn owned_named_path(path: &Path, directory: &Path, expected_name: &str) -> io::Result<bool> {
    if !fixed_name_candidate(path, expected_name)? {
        return Ok(false);
    }
    let parent = containing_directory(path);
    // Prove direct namespace spelling before following a possibly aliased private
    // directory. Volume lookup below supplies case behavior, never ownership.
    if direct_directory_spelling(parent, directory)? {
        return named(path, expected_name);
    }
    match fs::canonicalize(parent) {
        Ok(resolved) => {
            if direct_directory_spelling(&resolved, directory)?
                || resolved_logical_directory_reduction(parent, directory, &resolved)?
            {
                named(path, expected_name)
            } else {
                Ok(false)
            }
        }
        Err(source) => {
            // mkdir_all could make a missing/.. spelling resolve to the managed
            // parent. Refuse before that mutation, preserving the original error.
            let potential = resolve_existing_parent_prefix(parent)?;
            if (direct_directory_spelling(&potential, directory)?
                || resolved_logical_directory_reduction(parent, directory, &potential)?)
                && named(path, expected_name)?
            {
                Err(source)
            } else {
                Ok(false)
            }
        }
    }
}

/// Keep a reduction through the logical namespace owned even if it is aliased.
fn resolved_logical_directory_reduction(
    parent: &Path,
    directory: &Path,
    resolved: &Path,
) -> io::Result<bool> {
    for ancestor in parent.ancestors().skip(1) {
        if direct_directory_spelling(ancestor, directory)?
            && fs::canonicalize(ancestor).is_ok_and(|owner| owner == resolved)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Compare a fixed directory without following its final namespace component.
fn direct_directory_spelling(path: &Path, directory: &Path) -> io::Result<bool> {
    if path == directory {
        return Ok(true);
    }
    let Some(name) = directory.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Ok(false);
    };
    // Verbatim Windows paths with literal parent reductions can fail lookup
    // with ERROR_INVALID_NAME. Let the owning existing-prefix fallback resolve
    // them; this is not evidence of external ownership or permission denial.
    let unresolved = |source: &io::Error| {
        source.kind() == io::ErrorKind::NotFound
            || (cfg!(windows) && source.raw_os_error() == Some(123))
    };
    // A selected root can have arbitrary Unicode, unlike the product names.
    // Prove existing direct directory aliases by object identity, not by a
    // modern Unicode fold. Never use this proof for managed file leaves.
    let unicode = !name.is_ascii()
        || path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| !name.is_ascii());
    if unicode {
        match same_direct_directory(path, directory) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(source) if unresolved(&source) => {}
            Err(source) => return Err(source),
        }
    }
    if !fixed_name_candidate(path, name)? {
        return Ok(false);
    }
    let prefix = containing_directory(path);
    let same_parent = match (fs::canonicalize(prefix), directory.parent()) {
        (Ok(resolved), Some(parent)) => {
            resolved == parent || same_direct_directory(&resolved, parent)?
        }
        (Err(source), _) if unresolved(&source) => false,
        (Err(source), _) => return Err(source),
        (_, None) => false,
    };
    if same_parent {
        named(path, name)
    } else {
        Ok(false)
    }
}

/// Recognize the private directory before interpreting a relative config root.
///
/// # Errors
/// Returns native ownership/comparison errors for a possible directory alias.
pub(crate) fn state_directory_spelling(path: &Path) -> io::Result<bool> {
    if path.file_name().is_some_and(|name| name == STATE_DIRECTORY) {
        return Ok(true);
    }
    if !fixed_name_candidate(path, STATE_DIRECTORY)? {
        return Ok(false);
    }
    let parent = resolve_existing_parent_prefix(containing_directory(path))?;
    direct_directory_spelling(path, &parent.join(STATE_DIRECTORY))
}

/// Prove only existing direct directory aliases; file hardlinks do not imply naming rules.
fn same_direct_directory(left: &Path, right: &Path) -> io::Result<bool> {
    let (Some(left_metadata), Some(right_metadata)) =
        (optional_metadata(left)?, optional_metadata(right)?)
    else {
        return Ok(false);
    };
    if !left_metadata.is_dir()
        || indirect(&left_metadata)
        || !right_metadata.is_dir()
        || indirect(&right_metadata)
    {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Directory traversal and attributes do not require directory-data read
        // access. Do not introduce that right merely to compare identities.
        Ok(left_metadata.dev() == right_metadata.dev()
            && left_metadata.ino() == right_metadata.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
        const DIRECTORY_NOFOLLOW: u32 = 0x0200_0000 | 0x0020_0000;
        let identity = |path: &Path| -> io::Result<Handle> {
            let file = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .custom_flags(DIRECTORY_NOFOLLOW)
                .open(path)?;
            require_directory(path, &file.metadata()?)?;
            Handle::from_file(file)
        };
        Ok(identity(left)? == identity(right)?)
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(Handle::from_path(left)? == Handle::from_path(right)?)
    }
}

/// Select a path's lookup directory without treating an empty parent as root.
fn containing_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Resolve actual existing ancestors before reducing only an unresolved suffix.
fn resolve_existing_parent_prefix(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut prefix = absolute.as_path();
    let mut resolved = loop {
        match fs::canonicalize(prefix) {
            Ok(resolved) => break resolved,
            Err(source) => match prefix.parent() {
                Some(parent) => prefix = parent,
                None => return Err(source),
            },
        }
    };
    let suffix = absolute.strip_prefix(prefix).map_err(io::Error::other)?;
    for component in suffix.components() {
        match component {
            Component::Normal(name) => resolved.push(name),
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => {
                return Err(refusal(path, "unresolved parent suffix is not relative"));
            }
        }
    }
    Ok(resolved)
}

/// Select a possible fixed name without probing an unrelated filesystem.
fn fixed_name_candidate(path: &Path, expected: &str) -> io::Result<bool> {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Ok(false);
    };
    #[cfg(windows)]
    let name = windows_lookup_name(path, name);
    if name.eq_ignore_ascii_case(expected) {
        return Ok(true);
    }
    #[cfg(windows)]
    if windows_filesystem::short_name_candidate(std::ffi::OsStr::new(name)) {
        return Ok(true);
    }
    #[cfg(target_os = "linux")]
    if linux_access::fixed_name_matches(name, expected) {
        return Ok(true);
    }
    // This is only a candidate filter: APFS also folds multi-scalar aliases
    // such as the fi ligature. The containing filesystem decides ownership.
    if !name.is_ascii() && expected.is_ascii() {
        if unicode_name_matches(name, expected) || hfs_managed_name_matches(name, expected) {
            return Ok(true);
        }
        let pattern = format!(r"\A{}\z", regex::escape(expected));
        return regex::RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .map(|pattern| pattern.is_match(name))
            .map_err(io::Error::other);
    }
    Ok(false)
}

/// Keep ordinary Win32 leaf normalization out of literal native namespaces.
#[cfg(windows)]
fn windows_lookup_name<'a>(path: &Path, name: &'a str) -> &'a str {
    let literal = matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if prefix.kind().is_verbatim()
    );
    if literal || name.contains(':') {
        name
    } else {
        name.trim_end_matches(['.', ' '])
    }
}

/// Match the fixed ASCII product alphabet under HFS's frozen comparison rules.
///
/// TN1150 and Apple's HFS comparison data have no non-ASCII-to-ASCII mappings;
/// normalization adds none to this alphabet. Only these sixteen format characters
/// are ignored. In particular long-s, soft hyphen, CGJ and Kelvin remain distinct.
fn hfs_managed_name_matches(name: &str, expected: &str) -> bool {
    expected.is_ascii()
        && name
            .chars()
            .filter(|character| {
                !matches!(character,
                '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
                | '\u{206a}'..='\u{206f}' | '\u{feff}')
            })
            .map(|character| character.to_ascii_lowercase())
            .eq(expected
                .chars()
                .map(|character| character.to_ascii_lowercase()))
}

/// Compare ASCII product names using APFS's Unicode 9 full, non-Turkic fold.
fn unicode_name_matches(name: &str, expected: &str) -> bool {
    use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
    name.case_fold_with(Variant::Full, Locale::NonTurkic)
        .eq(expected.chars())
}

/// Match fixed components using the supported platform's case behavior.
// Keep one fallible caller contract for native directory/volume queries.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux")),
    allow(clippy::unnecessary_wraps)
)]
fn named(path: &Path, expected: &str) -> io::Result<bool> {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Ok(false);
    };
    #[cfg(windows)]
    let name = windows_lookup_name(path, name);
    if name == expected {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        let parent = resolve_existing_parent_prefix(containing_directory(path))?;
        if !name.eq_ignore_ascii_case(expected) {
            if !windows_filesystem::short_name_candidate(std::ffi::OsStr::new(name)) {
                return Ok(false);
            }
            let Some(entry) = windows_filesystem::entry_name(&parent.join(name))? else {
                return Ok(false);
            };
            // A native short alias names its long entry even in a sensitive
            // directory. Distinct hardlinks and reparse targets supply no proof.
            if entry == expected {
                return Ok(true);
            }
            if !entry
                .to_str()
                .is_some_and(|entry| entry.eq_ignore_ascii_case(expected))
            {
                return Ok(false);
            }
        }
        let mut prefix = parent.as_path();
        let probe = loop {
            match fs::canonicalize(prefix) {
                Ok(resolved) => break resolved,
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    prefix = prefix.parent().ok_or(source)?;
                }
                Err(source) => return Err(source),
            }
        };
        Ok(!windows_filesystem::case_sensitive(&probe)?)
    }
    #[cfg(target_os = "macos")]
    {
        if !fixed_name_candidate(path, expected)? {
            return Ok(false);
        }
        // Match the ownership proof: resolve the existing original prefix, then
        // reduce the unresolved suffix before choosing its destination volume.
        // A mount crossed by missing/../.. need not own the resulting parent.
        let parent = resolve_existing_parent_prefix(containing_directory(path))?;
        let mut prefix = parent.as_path();
        let probe = loop {
            match fs::canonicalize(prefix) {
                Ok(resolved) => break resolved,
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    prefix = match prefix.parent() {
                        Some(parent) => parent,
                        None => return Err(source),
                    };
                }
                Err(source) => return Err(source),
            }
        };
        let volume = whichdisk::resolve(&probe)?;
        match volume.case_sensitive() {
            Some(true) => Ok(false),
            Some(false) if name.eq_ignore_ascii_case(expected) => Ok(true),
            Some(false) if volume.fs_type().eq_ignore_ascii_case("apfs") => {
                Ok(unicode_name_matches(name, expected))
            }
            Some(false)
                if ["hfs", "hfs+", "hfsx"]
                    .iter()
                    .any(|filesystem| volume.fs_type().eq_ignore_ascii_case(filesystem)) =>
            {
                Ok(hfs_managed_name_matches(name, expected))
            }
            Some(false) => {
                // Retain the non-expanding fallback for other insensitive
                // filesystems; APFS and HFS use their dedicated rules above.
                let pattern = format!(r"\A{}\z", regex::escape(expected));
                regex::RegexBuilder::new(&pattern)
                    .case_insensitive(true)
                    .build()
                    .map(|pattern| pattern.is_match(name))
                    .map_err(io::Error::other)
            }
            None => Err(refusal(
                path,
                "managed case-alias volume behavior is unknown",
            )),
        }
    }
    #[cfg(target_os = "linux")]
    {
        linux_access::named(path, expected)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Ok(false)
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
