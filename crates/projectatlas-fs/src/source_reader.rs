//! Direct source reads anchored to one discovery-owned directory capability.

use crate::{FsError, FsResult};
use cap_fs_ext::DirExt;
#[cfg(not(unix))]
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::Dir;
#[cfg(not(unix))]
use cap_std::fs::OpenOptions;
use projectatlas_core::{IndexWorkControl, IndexWorkStage};
#[cfg(unix)]
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum retained ancestor depth; deeper sources use the ordinary secure walk.
const DIRECTORY_CURSOR_MAX_DEPTH: usize = 8;
/// Maximum extra retained directory handles across concurrent discoveries.
const DIRECTORY_HANDLE_CEILING: usize = 64;
/// Live handle occupancy only; no source path or capability is process-global.
static DIRECTORY_HANDLES: AtomicUsize = AtomicUsize::new(0);

/// Root capability retained only while the current source discovery runs.
#[derive(Debug)]
pub(crate) struct SourceRootReader {
    /// Selected spelling used for repository-relative component admission.
    root: PathBuf,
    /// Open selected root; source descendants never resolve from ambient paths.
    directory: Dir,
    /// Conservative process-wide occupancy bound captured for this discovery.
    directory_handle_limit: usize,
}

impl SourceRootReader {
    /// Open the already selected root, allowing its intentional root alias.
    pub(crate) fn open(root: &Path) -> FsResult<Self> {
        let directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|source| source_error(root, source))?;
        Ok(Self {
            root: root.to_path_buf(),
            directory,
            directory_handle_limit: directory_handle_limit(),
        })
    }

    /// Borrow a sibling-source cursor for one existing parallel walk visitor.
    pub(crate) fn reader(&self) -> SourceFileReader<'_> {
        SourceFileReader {
            root: self,
            cursor: None,
            occupied: &DIRECTORY_HANDLES,
            limit: self.directory_handle_limit,
            #[cfg(test)]
            hits: 0,
        }
    }

    /// Borrow the unchanged component walk for a single-target operation.
    pub(crate) fn uncached(&self) -> SourceFileReader<'_> {
        SourceFileReader {
            limit: 0,
            ..self.reader()
        }
    }

    /// Open a regular source without following any descendant symlink.
    ///
    /// Resolve native components separately: leaf-only no-follow would still
    /// accept a substituted parent. Each parent capability authorizes the next
    /// component, including when its original pathname changes concurrently.
    pub(crate) fn open_file(
        &self,
        path: &Path,
        admitted_metadata: &std::fs::Metadata,
        control: &IndexWorkControl,
    ) -> FsResult<(File, std::fs::Metadata)> {
        let relative = path.strip_prefix(&self.root).map_err(|source| {
            source_error(path, io::Error::new(io::ErrorKind::InvalidInput, source))
        })?;
        let mut components = relative.components().peekable();
        let mut directory = None;
        while let Some(component) = components.next() {
            control.check(IndexWorkStage::SourceHash)?;
            let Component::Normal(name) = component else {
                return Err(source_error(
                    path,
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "source path must contain only direct descendant components",
                    ),
                ));
            };
            let parent = directory.as_ref().unwrap_or(&self.directory);
            if components.peek().is_some() {
                directory = Some(
                    parent
                        .open_dir_nofollow(Path::new(name))
                        .map_err(|source| source_error(path, source))?,
                );
            } else {
                return open_source_leaf(parent, Path::new(name), path, admitted_metadata);
            }
        }
        Err(source_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "source path has no file component",
            ),
        ))
    }
}

/// One worker's optional last-parent capability chain; never shared or persisted.
#[derive(Debug)]
pub(crate) struct SourceFileReader<'root> {
    /// Selected root capability borrowed for this discovery only.
    root: &'root SourceRootReader,
    /// Sibling-only reuse; a parent change or error discards every ancestor.
    cursor: Option<SourceDirectoryCursor>,
    /// Process occupancy (or an isolated test counter), not a path registry.
    occupied: &'static AtomicUsize,
    /// Extra handle limit; zero selects the unchanged secure walk.
    limit: usize,
    /// Causal coverage distinguishes actual reuse from the uncached fallback.
    #[cfg(test)]
    hits: usize,
}

