//! Prove real optional-parser worker failure containment through normal CLI and MCP scan paths.

#![cfg(all(
    feature = "optional-parser-supervisor",
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]

use projectatlas_core::optional_parser_pack::PackPlatform;
use projectatlas_core::symbols::ParserKind;
use projectatlas_db::AtlasStore;
use serde::Deserialize;
use serde_json::Value;
#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use nix::sys::signal::{Signal, kill};
#[cfg(target_os = "linux")]
use nix::unistd::Pid;

/// Exact workflow-provided archive environment variable.
const OPTIONAL_PARSER_ARCHIVE_ENV: &str = "PROJECTATLAS_OPTIONAL_PARSER_ARCHIVE";
/// Repository-local database directory.
const ATLAS_DIR_NAME: &str = ".projectatlas";
/// Repository-local database file.
const ATLAS_DATABASE_FILE_NAME: &str = "projectatlas.db";
/// Number of pending optional files that keeps the one resident worker observable.
const PENDING_OPTIONAL_FILE_COUNT: usize = 1_024;
/// Functions per pending file, keeping the fixture bounded while adding real parser work.
const PENDING_OPTIONAL_FUNCTION_COUNT: usize = 256;
/// Maximum time to observe one exact scan-owned worker subtree.
const PROCESS_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time for the scan to report the worker crash and quiesce.
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time for the supervisor or broker to prove process cleanup.
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum time granted to one test-owned process-inspection helper.
const PROCESS_HELPER_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time granted to one foreground lifecycle command.
const PRODUCT_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Maximum time for one MCP response while a parser worker is suspended.
const MCP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time for a canceled background task to become terminal.
const MCP_TASK_CANCEL_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounded pending MCP protocol lines between the reader thread and test owner.
const MCP_RESPONSE_QUEUE_CAPACITY: usize = 16;
/// Short cooperative polling interval for process state.
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Maximum retained bytes per scan stream while the reader continues draining.
const CAPTURED_SCAN_STREAM_MAX_BYTES: usize = 1024 * 1024;
/// Maximum Linux process rows admitted into one descendant snapshot.
#[cfg(target_os = "linux")]
const LINUX_PROCESS_SNAPSHOT_MAX_ENTRIES: usize = 65_536;
/// Test-only Windows scan-PID input for the fixed process query.
#[cfg(target_os = "windows")]
const WINDOWS_SCAN_PID_ENV: &str = "PROJECTATLAS_TEST_SCAN_PID";
/// Test-only Windows tracked-PID input for the fixed process query.
#[cfg(target_os = "windows")]
const WINDOWS_PROCESS_PIDS_ENV: &str = "PROJECTATLAS_TEST_PROCESS_PIDS";
/// Test-only Windows termination PID input.
#[cfg(target_os = "windows")]
const WINDOWS_TERMINATE_PID_ENV: &str = "PROJECTATLAS_TEST_TERMINATE_PID";
/// Test-only Windows termination executable-name input.
#[cfg(target_os = "windows")]
const WINDOWS_TERMINATE_NAME_ENV: &str = "PROJECTATLAS_TEST_TERMINATE_NAME";
/// Test-only Windows termination process-start input.
#[cfg(target_os = "windows")]
const WINDOWS_TERMINATE_STARTED_ENV: &str = "PROJECTATLAS_TEST_TERMINATE_STARTED";
/// Test-only Windows suspension PID input.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROL_PID_ENV: &str = "PROJECTATLAS_TEST_CONTROL_PID";
/// Test-only Windows suspension executable-name input.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROL_NAME_ENV: &str = "PROJECTATLAS_TEST_CONTROL_NAME";
/// Test-only Windows suspension process-start input.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROL_STARTED_ENV: &str = "PROJECTATLAS_TEST_CONTROL_STARTED";
/// Test-only Windows suspension operation input.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROL_OPERATION_ENV: &str = "PROJECTATLAS_TEST_CONTROL_OPERATION";

/// Minimal captured output from one owned scan process.
struct CapturedOutput {
    /// Final child exit status.
    status: ExitStatus,
    /// Complete bounded command stdout.
    stdout: CapturedStream,
    /// Complete bounded command stderr.
    stderr: CapturedStream,
}

/// One fully drained child stream with bounded retained diagnostics.
struct CapturedStream {
    /// Retained prefix within the fixed diagnostic bound.
    bytes: Vec<u8>,
    /// Whether later drained bytes were discarded.
    truncated: bool,
}

/// Exact operating-system process identity used to avoid PID-only mutation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct ProcessIdentity {
    /// Process identifier.
    pid: u32,
    /// Direct parent process identifier observed during discovery.
    parent_pid: u32,
    /// Exact executable file name.
    name: String,
    /// Platform start token used to reject PID reuse.
    started: String,
}

impl ProcessIdentity {
    /// Return whether another observation is the same process despite reparenting.
    fn is_same_process(&self, other: &Self) -> bool {
        self.pid == other.pid && self.name == other.name && self.started == other.started
    }
}

/// Exact worker subtree expected beneath one normal scan.
struct RuntimeProcesses {
    /// Contained parser worker that receives the fault.
    worker: ProcessIdentity,
    /// Windows containment broker, absent on Linux.
    broker: Option<ProcessIdentity>,
}

impl RuntimeProcesses {
    /// Return every tracked descendant identity for cleanup verification.
    fn tracked(&self) -> Vec<ProcessIdentity> {
        let mut tracked = vec![self.worker.clone()];
        if let Some(broker) = &self.broker {
            tracked.push(broker.clone());
        }
        tracked
    }
}

/// Isolated default host storage used by every product subprocess.
struct HostState {
    /// Synthetic HOME directory.
    home: PathBuf,
    /// Synthetic Windows local application-data directory.
    local_app_data: PathBuf,
    /// Synthetic Unix data directory.
    xdg_data: PathBuf,
}

impl HostState {
    /// Create the isolated default storage roots beneath one test directory.
    fn create(root: &Path) -> io::Result<Self> {
        let state = Self {
            home: root.join("home"),
            local_app_data: root.join("local-app-data"),
            xdg_data: root.join("xdg-data"),
        };
        fs::create_dir_all(&state.home)?;
        fs::create_dir_all(&state.local_app_data)?;
        fs::create_dir_all(&state.xdg_data)?;
        Ok(state)
    }

    /// Apply the default storage roots to one product command.
    fn apply(&self, command: &mut Command) {
        command
            .env("HOME", &self.home)
            .env("LOCALAPPDATA", &self.local_app_data)
            .env("XDG_DATA_HOME", &self.xdg_data);
    }
}

