//! Restrictive, identity-bound macOS replacement staging directories.

use super::{ProjectStateGuard, RetainedStaging, refusal, require_directory};
use same_file::Handle;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Operate on an entry itself; never redirect ACL mutation through a leaf symlink.
pub(super) const NOFOLLOW_ACL: exacl::AclOption = exacl::AclOption::SYMLINK_ACL;

/// A non-recursively cleaned workspace, hardened before any payload file exists.
pub(super) struct ReplacementDirectory<'a> {
    /// Admission authority for the root and original publication directory.
    guard: &'a ProjectStateGuard,
    /// Private same-volume directory; automatic TempDir cleanup is disabled.
    path: PathBuf,
    /// Retained identity prevents removing a substituted directory.
    identity: Handle,
}

impl<'a> ReplacementDirectory<'a> {
    /// Isolate every replacement before path-based native ACL operations.
    pub(super) fn create(
        guard: &'a ProjectStateGuard,
        replacing: bool,
    ) -> io::Result<Option<Self>> {
        if !replacing {
            return Ok(None);
        }
        guard.verify_directories()?;
        let path = tempfile::Builder::new()
            .prefix(".publication-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&guard.directory)?
            .keep();
        // Do not let TempDir recursively clean a path before ownership is proved.
        let identity = Handle::from_path(&path).map_err(|operation| {
            io::Error::new(
                operation.kind(),
                RetainedStaging {
                    operation,
                    cleanup: io::Error::other(
                        "private staging directory identity unavailable; retained",
                    ),
                },
            )
        })?;
        let directory = Self {
            guard,
            path,
            identity,
        };
        let hardened = (|| {
            directory.verify()?;
            let file = fs::File::from(rustix::fs::openat(
                rustix::fs::CWD,
                &directory.path,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )?);
            if Handle::from_file(file.try_clone()?)? != directory.identity {
                return Err(refusal(
                    &directory.path,
                    "private staging directory object changed",
                ));
            }
            let current = file.metadata()?;
            if current.uid() != rustix::process::geteuid().as_raw() {
                return Err(refusal(
                    &directory.path,
                    "private staging directory creator changed",
                ));
            }
            exacl::setfacl(&[&directory.path], &[], Some(NOFOLLOW_ACL))?;
            // Keep the workspace owned by this writer. Destination ownership
            // belongs on the payload, never on its private traversal boundary.
            file.set_permissions(fs::Permissions::from_mode(0o700))?;
            directory.verify()?;
            if !exacl::getfacl(&directory.path, Some(NOFOLLOW_ACL))?.is_empty()
                || file.metadata()?.permissions().mode() & 0o777 != 0o700
                || fs::read_dir(&directory.path)?.next().is_some()
            {
                return Err(refusal(
                    &directory.path,
                    "private staging directory could not be made restrictive and empty",
                ));
            }
            Ok(())
        })();
        match hardened {
            Ok(()) => Ok(Some(directory)),
            Err(source) => directory.finish(Err::<(), _>(source)).map(|()| None),
        }
    }

    /// Use this exact private directory for payload creation.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Check both original directory authorities and the direct private object.
    pub(super) fn verify(&self) -> io::Result<()> {
        self.guard.verify_directories()?;
        require_directory(&self.path, &fs::symlink_metadata(&self.path)?)?;
        if Handle::from_path(&self.path)? != self.identity {
            return Err(refusal(
                &self.path,
                "private staging directory object changed",
            ));
        }
        Ok(())
    }

    /// Remove only the verified empty workspace, explicitly retaining uncertain entries.
    pub(super) fn finish<T>(self, publication: io::Result<T>) -> io::Result<T> {
        let cleanup = self.verify().and_then(|()| fs::remove_dir(&self.path));
        match (publication, cleanup) {
            (result, Ok(())) => result,
            (Err(operation), Err(ownership)) => Err(io::Error::new(
                operation.kind(),
                RetainedStaging {
                    operation,
                    cleanup: ownership,
                },
            )),
            (Ok(_), Err(ownership)) => Err(io::Error::new(
                ownership.kind(),
                RetainedStaging {
                    operation: io::Error::other(
                        "publication committed; private staging cleanup failed",
                    ),
                    cleanup: ownership,
                },
            )),
        }
    }
}