impl SourceFileReader<'_> {
    /// Open exact source bytes, releasing all retained ancestors on any error.
    pub(crate) fn open_file(
        &mut self,
        path: &Path,
        admitted_metadata: &std::fs::Metadata,
        control: &IndexWorkControl,
    ) -> FsResult<(File, std::fs::Metadata)> {
        let result = self.open_source(path, admitted_metadata, control);
        let retry = self.cursor.is_some()
            && matches!(&result, Err(error) if directory_handle_capacity_error(error));
        if result.is_err() {
            self.cursor = None;
        }
        if retry {
            // Another owner may consume the remaining descriptors after this
            // cursor was built. Release every extra handle before the retry.
            self.root.open_file(path, admitted_metadata, control)
        } else {
            result
        }
    }

    /// Reuse a verified sibling parent, or use the ordinary secure component walk.
    fn open_source(
        &mut self,
        path: &Path,
        admitted_metadata: &std::fs::Metadata,
        control: &IndexWorkControl,
    ) -> FsResult<(File, std::fs::Metadata)> {
        control.check(IndexWorkStage::SourceHash)?;
        let relative = path.strip_prefix(&self.root.root).map_err(|source| {
            source_error(path, io::Error::new(io::ErrorKind::InvalidInput, source))
        })?;
        let parent = relative.parent().unwrap_or(Path::new(""));
        let hit = self
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.parent == parent);
        if !hit {
            self.cursor = None;
            let depth = parent.components().count();
            if (1..=DIRECTORY_CURSOR_MAX_DEPTH).contains(&depth)
                && let Some(permit) =
                    DirectoryHandlePermit::acquire(self.occupied, depth, self.limit)
            {
                self.cursor = match SourceDirectoryCursor::open(
                    &self.root.directory,
                    parent,
                    path,
                    permit,
                    control,
                ) {
                    Ok(cursor) => Some(cursor),
                    // Logical occupancy cannot know the process's other open
                    // handles. Partial frames/permit have already been dropped.
                    Err(error) if directory_handle_capacity_error(&error) => None,
                    Err(error) => return Err(error),
                };
            }
        }
        let Some(cursor) = &self.cursor else {
            return self.root.open_file(path, admitted_metadata, control);
        };
        let directory = cursor.validate(&self.root.directory, path, control)?;
        // Do not let `file_name` silently normalize a non-direct leaf selector.
        let Some(Component::Normal(name)) = relative.components().next_back() else {
            return Err(source_error(
                path,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "source path has no direct file component",
                ),
            ));
        };
        #[cfg(test)]
        {
            self.hits += usize::from(hit);
        }
        open_source_leaf(directory, Path::new(name), path, admitted_metadata)
    }
}

/// Complete retained chain: close frames BEFORE returning their occupancy permit.
#[derive(Debug)]
struct SourceDirectoryCursor {
    /// Every ancestor remains live, including on Windows where names are pinned.
    frames: Vec<SourceDirectoryFrame>,
    /// Relative lexical parent used only to select candidate sibling reuse.
    parent: PathBuf,
    /// Declared last so Rust drops directory handles before refunding occupancy.
    _permit: DirectoryHandlePermit,
}

