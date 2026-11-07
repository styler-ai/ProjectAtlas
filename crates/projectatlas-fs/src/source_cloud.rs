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
/// Ordinary-client routing stays private to this single native test.
const CLIENT_ROOT_ENV: &str = "PROJECTATLAS_CLOUD_SOURCE_CLIENT_ROOT";
/// Written only after the ordinary client completed both actual source scans.
const CLIENT_COMPLETION_FILE: &str = "cloud-source-client-proof-complete";
/// Exact client completion payload prevents a zero-test harness exit proving work.
const CLIENT_COMPLETION_BYTES: &[u8] = b"online-source,nested-population,hydrated-source\n";
/// Exact self-test selector, avoiding a second provider or recursive test run.
const TEST_SELECTOR: &str =
    "source_reader::cloud_files::source_scan_hydrates_online_cloud_placeholders";
/// Fetch callbacks served by the parent provider process.
static FETCH_COUNT: AtomicU32 = AtomicU32::new(0);
/// Directory populations requested by the separate ordinary client.
static POPULATION_COUNT: AtomicU32 = AtomicU32::new(0);
/// First native transfer failure, retained without unwinding across the callback.
static TRANSFER_FAILURE: AtomicI32 = AtomicI32::new(0);
/// Keep the registered table and its functions live through provider disconnect.
static CALLBACKS: [CF_CALLBACK_REGISTRATION; 3] = [
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
        if self.registered {
            // SAFETY: This fixture owns the registration at this live terminated path.
            check("CfUnregisterSyncRoot", unsafe {
                CfUnregisterSyncRoot(self.root.as_ptr())
            })?;
            self.registered = false;
        }
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

/// Diagnose whether reopening the captured file object restores normal recall.
/// This is diagnostic-only after an already failed scanner assertion.
fn captured_reopen_hash(root: &Path, path: &Path) -> io::Result<String> {
    use cap_fs_ext::Reopen;
    use projectatlas_core::{IndexCancellation, IndexWorkControl};
    use std::io::Read;

    let control = IndexWorkControl::new(IndexCancellation::new(), None);
    let reader = super::SourceRootReader::open(root).map_err(io::Error::other)?;
    let (file, _) = reader
        .open_file(path, &fs::symlink_metadata(path)?, &control)
        .map_err(io::Error::other)?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    let mut reopened = file.reopen(&options)?;
    let mut bytes = Vec::new();
    reopened.read_to_end(&mut bytes)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// Perform real reads as an ordinary consumer, outside the provider process.
fn run_client(root: &Path) -> io::Result<()> {
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
    let expected = blake3::hash(&SOURCE_BYTES).to_hex().to_string();
    for _ in 0..2 {
        let nodes = scan_repo(root, &ScanOptions::default()).map_err(io::Error::other)?;
        if !["source.rs", "nested/nested-source.rs"].iter().all(|path| {
            nodes
                .iter()
                .any(|node| node.path == *path && node.content_hash.as_ref() == Some(&expected))
        }) {
            // Capture native state before diagnostic followed reads can hydrate
            // anything; a missing row and wrong bytes need different repairs.
            let source_disk = on_disk_bytes(&source);
            let nested_disk = on_disk_bytes(&root.join("nested/nested-source.rs"));
            let source_attributes =
                fs::symlink_metadata(&source).map(|metadata| metadata.file_attributes());
            let reopened_source = captured_reopen_hash(root, &source);
            let reopened_nested = captured_reopen_hash(root, &root.join("nested/nested-source.rs"));
            let ordinary_source =
                fs::read(&source).map(|bytes| blake3::hash(&bytes).to_hex().to_string());
            let ordinary_nested = fs::read(root.join("nested/nested-source.rs"))
                .map(|bytes| blake3::hash(&bytes).to_hex().to_string());
            let observed: Vec<_> = nodes
                .iter()
                .take(8)
                .map(|node| (&node.path, &node.kind, node.size_bytes, &node.content_hash))
                .collect();
            return Err(io::Error::other(format!(
                "scanner changed online or hydrated source bytes: expected={expected}; rows={observed:?}; total_rows={}; source_disk={source_disk:?}; source_attributes={source_attributes:?}; nested_disk={nested_disk:?}; reopened_source={reopened_source:?}; reopened_nested={reopened_nested:?}; ordinary_source={ordinary_source:?}; ordinary_nested={ordinary_nested:?}",
                nodes.len()
            )));
        }
    }
    Ok(())
}

/// Run exactly one ordinary client with a bounded callback-wait lifetime.
fn run_client_process(root: &Path) -> io::Result<()> {
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", TEST_SELECTOR, "--nocapture"])
        .env(CLIENT_ROOT_ENV, root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    let stop_reason = loop {
        match child.try_wait() {
            Ok(Some(_)) => break None,
            Ok(None) => {}
            Err(error) => break Some(format!("waiting for client failed: {error}")),
        }
        if started.elapsed() > Duration::from_secs(30) {
            break Some("client exceeded its 30-second read budget".to_owned());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let mut kill_error = None;
    if let Some(reason) = &stop_reason {
        // Dropping Child does not terminate it. Wait errors and timeouts both
        // require termination before provider disconnection or root cleanup.
        kill_error = child.kill().err();
        let termination_started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if termination_started.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                state => {
                    return Err(io::Error::other(format!(
                        "{reason}; client {} termination/reaping not confirmed: kill_error={kill_error:?}; state={state:?}",
                        child.id()
                    )));
                }
            }
        }
    }
    // Exit was observed above, so output collection cannot wait for a live
    // scanner. This direct child creates no subprocesses holding its pipes.
    let output = child.wait_with_output().map_err(|error| {
        io::Error::other(format!(
            "client output collection failed: {error}; stop_reason={stop_reason:?}; kill_error={kill_error:?}"
        ))
    })?;
    if stop_reason.is_some() || !output.status.success() {
        return Err(io::Error::other(format!(
            "native Cloud Files client failed (stop_reason={stop_reason:?}, kill_error={kill_error:?}, fetches={}, populations={}, transfer={:#010x}): {}{}",
            FETCH_COUNT.load(Ordering::Acquire),
            POPULATION_COUNT.load(Ordering::Acquire),
            TRANSFER_FAILURE.load(Ordering::Acquire),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if fs::read(root.join(CLIENT_COMPLETION_FILE))? != CLIENT_COMPLETION_BYTES {
        return Err(io::Error::other(
            "Cloud Files client did not complete actual source proof",
        ));
    }
    Ok(())
}

/// Register and serve placeholders while the distinct ordinary client reads them.
fn serve_client(root: &Path, provider: &mut Provider) -> io::Result<()> {
    let provider_name: Vec<u16> = "ProjectAtlas source-read test"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let provider_version: Vec<u16> = "1".encode_utf16().chain(Some(0)).collect();
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
    // correctly sized. The parent test owns this empty TempDir and retains
    // provider ownership while the separate ordinary client performs reads.
    check("CfRegisterSyncRoot", unsafe {
        CfRegisterSyncRoot(
            provider.root.as_ptr(),
            &raw const registration,
            &raw const policies,
            CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT,
        )
    })?;
    provider.registered = true;
    let mut connection = 0;
    // SAFETY: The terminated callback table and its functions are process-static,
    // so both outlive disconnect. No borrowed callback context is retained.
    // This fixture owns the root.
    check("CfConnectSyncRoot", unsafe {
        CfConnectSyncRoot(
            provider.root.as_ptr(),
            CALLBACKS.as_ptr(),
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
    if on_disk_bytes(&root.join("source.rs"))? != 0
        || on_disk_bytes(&root.join("baseline.rs"))? != 0
    {
        return Err(io::Error::other(
            "fixture did not start with online-only placeholders",
        ));
    }
    run_client_process(root)?;
    if POPULATION_COUNT.load(Ordering::Acquire) == 0
        || on_disk_bytes(&root.join("nested/nested-source.rs"))? < SOURCE_BYTES.len() as i64
    {
        return Err(io::Error::other(
            "ordinary client did not populate and hydrate the nested source",
        ));
    }
    if FETCH_COUNT.load(Ordering::Acquire) < 3
        || on_disk_bytes(&root.join("source.rs"))? < SOURCE_BYTES.len() as i64
    {
        return Err(io::Error::other(
            "ordinary client did not hydrate all online sources",
        ));
    }
    Ok(())
}

/// Surface cleanup failures after every completed, failed or timed-out client.
fn run_provider(root: &Path) -> io::Result<()> {
    let mut provider = Provider {
        root: wide(root),
        connection: None,
        registered: false,
    };
    let proof = serve_client(root, &mut provider);
    let cleanup = provider.finish();
    match (proof, cleanup) {
        (Err(proof), Err(cleanup)) => Err(io::Error::other(format!(
            "{proof}; provider cleanup also failed: {cleanup}"
        ))),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => check(
            "CfExecute transfer",
            TRANSFER_FAILURE.load(Ordering::Acquire),
        ),
    }
}

#[test]
fn source_scan_hydrates_online_cloud_placeholders() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(root) = std::env::var_os(CLIENT_ROOT_ENV) {
        let root = Path::new(&root);
        run_client(root)?;
        fs::write(root.join(CLIENT_COMPLETION_FILE), CLIENT_COMPLETION_BYTES)?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    run_provider(temp.path())?;
    Ok(())
}
