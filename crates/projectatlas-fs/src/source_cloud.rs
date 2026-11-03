//! Isolated native Cloud Files proof for the real scanner's no-follow source reads.
//!
//! This test-only provider serves fixed bytes in an owned temporary sync root.
//! It uses official Windows bindings, no account, network or installed provider.

use crate::{ScanOptions, scan_repo};
use std::fs;
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Storage::CloudFilters::*;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES,
};

/// Exact fixed provider payload; page size makes the transfer range unambiguous.
const SOURCE_BYTES: [u8; 4096] = [b'x'; 4096];
/// Child routing stays private to this single native test.
const WORKER_ROOT_ENV: &str = "PROJECTATLAS_CLOUD_SOURCE_TEST_ROOT";
/// Written only after the child completed both actual scans and native cleanup.
const COMPLETION_FILE: &str = "cloud-source-proof-complete";
/// Exact completion payload prevents a zero-test harness exit from proving work.
const COMPLETION_BYTES: &[u8] = b"online-source,nested-population,hydrated-source,cleanup\n";
/// Exact self-test selector, avoiding a second provider or recursive test run.
const TEST_SELECTOR: &str =
    "source_reader::cloud_files::source_scan_hydrates_online_cloud_placeholders";
/// Fetch callbacks observed in the isolated child process.
static FETCH_COUNT: AtomicU32 = AtomicU32::new(0);
/// Directory populations requested by the isolated native scanner.
static POPULATION_COUNT: AtomicU32 = AtomicU32::new(0);
/// First native transfer failure, retained without unwinding across the callback.
static TRANSFER_FAILURE: AtomicI32 = AtomicI32::new(0);

/// Return a terminated native path with no replacement-character conversion.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Preserve the failing official API name and native HRESULT.
fn check(operation: &str, result: i32) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::other(format!(
            "{operation} failed: HRESULT {result:#010x}"
        )))
    } else {
        Ok(())
    }
}

