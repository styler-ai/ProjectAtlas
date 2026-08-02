//! Enforce the supported local-filesystem and `SQLite` connection profile.

use crate::{DbError, DbResult};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::path::{Component, Prefix};

/// Required journal mode for live project databases.
pub(crate) const REQUIRED_JOURNAL_MODE: &str = "wal";
/// Required synchronous mode for authored and derived project state.
const REQUIRED_SYNCHRONOUS_MODE: i64 = 2;
/// Human-readable name for the required synchronous mode.
pub(crate) const REQUIRED_SYNCHRONOUS_NAME: &str = "FULL";
/// Maximum pause between bounded retries while another opener establishes WAL.
const JOURNAL_MODE_RETRY_INTERVAL: Duration = Duration::from_millis(10);
/// Maximum time ordinary read and write connections wait for database contention.
pub(crate) const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum diagnostic length retained from an operating-system error.
const MAX_REASON_CHARS: usize = 512;

/// Known local filesystems for supported Windows hosts.
#[cfg(any(windows, test))]
const WINDOWS_LOCAL_FILESYSTEM_TYPES: &[&str] = &["exfat", "fat", "fat32", "ntfs", "refs"];
/// Known local filesystems for supported Linux hosts and containers.
#[cfg(any(target_os = "linux", test))]
const LINUX_LOCAL_FILESYSTEM_TYPES: &[&str] = &[
    "btrfs", "ext2", "ext3", "ext4", "f2fs", "overlay", "xfs", "zfs",
];
/// Known local filesystems for supported macOS hosts.
#[cfg(any(target_os = "macos", test))]
const MACOS_LOCAL_FILESYSTEM_TYPES: &[&str] =
    &["apfs", "exfat", "fat", "fat32", "hfs", "hfs+", "msdos"];

/// Known network or distributed filesystems that cannot host live WAL state.
const UNSUPPORTED_FILESYSTEM_TYPES: &[&str] = &[
    "9p",
    "afs",
    "ceph",
    "cifs",
    "davfs",
    "glusterfs",
    "lustre",
    "nfs",
    "nfs4",
    "smb",
    "smb2",
    "smb3",
    "smbfs",
    "sshfs",
    "webdav",
];

/// Network-backed FUSE families that are rejected without accepting all FUSE mounts.
const UNSUPPORTED_FILESYSTEM_PREFIXES: &[&str] = &[
    "fuse.ceph",
    "fuse.davfs",
    "fuse.glusterfs",
    "fuse.lustre",
    "fuse.sshfs",
    "fuse.webdav",
];

/// Closed filesystem classification used by the connection gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FilesystemSupport {
    /// The filesystem is a known local implementation with WAL-compatible primitives.
    SupportedLocal,
    /// The filesystem is known to be networked or distributed.
    UnsupportedNetwork,
    /// The filesystem could not be classified safely.
    Uncertain,
}

/// Exact content-free location state captured before a database connection opens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DatabaseLocation {
    /// Whether the database path itself existed at inspection time.
    pub(crate) database_exists: bool,
    /// Canonical file or nearest-existing-parent path used for resolution.
    canonical_probe: PathBuf,
    /// Owning mount point.
    mount_point: PathBuf,
    /// Owning device identity.
    device: OsString,
    /// Normalized filesystem type.
    filesystem_type: String,
}

/// Whether an already-open connection may establish WAL or must already use it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JournalModePolicy {
    /// Establish and verify WAL for a validated create/migration/open path.
    EnsureWal,
    /// Require the existing live database to already use WAL.
    RequireWal,
}

/// Validate the database location without creating its parent or opening `SQLite`.
///
/// # Errors
///
/// Returns a typed error if local WAL-safe filesystem placement cannot be proven.
pub fn validate_database_location(path: &Path) -> DbResult<()> {
    inspect_database_location(path).map(drop)
}