impl SourceDirectoryCursor {
    /// Acquire ancestors through the same canonical no-follow component opener.
    fn open(
        root: &Dir,
        parent: &Path,
        path: &Path,
        permit: DirectoryHandlePermit,
        control: &IndexWorkControl,
    ) -> FsResult<Self> {
        let mut cursor = Self {
            frames: Vec::with_capacity(permit.count),
            parent: parent.to_path_buf(),
            _permit: permit,
        };
        for component in parent.components() {
            control.check(IndexWorkStage::SourceHash)?;
            let Component::Normal(name) = component else {
                return Err(source_error(
                    path,
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "source path must contain only direct descendant components",
                    ),
                ));
            };
            let directory = cursor
                .frames
                .last()
                .map_or(root, |frame| &frame.directory)
                .open_dir_nofollow(Path::new(name))
                .map_err(|source| source_error(path, source))?;
            #[cfg(unix)]
            let identity = {
                use cap_fs_ext::MetadataExt;
                let metadata = directory
                    .dir_metadata()
                    .map_err(|source| source_error(path, source))?;
                (metadata.dev(), metadata.ino())
            };
            cursor.frames.push(SourceDirectoryFrame {
                directory,
                #[cfg(unix)]
                name: name.to_os_string(),
                #[cfg(unix)]
                identity,
            });
        }
        Ok(cursor)
    }

    /// Revalidate every retained ancestor before granting a sibling source open.
    fn validate<'cursor>(
        &'cursor self,
        root: &'cursor Dir,
        path: &Path,
        control: &IndexWorkControl,
    ) -> FsResult<&'cursor Dir> {
        let mut parent = root;
        for frame in &self.frames {
            control.check(IndexWorkStage::SourceHash)?;
            #[cfg(unix)]
            {
                use cap_fs_ext::MetadataExt;
                let metadata = parent
                    .symlink_metadata(Path::new(&frame.name))
                    .map_err(|source| source_error(path, source))?;
                if !metadata.is_dir()
                    || metadata.is_symlink()
                    || (metadata.dev(), metadata.ino()) != frame.identity
                {
                    return Err(source_error(
                        path,
                        io::Error::other(
                            "source parent identity changed after directory admission",
                        ),
                    ));
                }
            }
            #[cfg(windows)]
            {
                // Every name is pinned by no-delete sharing, but write sharing
                // still permits setting a junction in place on an empty directory.
                // Query the held object freshly, without following a new pathname
                // or treating ordinary Cloud Files tags as source indirection.
                if !direct_directory(&frame.directory)
                    .map_err(|source| source_error(path, source))?
                {
                    return Err(source_error(
                        path,
                        io::Error::other("source parent became indirect after directory admission"),
                    ));
                }
            }
            parent = &frame.directory;
        }
        Ok(parent)
    }
}

/// Query only fresh attributes for ordinary held directories; reparse points
/// retain the canonical backend's full classification, including Cloud Files.
#[cfg(windows)]
fn direct_directory(directory: &Dir) -> io::Result<bool> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let attributes = winx::winapi_util::file::information(directory)?.file_attributes();
    if attributes & u64::from(FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        let metadata = directory.dir_metadata()?;
        return Ok(metadata.is_dir() && !metadata.is_symlink());
    }
    Ok(attributes & u64::from(FILE_ATTRIBUTE_DIRECTORY) != 0)
}

/// One directly opened ancestor, plus Unix identity for fresh name revalidation.
#[derive(Debug)]
struct SourceDirectoryFrame {
    /// Canonical no-follow directory capability.
    directory: Dir,
    /// Single native name beneath the previous retained parent.
    #[cfg(unix)]
    name: OsString,
    /// Device/inode captured from the opened directory, never ambient metadata.
    #[cfg(unix)]
    identity: (u64, u64),
}

/// Nonblocking reservation for extra handles across concurrent source walkers.
#[derive(Debug)]
struct DirectoryHandlePermit {
    /// Atomic live occupancy; dropping never requires a lock or filesystem I/O.
    occupied: &'static AtomicUsize,
    /// Full chain reservation, including partially constructed cursors.
    count: usize,
}

impl DirectoryHandlePermit {
    /// Reserve only when the complete chain fits, otherwise leave occupancy alone.
    fn acquire(occupied: &'static AtomicUsize, count: usize, limit: usize) -> Option<Self> {
        occupied
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(count).filter(|next| *next <= limit)
            })
            .ok()
            .map(|_| Self { occupied, count })
    }
}

