//! Admission and publication of automatic project-owned initialization files.

use same_file::Handle;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

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
            StateAdmission::Database | StateAdmission::Generated => root.join(STATE_DIRECTORY),
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
                Some(path.to_path_buf())
            }
            _ => None,
        };
        let automatic_document = match admission {
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
        if matches!(self.admission, StateAdmission::Database) {
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
        #[cfg(unix)]
        let mut builder = tempfile::Builder::new();
        #[cfg(not(unix))]
        let builder = tempfile::Builder::new();
        #[cfg(unix)]
        if metadata.is_none() {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o666));
        }
        #[cfg(not(windows))]
        let mut temporary = builder.tempfile_in(&self.directory)?;
        #[cfg(windows)]
        let mut temporary = {
            use std::os::windows::fs::OpenOptionsExt;
            // Deny other data handles while inherited access is being replaced.
            // Delete sharing is required for atomic publication of our open file.
            builder.make_in(&self.directory, |path| {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .share_mode(0x0000_0004)
                    .open(path)
            })?
        };
        let prepared = (|| {
            if let Some(metadata) = &metadata {
                preserve_replacement_access(path, metadata, &temporary)?;
            }
            temporary.write_all(bytes)?;
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
            Ok(_) => Ok(true),
            Err(mut failure) => {
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
                                return Err(self.staging_failure(&mut failure.file, source));
                            }
                            Ok(None) => {}
                        }
                    }
                    let existing = self.verify().and_then(|()| optional_metadata(path));
                    match existing {
                        Ok(Some(metadata)) if !indirect(&metadata) && metadata.is_file() => {
                            return Ok(false);
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

/// Keep access controls on the staged object before writing or publishing bytes.
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
    #[cfg(target_os = "linux")]
    let access_acls = {
        use xattr::FileExt;
        // The kernel owns these ACL encodings; copy them without interpreting
        // entries or numeric principals. Both are fixed access-control roles.
        const ACCESS_ACLS: [&str; 2] = ["system.posix_acl_access", "system.nfs4_acl"];
        let mut captured = Vec::with_capacity(ACCESS_ACLS.len());
        for name in ACCESS_ACLS {
            let access = match xattr::get(path, name) {
                Ok(value) => value,
                Err(source) if source.kind() == io::ErrorKind::Unsupported => continue,
                Err(source) => return Err(source),
            };
            if let Some(access) = &access {
                staged.as_file().set_xattr(name, access)?;
            } else if staged.as_file().get_xattr(name)?.is_some() {
                staged.as_file().remove_xattr(name)?;
            }
            captured.push((name, access));
        }
        captured
    };
    #[cfg(target_os = "macos")]
    let access_acl = {
        let access = exacl::getfacl(path, None)?;
        exacl::setfacl(&[staged.path()], &access, None)?;
        access
    };
    #[cfg(windows)]
    preserve_windows_access(path, staged)?;
    // chown can clear mode bits, and a POSIX ACL controls the group-class mask.
    // Restore the original mode only after restoring ownership and ACL entries.
    staged.as_file().set_permissions(metadata.permissions())?;
    #[cfg(target_os = "linux")]
    {
        use xattr::FileExt;
        for (name, access) in access_acls {
            if staged.as_file().get_xattr(name)? != access {
                return Err(refusal(
                    path,
                    "staged access ACL differs from the destination",
                ));
            }
        }
    }
    #[cfg(target_os = "macos")]
    if exacl::getfacl(staged.path(), None)? != access_acl {
        return Err(refusal(
            path,
            "staged access ACL differs from the destination",
        ));
    }
    Ok(())
}

/// Copy the destination DACL and inheritance policy using native file handles.
#[cfg(windows)]
fn preserve_windows_access(path: &Path, staged: &tempfile::NamedTempFile) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::constants::{AccessRights, SeObjectType, SecurityInformation};
    use windows_permissions::wrappers::{
        ConvertSecurityDescriptorToStringSecurityDescriptor, GetSecurityInfo, SetSecurityInfo,
    };

    const DACL_PREFIX: &str = "D:";
    const NULL_DACL_FLAG: &str = "NO_ACCESS_CONTROL";
    const PROTECTED_FLAG: char = 'P';
    let original = fs::OpenOptions::new()
        .access_mode(AccessRights::ReadControl.bits())
        .open(path)?;
    let information =
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
    let access = GetSecurityInfo(&original, SeObjectType::SE_FILE_OBJECT, information)?;
    let current = GetSecurityInfo(staged.as_file(), SeObjectType::SE_FILE_OBJECT, information)?;
    let owner_changed = access.owner() != current.owner();
    let group_changed = access.group() != current.group();
    // This safe wrapper exposes no control-bit getter. Read only the native
    // DACL SDDL header, never ACE text or a lossy filesystem path conversion.
    let sddl =
        ConvertSecurityDescriptorToStringSecurityDescriptor(&access, SecurityInformation::Dacl)?;
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
    let mut rights = AccessRights::ReadControl | AccessRights::WriteDac;
    if owner_changed {
        information |= SecurityInformation::Owner;
        rights |= AccessRights::WriteOwner;
    }
    if group_changed {
        information |= SecurityInformation::Group;
        rights |= AccessRights::WriteOwner;
    }
    let mut destination = fs::OpenOptions::new()
        .access_mode(rights.bits())
        .open(staged.path())?;
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
    if !owner_matches || !group_matches || !windows_file_dacl_matches(&sddl, &applied_sddl)? {
        let difference = windows_dacl_difference(&sddl, &applied_sddl)?;
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
    if !fixed_name_candidate(path, name)? {
        return Ok(false);
    }
    let prefix = containing_directory(path);
    if fs::canonicalize(prefix).is_ok_and(|resolved| Some(resolved.as_path()) == directory.parent())
    {
        named(path, name)
    } else {
        Ok(false)
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
    if name.eq_ignore_ascii_case(expected) {
        return Ok(true);
    }
    // This is only a candidate filter: APFS also folds multi-scalar aliases
    // such as the fi ligature. The containing filesystem decides ownership.
    if !name.is_ascii() && expected.is_ascii() {
        if unicode_name_matches(name, expected) {
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

/// Compare ASCII product names using APFS's Unicode 9 full, non-Turkic fold.
fn unicode_name_matches(name: &str, expected: &str) -> bool {
    use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
    name.case_fold_with(Variant::Full, Locale::NonTurkic)
        .eq(expected.chars())
}

/// Match fixed components using the supported platform's case behavior.
// Keep one fallible caller contract: only macOS needs a native volume query.
#[cfg_attr(not(target_os = "macos"), allow(clippy::unnecessary_wraps))]
fn named(path: &Path, expected: &str) -> io::Result<bool> {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Ok(false);
    };
    if name == expected {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        Ok(name.eq_ignore_ascii_case(expected))
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
            Some(false) => {
                // HFS+ uses older non-expanding comparison rules; do not apply
                // APFS's full fold to distinct names on another filesystem.
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
    #[cfg(not(any(windows, target_os = "macos")))]
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