/// Inspect the exact database path or its nearest existing parent.
pub(crate) fn inspect_database_location(path: &Path) -> DbResult<DatabaseLocation> {
    let absolute = absolute_database_path(path)?;
    #[cfg(windows)]
    reject_unsupported_windows_prefix(&absolute)?;
    let (database_exists, probe) = database_probe_path(&absolute)?;
    let resolved = whichdisk::resolve(&probe).map_err(|source| {
        filesystem_uncertain(
            &absolute,
            None,
            None,
            format!("filesystem resolution failed: {}", bounded_reason(&source)),
        )
    })?;
    let mount_point = resolved.mount_point().to_path_buf();
    let device = resolved.device().to_os_string();
    let filesystem_type = resolved.fs_type().trim().to_ascii_lowercase();

    #[cfg(windows)]
    if device.as_os_str() == mount_point.as_os_str() {
        return Err(filesystem_uncertain(
            &absolute,
            Some(&mount_point),
            nonempty_filesystem_type(&filesystem_type),
            "Windows did not resolve a local volume identity".to_string(),
        ));
    }

    match classify_filesystem_type(&filesystem_type) {
        FilesystemSupport::SupportedLocal => Ok(DatabaseLocation {
            database_exists,
            canonical_probe: resolved.canonical_path().to_path_buf(),
            mount_point,
            device,
            filesystem_type,
        }),
        FilesystemSupport::UnsupportedNetwork => Err(DbError::DatabaseFilesystemUnsupported {
            path: absolute,
            mount_point: Some(mount_point),
            filesystem_type: nonempty_filesystem_type(&filesystem_type),
        }),
        FilesystemSupport::Uncertain => Err(filesystem_uncertain(
            &absolute,
            Some(&mount_point),
            nonempty_filesystem_type(&filesystem_type),
            "filesystem type is not in the supported local profile".to_string(),
        )),
    }
}

/// Open one writable connection after revalidating the captured location.
pub(crate) fn open_writable_connection(
    path: &Path,
    flags: OpenFlags,
    expected_location: &DatabaseLocation,
    busy_timeout: Duration,
    journal_policy: JournalModePolicy,
) -> DbResult<Connection> {
    revalidate_database_location(path, expected_location)?;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(busy_timeout)?;
    configure_writable_connection(&connection)?;
    match journal_policy {
        JournalModePolicy::EnsureWal => {
            establish_wal_with_bounded_retry(&connection, busy_timeout)?;
        }
        JournalModePolicy::RequireWal => {}
    }
    verify_journal_mode(&connection)?;
    connection.pragma_update(None, "synchronous", REQUIRED_SYNCHRONOUS_NAME)?;
    verify_synchronous_mode(&connection)?;
    verify_busy_timeout(&connection, busy_timeout)?;
    Ok(connection)
}

/// Establish WAL while another validated opener may be migrating the same database.
fn establish_wal_with_bounded_retry(
    connection: &Connection,
    busy_timeout: Duration,
) -> DbResult<()> {
    let deadline = Instant::now() + busy_timeout;
    loop {
        match connection.pragma_update(None, "journal_mode", REQUIRED_JOURNAL_MODE) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                ) && Instant::now() < deadline =>
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(JOURNAL_MODE_RETRY_INTERVAL.min(remaining));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Open one read-only connection after revalidating the captured location.
pub(crate) fn open_read_only_connection(
    path: &Path,
    expected_location: &DatabaseLocation,
) -> DbResult<Connection> {
    revalidate_database_location(path, expected_location)?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    connection.execute_batch("PRAGMA query_only = ON")?;
    verify_query_only(&connection)?;
    verify_busy_timeout(&connection, SQLITE_BUSY_TIMEOUT)?;
    Ok(connection)
}

/// Verify that a validated current read snapshot observes the live WAL profile.
pub(crate) fn verify_current_read_profile(connection: &Connection) -> DbResult<()> {
    verify_journal_mode(connection)
}

/// Enable and verify connection-local foreign-key enforcement.
pub(crate) fn configure_writable_connection(connection: &Connection) -> DbResult<()> {
    connection.pragma_update(None, "foreign_keys", true)?;
    let enabled =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))?;
    if enabled != 1 {
        return Err(operating_profile_error("foreign_keys", "ON", enabled));
    }
    Ok(())
}

/// Re-resolve a database path immediately before opening it.
fn revalidate_database_location(path: &Path, expected: &DatabaseLocation) -> DbResult<()> {
    let found = inspect_database_location(path)?;
    if &found == expected {
        return Ok(());
    }
    Err(filesystem_uncertain(
        &absolute_database_path(path)?,
        Some(&found.mount_point),
        nonempty_filesystem_type(&found.filesystem_type),
        "filesystem location changed between preflight and connection open".to_string(),
    ))
}

/// Resolve a relative database path without assuming a platform-specific root.
fn absolute_database_path(path: &Path) -> DbResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|source| {
            filesystem_uncertain(
                path,
                None,
                None,
                format!(
                    "current directory could not be resolved: {}",
                    bounded_reason(&source)
                ),
            )
        })
}