impl Drop for DirectoryHandlePermit {
    fn drop(&mut self) {
        self.occupied.fetch_sub(self.count, Ordering::Relaxed);
    }
}

/// Reserve a small fraction of Unix descriptors; unknown capacity disables reuse.
fn directory_handle_limit() -> usize {
    #[cfg(unix)]
    {
        use nix::sys::resource::{Resource, getrlimit};
        getrlimit(Resource::RLIMIT_NOFILE)
            .ok()
            .map_or(0, |(soft, _)| {
                usize::try_from(soft / 8)
                    .unwrap_or(usize::MAX)
                    .min(DIRECTORY_HANDLE_CEILING)
            })
    }
    #[cfg(windows)]
    {
        DIRECTORY_HANDLE_CEILING
    }
    #[cfg(not(any(unix, windows)))]
    {
        0
    }
}

/// Only native handle-capacity errors permit the lower-handle traversal retry.
fn directory_handle_capacity_error(error: &FsError) -> bool {
    let FsError::Io { source, .. } = error else {
        return false;
    };
    #[cfg(unix)]
    {
        matches!(
            source.raw_os_error(),
            Some(nix::libc::EMFILE | nix::libc::ENFILE)
        )
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            ERROR_NO_SYSTEM_RESOURCES, ERROR_TOO_MANY_OPEN_FILES,
        };
        source.raw_os_error().is_some_and(|code| {
            code == ERROR_TOO_MANY_OPEN_FILES as i32 || code == ERROR_NO_SYSTEM_RESOURCES as i32
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = source;
        false
    }
}

/// Open and admit the regular leaf identically for cached and uncached parents.
fn open_source_leaf(
    parent: &Dir,
    name: &Path,
    path: &Path,
    admitted_metadata: &std::fs::Metadata,
) -> FsResult<(File, std::fs::Metadata)> {
    // This boundary accepts exactly one child. Native openat's no-follow flag
    // protects the leaf, so an intermediate component must never reach it.
    let mut components = name.components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(component)), None) if name.as_os_str() == component
    ) {
        return Err(source_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "source leaf must be one direct child",
            ),
        ));
    }
    #[cfg(unix)]
    let file = {
        use nix::fcntl::{OFlag, openat};
        use nix::sys::stat::Mode;

        // The parent is already an admitted capability; the basename needs no
        // general multi-component resolver. Keep no-follow, nonblocking special
        // file refusal and close-on-exec, with ownership transferred safely.
        let descriptor = openat(
            parent,
            name,
            OFlag::O_RDONLY
                | OFlag::O_NOFOLLOW
                | OFlag::O_NONBLOCK
                | OFlag::O_NOCTTY
                | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|source| source_error(path, source.into()))?;
        File::from(descriptor)
    };
    #[cfg(not(unix))]
    let file = {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        parent
            .open_with(name, &options)
            .map_err(|source| source_error(path, source))?
            .into_std()
    };
    let opened_metadata = file
        .metadata()
        .map_err(|source| source_error(path, source))?;
    if !opened_metadata.is_file() {
        return Err(source_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "selected source is not a regular file",
            ),
        ));
    }
    // The walker uses ambient paths while this reader retains the
    // selected root. Unix allows that root to move during discovery;
    // bind the opened source to the walker's admitted file object so
    // replacement-tree metadata cannot label bytes from the old root.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if admitted_metadata.dev() != opened_metadata.dev()
            || admitted_metadata.ino() != opened_metadata.ino()
        {
            return Err(source_error(
                path,
                io::Error::other("source file identity changed after metadata admission"),
            ));
        }
    }
    // Windows directory capabilities exclude delete sharing while
    // resolving the root/parents. File replacement remains allowed;
    // the enclosing exact source revalidation still owns publication.
    #[cfg(not(unix))]
    let _ = admitted_metadata;
    Ok((file, opened_metadata))
}

