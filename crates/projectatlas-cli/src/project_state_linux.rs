//! Handle-bound Linux access policy and unnamed replacement preparation.

use super::{
    ProjectStateGuard, containing_directory, refusal, require_regular,
    resolve_existing_parent_prefix,
};
use rustix::fs::{
    AtFlags, CWD, Mode, OFlags, StatxAttributes, StatxFlags, fstatfs, ioctl_getflags, linkat,
    openat, statx,
};
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

/// Linux UAPI bits not named by the locked rustix `IFlags` (which retains them).
const CASEFOLD_FLAG: u32 = 0x4000_0000;
/// A sealed inode cannot be replaced with an unsealed generated file.
const VERITY_FLAG: u32 = 0x0010_0000;

/// Project fixed ASCII names under Linux's frozen Unicode 12.1 NFDICF model.
///
/// Derived from official UCD 12.1, independently checked against every assigned
/// scalar: 19 assigned default-ignorable ranges and 13 ASCII-only expansions.
/// Canonical reordering cannot make a remaining non-ASCII output equal ASCII.
/// This is deliberately not a general Unicode normalizer or APFS comparison.
pub(super) fn fixed_name_matches(name: &str, expected: &str) -> bool {
    if !expected.is_ascii() {
        return false;
    }
    let mut remaining = expected.as_bytes();
    for character in name.chars() {
        let ascii = [character.to_ascii_lowercase() as u8];
        let chunk: &[u8] = match character {
            '\u{00ad}'
            | '\u{034f}'
            | '\u{061c}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
            | '\u{ffa0}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
            | '\u{e0100}'..='\u{e01ef}' => b"",
            '\u{00df}' | '\u{1e9e}' => b"ss",
            '\u{017f}' => b"s",
            '\u{037e}' => b";",
            '\u{1fef}' => b"`",
            '\u{212a}' => b"k",
            '\u{fb00}' => b"ff",
            '\u{fb01}' => b"fi",
            '\u{fb02}' => b"fl",
            '\u{fb03}' => b"ffi",
            '\u{fb04}' => b"ffl",
            '\u{fb05}' | '\u{fb06}' => b"st",
            character if character.is_ascii() => &ascii,
            _ => return false,
        };
        for byte in chunk {
            match remaining.split_first() {
                Some((candidate, suffix)) if candidate.to_ascii_lowercase() == *byte => {
                    remaining = suffix;
                }
                _ => return false,
            }
        }
    }
    remaining.is_empty()
}

/// Compare only an already established potential managed namespace component.
pub(super) fn named(path: &Path, expected: &str) -> io::Result<bool> {
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| refusal(path, "managed filename is not UTF-8"))?;
    let parent = resolve_existing_parent_prefix(containing_directory(path))?;
    let mut prefix = parent.as_path();
    let directory = loop {
        match openat(
            CWD,
            prefix,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(directory) => break directory,
            Err(rustix::io::Errno::NOENT) => {
                prefix = prefix
                    .parent()
                    .ok_or_else(|| refusal(path, "managed containing directory is unavailable"))?;
            }
            Err(source) => return Err(source.into()),
        }
    };
    // Linux superblock magic numbers are the low 32 bits, including on signed
    // native fsword_t targets. Keep driver policy here, after namespace proof.
    let filesystem = fstatfs(&directory)?.f_type as u32;
    let flags = match ioctl_getflags(&directory) {
        Ok(flags) => flags.bits(),
        Err(rustix::io::Errno::NOTTY | rustix::io::Errno::OPNOTSUPP)
            if matches!(filesystem, 0x0102_1994 | 0x9123_683e) =>
        {
            0
        }
        Err(source) => return Err(source.into()),
    };
    let folded = flags & CASEFOLD_FLAG != 0;
    match filesystem {
        // Mounted ext4/f2fs encoding maps select UTF-8 12.1, not a userspace
        // locale or current Unicode library version.
        0xef53 | 0xf2f5_2010 => Ok(folded && fixed_name_matches(name, expected)),
        // XFS's legacy native insensitive model is ASCII-only.
        0x5846_5342 => Ok(folded && name.eq_ignore_ascii_case(expected)),
        // These native sensitive models require a flag for any casefold mode.
        0x0102_1994 | 0x9123_683e if !folded => Ok(false),
        _ => Err(refusal(
            path,
            "managed case-alias filesystem comparison model is unknown",
        )),
    }
}

/// Query a retained source's sealing state without opening a mutable pathname.
fn require_unsealed(file: &File, path: &Path) -> io::Result<()> {
    let sealed = match statx(file, "", AtFlags::EMPTY_PATH, StatxFlags::empty()) {
        Ok(state) if state.stx_attributes_mask.contains(StatxAttributes::VERITY) => {
            Some(state.stx_attributes.contains(StatxAttributes::VERITY))
        }
        Ok(_)
        | Err(rustix::io::Errno::NOSYS | rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOTTY) => {
            None
        }
        Err(source) => return Err(source.into()),
    };
    let sealed = match sealed {
        Some(sealed) => sealed,
        None => match ioctl_getflags(file) {
            Ok(flags) => flags.bits() & VERITY_FLAG != 0,
            // A conforming fs-verity inode is observable through GETFLAGS on
            // Linux 5.4, or statx since 5.5. Both unsupported means the feature
            // is unavailable, not a permission or I/O failure to inspect it.
            Err(rustix::io::Errno::NOTTY | rustix::io::Errno::OPNOTSUPP) => false,
            Err(source) => return Err(source.into()),
        },
    };
    if sealed {
        return Err(refusal(
            path,
            "fs-verity-protected output cannot be replaced",
        ));
    }
    Ok(())
}

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
        require_unsealed(&source, path)?;
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
        require_unsealed(&self.source, path)?;
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