/// Return the existing database path or nearest existing parent for volume resolution.
fn database_probe_path(path: &Path) -> DbResult<(bool, PathBuf)> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Ok((true, path.to_path_buf())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(filesystem_uncertain(
                path,
                None,
                None,
                format!(
                    "database metadata could not be inspected: {}",
                    bounded_reason(&source)
                ),
            ));
        }
    }

    let mut candidate = path.parent();
    while let Some(ancestor) = candidate {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => return Ok((false, ancestor.to_path_buf())),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                candidate = ancestor.parent();
            }
            Err(source) => {
                return Err(filesystem_uncertain(
                    path,
                    None,
                    None,
                    format!(
                        "parent metadata could not be inspected: {}",
                        bounded_reason(&source)
                    ),
                ));
            }
        }
    }
    Err(filesystem_uncertain(
        path,
        None,
        None,
        "no existing parent is available for filesystem resolution".to_string(),
    ))
}

/// Classify one filesystem type against the supported host profile.
fn classify_filesystem_type(filesystem_type: &str) -> FilesystemSupport {
    classify_filesystem_type_with_local(filesystem_type, supported_local_filesystem_types())
}

/// Classify one filesystem type against explicit local values for deterministic tests.
fn classify_filesystem_type_with_local(
    filesystem_type: &str,
    supported_local: &[&str],
) -> FilesystemSupport {
    let normalized = filesystem_type.trim().to_ascii_lowercase();
    if UNSUPPORTED_FILESYSTEM_TYPES.contains(&normalized.as_str())
        || UNSUPPORTED_FILESYSTEM_PREFIXES
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
    {
        return FilesystemSupport::UnsupportedNetwork;
    }
    if supported_local.contains(&normalized.as_str()) {
        FilesystemSupport::SupportedLocal
    } else {
        FilesystemSupport::Uncertain
    }
}

/// Known local filesystems for supported Windows hosts.
#[cfg(windows)]
fn supported_local_filesystem_types() -> &'static [&'static str] {
    WINDOWS_LOCAL_FILESYSTEM_TYPES
}

/// Known local filesystems for supported Linux hosts and containers.
#[cfg(target_os = "linux")]
fn supported_local_filesystem_types() -> &'static [&'static str] {
    LINUX_LOCAL_FILESYSTEM_TYPES
}

/// Known local filesystems for supported macOS hosts.
#[cfg(target_os = "macos")]
fn supported_local_filesystem_types() -> &'static [&'static str] {
    MACOS_LOCAL_FILESYSTEM_TYPES
}

/// Conservatively reject unclassified filesystems on other targets.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn supported_local_filesystem_types() -> &'static [&'static str] {
    &[]
}

/// Reject Windows UNC and mapped-drive state before resolving a remote path.
#[cfg(windows)]
fn reject_unsupported_windows_prefix(path: &Path) -> DbResult<()> {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return Err(filesystem_uncertain(
            path,
            None,
            None,
            "absolute Windows database path has no volume prefix".to_string(),
        ));
    };
    let drive = match prefix.kind() {
        Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) => {
            return Err(DbError::DatabaseFilesystemUnsupported {
                path: path.to_path_buf(),
                mount_point: None,
                filesystem_type: None,
            });
        }
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        Prefix::DeviceNS(_) | Prefix::Verbatim(_) => {
            return Err(filesystem_uncertain(
                path,
                None,
                None,
                "Windows device path is not a supported database location".to_string(),
            ));
        }
    };
    let volumes = whichdisk::list().map_err(|source| {
        filesystem_uncertain(
            path,
            None,
            None,
            format!(
                "local Windows volume inventory failed: {}",
                bounded_reason(&source)
            ),
        )
    })?;
    let local = volumes.iter().any(|volume| {
        windows_drive_letter(volume.mount_point())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&drive))
    });
    if local {
        Ok(())
    } else {
        Err(filesystem_uncertain(
            path,
            None,
            None,
            "Windows drive is not present in the local fixed/removable volume inventory"
                .to_string(),
        ))
    }
}

/// Return the Windows drive letter from a mounted local volume path.
#[cfg(windows)]
fn windows_drive_letter(path: &Path) -> Option<u8> {
    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter),
        Prefix::UNC(_, _)
        | Prefix::VerbatimUNC(_, _)
        | Prefix::DeviceNS(_)
        | Prefix::Verbatim(_) => None,
    }
}

/// Verify the durable database journal mode.
fn verify_journal_mode(connection: &Connection) -> DbResult<()> {
    let found =
        connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?;
    if found.eq_ignore_ascii_case(REQUIRED_JOURNAL_MODE) {
        Ok(())
    } else {
        Err(DbError::DatabaseOperatingProfile {
            setting: "journal_mode",
            expected: REQUIRED_JOURNAL_MODE.to_string(),
            found,
        })
    }
}

