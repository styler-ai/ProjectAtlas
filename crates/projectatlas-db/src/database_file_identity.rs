//! Retained native database identity bound to the actual opened SQLite file.

use crate::AtlasStore;
use rusqlite::Connection;
#[cfg(unix)]
use std::fs::{self, Metadata};
#[cfg(unix)]
use std::mem::ManuallyDrop;
#[cfg(unix)]
use std::ops::{Deref, DerefMut};
use std::path::Path;
#[cfg(unix)]
use std::sync::{Arc, Mutex, OnceLock};

/// Keep SQLite in charge of the descriptor while optional identities outlive a store.
///
/// The live owner remains `Send` and not `Sync`, exactly like `Connection`. Its
/// lazy lease exposes no connection operations and can be shared with observers.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct DatabaseConnection {
    /// Present for the entire live owner; moved exactly once by `Drop`.
    connection: ManuallyDrop<Connection>,
    /// Allocate a lifetime handoff only when native identity is actually captured.
    identity_lease: OnceLock<Arc<Mutex<ConnectionLeaseState>>>,
}

/// Other platforms keep their existing connection ownership and conservative identity.
#[cfg(not(unix))]
pub(crate) type DatabaseConnection = Connection;

/// Adopt the platform's connection ownership without changing other platforms.
pub(crate) fn own_connection(connection: Connection) -> DatabaseConnection {
    #[cfg(unix)]
    {
        connection.into()
    }
    #[cfg(not(unix))]
    {
        connection
    }
}

/// One shared native-object lifetime, never one connection per identity clone.
#[cfg(unix)]
#[derive(Debug)]
enum ConnectionLeaseState {
    /// The original connection is still owned by its store.
    Active,
    /// The store retired successfully; retain only its idle SQLite-owned resource.
    Retained(Connection),
    /// Failed cleanup cannot leave a reusable native key as admission authority.
    Invalid,
}

#[cfg(unix)]
impl ConnectionLeaseState {
    /// Validity is separate from equality of the captured native keys.
    fn is_valid(&self) -> bool {
        match self {
            Self::Active => true,
            Self::Retained(connection) => connection.is_autocommit(),
            Self::Invalid => false,
        }
    }
}

#[cfg(unix)]
impl From<Connection> for DatabaseConnection {
    fn from(connection: Connection) -> Self {
        Self {
            connection: ManuallyDrop::new(connection),
            identity_lease: OnceLock::new(),
        }
    }
}

#[cfg(unix)]
impl Deref for DatabaseConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

#[cfg(unix)]
impl DerefMut for DatabaseConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

#[cfg(unix)]
impl DatabaseConnection {
    /// Share the existing SQLite connection's lifetime without opening another path.
    fn retain_identity(&self) -> Arc<Mutex<ConnectionLeaseState>> {
        Arc::clone(
            self.identity_lease
                .get_or_init(|| Arc::new(Mutex::new(ConnectionLeaseState::Active))),
        )
    }
}

#[cfg(unix)]
impl Drop for DatabaseConnection {
    #[allow(
        unsafe_code,
        reason = "Drop is the sole move out of the live connection, so SQLite can own final descriptor close without a second open or a panicking dereference"
    )]
    fn drop(&mut self) {
        // Invalidate before taking the resource: unwinding during cleanup must
        // never leave an identity claiming that a now-closed object is pinned.
        let mut lease = self.identity_lease.get().map(|lease| {
            lease
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        });
        if let Some(state) = lease.as_mut() {
            **state = ConnectionLeaseState::Invalid;
        }
        // SAFETY: construction initializes this value, live dereferences only
        // borrow it, and this Drop invocation is its sole move. ManuallyDrop
        // suppresses a second destructor after the local value is transferred
        // or dropped below. No reference to this wrapper escapes its destruction.
        let connection = unsafe { ManuallyDrop::take(&mut self.connection) };
        let Some(mut state) = lease else {
            return;
        };
        // Closing a store used to roll back unfinished work. Keep that behavior
        // before retaining an idle file object; an identity must not hold a read
        // snapshot or an uncommitted write after the store is gone.
        if !connection.is_autocommit() && connection.execute_batch("ROLLBACK").is_err() {
            return;
        }
        connection.flush_prepared_statement_cache();
        if connection.release_memory().is_err() {
            return;
        }
        *state = ConnectionLeaseState::Retained(connection);
    }
}

