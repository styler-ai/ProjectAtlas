//! Direct source reads anchored to one discovery-owned directory capability.

use crate::{FsError, FsResult};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use projectatlas_core::{IndexWorkControl, IndexWorkStage};
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Root capability retained only while the current source discovery runs.
#[derive(Debug)]
pub(crate) struct SourceRootReader {
    /// Selected spelling used for repository-relative component admission.
    root: PathBuf,
    /// Open selected root; source descendants never resolve from ambient paths.
    directory: Dir,
}

impl SourceRootReader {
    /// Open the already selected root, allowing its intentional root alias.
    pub(crate) fn open(root: &Path) -> FsResult<Self> {
        let directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|source| source_error(root, source))?;
        Ok(Self {
            root: root.to_path_buf(),
            directory,
        })
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
                let mut options = OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No);
                #[cfg(unix)]
                {
                    use cap_fs_ext::OpenOptionsExt;
                    options.custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY);
                }
                let file = parent
                    .open_with(Path::new(name), &options)
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
                            io::Error::other(
                                "source file identity changed after metadata admission",
                            ),
                        ));
                    }
                }
                // Windows directory capabilities exclude delete sharing while
                // resolving the root/parents. File replacement remains allowed;
                // the enclosing exact source revalidation still owns publication.
                #[cfg(not(unix))]
                let _ = admitted_metadata;
                return Ok((file, opened_metadata));
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
    fn source_reader_uses_native_non_utf8_components() -> Result<(), Box<dyn Error>> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let directory = root.join(OsStr::from_bytes(b"native-\xff"));
        fs::create_dir(&directory)?;
        let path = directory.join(OsStr::from_bytes(b"source-\xfe.rs"));
        fs::write(&path, b"native source bytes")?;
        if read_source(&SourceRootReader::open(root)?, &path)? != b"native source bytes" {
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

// Only the isolated native fixture calls Windows Cloud Files APIs directly.
#[cfg(all(test, windows))]
#[allow(unsafe_code)]
#[path = "source_cloud.rs"]
mod cloud_files;
