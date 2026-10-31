//! Handle-bound Linux access policy and unnamed replacement preparation.

use super::{ProjectStateGuard, refusal, require_regular};
use rustix::fs::{AtFlags, CWD, Mode, OFlags, linkat, openat};
use std::fs::{File, Metadata, Permissions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use xattr::FileExt;

/// Kernel-owned access attributes, excluding content-bound signatures and socket/dir labels.
const ACCESS_ATTRIBUTES: [&str; 6] = [
    "system.posix_acl_access",
    "system.nfs4_acl",
    "security.selinux",
    "security.SMACK64",
    "security.SMACK64EXEC",
    "security.SMACK64MMAP",
];

/// A retained regular destination and its opaque native access policy.
pub(super) struct ReplacementPolicy {
    /// Keep the snapshotted inode alive through publication.
    source: File,
    /// Ownership, mode and filesystem identity associated with that inode.
    metadata: Metadata,
    /// Supported native attributes, including verified absence.
    attributes: Vec<(&'static str, Option<Vec<u8>>)>,
}

impl ReplacementPolicy {
    /// Snapshot through a no-follow descriptor, without requiring data-read access on write-only files.
    pub(super) fn capture(path: &Path, expected: &Metadata) -> io::Result<Self> {
        let flags = OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        let source = match openat(CWD, path, flags | OFlags::RDONLY, Mode::empty()) {
            Ok(file) => file,
            Err(rustix::io::Errno::ACCESS) => {
                openat(CWD, path, flags | OFlags::WRONLY, Mode::empty())?
            }
            Err(source) => return Err(source.into()),
        };
        let source = File::from(source);
        let metadata = source.metadata()?;
        require_regular(path, &metadata)?;
        if metadata.dev() != expected.dev() || metadata.ino() != expected.ino() {
            return Err(refusal(
                path,
                "destination object changed before access capture",
            ));
        }
        let mut attributes = Vec::with_capacity(ACCESS_ATTRIBUTES.len());
        for name in ACCESS_ATTRIBUTES {
            match source.get_xattr(name) {
                Ok(value) => attributes.push((name, value)),
                Err(source) if source.kind() == io::ErrorKind::Unsupported => {}
                Err(source) => return Err(source),
            }
        }
        Ok(Self {
            source,
            metadata,
            attributes,
        })
    }

    /// Create a probe with the destination's ordinary mode, filtered by current umask.
    pub(super) fn permissions(&self) -> Permissions {
        self.metadata.permissions()
    }

    /// Prove the complete observable policy already matches, without any setters.
    pub(super) fn matches(&self, file: &File) -> io::Result<bool> {
        let current = file.metadata()?;
        if current.dev() != self.metadata.dev()
            || current.uid() != self.metadata.uid()
            || current.gid() != self.metadata.gid()
            || current.mode() != self.metadata.mode()
        {
            return Ok(false);
        }
        for (name, value) in &self.attributes {
            if file.get_xattr(name)? != *value {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Refuse replacement after the admitted destination or its captured policy changes.
    pub(super) fn verify_destination(&self, path: &Path) -> io::Result<()> {
        let current = std::fs::symlink_metadata(path)?;
        require_regular(path, &current)?;
        if current.dev() != self.metadata.dev()
            || current.ino() != self.metadata.ino()
            || !self.matches(&self.source)?
        {
            return Err(refusal(path, "destination object or access policy changed"));
        }
        Ok(())
    }

    /// Apply only changed attributes, then prove the complete policy before naming.
    pub(super) fn apply(&self, file: &File) -> io::Result<()> {
        let current = file.metadata()?;
        if current.uid() != self.metadata.uid() || current.gid() != self.metadata.gid() {
            std::os::unix::fs::fchown(file, Some(self.metadata.uid()), Some(self.metadata.gid()))?;
        }
        for (name, value) in &self.attributes {
            if file.get_xattr(name)? == *value {
                continue;
            }
            if let Some(value) = value {
                file.set_xattr(name, value)?;
            } else {
                file.remove_xattr(name)?;
            }
        }
        if file.metadata()?.mode() != self.metadata.mode() {
            file.set_permissions(self.metadata.permissions())?;
        }
        if !self.matches(file)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "anonymous stage access policy differs from destination",
            ));
        }
        Ok(())
    }

    /// Prepare the complete replacement while unnamed; only then expose a staging name.
    pub(super) fn anonymous_stage(
        &self,
        builder: &tempfile::Builder<'_, '_>,
        guard: &ProjectStateGuard,
        bytes: &[u8],
    ) -> io::Result<tempfile::NamedTempFile> {
        builder.make_in(&guard.directory, |path| {
            guard.verify_directories()?;
            let mut file = File::from(openat(
                CWD,
                &guard.directory,
                OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?);
            // Complete data I/O under the creator's policy while the file has no
            // name. A changed SELinux SID may revoke writes on this very handle.
            file.write_all(bytes)?;
            file.sync_all()?;
            self.apply(&file)?;
            file.sync_all()?;
            if !self.matches(&file)? {
                return Err(refusal(
                    path,
                    "anonymous stage access policy changed while writing",
                ));
            }
            guard.verify_directories()?;
            // AT_EMPTY_PATH requires CAP_DAC_READ_SEARCH. The proc-fd route is
            // the documented unprivileged link alternative for O_TMPFILE.
            match linkat(&file, "", CWD, path, AtFlags::EMPTY_PATH) {
                Ok(()) => {}
                Err(rustix::io::Errno::PERM | rustix::io::Errno::NOENT) => {
                    guard.verify_directories()?;
                    let descriptor = format!("/proc/self/fd/{}", file.as_raw_fd());
                    linkat(CWD, descriptor.as_str(), CWD, path, AtFlags::SYMLINK_FOLLOW)?;
                }
                Err(source) => return Err(source.into()),
            }
            Ok(file)
        })
    }
}