/// Best-effort lifecycle cleanup for early test failure.
struct PackCleanup<'a> {
    /// Selected test repository.
    repo: &'a Path,
    /// Isolated default host state.
    host: &'a HostState,
    /// Whether explicit cleanup already succeeded.
    active: bool,
}

impl<'a> PackCleanup<'a> {
    /// Arm cleanup before parser-pack installation can mutate state.
    const fn new(repo: &'a Path, host: &'a HostState) -> Self {
        Self {
            repo,
            host,
            active: true,
        }
    }

    /// Record successful explicit lifecycle cleanup.
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for PackCleanup<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut command = projectatlas_command(self.repo, self.host);
        command.args(["--format", "json", "parser-pack", "remove"]);
        drop(run_bounded_command(
            &mut command,
            PRODUCT_COMMAND_TIMEOUT,
            "parser-pack cleanup",
        ));
    }
}

/// Owned scan child with pipe draining and fail-safe subtree cleanup.
struct ScanProcess {
    /// Exact normal scan child.
    child: Option<Child>,
    /// Concurrent stdout drain.
    stdout: Option<JoinHandle<io::Result<CapturedStream>>>,
    /// Concurrent stderr drain.
    stderr: Option<JoinHandle<io::Result<CapturedStream>>>,
    /// Exact discovered worker identities owned until a normal scan exit is observed.
    tracked_runtime: Vec<ProcessIdentity>,
    /// Whether exceptional-path subtree cleanup remains armed.
    cleanup_armed: bool,
}

impl ScanProcess {
    /// Spawn one normal JSON scan under isolated default host storage.
    fn spawn(repo: &Path, host: &HostState) -> io::Result<Self> {
        let mut command = projectatlas_command(repo, host);
        command
            .args(["--format", "json", "scan"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("scan stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("scan stderr was not piped"))?;
        Ok(Self {
            child: Some(child),
            stdout: Some(capture_pipe(stdout)),
            stderr: Some(capture_pipe(stderr)),
            tracked_runtime: Vec::new(),
            cleanup_armed: true,
        })
    }

    /// Retain exact worker identities before the test injects its process fault.
    fn track_runtime(&mut self, runtime: &RuntimeProcesses) {
        self.tracked_runtime = runtime.tracked();
    }

    /// Return the exact owned scan PID.
    fn pid(&self) -> io::Result<u32> {
        self.child
            .as_ref()
            .map(Child::id)
            .ok_or_else(|| io::Error::other("scan child was already consumed"))
    }

    /// Return whether the owned scan is still running.
    fn is_running(&mut self) -> io::Result<bool> {
        self.child
            .as_mut()
            .ok_or_else(|| io::Error::other("scan child was already consumed"))?
            .try_wait()
            .map(|status| status.is_none())
    }

    /// Wait boundedly for scan failure while continuing to drain both pipes.
    fn finish(mut self, timeout: Duration) -> io::Result<CapturedOutput> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("scan exit deadline overflowed"))?;
        let status = loop {
            let child = self
                .child
                .as_mut()
                .ok_or_else(|| io::Error::other("scan child was already consumed"))?;
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let runtime_cleanup =
                    force_process_cleanup(&self.tracked_runtime, PROCESS_CLEANUP_TIMEOUT);
                let child = self
                    .child
                    .as_mut()
                    .ok_or_else(|| io::Error::other("scan child was already consumed"))?;
                if let Err(source) = child.kill()
                    && child.try_wait()?.is_none()
                {
                    return Err(source);
                }
                let status = child.wait()?;
                let stdout = join_capture(self.stdout.take(), "stdout")?;
                let stderr = join_capture(self.stderr.take(), "stderr")?;
                self.child.take();
                if runtime_cleanup.is_ok() {
                    self.cleanup_armed = false;
                }
                return Err(io::Error::other(format!(
                    "scan did not exit after worker termination: status={status}; runtime_cleanup={}\nstdout:\n{}\nstderr:\n{}",
                    runtime_cleanup
                        .as_ref()
                        .map_or_else(ToString::to_string, |()| "complete".to_owned()),
                    captured_stream_text(&stdout),
                    captured_stream_text(&stderr)
                )));
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        };
        let stdout = join_capture(self.stdout.take(), "stdout")?;
        let stderr = join_capture(self.stderr.take(), "stderr")?;
        self.child.take();
        self.cleanup_armed = false;
        Ok(CapturedOutput {
            status,
            stdout,
            stderr,
        })
    }
}

impl Drop for ScanProcess {
    fn drop(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        let scan_pid = self.child.as_ref().map(Child::id);
        let mut tracked = self.tracked_runtime.clone();
        if let Some(scan_pid) = scan_pid
            && let Ok(descendants) = runtime_descendants(scan_pid)
        {
            for process in descendants {
                if !tracked
                    .iter()
                    .any(|expected| expected.is_same_process(&process))
                {
                    tracked.push(process);
                }
            }
        }
        drop(force_process_cleanup(&tracked, PROCESS_HELPER_TIMEOUT));
        if let Some(child) = self.child.as_mut() {
            drop(child.kill());
            drop(child.wait());
        }
        self.child.take();
        if let Some(stdout) = self.stdout.take() {
            drop(stdout.join());
        }
        if let Some(stderr) = self.stderr.take() {
            drop(stderr.join());
        }
    }
}

/// Persistent real MCP stdio child used while one background scan remains active.
struct McpProcess {
    /// Exact MCP server process.
    child: Option<Child>,
    /// Owned protocol input.
    stdin: Option<ChildStdin>,
    /// Bounded protocol lines emitted by the reader thread.
    responses: Receiver<io::Result<String>>,
    /// Protocol stdout reader.
    stdout_reader: Option<JoinHandle<()>>,
    /// Concurrent bounded stderr drain.
    stderr_reader: Option<JoinHandle<io::Result<CapturedStream>>>,
    /// Exact optional runtime descendants retained for fail-safe cleanup.
    tracked_runtime: Vec<ProcessIdentity>,
    /// Next unique JSON-RPC request identity.
    next_request_id: u64,
}

impl McpProcess {
    /// Spawn and initialize one MCP server bound to the test repository database.
    fn spawn(repo: &Path, database: &Path, host: &HostState) -> Result<Self, Box<dyn Error>> {
        let mut command = projectatlas_command(repo, host);
        command
            .arg("--db")
            .arg(database)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("MCP stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("MCP stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("MCP stderr was not piped"))?;
        let (responses_sender, responses) = mpsc::sync_channel(MCP_RESPONSE_QUEUE_CAPACITY);
        let stdout_reader = thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let event = match stdout.read_line(&mut line) {
                    Ok(0) => Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "MCP stdout closed",
                    )),
                    Ok(_) => Ok(line),
                    Err(source) => Err(source),
                };
                let terminal = event.is_err();
                match responses_sender.try_send(event) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_event) | TrySendError::Disconnected(_event)) => break,
                }
                if terminal {
                    break;
                }
            }
        });
        let mut process = Self {
            child: Some(child),
            stdin: Some(stdin),
            responses,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(capture_pipe(stderr)),
            tracked_runtime: Vec::new(),
            next_request_id: 1,
        };
        let initialize = process.request(
            "initialize",
            &serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "projectatlas-optional-parser-stall-e2e",
                    "version": "0.1.0"
                }
            }),
        )?;
        if initialize.get("result").is_none() {
            return Err(io::Error::other("MCP initialize response omitted result").into());
        }
        process.notify("notifications/initialized", &serde_json::json!({}))?;
        Ok(process)
    }

    /// Return the exact MCP server PID.
    fn pid(&self) -> Result<u32, Box<dyn Error>> {
        self.child
            .as_ref()
            .map(Child::id)
            .ok_or_else(|| io::Error::other("MCP child was already consumed").into())
    }

    /// Return whether the MCP server is still running.
    fn is_running(&mut self) -> Result<bool, Box<dyn Error>> {
        Ok(self
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("MCP child was already consumed"))?
            .try_wait()?
            .is_none())
    }

    /// Retain exact runtime descendants for exceptional-path cleanup.
    fn track_runtime(&mut self, runtime: &RuntimeProcesses) {
        self.tracked_runtime = runtime.tracked();
    }

    /// Call one production MCP tool and return its text content.
    fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
        let response = self.request(
            "tools/call",
            &serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        if response
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return Err(
                io::Error::other(format!("MCP tool {name} returned an error: {response}")).into(),
            );
        }
        response
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| io::Error::other(format!("MCP tool {name} returned no text")).into())
    }

    /// Send one request and receive its matching response within the fixed deadline.
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, Box<dyn Error>> {
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("MCP request identity overflowed"))?;
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        }))?;
        let deadline = Instant::now()
            .checked_add(MCP_RESPONSE_TIMEOUT)
            .ok_or_else(|| io::Error::other("MCP response deadline overflowed"))?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("MCP request {request_id} for {method} timed out"),
                )
                .into());
            }
            let line = self
                .responses
                .recv_timeout(remaining)
                .map_err(|source| io::Error::new(io::ErrorKind::TimedOut, source))??;
            let response: Value = serde_json::from_str(line.trim())?;
            if response.get("id").and_then(Value::as_u64) == Some(request_id) {
                return Ok(response);
            }
        }
    }

    /// Send one notification without waiting for a response.
    fn notify(&mut self, method: &str, params: &Value) -> Result<(), Box<dyn Error>> {
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    /// Write and flush one newline-delimited MCP message.
    fn write_message(&mut self, message: &Value) -> Result<(), Box<dyn Error>> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("MCP stdin was already closed"))?;
        serde_json::to_writer(&mut *stdin, message)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    /// Close the MCP session and require a clean bounded process exit.
    fn shutdown(mut self) -> Result<(), Box<dyn Error>> {
        self.stdin.take();
        let deadline = Instant::now()
            .checked_add(PROCESS_EXIT_TIMEOUT)
            .ok_or_else(|| io::Error::other("MCP shutdown deadline overflowed"))?;
        let status = loop {
            let child = self
                .child
                .as_mut()
                .ok_or_else(|| io::Error::other("MCP child was already consumed"))?;
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                let _status = child.wait()?;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "MCP server did not exit after stdin closed",
                )
                .into());
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        };
        self.child.take();
        if let Some(reader) = self.stdout_reader.take() {
            reader
                .join()
                .map_err(|_panic| io::Error::other("MCP stdout reader panicked"))?;
        }
        let stderr = join_capture(self.stderr_reader.take(), "MCP stderr")?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "MCP server exited unsuccessfully: status={status} stderr={}",
                captured_stream_text(&stderr)
            ))
            .into());
        }
        Ok(())
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        self.stdin.take();
        drop(force_process_cleanup(
            &self.tracked_runtime,
            PROCESS_HELPER_TIMEOUT,
        ));
        if let Some(child) = self.child.as_mut() {
            drop(child.kill());
            drop(child.wait());
        }
        self.child.take();
        if let Some(reader) = self.stdout_reader.take() {
            drop(reader.join());
        }
        if let Some(reader) = self.stderr_reader.take() {
            drop(reader.join());
        }
    }
}