/// Serve the complete fixed file from memory when Windows requests hydration.
unsafe extern "system" fn fetch_data(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: Cloud Files supplies this callback structure for this invocation.
    // Its connection/transfer/request keys are copied only while it is valid.
    let info = unsafe { &*info };
    let operation = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        ..CF_OPERATION_INFO::default()
    };
    let mut parameters = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: 0,
                Buffer: SOURCE_BYTES.as_ptr().cast(),
                Offset: 0,
                Length: SOURCE_BYTES.len() as i64,
            },
        },
    };
    // SAFETY: Both structures have official native layout and correct sizes.
    // The payload is static, valid for the entire synchronous transfer call.
    FETCH_COUNT.fetch_add(1, Ordering::Release);
    let result = unsafe { CfExecute(&raw const operation, &raw mut parameters) };
    if result < 0 {
        let _first_failure =
            TRANSFER_FAILURE.compare_exchange(0, result, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Populate the one partial directory with an online source child on demand.
unsafe extern "system" fn fetch_placeholders(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: Windows provides this callback structure for this invocation only.
    let info = unsafe { &*info };
    let name = wide(Path::new("nested-source.rs"));
    let identity = [1_u8];
    let mut placeholder = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: name.as_ptr(),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                CreationTime: 116_444_736_000_000_000,
                LastAccessTime: 116_444_736_000_000_000,
                LastWriteTime: 116_444_736_000_000_000,
                ChangeTime: 116_444_736_000_000_000,
                FileAttributes: FILE_ATTRIBUTE_NORMAL,
            },
            FileSize: SOURCE_BYTES.len() as i64,
        },
        FileIdentity: identity.as_ptr().cast(),
        FileIdentityLength: 1,
        Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
        ..CF_PLACEHOLDER_CREATE_INFO::default()
    };
    let operation = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        ..CF_OPERATION_INFO::default()
    };
    let mut parameters = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_4>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferPlaceholders: CF_OPERATION_PARAMETERS_0_4 {
                Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_DISABLE_ON_DEMAND_POPULATION,
                CompletionStatus: 0,
                PlaceholderTotalCount: 1,
                PlaceholderArray: &raw mut placeholder,
                PlaceholderCount: 1,
                EntriesProcessed: 0,
            },
        },
    };
    POPULATION_COUNT.fetch_add(1, Ordering::Release);
    // SAFETY: The official sized operation, one placeholder, name and identity
    // remain live until synchronous transfer returns; only our partial child
    // directory can request this callback. No caller-supplied paths are used.
    let result = unsafe { CfExecute(&raw const operation, &raw mut parameters) };
    if result < 0 {
        let _first_failure =
            TRANSFER_FAILURE.compare_exchange(0, result, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Own exactly one temporary registration and connection, including failure cleanup.
struct Provider {
    /// NUL-terminated owned temporary sync-root path.
    root: Vec<u16>,
    /// Connection exists only between successful connect and disconnect.
    connection: Option<CF_CONNECTION_KEY>,
    /// Registration exists until explicit successful cleanup or RAII fallback.
    registered: bool,
}

impl Provider {
    /// Report normal cleanup failures, retaining RAII fallback for early errors.
    fn finish(mut self) -> io::Result<()> {
        if let Some(connection) = self.connection {
            // SAFETY: This key was returned by this fixture's successful connect.
            check("CfDisconnectSyncRoot", unsafe {
                CfDisconnectSyncRoot(connection)
            })?;
            self.connection = None;
        }
        // SAFETY: This fixture owns the registration at this live terminated path.
        check("CfUnregisterSyncRoot", unsafe {
            CfUnregisterSyncRoot(self.root.as_ptr())
        })?;
        self.registered = false;
        Ok(())
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            // SAFETY: Only this fixture owns this still-live connection key.
            let _cleanup = unsafe { CfDisconnectSyncRoot(connection) };
        }
        if self.registered {
            // SAFETY: The path buffer outlives this cleanup of our own registration.
            let _cleanup = unsafe { CfUnregisterSyncRoot(self.root.as_ptr()) };
        }
    }
}

/// Query physical hydration state using metadata-only access, without data recall.
fn on_disk_bytes(path: &Path) -> io::Result<i64> {
    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut info = CF_PLACEHOLDER_STANDARD_INFO::default();
    let mut returned = 0;
    // SAFETY: The live file handle, correctly aligned official structure and its
    // exact buffer size remain valid until the synchronous API returns. The
    // fixture's one-byte identity fits the trailing FileIdentity field.
    check("CfGetPlaceholderInfo", unsafe {
        CfGetPlaceholderInfo(
            file.as_raw_handle(),
            CF_PLACEHOLDER_INFO_STANDARD,
            (&raw mut info).cast(),
            size_of::<CF_PLACEHOLDER_STANDARD_INFO>() as u32,
            &raw mut returned,
        )
    })?;
    Ok(info.OnDiskDataSize)
}

/// Exercise baseline followed reads, then actual scanner hydration and warm reads.
fn run_provider(root: &Path) -> io::Result<()> {
    let provider_name: Vec<u16> = "ProjectAtlas source-read test"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let provider_version: Vec<u16> = "1".encode_utf16().chain(Some(0)).collect();
    let mut provider = Provider {
        root: wide(root),
        connection: None,
        registered: false,
    };
    let registration = CF_SYNC_REGISTRATION {
        StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
        ProviderName: provider_name.as_ptr(),
        ProviderVersion: provider_version.as_ptr(),
        ProviderId: windows_sys::core::GUID::from_u128(0xf92b67b9_fef4_47cc_8b31_629f1e80ac78),
        ..CF_SYNC_REGISTRATION::default()
    };
    let policies = CF_SYNC_POLICIES {
        StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
        Hydration: CF_HYDRATION_POLICY {
            Primary: CF_HYDRATION_POLICY_FULL,
            Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
        },
        Population: CF_POPULATION_POLICY {
            Primary: CF_POPULATION_POLICY_FULL,
            Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
        },
        ..CF_SYNC_POLICIES::default()
    };
    // SAFETY: All strings, root path and official input structures are live and
    // correctly sized. The worker root is a trusted test-harness input; the
    // normal parent invocation supplies its own empty TempDir.
    check("CfRegisterSyncRoot", unsafe {
        CfRegisterSyncRoot(
            provider.root.as_ptr(),
            &raw const registration,
            &raw const policies,
            CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT,
        )
    })?;
    provider.registered = true;
    let callbacks = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: Some(fetch_data),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
            Callback: Some(fetch_placeholders),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];
    let mut connection = 0;
    // SAFETY: The callback table is terminated, its functions are process-static,
    // and no borrowed callback context is retained. This fixture owns the root.
    check("CfConnectSyncRoot", unsafe {
        CfConnectSyncRoot(
            provider.root.as_ptr(),
            callbacks.as_ptr(),
            std::ptr::null(),
            CF_CONNECT_FLAG_NONE,
            &raw mut connection,
        )
    })?;
    provider.connection = Some(connection);
    let metadata = fs::metadata(root)?;
    for (name, is_directory) in [
        ("baseline.rs", false),
        ("source.rs", false),
        ("nested", true),
    ] {
        let name = wide(Path::new(name));
        let identity = [1_u8];
        let mut placeholder = CF_PLACEHOLDER_CREATE_INFO {
            RelativeFileName: name.as_ptr(),
            FsMetadata: CF_FS_METADATA {
                BasicInfo: FILE_BASIC_INFO {
                    CreationTime: metadata.creation_time() as i64,
                    LastAccessTime: metadata.last_access_time() as i64,
                    LastWriteTime: metadata.last_write_time() as i64,
                    ChangeTime: metadata.last_write_time() as i64,
                    FileAttributes: if is_directory {
                        FILE_ATTRIBUTE_DIRECTORY
                    } else {
                        FILE_ATTRIBUTE_NORMAL
                    },
                },
                FileSize: if is_directory {
                    0
                } else {
                    SOURCE_BYTES.len() as i64
                },
            },
            FileIdentity: identity.as_ptr().cast(),
            FileIdentityLength: identity.len() as u32,
            Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
            ..CF_PLACEHOLDER_CREATE_INFO::default()
        };
        let mut processed = 0;
        // SAFETY: One mutable official structure, filename and identity are live
        // for this synchronous call. It creates only a fixed child of our root.
        check("CfCreatePlaceholders", unsafe {
            CfCreatePlaceholders(
                provider.root.as_ptr(),
                &raw mut placeholder,
                1,
                CF_CREATE_FLAG_NONE,
                &raw mut processed,
            )
        })?;
        check("placeholder result", placeholder.Result)?;
        if processed != 1 {
            return Err(io::Error::other("placeholder was not created"));
        }
    }
    let source = root.join("source.rs");
    if on_disk_bytes(&source)? != 0 || on_disk_bytes(&root.join("baseline.rs"))? != 0 {
        return Err(io::Error::other(
            "fixture did not start with online-only placeholders",
        ));
    }
    if fs::read(root.join("baseline.rs"))? != SOURCE_BYTES {
        return Err(io::Error::other(
            "ordinary followed-open hydration failed its baseline",
        ));
    }
    if on_disk_bytes(&source)? != 0 {
        return Err(io::Error::other("baseline unexpectedly hydrated source"));
    }
    let before = FETCH_COUNT.load(Ordering::Acquire);
    let expected = blake3::hash(&SOURCE_BYTES).to_hex().to_string();
    for _ in 0..2 {
        let nodes = scan_repo(root, &ScanOptions::default()).map_err(io::Error::other)?;
        if !["source.rs", "nested/nested-source.rs"].iter().all(|path| {
            nodes
                .iter()
                .any(|node| node.path == *path && node.content_hash.as_ref() == Some(&expected))
        }) {
            return Err(io::Error::other(
                "scanner changed online or hydrated source bytes",
            ));
        }
    }
    if POPULATION_COUNT.load(Ordering::Acquire) == 0
        || on_disk_bytes(&root.join("nested/nested-source.rs"))? < SOURCE_BYTES.len() as i64
    {
        return Err(io::Error::other(
            "scanner did not populate and hydrate the nested source",
        ));
    }
    if FETCH_COUNT.load(Ordering::Acquire) <= before
        || on_disk_bytes(&source)? < SOURCE_BYTES.len() as i64
    {
        return Err(io::Error::other(
            "scanner read did not hydrate the online source",
        ));
    }
    provider.finish()?;
    check(
        "CfExecute transfer",
        TRANSFER_FAILURE.load(Ordering::Acquire),
    )
}

#[test]
fn source_scan_hydrates_online_cloud_placeholders() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(root) = std::env::var_os(WORKER_ROOT_ENV) {
        let root = Path::new(&root);
        run_provider(root)?;
        fs::write(root.join(COMPLETION_FILE), COMPLETION_BYTES)?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", TEST_SELECTOR, "--nocapture"])
        .env(WORKER_ROOT_ENV, temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if started.elapsed() > Duration::from_secs(30) {
            // Exiting between try_wait and kill is harmless; always reap below.
            let _kill_result = child.kill();
            break true;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let output = child.wait_with_output()?;
    if timed_out || !output.status.success() {
        // SAFETY: The child has terminated and the parent owns this exact TempDir.
        // Recover a fixture registration if termination interrupted child cleanup.
        let _cleanup = unsafe { CfUnregisterSyncRoot(wide(temp.path()).as_ptr()) };
    }
    if timed_out || !output.status.success() {
        return Err(io::Error::other(format!(
            "native Cloud Files fixture failed (timeout={timed_out}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    if fs::read(temp.path().join(COMPLETION_FILE))? != COMPLETION_BYTES {
        return Err(
            io::Error::other("Cloud Files worker did not complete actual source proof").into(),
        );
    }
    Ok(())
}
