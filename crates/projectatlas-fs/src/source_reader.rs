//! Direct source reads anchored to one discovery-owned directory capability.

use crate::{FsError, FsResult};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
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
        if result.is_err() {
            self.cursor = None;
        }
        result
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
                self.cursor = Some(SourceDirectoryCursor::open(
                    &self.root.directory,
                    parent,
                    path,
                    permit,
                    control,
                )?);
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
                let metadata = frame
                    .directory
                    .dir_metadata()
                    .map_err(|source| source_error(path, source))?;
                if !metadata.is_dir() || metadata.is_symlink() {
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

/// Open and admit the regular leaf identically for cached and uncached parents.
fn open_source_leaf(
    parent: &Dir,
    name: &Path,
    path: &Path,
    admitted_metadata: &std::fs::Metadata,
) -> FsResult<(File, std::fs::Metadata)> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_fs_ext::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY);
    }
    let file = parent
        .open_with(name, &options)
        .map_err(|source| source_error(path, source))?
        .into_std();
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