/// Fail-safe ownership of one deliberately suspended worker.
struct SuspendedProcess {
    /// Exact process identity validated before suspension.
    process: ProcessIdentity,
    /// Whether exceptional-path resume remains armed.
    resume_armed: bool,
}

impl SuspendedProcess {
    /// Suspend one exact process and arm best-effort resume.
    fn suspend(process: ProcessIdentity) -> Result<Self, Box<dyn Error>> {
        suspend_process(&process)?;
        Ok(Self {
            process,
            resume_armed: true,
        })
    }

    /// Record that supervisor cleanup removed the suspended process.
    fn disarm_after_exit(&mut self) {
        self.resume_armed = false;
    }
}

impl Drop for SuspendedProcess {
    fn drop(&mut self) {
        if self.resume_armed {
            drop(resume_process(&self.process));
        }
    }
}

/// Exercise one real contained worker crash before `SQLite` publication.
#[test]
#[ignore = "requires one exact workflow-built optional parser-pack archive"]
fn contained_worker_crash_preserves_active_generation() -> Result<(), Box<dyn Error>> {
    let archive = std::env::var_os(OPTIONAL_PARSER_ARCHIVE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("real optional parser archive environment is absent"))?
        .canonicalize()?;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let source_dir = repo.join("src");
    let pending_dir = repo.join("pending");
    let host = HostState::create(&temp.path().join("host-state"))?;
    let database = repo.join(ATLAS_DIR_NAME).join(ATLAS_DATABASE_FILE_NAME);
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("lib.rs"), "pub fn built_in() {}\n")?;
    fs::write(
        source_dir.join("baseline.awk"),
        "BEGIN { print \"baseline\" }\n",
    )?;

    run_json(&repo, &host, &[OsStr::new("init")])?;
    let mut pack_cleanup = PackCleanup::new(&repo, &host);
    let verified = run_json(
        &repo,
        &host,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("verify"),
            OsStr::new("--archive"),
            archive.as_os_str(),
        ],
    )?;
    let artifact = json_string(&verified, &["artifact", "artifact"])?.to_owned();
    run_json(
        &repo,
        &host,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("install"),
            OsStr::new("--archive"),
            archive.as_os_str(),
        ],
    )?;
    run_json(
        &repo,
        &host,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("enable"),
            OsStr::new("--artifact"),
            OsStr::new(&artifact),
        ],
    )?;
    run_json(&repo, &host, &[OsStr::new("scan")])?;

    let baseline_store = AtlasStore::open_read_only(&database)?;
    let baseline_publication = baseline_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("baseline publication is missing"))?;
    let baseline_node = baseline_store
        .load_node_by_path("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("baseline optional node is missing"))?;
    let baseline_parse = baseline_store
        .load_source_parse_metadata("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("baseline optional parse metadata is missing"))?;
    if baseline_parse.parser != ParserKind::TreeSitter {
        return Err(io::Error::other("baseline optional source was not grammar parsed").into());
    }
    drop(baseline_store);

    fs::create_dir(&pending_dir)?;
    let pending_source = pending_optional_source()?;
    for index in 0..PENDING_OPTIONAL_FILE_COUNT {
        fs::write(
            pending_dir.join(format!("work-{index:04}.awk")),
            pending_source.as_bytes(),
        )?;
    }

    let mut scan = ScanProcess::spawn(&repo, &host)?;
    let runtime = wait_for_runtime_processes(&mut scan, PROCESS_DISCOVERY_TIMEOUT)?;
    let tracked = runtime.tracked();
    terminate_process(&runtime.worker)?;
    let output = scan.finish(PROCESS_EXIT_TIMEOUT)?;
    wait_for_process_cleanup(&tracked, PROCESS_CLEANUP_TIMEOUT)?;
    require_optional_worker_failure(&output)?;
    let retained_store = AtlasStore::open_read_only(&database)?;
    let retained_publication = retained_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("retained publication is missing"))?;
    let retained_node = retained_store
        .load_node_by_path("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("retained baseline node is missing"))?;
    let retained_parse = retained_store
        .load_source_parse_metadata("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("retained baseline parse metadata is missing"))?;
    if retained_publication != baseline_publication
        || retained_node != baseline_node
        || retained_parse != baseline_parse
    {
        return Err(io::Error::other(
            "worker crash changed the active generation or its baseline facts",
        )
        .into());
    }
    if retained_store
        .load_node_by_path("pending/work-0000.awk")?
        .is_some()
        || retained_store
            .load_source_parse_metadata("pending/work-0000.awk")?
            .is_some()
    {
        return Err(io::Error::other("worker crash exposed uncommitted pending source").into());
    }
    drop(retained_store);

    run_json(
        &repo,
        &host,
        &[OsStr::new("parser-pack"), OsStr::new("remove")],
    )?;
    pack_cleanup.disarm();
    Ok(())
}

/// Prove a stalled contained worker cannot monopolize MCP control or published reads.
#[test]
#[ignore = "requires one exact workflow-built optional parser-pack archive"]
fn stalled_worker_cancellation_preserves_mcp_reads_and_active_generation()
-> Result<(), Box<dyn Error>> {
    let archive = std::env::var_os(OPTIONAL_PARSER_ARCHIVE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("real optional parser archive environment is absent"))?
        .canonicalize()?;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let source_dir = repo.join("src");
    let pending_dir = repo.join("pending");
    let host = HostState::create(&temp.path().join("host-state"))?;
    let database = repo.join(ATLAS_DIR_NAME).join(ATLAS_DATABASE_FILE_NAME);
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("lib.rs"), "pub fn built_in() {}\n")?;
    fs::write(
        source_dir.join("baseline.awk"),
        "BEGIN { print \"baseline\" }\n",
    )?;

    run_json(&repo, &host, &[OsStr::new("init")])?;
    let mut pack_cleanup = PackCleanup::new(&repo, &host);
    let installed = run_json(
        &repo,
        &host,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("install"),
            OsStr::new("--archive"),
            archive.as_os_str(),
        ],
    )?;
    let artifact = json_string(&installed, &["artifact", "artifact"])?.to_owned();
    run_json(
        &repo,
        &host,
        &[
            OsStr::new("parser-pack"),
            OsStr::new("enable"),
            OsStr::new("--artifact"),
            OsStr::new(&artifact),
        ],
    )?;
    run_json(&repo, &host, &[OsStr::new("scan")])?;

    let baseline_store = AtlasStore::open_read_only(&database)?;
    let baseline_publication = baseline_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("baseline publication is missing"))?;
    let baseline_node = baseline_store
        .load_node_by_path("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("baseline optional node is missing"))?;
    let baseline_parse = baseline_store
        .load_source_parse_metadata("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("baseline optional parse metadata is missing"))?;
    if baseline_parse.parser != ParserKind::TreeSitter {
        return Err(io::Error::other("baseline optional source was not grammar parsed").into());
    }
    drop(baseline_store);

    fs::create_dir(&pending_dir)?;
    let pending_source = pending_optional_source()?;
    for index in 0..PENDING_OPTIONAL_FILE_COUNT {
        fs::write(
            pending_dir.join(format!("work-{index:04}.awk")),
            pending_source.as_bytes(),
        )?;
    }

    let mut mcp = McpProcess::spawn(&repo, &database, &host)?;
    let task_start = mcp.call_tool(
        "atlas_scan",
        &serde_json::json!({"background": true, "max_workers": 1}),
    )?;
    let task_id = toon_scalar(&task_start, "task_id")
        .ok_or_else(|| io::Error::other(format!("background scan omitted task id: {task_start}")))?
        .to_owned();
    let runtime = wait_for_mcp_runtime_processes(&mut mcp, PROCESS_DISCOVERY_TIMEOUT)?;
    let tracked = runtime.tracked();
    mcp.track_runtime(&runtime);
    let mut suspended = SuspendedProcess::suspend(runtime.worker.clone())?;

    let running = mcp.call_tool(
        "atlas_task_status",
        &serde_json::json!({"task_id": task_id.clone()}),
    )?;
    if toon_scalar(&running, "state") != Some("running") {
        return Err(io::Error::other(format!(
            "stalled background scan did not remain running: {running}"
        ))
        .into());
    }
    let overview = mcp.call_tool("atlas_overview", &serde_json::json!({}))?;
    if !overview.contains("overview:") {
        return Err(io::Error::other(format!(
            "stalled parser worker blocked or invalidated a normal indexed read: {overview}"
        ))
        .into());
    }

    let canceled = mcp.call_tool(
        "atlas_task_cancel",
        &serde_json::json!({"task_id": task_id.clone()}),
    )?;
    if toon_scalar(&canceled, "result") != Some("cancellation_requested") {
        return Err(io::Error::other(format!(
            "task cancellation was not accepted while the worker was stalled: {canceled}"
        ))
        .into());
    }
    wait_for_canceled_mcp_task(&mut mcp, &task_id, MCP_TASK_CANCEL_TIMEOUT)?;
    wait_for_process_cleanup(&tracked, PROCESS_CLEANUP_TIMEOUT)?;
    suspended.disarm_after_exit();

    let retained_store = AtlasStore::open_read_only(&database)?;
    let retained_publication = retained_store
        .index_publication()?
        .ok_or_else(|| io::Error::other("retained publication is missing"))?;
    let retained_node = retained_store
        .load_node_by_path("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("retained baseline node is missing"))?;
    let retained_parse = retained_store
        .load_source_parse_metadata("src/baseline.awk")?
        .ok_or_else(|| io::Error::other("retained baseline parse metadata is missing"))?;
    if retained_publication != baseline_publication
        || retained_node != baseline_node
        || retained_parse != baseline_parse
    {
        return Err(io::Error::other(
            "stalled-worker cancellation changed the active generation or baseline facts",
        )
        .into());
    }
    if retained_store
        .load_node_by_path("pending/work-0000.awk")?
        .is_some()
        || retained_store
            .load_source_parse_metadata("pending/work-0000.awk")?
            .is_some()
    {
        return Err(io::Error::other(
            "stalled-worker cancellation exposed uncommitted pending source",
        )
        .into());
    }
    drop(retained_store);

    mcp.shutdown()?;
    run_json(
        &repo,
        &host,
        &[OsStr::new("parser-pack"), OsStr::new("remove")],
    )?;
    pack_cleanup.disarm();
    Ok(())
}

/// Build one bounded real optional-language fixture shared by pending files.
fn pending_optional_source() -> Result<String, std::fmt::Error> {
    let mut source = String::with_capacity(PENDING_OPTIONAL_FUNCTION_COUNT * 48);
    source.push_str("BEGIN { total = 0 }\n");
    for index in 0..PENDING_OPTIONAL_FUNCTION_COUNT {
        writeln!(
            source,
            "function worker_{index}(value) {{ return value + {index} }}"
        )?;
    }
    Ok(source)
}

/// Create a normal product command under the selected repository and host state.
fn projectatlas_command(repo: &Path, host: &HostState) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("projectatlas"));
    command.current_dir(repo);
    host.apply(&mut command);
    command
}