/// A retained native file object captured from a verified opened database.
///
/// The original SQLite-owned connection prevents native identity reuse while an
/// observer retains this witness. Failed retirement invalidates its authority.
/// Unsupported native/VFS identity produces no witness; callers must
/// keep their conservative handling of namespace events in that case.
#[derive(Clone, Debug)]
pub struct DatabaseFileIdentity {
    /// Retain SQLite's own file lifetime; never close an independent descriptor.
    #[cfg(unix)]
    lease: Arc<Mutex<ConnectionLeaseState>>,
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
        #[cfg(unix)]
        {
            let Ok(lease) = self.lease.lock() else {
                return false;
            };
            // Follow supported database aliases just as the former file open
            // did, without opening/closing the inode or waiting on a FIFO.
            lease.is_valid()
                && fs::metadata(path).ok().as_ref().and_then(native_key) == Some(self.key)
        }
        #[cfg(not(unix))]
        {
            let _path = path;
            false
        }
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
fn capture_database_file_identity(connection: &DatabaseConnection) -> Option<DatabaseFileIdentity> {
    #[cfg(unix)]
    {
        let path = Path::new(connection.path()?);
        let key = native_key(&fs::metadata(path).ok()?)?;
        let identity = DatabaseFileIdentity {
            lease: connection.retain_identity(),
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

/// Obtain only complete native identifiers supported by this witness.
#[cfg(unix)]
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
    use super::{DatabaseConnection, capture_database_file_identity};
    use rusqlite::Connection;
    use std::error::Error;
    use std::fs;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    /// Execute one real peer connection without sharing this process's VFS state.
    fn peer_connection(
        path: &std::path::Path,
        mode: &str,
        expected: i64,
    ) -> Result<(), Box<dyn Error>> {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "database_file_identity::tests::sqlite_lock_peer_process",
                "--nocapture",
            ])
            .env("PROJECTATLAS_TEST_SQLITE_LOCK_PATH", path)
            .env("PROJECTATLAS_TEST_SQLITE_LOCK_MODE", mode)
            .env(
                "PROJECTATLAS_TEST_SQLITE_LOCK_EXPECTED",
                expected.to_string(),
            )
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "SQLite peer {mode} failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn sqlite_lock_peer_process() -> Result<(), Box<dyn Error>> {
        let Some(path) = std::env::var_os("PROJECTATLAS_TEST_SQLITE_LOCK_PATH") else {
            return Ok(());
        };
        let mode = std::env::var("PROJECTATLAS_TEST_SQLITE_LOCK_MODE")?;
        let expected: i64 = std::env::var("PROJECTATLAS_TEST_SQLITE_LOCK_EXPECTED")?.parse()?;
        if mode == "shared-lock" {
            use nix::fcntl::{FcntlArg, fcntl};
            let file = fs::File::open(path)?;
            // SQLite's POSIX shared-byte region; querying from a separate
            // process observes the parent's kernel locks without acquiring one.
            let mut lock = nix::libc::flock {
                l_type: nix::libc::F_WRLCK as _,
                l_whence: nix::libc::SEEK_SET as _,
                l_start: 0x4000_0002,
                l_len: 510,
                l_pid: 0,
            };
            fcntl(&file, FcntlArg::F_GETLK(&mut lock))?;
            if i32::from(lock.l_type) == nix::libc::F_UNLCK || i64::from(lock.l_pid) != expected {
                return Err(format!("SQLite main-file shared lock lost: owner={}, kind={}, expected owner={expected}", lock.l_pid, lock.l_type).into());
            }
            return Ok(());
        }
        let peer = Connection::open(path)?;
        peer.busy_timeout(Duration::ZERO)?;
        match mode.as_str() {
            "wal-write" | "wal-read" => {
                let value: i64 =
                    peer.query_row("SELECT MAX(value) FROM lock_probe", [], |row| row.get(0))?;
                if value != expected {
                    return Err(format!(
                        "peer observed {value}, expected committed value {expected}"
                    )
                    .into());
                }
                if mode == "wal-write" {
                    peer.execute("INSERT INTO lock_probe VALUES(?1)", [expected + 1])?;
                }
            }
            "metadata-read" | "metadata-write" | "metadata-checkpoint" => {
                let value: i64 = peer.query_row(
                    "SELECT CAST(value AS INTEGER) FROM metadata WHERE key='identity-lease-test'",
                    [],
                    |row| row.get(0),
                )?;
                if value != expected {
                    return Err(format!("peer metadata value {value}, expected {expected}").into());
                }
                if mode != "metadata-read" {
                    peer.execute(
                        "UPDATE metadata SET value=?1 WHERE key='identity-lease-test'",
                        [expected + 1],
                    )?;
                }
                if mode == "metadata-checkpoint" {
                    let busy: i64 =
                        peer.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
                    if busy != 0 {
                        return Err(
                            "retired store kept a read snapshot or write transaction".into()
                        );
                    }
                }
            }
            "reserved" => {
                let result = peer.execute_batch("BEGIN IMMEDIATE");
                if expected == 0 {
                    match result {
                        Err(rusqlite::Error::SqliteFailure(error, _))
                            if error.code == rusqlite::ErrorCode::DatabaseBusy => {}
                        other => {
                            return Err(
                                format!("peer bypassed a live reserved lock: {other:?}").into()
                            );
                        }
                    }
                } else {
                    result?;
                    peer.execute_batch("ROLLBACK")?;
                }
            }
            _ => return Err("unknown SQLite lock peer mode".into()),
        }
        Ok(())
    }

    fn verify_live_wal_identity(operation: &str) -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("selected.db");
        let mut connection: DatabaseConnection = Connection::open(&path)?.into();
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE lock_probe(value INTEGER); INSERT INTO lock_probe VALUES(1);")?;
        peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
        let mut identity =
            Some(capture_database_file_identity(&connection).ok_or("identity missing")?);
        if operation != "capture" {
            drop(connection);
            connection = Connection::open(&path)?.into();
            let _: i64 =
                connection.query_row("SELECT MAX(value) FROM lock_probe", [], |row| row.get(0))?;
            peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
            if operation == "validate" {
                if !identity
                    .as_ref()
                    .ok_or("identity missing")?
                    .matches_path(&path)
                {
                    return Err("unchanged database identity was refused".into());
                }
            } else {
                drop(identity.take());
            }
        }
        peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
        peer_connection(&path, "wal-write", 1)?;
        if !path.with_extension("db-wal").is_file() || !path.with_extension("db-shm").is_file() {
            return Err(format!("identity {operation} let a peer remove the live WAL/SHM").into());
        }
        let value: i64 =
            connection.query_row("SELECT MAX(value) FROM lock_probe", [], |row| row.get(0))?;
        if value != 2 {
            return Err(format!("identity {operation} hid the peer commit").into());
        }
        connection.execute("INSERT INTO lock_probe VALUES(3)", [])?;
        peer_connection(&path, "wal-read", 3)?;
        drop(identity);
        drop(connection);
        Ok(())
    }

