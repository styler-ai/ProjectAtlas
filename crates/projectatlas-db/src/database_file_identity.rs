//! Retained native database identity bound to the actual opened SQLite file.

use crate::AtlasStore;
use rusqlite::Connection;
use std::fs::{File, Metadata};
use std::path::Path;
use std::sync::Arc;

/// A retained native file object captured from a verified opened database.
///
/// The open file prevents native identity reuse while an observer retains this
/// witness. Unsupported native/VFS identity produces no witness; callers must
/// keep their conservative handling of namespace events in that case.
#[derive(Clone, Debug)]
pub struct DatabaseFileIdentity {
    /// Retain the object rather than only a reusable inode number.
    file: Arc<File>,
    /// Native device and inode on supported Unix filesystems.
    key: (u64, u64),
}

impl PartialEq for DatabaseFileIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for DatabaseFileIdentity {}

impl DatabaseFileIdentity {
    /// Check the selected path without interpreting path spelling as file identity.
    #[must_use]
    pub fn matches_path(&self, path: &Path) -> bool {
        self.file.metadata().ok().as_ref().and_then(native_key) == Some(self.key)
            && open_identity_file(path)
                .and_then(|file| file.metadata())
                .ok()
                .as_ref()
                .and_then(native_key)
                == Some(self.key)
    }

    /// Check the actual opened database, including a replacement-and-restore race.
    #[must_use]
    pub fn matches_store(&self, store: &AtlasStore) -> bool {
        self.matches_connection(&store.connection)
    }

    /// Bind the current namespace check to SQLite's own opened file descriptor.
    fn matches_connection(&self, connection: &Connection) -> bool {
        opened_database_still_selected(connection)
            && connection
                .path()
                .is_some_and(|path| self.matches_path(Path::new(path)))
    }
}

impl AtlasStore {
    /// Capture optional native identity for the main file actually read by this store.
    ///
    /// Unknown, inaccessible, in-memory or unsupported VFS/native identities return
    /// `None`. In particular, this does not equate Windows files using a truncated
    /// file index; an observer must retain conservative structural-event handling.
    #[must_use]
    pub fn database_file_identity(&self) -> Option<DatabaseFileIdentity> {
        capture_database_file_identity(&self.connection)
    }
}

/// Capture a retained object and verify its relationship to the live SQLite file.
fn capture_database_file_identity(connection: &Connection) -> Option<DatabaseFileIdentity> {
    #[cfg(unix)]
    {
        let path = Path::new(connection.path()?);
        let file = open_identity_file(path).ok()?;
        let key = native_key(&file.metadata().ok()?)?;
        let identity = DatabaseFileIdentity {
            file: Arc::new(file),
            key,
        };
        identity.matches_connection(connection).then_some(identity)
    }
    #[cfg(not(unix))]
    {
        let _connection = connection;
        None
    }
}

/// Follow supported database aliases without waiting for a FIFO writer.
///
/// The opened object is checked with `fstat` by the caller; a metadata precheck
/// alone cannot prevent replacement between inspection and opening. Symlink
/// selection and canonical-target continuity remain the observer's responsibility.
fn open_identity_file(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::{CWD, Mode, OFlags, openat};
        Ok(File::from(openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
            Mode::empty(),
        )?))
    }
    #[cfg(not(unix))]
    {
        let _path = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "native database file identity is unavailable",
        ))
    }
}

/// Obtain only complete native identifiers supported by this witness.
fn native_key(metadata: &Metadata) -> Option<(u64, u64)> {
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _metadata = metadata;
        None
    }
}

/// Ask the owning VFS, rather than a separate path stat, about the opened file.
fn opened_database_still_selected(connection: &Connection) -> bool {
    #[cfg(unix)]
    {
        crate::schema::opened_database_still_at_path(connection)
    }
    #[cfg(not(unix))]
    {
        let _connection = connection;
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::capture_database_file_identity;
    use rusqlite::Connection;
    use std::error::Error;
    use std::fs;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    #[test]
    fn database_identity_rejects_fifo_replacement_without_waiting_for_a_writer()
    -> Result<(), Box<dyn Error>> {
        use rustix::fs::{CWD, Mode, OFlags, mkfifoat, openat};

        for capture in [false, true] {
            let temp = tempfile::tempdir()?;
            let selected = temp.path().join("selected.db");
            let connection = Connection::open(&selected)?;
            connection.execute_batch("CREATE TABLE identity(value TEXT);")?;
            let identity = capture_database_file_identity(&connection).ok_or("identity missing")?;
            fs::rename(&selected, temp.path().join("original.db"))?;
            mkfifoat(CWD, &selected, Mode::RUSR | Mode::WUSR)?;
            let (send, receive) = sync_channel(1);
            let checked_path = selected.clone();
            let worker = std::thread::spawn(move || {
                let accepted = if capture {
                    capture_database_file_identity(&connection).is_some()
                } else {
                    identity.matches_path(&checked_path)
                };
                let _sent = send.send(accepted);
            });
            let result = receive.recv_timeout(Duration::from_secs(2));
            // Release an accidentally blocking implementation before joining it;
            // corrected code never needs a FIFO writer to finish its check.
            let _writer = if result.is_err() {
                openat(
                    CWD,
                    &selected,
                    OFlags::WRONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .ok()
            } else {
                None
            };
            worker
                .join()
                .map_err(|_panic| "database identity worker panicked")?;
            if result? {
                return Err("FIFO replacement supplied native database identity".into());
            }
        }
        Ok(())
    }

    #[test]
    fn retained_database_identity_rejects_a_copied_replacement() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let selected = temp.path().join("selected.db");
        let connection = Connection::open(&selected)?;
        connection.execute_batch(
            "CREATE TABLE identity(value TEXT); INSERT INTO identity VALUES('same');",
        )?;
        let original = capture_database_file_identity(&connection).ok_or("identity missing")?;
        if !original.matches_connection(&connection) {
            return Err("unchanged opened database lost its native identity".into());
        }
        drop(connection);
        let replacement = temp.path().join("replacement.db");
        fs::copy(&selected, &replacement)?;
        fs::rename(&selected, temp.path().join("original.db"))?;
        fs::rename(&replacement, &selected)?;
        let copied = Connection::open(&selected)?;
        let next = capture_database_file_identity(&copied).ok_or("replacement identity missing")?;
        if original.matches_connection(&copied) || original == next {
            return Err("copied database retained the original native identity".into());
        }
        Ok(())
    }

    #[test]
    fn opened_database_identity_rejects_replacement_then_restore() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let selected = temp.path().join("selected.db");
        let original = Connection::open(&selected)?;
        original.execute_batch("CREATE TABLE identity(value TEXT);")?;
        let identity = capture_database_file_identity(&original).ok_or("identity missing")?;
        drop(original);
        let saved = temp.path().join("original.db");
        fs::rename(&selected, &saved)?;
        let replacement = Connection::open(&selected)?;
        replacement.execute_batch("CREATE TABLE identity(value TEXT);")?;
        fs::rename(&selected, temp.path().join("replacement.db"))?;
        fs::rename(&saved, &selected)?;
        if !identity.matches_path(&selected)
            || identity.matches_connection(&replacement)
            || capture_database_file_identity(&replacement).is_some()
        {
            return Err("restored pathname certified the still-open replacement database".into());
        }
        Ok(())
    }
}