/// Run one successful normal product command and decode its JSON result.
fn run_json(repo: &Path, host: &HostState, arguments: &[&OsStr]) -> Result<Value, Box<dyn Error>> {
    let mut command = projectatlas_command(repo, host);
    command.arg("--format").arg("json").args(arguments);
    let output = run_bounded_command(
        &mut command,
        PRODUCT_COMMAND_TIMEOUT,
        "projectatlas command",
    )?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "projectatlas command failed\nstdout:\n{}\nstderr:\n{}",
            captured_stream_text(&output.stdout),
            captured_stream_text(&output.stderr)
        ))
        .into());
    }
    serde_json::from_slice(&output.stdout.bytes).map_err(Into::into)
}

/// Require the exact worker/supervisor failure family rather than any non-zero scan exit.
fn require_optional_worker_failure(output: &CapturedOutput) -> Result<(), Box<dyn Error>> {
    let stderr = captured_stream_text(&output.stderr);
    let diagnostic = stderr.to_ascii_lowercase();
    let worker_failure = diagnostic.contains("optional parser child exited")
        || (diagnostic.contains("optional parser") && diagnostic.contains("i/o failed"))
        || diagnostic.contains("optional parser operation failed and cleanup also failed")
        || (diagnostic.contains("optional parser operation failed:")
            && diagnostic.contains("mandatory cleanup also failed"))
        || diagnostic.contains("optional parser cleanup failed");
    if !output.status.success() && worker_failure {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "scan did not report an optional-parser child/pipe/cleanup failure: status={}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        captured_stream_text(&output.stdout),
        stderr
    ))
    .into())
}