/// Verify the connection-local durability mode.
fn verify_synchronous_mode(connection: &Connection) -> DbResult<()> {
    let found = connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))?;
    if found == REQUIRED_SYNCHRONOUS_MODE {
        Ok(())
    } else {
        Err(operating_profile_error(
            "synchronous",
            REQUIRED_SYNCHRONOUS_NAME,
            found,
        ))
    }
}

/// Verify that a read connection cannot issue database mutations.
fn verify_query_only(connection: &Connection) -> DbResult<()> {
    let found = connection.pragma_query_value(None, "query_only", |row| row.get::<_, i64>(0))?;
    if found == 1 {
        Ok(())
    } else {
        Err(operating_profile_error("query_only", "ON", found))
    }
}

/// Verify the bounded ordinary-writer wait configured on the connection.
fn verify_busy_timeout(connection: &Connection, expected: Duration) -> DbResult<()> {
    let found = connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, u64>(0))?;
    let expected_millis = expected.as_millis();
    if u128::from(found) == expected_millis {
        Ok(())
    } else {
        Err(DbError::DatabaseOperatingProfile {
            setting: "busy_timeout",
            expected: expected_millis.to_string(),
            found: found.to_string(),
        })
    }
}

/// Build a typed connection-profile postcondition error.
fn operating_profile_error(setting: &'static str, expected: &'static str, found: i64) -> DbError {
    DbError::DatabaseOperatingProfile {
        setting,
        expected: expected.to_string(),
        found: found.to_string(),
    }
}

/// Build a typed uncertain-filesystem error.
fn filesystem_uncertain(
    path: &Path,
    mount_point: Option<&Path>,
    filesystem_type: Option<String>,
    reason: String,
) -> DbError {
    DbError::DatabaseFilesystemUncertain {
        path: path.to_path_buf(),
        mount_point: mount_point.map(Path::to_path_buf),
        filesystem_type,
        reason,
    }
}

/// Return a non-empty filesystem type for diagnostics.
fn nonempty_filesystem_type(filesystem_type: &str) -> Option<String> {
    (!filesystem_type.is_empty()).then(|| filesystem_type.to_string())
}

/// Bound operating-system diagnostics without dropping their root cause.
fn bounded_reason(source: &io::Error) -> String {
    source.to_string().chars().take(MAX_REASON_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn filesystem_classification_distinguishes_local_remote_and_unknown() {
        let local = ["apfs", "ext4", "ntfs", "overlay"];
        assert_eq!(
            classify_filesystem_type_with_local("EXT4", &local),
            FilesystemSupport::SupportedLocal
        );
        assert_eq!(
            classify_filesystem_type_with_local("fuse.sshfs", &local),
            FilesystemSupport::UnsupportedNetwork
        );
        assert_eq!(
            classify_filesystem_type_with_local("", &local),
            FilesystemSupport::Uncertain
        );
        assert_eq!(
            classify_filesystem_type_with_local("unknown-local", &local),
            FilesystemSupport::Uncertain
        );
    }

    #[test]
    fn platform_profiles_keep_ephemeral_memory_filesystems_out_of_durable_storage() {
        assert!(!LINUX_LOCAL_FILESYSTEM_TYPES.contains(&"ramfs"));
        assert!(!LINUX_LOCAL_FILESYSTEM_TYPES.contains(&"tmpfs"));
        assert!(LINUX_LOCAL_FILESYSTEM_TYPES.contains(&"overlay"));
        assert!(WINDOWS_LOCAL_FILESYSTEM_TYPES.contains(&"ntfs"));
        assert!(MACOS_LOCAL_FILESYSTEM_TYPES.contains(&"apfs"));
    }

    #[test]
    fn missing_database_uses_nearest_existing_parent() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database = temp
            .path()
            .join("nested")
            .join("atlas")
            .join("projectatlas.db");
        let location = inspect_database_location(&database)?;
        if location.database_exists {
            return Err(io::Error::other("missing database was reported as existing").into());
        }
        if location.canonical_probe != temp.path().canonicalize()? {
            return Err(io::Error::other("nearest existing parent was not resolved").into());
        }
        if database.exists() {
            return Err(io::Error::other("location inspection created the database").into());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn unc_database_path_is_rejected_before_resolution() {
        let database = PathBuf::from(["", "", "server", "share", "projectatlas.db"].join(r"\"));
        assert!(matches!(
            reject_unsupported_windows_prefix(&database),
            Err(DbError::DatabaseFilesystemUnsupported { .. })
        ));
    }
}