/// Preserve the caller's source path together with the native error.
fn source_error(path: &Path, source: io::Error) -> FsError {
    FsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::IndexCancellation;
    use std::error::Error;
    use std::fs;
    use std::io::Read;

    /// Read exact bytes through the actual discovery-owned source opener.
    fn read_source(reader: &SourceRootReader, path: &Path) -> FsResult<Vec<u8>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let mut bytes = Vec::new();
        reader
            .open_file(
                path,
                &fs::symlink_metadata(path).map_err(|source| source_error(path, source))?,
                &control,
            )?
            .0
            .read_to_end(&mut bytes)
            .map_err(|source| source_error(path, source))?;
        Ok(bytes)
    }

    #[test]
    fn source_reads_preserve_native_names_hardlinks_and_open_file_replacement()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let directory = root.join("nested λ").join("sources");
        fs::create_dir_all(&directory)?;
        let path = directory.join("source λ.rs");
        let bytes = b"fn before_save() {}\n";
        fs::write(&path, bytes)?;
        let hardlink = directory.join("hardlink.rs");
        fs::hard_link(&path, &hardlink)?;
        let reader = SourceRootReader::open(&root)?;
        for source in [&path, &hardlink] {
            if read_source(&reader, source)? != bytes {
                return Err("source reader changed ordinary or hardlinked source bytes".into());
            }
        }
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (mut retained, _) = reader.open_file(&path, &fs::symlink_metadata(&path)?, &control)?;
        let replacement = directory.join("saved.rs");
        let saved = b"fn after_save() {}\n";
        fs::write(&replacement, saved)?;
        fs::rename(&replacement, &path)?;
        let mut original = Vec::new();
        retained.read_to_end(&mut original)?;
        if original != bytes
            || read_source(&reader, &path)? != saved
            || read_source(&reader, &hardlink)? != bytes
        {
            return Err(
                "source capability broke file replacement or retained file semantics".into(),
            );
        }
        let metadata = fs::metadata(&path)?;
        if !matches!(reader.open_file(&root.join("absent.rs"), &metadata, &control), Err(FsError::Io { source, .. })
            if source.kind() == io::ErrorKind::NotFound)
            || reader.open_file(&directory, &metadata, &control).is_ok()
            || reader
                .open_file(&root.join("../outside.rs"), &metadata, &control)
                .is_ok()
        {
            return Err("source capability changed absent, nonregular or parent admission".into());
        }
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let cancelled = IndexWorkControl::new(cancellation, None);
        if !matches!(
            reader.open_file(&path, &metadata, &cancelled),
            Err(FsError::IndexWork(_))
        ) {
            return Err("source component traversal ignored cancellation".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_reader_preserves_names_accepted_by_native_filesystem() -> Result<(), Box<dyn Error>> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let reader = SourceRootReader::open(root)?;
        let unicode = root.join("source-λ.rs");
        fs::write(&unicode, b"native Unicode source")?;
        if read_source(&reader, &unicode)? != b"native Unicode source" {
            return Err("source reader replaced native Unicode path components".into());
        }
        let directory = root.join(OsStr::from_bytes(b"native-\xff"));
        let path = directory.join(OsStr::from_bytes(b"source-\xfe.rs"));
        // APFS permits only valid UTF-8 names. Probe the native creation boundary,
        // not the source reader: an EILSEQ from an actual read must still fail.
        // Other native creation errors (permissions, storage, etc.) also fail.
        // https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html
        match fs::create_dir(&directory).and_then(|()| fs::write(&path, b"native source bytes")) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(nix::libc::EILSEQ) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if read_source(&reader, &path)? != b"native source bytes" {
            return Err("source reader replaced native path components".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_leaf_preserves_native_open_flags_and_refuses_indirect_or_nonregular_children()
    -> Result<(), Box<dyn Error>> {
        use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;

        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let path = root.join("source.rs");
        fs::write(&path, b"direct source")?;
        let metadata = fs::symlink_metadata(&path)?;
        let reader = SourceRootReader::open(root)?;
        let (mut file, _) =
            open_source_leaf(&reader.directory, Path::new("source.rs"), &path, &metadata)?;
        let flags = OFlag::from_bits_truncate(fcntl(&file, FcntlArg::F_GETFL)?);
        let descriptor_flags = FdFlag::from_bits_truncate(fcntl(&file, FcntlArg::F_GETFD)?);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes != b"direct source"
            || !flags.contains(OFlag::O_NONBLOCK)
            || !descriptor_flags.contains(FdFlag::FD_CLOEXEC)
        {
            return Err("direct source open changed bytes or native descriptor flags".into());
        }
        std::os::unix::fs::symlink(&path, root.join("alias.rs"))?;
        fs::create_dir(root.join("directory"))?;
        mkfifo(&root.join("pipe.rs"), Mode::S_IRUSR | Mode::S_IWUSR)?;
        for name in ["alias.rs", "directory", "pipe.rs"] {
            if open_source_leaf(
                &reader.directory,
                Path::new(name),
                &root.join(name),
                &metadata,
            )
            .is_ok()
            {
                return Err(
                    "source leaf followed indirection or admitted a nonregular file".into(),
                );
            }
        }
        fs::create_dir(root.join("nested"))?;
        fs::hard_link(&path, root.join("nested/source.rs"))?;
        for name in [
            Path::new("nested/source.rs"),
            Path::new("../source.rs"),
            Path::new("source.rs/"),
            Path::new("."),
            &path,
        ] {
            if !matches!(open_source_leaf(&reader.directory, name, &path, &metadata),
                Err(FsError::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput)
            {
                return Err(
                    "native leaf boundary accepted a selector beyond one direct child".into(),
                );
            }
        }
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn source_reader_allows_selected_root_alias_without_retaining_an_idle_handle()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        fs::write(root.join("source.rs"), b"selected root source")?;
        let alias = temp.path().join("selected");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, &alias)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&root, &alias)?;
        let reader = SourceRootReader::open(&alias)?;
        if read_source(&reader, &alias.join("source.rs"))? != b"selected root source" {
            return Err("intentional selected root alias was refused".into());
        }
        #[cfg(windows)]
        if fs::rename(&root, temp.path().join("blocked-during-discovery")).is_ok() {
            return Err("Windows source directory capability lost rename exclusion".into());
        }
        drop(reader);
        let moved = temp.path().join("moved");
        fs::rename(&root, &moved)?;
        if read_source(&SourceRootReader::open(&moved)?, &moved.join("source.rs"))?
            != b"selected root source"
        {
            return Err("completed discovery prevented the next selected root move".into());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn source_reader_refuses_junction_ancestor_after_directory_admission()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let directory = root.join("src");
        let redirected = root.join("other");
        fs::create_dir_all(&directory)?;
        fs::create_dir(&redirected)?;
        fs::write(directory.join("source.rs"), b"direct source")?;
        fs::write(redirected.join("source.rs"), b"redirected source")?;
        let reader = SourceRootReader::open(&root)?;
        if read_source(&reader, &directory.join("source.rs"))? != b"direct source" {
            return Err("direct source fixture was not admitted".into());
        }
        fs::rename(&directory, root.join("saved-directory"))?;
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&directory)
            .arg(&redirected)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "junction fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        if read_source(&reader, &directory.join("source.rs")).is_ok() {
            return Err(
                "source reader followed an admitted directory replaced by a junction".into(),
            );
        }
        fs::remove_dir(&directory)?;
        fs::rename(root.join("saved-directory"), &directory)?;
        if read_source(&reader, &directory.join("source.rs"))? != b"direct source" {
            return Err("restored direct directory was not admitted".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "source_cursor_tests.rs"]
mod cursor_tests;

// Only the isolated native fixture calls Windows Cloud Files APIs directly.
#[cfg(all(test, windows))]
#[allow(unsafe_code)]
#[path = "source_cloud.rs"]
mod cloud_files;