/// Run one owned subprocess with concurrent bounded stream draining and a hard wait ceiling.
fn run_bounded_command(
    command: &mut Command,
    timeout: Duration,
    operation: &'static str,
) -> io::Result<CapturedOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other(format!("{operation} stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other(format!("{operation} stderr was not piped")))?;
    let stdout = capture_pipe(stdout);
    let stderr = capture_pipe(stderr);
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other(format!("{operation} deadline overflowed")))?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            if let Err(source) = child.kill()
                && child.try_wait()?.is_none()
            {
                return Err(source);
            }
            drop(child.wait());
            drop(stdout);
            drop(stderr);
            return Err(io::Error::other(format!(
                "{operation} did not exit within {timeout:?}"
            )));
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };
    Ok(CapturedOutput {
        status,
        stdout: stdout
            .join()
            .map_err(|_panic| io::Error::other(format!("{operation} stdout reader panicked")))??,
        stderr: stderr
            .join()
            .map_err(|_panic| io::Error::other(format!("{operation} stderr reader panicked")))??,
    })
}

/// Read one required JSON string at a closed path.
fn json_string<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, Box<dyn Error>> {
    let mut current = value;
    for key in path {
        current = current
            .get(key)
            .ok_or_else(|| io::Error::other(format!("JSON key `{key}` is missing")))?;
    }
    current
        .as_str()
        .ok_or_else(|| io::Error::other("JSON value is not a string").into())
}

/// Return one unquoted scalar from a compact TOON response.
fn toon_scalar<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (candidate, value) = line.trim().split_once(':')?;
        (candidate == key).then(|| value.trim().trim_matches('"'))
    })
}

/// Wait for one canceled task record through the real MCP status tool.
fn wait_for_canceled_mcp_task(
    mcp: &mut McpProcess,
    task_id: &str,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("MCP task cancellation deadline overflowed"))?;
    loop {
        let status = mcp.call_tool(
            "atlas_task_status",
            &serde_json::json!({"task_id": task_id}),
        )?;
        if toon_scalar(&status, "state") == Some("canceled") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("MCP task {task_id} did not become canceled: {status}"),
            )
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Drain one child pipe on its own bounded-lifetime thread.
fn capture_pipe<R>(mut pipe: R) -> JoinHandle<io::Result<CapturedStream>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 8 * 1024];
        let mut truncated = false;
        loop {
            let read = pipe.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let retained = CAPTURED_SCAN_STREAM_MAX_BYTES.saturating_sub(bytes.len());
            let retained = retained.min(read);
            bytes.extend_from_slice(&buffer[..retained]);
            truncated |= retained < read;
        }
        Ok(CapturedStream { bytes, truncated })
    })
}

/// Join one pipe drain while preserving I/O and panic failures.
fn join_capture(
    handle: Option<JoinHandle<io::Result<CapturedStream>>>,
    stream: &'static str,
) -> io::Result<CapturedStream> {
    handle
        .ok_or_else(|| io::Error::other(format!("scan {stream} reader was already consumed")))?
        .join()
        .map_err(|_panic| io::Error::other(format!("scan {stream} reader panicked")))?
}