    #[test]
    fn database_identity_preserves_live_wal_during_capture() -> Result<(), Box<dyn Error>> {
        verify_live_wal_identity("capture")
    }

    #[test]
    fn database_identity_preserves_live_wal_during_validate() -> Result<(), Box<dyn Error>> {
        verify_live_wal_identity("validate")
    }

    #[test]
    fn database_identity_preserves_live_wal_during_retire() -> Result<(), Box<dyn Error>> {
        verify_live_wal_identity("retire")
    }

    fn verify_reserved_identity_lock(operation: &str) -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("selected.db");
        let connection: DatabaseConnection = Connection::open(&path)?.into();
        connection
            .execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE lock_probe(value INTEGER);")?;
        let mut identity = if operation == "capture" {
            None
        } else {
            Some(capture_database_file_identity(&connection).ok_or("identity missing")?)
        };
        connection.execute_batch("BEGIN IMMEDIATE")?;
        match operation {
            "capture" => {
                identity =
                    Some(capture_database_file_identity(&connection).ok_or("identity missing")?);
            }
            "validate" => {
                if !identity
                    .as_ref()
                    .ok_or("identity missing")?
                    .matches_path(&path)
                {
                    return Err("unchanged reserved database identity was refused".into());
                }
            }
            "retire" => drop(identity.take()),
            _ => return Err("unknown identity lock operation".into()),
        }
        peer_connection(&path, "reserved", 0)?;
        connection.execute_batch("ROLLBACK")?;
        peer_connection(&path, "reserved", 1)?;
        drop(identity);
        Ok(())
    }

    #[test]
    fn database_identity_preserves_reserved_lock_during_capture() -> Result<(), Box<dyn Error>> {
        verify_reserved_identity_lock("capture")
    }

    #[test]
    fn database_identity_preserves_reserved_lock_during_validate() -> Result<(), Box<dyn Error>> {
        verify_reserved_identity_lock("validate")
    }

    #[test]
    fn database_identity_preserves_reserved_lock_during_retire() -> Result<(), Box<dyn Error>> {
        verify_reserved_identity_lock("retire")
    }

    #[test]
    fn store_identity_clones_preserve_locks_after_store_and_final_clone_drop()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("selected.db");
        let alias = temp.path().join("alias.db");
        let store = crate::AtlasStore::open(&path)?;
        store.connection.execute(
            "INSERT INTO metadata(key,value) VALUES('identity-lease-test','1')",
            [],
        )?;
        std::os::unix::fs::symlink(&path, &alias)?;
        let identity = store
            .database_file_identity()
            .ok_or("store identity missing")?;
        let second = store
            .database_file_identity()
            .ok_or("second identity missing")?;
        let clone = identity.clone();
        if !std::sync::Arc::ptr_eq(&identity.lease, &second.lease)
            || !identity.matches_path(&alias)
            || !identity.matches_store(&store)
        {
            return Err("store identities did not share their original connection/alias".into());
        }
        drop(store);
        peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
        if !clone.matches_path(&alias) || identity != clone {
            return Err("store retirement lost retained native identity".into());
        }
        let other = Connection::open(&path)?;
        let _: i64 = other.query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))?;
        drop(identity);
        drop(second);
        peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
        drop(clone);
        peer_connection(&path, "shared-lock", i64::from(std::process::id()))?;
        peer_connection(&path, "metadata-write", 1)?;
        let value: i64 = other.query_row(
            "SELECT CAST(value AS INTEGER) FROM metadata WHERE key='identity-lease-test'",
            [],
            |row| row.get(0),
        )?;
        if value != 2 {
            return Err("peer commit became invisible after final identity close".into());
        }
        other.execute(
            "UPDATE metadata SET value='3' WHERE key='identity-lease-test'",
            [],
        )?;
        peer_connection(&path, "metadata-read", 3)?;
        Ok(())
    }

    #[test]
    fn store_retirement_releases_read_and_write_transactions_before_retaining_identity()
    -> Result<(), Box<dyn Error>> {
        for writing in [false, true] {
            let temp = tempfile::tempdir()?;
            let path = temp.path().join("selected.db");
            let store = crate::AtlasStore::open(&path)?;
            store.connection.execute(
                "INSERT INTO metadata(key,value) VALUES('identity-lease-test','1')",
                [],
            )?;
            if writing {
                store.connection.execute_batch("BEGIN IMMEDIATE; UPDATE metadata SET value='99' WHERE key='identity-lease-test'")?;
            } else {
                store.begin_index_read_snapshot()?;
                let _: i64 =
                    store
                        .connection
                        .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))?;
            }
            let identity = store
                .database_file_identity()
                .ok_or("store identity missing")?;
            drop(store);
            if !identity.matches_path(&path) {
                return Err("successful retirement invalidated its pin".into());
            }
            peer_connection(&path, "metadata-checkpoint", 1)?;
            peer_connection(&path, "metadata-read", 2)?;
        }
        Ok(())
    }

    #[test]
    fn failed_store_retirement_invalidates_identity_and_closes_through_sqlite()
    -> Result<(), Box<dyn Error>> {
        use rusqlite::hooks::{AuthContext, Authorization};
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("selected.db");
        let store = crate::AtlasStore::open(&path)?;
        store.connection.execute_batch("INSERT INTO metadata(key,value) VALUES('identity-lease-test','1'); BEGIN IMMEDIATE; UPDATE metadata SET value='99' WHERE key='identity-lease-test';")?;
        let identity = store
            .database_file_identity()
            .ok_or("store identity missing")?;
        let clone = identity.clone();
        store
            .connection
            .authorizer(Some(|_: AuthContext<'_>| Authorization::Deny))?;
        drop(store);
        if identity.matches_path(&path) || clone.matches_path(&path) || identity != clone {
            return Err(
                "failed retirement retained authority or broke captured-key equality".into(),
            );
        }
        peer_connection(&path, "metadata-checkpoint", 1)?;
        peer_connection(&path, "metadata-read", 2)?;
        Ok(())
    }

    #[test]
    fn retired_store_close_preserves_a_replacement_databases_wal() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("selected.db");
        let saved = temp.path().join("retired.db");
        let store = crate::AtlasStore::open(&path)?;
        store.connection.execute(
            "INSERT INTO metadata(key,value) VALUES('identity-lease-test','1')",
            [],
        )?;
        let identity = store
            .database_file_identity()
            .ok_or("store identity missing")?;
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let source = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            if source.exists() {
                fs::rename(
                    &source,
                    std::path::PathBuf::from(format!("{}{suffix}", saved.display())),
                )?;
            }
        }
        let replacement = crate::AtlasStore::open(&path)?;
        replacement.connection.execute(
            "INSERT INTO metadata(key,value) VALUES('identity-lease-test','10')",
            [],
        )?;
        if identity.matches_store(&replacement) || identity.matches_path(&path) {
            return Err("retired native identity admitted its replacement".into());
        }
        drop(identity);
        if !path.with_extension("db-wal").is_file() || !path.with_extension("db-shm").is_file() {
            return Err("retired connection removed replacement WAL/SHM".into());
        }
        peer_connection(&path, "metadata-write", 10)?;
        let value: i64 = replacement.connection.query_row(
            "SELECT CAST(value AS INTEGER) FROM metadata WHERE key='identity-lease-test'",
            [],
            |row| row.get(0),
        )?;
        if value != 11 {
            return Err("replacement publication became invisible".into());
        }
        Ok(())
    }

    #[test]
    fn database_identity_rejects_fifo_replacement_without_waiting_for_a_writer()
    -> Result<(), Box<dyn Error>> {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        use std::os::unix::fs::OpenOptionsExt;

        for capture in [false, true] {
            let temp = tempfile::tempdir()?;
            let selected = temp.path().join("selected.db");
            let connection: DatabaseConnection = Connection::open(&selected)?.into();
            connection.execute_batch("CREATE TABLE identity(value TEXT);")?;
            let identity = capture_database_file_identity(&connection).ok_or("identity missing")?;
            fs::rename(&selected, temp.path().join("original.db"))?;
            mkfifo(&selected, Mode::S_IRUSR | Mode::S_IWUSR)?;
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
                fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
                    .open(&selected)
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
        let connection: DatabaseConnection = Connection::open(&selected)?.into();
        connection.execute_batch(
            "CREATE TABLE identity(value TEXT); INSERT INTO identity VALUES('same');",
        )?;
        let original = capture_database_file_identity(&connection).ok_or("identity missing")?;
        if !original.matches_connection(&connection) {
            return Err("unchanged opened database lost its native identity".into());
        }
        let replacement = temp.path().join("replacement.db");
        connection.backup(rusqlite::MAIN_DB, &replacement, None)?;
        drop(connection);
        fs::rename(&selected, temp.path().join("original.db"))?;
        fs::rename(&replacement, &selected)?;
        let copied: DatabaseConnection = Connection::open(&selected)?.into();
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
        let original: DatabaseConnection = Connection::open(&selected)?.into();
        original.execute_batch("CREATE TABLE identity(value TEXT);")?;
        let identity = capture_database_file_identity(&original).ok_or("identity missing")?;
        drop(original);
        let saved = temp.path().join("original.db");
        fs::rename(&selected, &saved)?;
        let replacement: DatabaseConnection = Connection::open(&selected)?.into();
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