/// Render bounded captured diagnostics with honest truncation state.
fn captured_stream_text(stream: &CapturedStream) -> String {
    let suffix = if stream.truncated {
        "\n[diagnostics truncated after 1 MiB]"
    } else {
        ""
    };
    format!("{}{suffix}", String::from_utf8_lossy(&stream.bytes))
}

/// Wait for exactly one platform-owned worker subtree below the scan.
fn wait_for_runtime_processes(
    scan: &mut ScanProcess,
    timeout: Duration,
) -> Result<RuntimeProcesses, Box<dyn Error>> {
    let scan_pid = scan.pid()?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("process discovery deadline overflowed"))?;
    loop {
        let descendants = runtime_descendants(scan_pid)?;
        if let Some(runtime) = classify_runtime_processes(scan_pid, &descendants)? {
            scan.track_runtime(&runtime);
            return Ok(runtime);
        }
        if !scan.is_running()? {
            return Err(io::Error::other(
                "normal scan exited before its contained worker became observable",
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "contained worker was not observed within {timeout:?}"
            ))
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Wait for the exact optional runtime subtree owned by one live MCP server.
fn wait_for_mcp_runtime_processes(
    mcp: &mut McpProcess,
    timeout: Duration,
) -> Result<RuntimeProcesses, Box<dyn Error>> {
    let mcp_pid = mcp.pid()?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("MCP process discovery deadline overflowed"))?;
    loop {
        let descendants = runtime_descendants(mcp_pid)?;
        if let Some(runtime) = classify_runtime_processes(mcp_pid, &descendants)? {
            return Ok(runtime);
        }
        if !mcp.is_running()? {
            return Err(io::Error::other(
                "MCP server exited before its contained worker became observable",
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "contained MCP worker was not observed within {timeout:?}"
            ))
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Validate the exact one-worker platform topology when it is complete.
fn classify_runtime_processes(
    scan_pid: u32,
    descendants: &[ProcessIdentity],
) -> Result<Option<RuntimeProcesses>, Box<dyn Error>> {
    let platform = host_pack_platform();
    let workers = descendants
        .iter()
        .filter(|process| process_name_eq(&process.name, platform.worker_file_name()))
        .collect::<Vec<_>>();
    let brokers = descendants
        .iter()
        .filter(|process| {
            platform
                .containment_broker_file_name()
                .is_some_and(|name| process_name_eq(&process.name, name))
        })
        .collect::<Vec<_>>();
    if workers.len() > 1
        || brokers.len() > usize::from(platform.containment_broker_file_name().is_some())
    {
        return Err(io::Error::other(
            "normal scan owned more than one optional worker or containment broker",
        )
        .into());
    }
    let Some(worker) = workers.first() else {
        return Ok(None);
    };
    match platform {
        PackPlatform::LinuxX86_64 => {
            if worker.parent_pid != scan_pid || !brokers.is_empty() {
                return Err(io::Error::other("Linux optional worker topology is invalid").into());
            }
            Ok(Some(RuntimeProcesses {
                worker: (*worker).clone(),
                broker: None,
            }))
        }
        PackPlatform::WindowsX86_64 => {
            let Some(broker) = brokers.first() else {
                return Ok(None);
            };
            if broker.parent_pid != scan_pid || worker.parent_pid != broker.pid {
                return Err(io::Error::other("Windows optional worker topology is invalid").into());
            }
            Ok(Some(RuntimeProcesses {
                worker: (*worker).clone(),
                broker: Some((*broker).clone()),
            }))
        }
    }
}

/// Wait until every exact tracked worker/broker identity is gone.
fn wait_for_process_cleanup(
    tracked: &[ProcessIdentity],
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("process cleanup deadline overflowed"))?;
    loop {
        let current = current_processes(tracked)?;
        let live = tracked
            .iter()
            .filter(|expected| {
                current
                    .iter()
                    .any(|observed| expected.is_same_process(observed))
            })
            .collect::<Vec<_>>();
        if live.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let forced_cleanup = force_process_cleanup(tracked, PROCESS_CLEANUP_TIMEOUT);
            return Err(io::Error::other(format!(
                "scan retained optional runtime process IDs: {}; forced_cleanup={}",
                live.iter()
                    .map(|process| process.pid.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                forced_cleanup
                    .as_ref()
                    .map_or_else(ToString::to_string, |()| "confirmed".to_owned())
            ))
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Terminate and then confirm disappearance of exact tracked worker identities.
fn force_process_cleanup(
    tracked: &[ProcessIdentity],
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let mut live = current_processes(tracked)?;
    live.sort_by_key(|process| !is_worker_name(&process.name));
    for process in &live {
        terminate_process(process)?;
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("forced process cleanup deadline overflowed"))?;
    loop {
        let current = current_processes(&live)?;
        let retained = live
            .iter()
            .filter(|expected| {
                current
                    .iter()
                    .any(|observed| expected.is_same_process(observed))
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "forced cleanup retained exact process IDs: {}",
                retained
                    .iter()
                    .map(|process| process.pid.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Return whether two platform process names are equal.
fn process_name_eq(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

/// Return whether the executable name is the accepted worker name.
fn is_worker_name(name: &str) -> bool {
    process_name_eq(name, host_pack_platform().worker_file_name())
}

/// Return the accepted optional-pack platform for this compiled test.
const fn host_pack_platform() -> PackPlatform {
    #[cfg(target_os = "linux")]
    {
        PackPlatform::LinuxX86_64
    }
    #[cfg(target_os = "windows")]
    {
        PackPlatform::WindowsX86_64
    }
}

/// Enumerate exact optional runtime descendants of one Linux scan.
#[cfg(target_os = "linux")]
fn runtime_descendants(scan_pid: u32) -> Result<Vec<ProcessIdentity>, Box<dyn Error>> {
    let mut processes = BTreeMap::new();
    for entry in fs::read_dir("/proc")? {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        if processes.len() == LINUX_PROCESS_SNAPSHOT_MAX_ENTRIES {
            return Err(io::Error::other("Linux process snapshot exceeded its row bound").into());
        }
        if let Some(process) = linux_process_identity(pid)? {
            processes.insert(pid, process);
        }
    }
    Ok(processes
        .values()
        .filter(|process| {
            is_optional_runtime_name(&process.name)
                && linux_process_is_descendant(process, scan_pid, &processes)
        })
        .cloned()
        .collect())
}

/// Follow a bounded Linux parent chain back to the exact owned scan.
#[cfg(target_os = "linux")]
fn linux_process_is_descendant(
    process: &ProcessIdentity,
    scan_pid: u32,
    processes: &BTreeMap<u32, ProcessIdentity>,
) -> bool {
    let mut parent = process.parent_pid;
    let mut visited = BTreeSet::new();
    while visited.len() < 64 && visited.insert(parent) {
        if parent == scan_pid {
            return true;
        }
        let Some(ancestor) = processes.get(&parent) else {
            return false;
        };
        parent = ancestor.parent_pid;
    }
    false
}

/// Read one Linux PID, parent, executable name, and kernel start tick.
#[cfg(target_os = "linux")]
fn linux_process_identity(pid: u32) -> Result<Option<ProcessIdentity>, Box<dyn Error>> {
    let process_root = PathBuf::from(format!("/proc/{pid}"));
    let stat = match fs::read_to_string(process_root.join("stat")) {
        Ok(stat) => stat,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let close = stat
        .rfind(')')
        .ok_or_else(|| io::Error::other("Linux process stat has no command terminator"))?;
    let mut fields = stat
        .get(close + 1..)
        .ok_or_else(|| io::Error::other("Linux process stat suffix is invalid"))?
        .split_whitespace();
    let parent_pid = fields
        .nth(1)
        .ok_or_else(|| io::Error::other("Linux process stat has no parent PID"))?
        .parse::<u32>()?;
    let started = fields
        .nth(17)
        .ok_or_else(|| io::Error::other("Linux process stat has no start tick"))?
        .to_string();
    let executable = match fs::read_link(process_root.join("exe")) {
        Ok(executable) => executable,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let name = executable
        .file_name()
        .ok_or_else(|| io::Error::other("Linux process executable has no file name"))?
        .to_string_lossy()
        .into_owned();
    Ok(Some(ProcessIdentity {
        pid,
        parent_pid,
        name,
        started,
    }))
}

/// Read the still-live subset of exact tracked Linux identities.
#[cfg(target_os = "linux")]
fn current_processes(tracked: &[ProcessIdentity]) -> Result<Vec<ProcessIdentity>, Box<dyn Error>> {
    tracked
        .iter()
        .filter_map(|process| linux_process_identity(process.pid).transpose())
        .collect()
}

/// Send SIGKILL only to one exact tracked Linux worker identity.
#[cfg(target_os = "linux")]
fn terminate_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    let Some(current) = linux_process_identity(process.pid)? else {
        return Ok(());
    };
    if !process.is_same_process(&current) {
        return Err(io::Error::other("refusing to terminate a reused Linux PID").into());
    }
    let raw_pid = i32::try_from(process.pid)?;
    kill(Pid::from_raw(raw_pid), Signal::SIGKILL)?;
    Ok(())
}

/// Suspend one exact Linux worker and confirm its kernel stopped state.
#[cfg(target_os = "linux")]
fn suspend_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    let Some(current) = linux_process_identity(process.pid)? else {
        return Err(io::Error::other("Linux worker exited before suspension").into());
    };
    if !process.is_same_process(&current) {
        return Err(io::Error::other("refusing to suspend a reused Linux PID").into());
    }
    let raw_pid = i32::try_from(process.pid)?;
    kill(Pid::from_raw(raw_pid), Signal::SIGSTOP)?;
    let deadline = Instant::now()
        .checked_add(PROCESS_HELPER_TIMEOUT)
        .ok_or_else(|| io::Error::other("Linux suspension deadline overflowed"))?;
    loop {
        let status = fs::read_to_string(format!("/proc/{}/status", process.pid))?;
        if status.lines().any(|line| {
            line.strip_prefix("State:")
                .is_some_and(|state| matches!(state.trim().as_bytes().first(), Some(b'T' | b't')))
        }) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Linux worker did not enter a stopped state",
            )
            .into());
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Resume one exact Linux worker when it still exists.
#[cfg(target_os = "linux")]
fn resume_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    let Some(current) = linux_process_identity(process.pid)? else {
        return Ok(());
    };
    if !process.is_same_process(&current) {
        return Err(io::Error::other("refusing to resume a reused Linux PID").into());
    }
    let raw_pid = i32::try_from(process.pid)?;
    kill(Pid::from_raw(raw_pid), Signal::SIGCONT)?;
    Ok(())
}

/// Return whether one name belongs to the accepted worker/broker surface.
fn is_optional_runtime_name(name: &str) -> bool {
    let platform = host_pack_platform();
    process_name_eq(name, platform.worker_file_name())
        || platform
            .containment_broker_file_name()
            .is_some_and(|expected| process_name_eq(name, expected))
}

/// `PowerShell` process-tree query restricted to one exact scan PID.
#[cfg(target_os = "windows")]
const WINDOWS_DESCENDANT_QUERY: &str = r#"
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$scanPid = [uint32][Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_SCAN_PID')
$rows = @()
$children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $scanPid")
foreach ($child in $children) {
  $rows += [pscustomobject]@{
    pid = [uint32]$child.ProcessId
    parent_pid = [uint32]$child.ParentProcessId
    name = [string]$child.Name
    started = ([DateTime]$child.CreationDate).ToUniversalTime().Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)
  }
  $childPid = [uint32]$child.ProcessId
  foreach ($grandchild in @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $childPid")) {
    $rows += [pscustomobject]@{
      pid = [uint32]$grandchild.ProcessId
      parent_pid = [uint32]$grandchild.ParentProcessId
      name = [string]$grandchild.Name
      started = ([DateTime]$grandchild.CreationDate).ToUniversalTime().Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)
    }
  }
}
ConvertTo-Json -InputObject $rows -Compress
"#;

/// `PowerShell` exact-PID identity query used after the scan exits.
#[cfg(target_os = "windows")]
const WINDOWS_IDENTITY_QUERY: &str = r#"
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$rows = @()
foreach ($value in ([Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_PROCESS_PIDS') -split ',')) {
  if ([string]::IsNullOrWhiteSpace($value)) { continue }
  $pidValue = [uint32]$value
  $process = Get-CimInstance Win32_Process -Filter "ProcessId = $pidValue"
  if ($null -ne $process) {
    $rows += [pscustomobject]@{
      pid = [uint32]$process.ProcessId
      parent_pid = [uint32]$process.ParentProcessId
      name = [string]$process.Name
      started = ([DateTime]$process.CreationDate).ToUniversalTime().Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)
    }
  }
}
ConvertTo-Json -InputObject $rows -Compress
"#;

/// `PowerShell` exact-identity termination with start-token validation.
#[cfg(target_os = "windows")]
const WINDOWS_TERMINATE_PROCESS: &str = r#"
$pidValue = [uint32][Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_TERMINATE_PID')
$expectedName = [Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_TERMINATE_NAME')
$expectedStarted = [Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_TERMINATE_STARTED')
$process = Get-CimInstance Win32_Process -Filter "ProcessId = $pidValue"
if ($null -eq $process) { exit 0 }
$started = ([DateTime]$process.CreationDate).ToUniversalTime().Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)
if (-not ([string]$process.Name).Equals($expectedName, [StringComparison]::OrdinalIgnoreCase) -or $started -ne $expectedStarted) {
  exit 41
}
$result = Invoke-CimMethod -InputObject $process -MethodName Terminate
if ([uint32]$result.ReturnValue -ne 0) { exit 42 }
"#;

/// Exact-identity Windows process suspension and resume through native process handles.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROL_PROCESS: &str = r#"
$pidValue = [uint32][Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_CONTROL_PID')
$expectedName = [Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_CONTROL_NAME')
$expectedStarted = [Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_CONTROL_STARTED')
$operation = [Environment]::GetEnvironmentVariable('PROJECTATLAS_TEST_CONTROL_OPERATION')
$process = Get-CimInstance Win32_Process -Filter "ProcessId = $pidValue"
if ($null -eq $process) {
  if ($operation -eq 'resume') { exit 0 }
  exit 51
}
$started = ([DateTime]$process.CreationDate).ToUniversalTime().Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)
if (-not ([string]$process.Name).Equals($expectedName, [StringComparison]::OrdinalIgnoreCase) -or $started -ne $expectedStarted) {
  exit 52
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ProjectAtlasTestProcessControl {
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern IntPtr OpenProcess(uint access, bool inheritHandle, uint processId);
  [DllImport("kernel32.dll", SetLastError = true)]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool CloseHandle(IntPtr handle);
  [DllImport("ntdll.dll")]
  public static extern int NtSuspendProcess(IntPtr processHandle);
  [DllImport("ntdll.dll")]
  public static extern int NtResumeProcess(IntPtr processHandle);
}
'@
$handle = [ProjectAtlasTestProcessControl]::OpenProcess(0x0800, $false, $pidValue)
if ($handle -eq [IntPtr]::Zero) { exit 53 }
try {
  if ($operation -eq 'suspend') {
    $status = [ProjectAtlasTestProcessControl]::NtSuspendProcess($handle)
  } elseif ($operation -eq 'resume') {
    $status = [ProjectAtlasTestProcessControl]::NtResumeProcess($handle)
  } else {
    exit 54
  }
  if ($status -ne 0) { exit 55 }
} finally {
  [void][ProjectAtlasTestProcessControl]::CloseHandle($handle)
}
"#;

/// Enumerate exact optional runtime descendants of one Windows scan.
#[cfg(target_os = "windows")]
fn runtime_descendants(scan_pid: u32) -> Result<Vec<ProcessIdentity>, Box<dyn Error>> {
    let rows = powershell_processes(
        WINDOWS_DESCENDANT_QUERY,
        &[(WINDOWS_SCAN_PID_ENV, scan_pid.to_string())],
    )?;
    Ok(rows
        .into_iter()
        .filter(|process| is_optional_runtime_name(&process.name))
        .collect())
}

/// Read the still-live subset of exact tracked Windows identities.
#[cfg(target_os = "windows")]
fn current_processes(tracked: &[ProcessIdentity]) -> Result<Vec<ProcessIdentity>, Box<dyn Error>> {
    let arguments = tracked
        .iter()
        .map(|process| process.pid.to_string())
        .collect::<Vec<_>>();
    powershell_processes(
        WINDOWS_IDENTITY_QUERY,
        &[(WINDOWS_PROCESS_PIDS_ENV, arguments.join(","))],
    )
}

/// Terminate only one exact tracked Windows process identity.
#[cfg(target_os = "windows")]
fn terminate_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    let environment = [
        (WINDOWS_TERMINATE_PID_ENV, process.pid.to_string()),
        (WINDOWS_TERMINATE_NAME_ENV, process.name.clone()),
        (WINDOWS_TERMINATE_STARTED_ENV, process.started.clone()),
    ];
    let mut command = windows_powershell(WINDOWS_TERMINATE_PROCESS);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = run_bounded_command(
        &mut command,
        PROCESS_HELPER_TIMEOUT,
        "exact Windows process termination",
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "exact Windows process termination failed: status={} stderr={}",
        output.status,
        captured_stream_text(&output.stderr)
    ))
    .into())
}

/// Suspend one exact Windows worker through a process suspend/resume handle.
#[cfg(target_os = "windows")]
fn suspend_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    control_windows_process(process, "suspend")
}

/// Resume one exact Windows worker when it still exists.
#[cfg(target_os = "windows")]
fn resume_process(process: &ProcessIdentity) -> Result<(), Box<dyn Error>> {
    control_windows_process(process, "resume")
}

/// Apply one closed Windows suspend/resume operation after exact identity validation.
#[cfg(target_os = "windows")]
fn control_windows_process(
    process: &ProcessIdentity,
    operation: &'static str,
) -> Result<(), Box<dyn Error>> {
    let environment = [
        (WINDOWS_CONTROL_PID_ENV, process.pid.to_string()),
        (WINDOWS_CONTROL_NAME_ENV, process.name.clone()),
        (WINDOWS_CONTROL_STARTED_ENV, process.started.clone()),
        (WINDOWS_CONTROL_OPERATION_ENV, operation.to_owned()),
    ];
    let mut command = windows_powershell(WINDOWS_CONTROL_PROCESS);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = run_bounded_command(
        &mut command,
        PROCESS_HELPER_TIMEOUT,
        "exact Windows process control",
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "exact Windows process {operation} failed: status={} stderr={}",
        output.status,
        captured_stream_text(&output.stderr)
    ))
    .into())
}

/// Run one bounded Windows process query and decode its JSON rows.
#[cfg(target_os = "windows")]
fn powershell_processes(
    script: &str,
    environment: &[(&str, String)],
) -> Result<Vec<ProcessIdentity>, Box<dyn Error>> {
    let mut command = windows_powershell(script);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = run_bounded_command(
        &mut command,
        PROCESS_HELPER_TIMEOUT,
        "Windows process query",
    )?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Windows process query failed: status={} stderr={}",
            output.status,
            captured_stream_text(&output.stderr)
        ))
        .into());
    }
    if output.stdout.truncated {
        return Err(io::Error::other("Windows process query exceeded its output bound").into());
    }
    serde_json::from_slice(&output.stdout.bytes).map_err(Into::into)
}

/// Build the inbox Windows `PowerShell` command without trusting PATH.
#[cfg(target_os = "windows")]
fn windows_powershell(script: &str) -> Command {
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let executable = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let mut command = Command::new(executable);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .stdin(Stdio::null());
    command
}
