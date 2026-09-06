//! Private support shared by the split CLI integration-test binaries.
#![allow(dead_code)]

use projectatlas_db::AtlasStore;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) const MCP_CONTRACT_EXECUTABLE_ENV: &str = "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE";
pub(super) const MCP_CONTRACT_METADATA_CANARY: &str = "mcp_contract_metadata_canary";
pub(super) const GIT_REPOSITORY_ENVIRONMENT_VARIABLES: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct McpDatabaseSnapshot {
    pub(super) authoritative: BTreeMap<String, String>,
    pub(super) usage: BTreeMap<String, String>,
    pub(super) authored_purposes: BTreeMap<String, String>,
    pub(super) metadata_canary: Option<String>,
    pub(super) project_instance_id: Option<String>,
    pub(super) usage_calls: usize,
    pub(super) usage_events: Vec<String>,
    pub(super) active_usage_instances: usize,
    pub(super) sealed_mcp_instances: usize,
    pub(super) generation: u64,
    pub(super) purpose_revision: u64,
    pub(super) publication_state: String,
}

pub(super) fn synchronize_prompt_exit_before_delayed_observation(
    child: &mut Child,
    label: &str,
    exit_probe_error: Option<io::Error>,
) -> io::Result<()> {
    if let Some(error) = exit_probe_error {
        return Err(error);
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or_else(|| io::Error::other("test child exit deadline overflowed"))?;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{label} did not exit before the delayed observer was released"),
            ));
        }
        thread::sleep(Duration::from_millis(25).min(remaining));
    }
}

/// Launch a real MCP stdio child and return stdout after stdin closes.
pub(super) fn run_mcp_stdio(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env(executable, cwd, args, messages, &[])
}

/// Launch a real MCP stdio child and close stdin only after every request has a response.
pub(super) fn run_mcp_stdio_with_env(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env_and_test_delay_and_kill(
        executable,
        cwd,
        args,
        messages,
        environment,
        Duration::from_secs(10),
        None,
        false,
        &mut |child| child.kill(),
    )
}

pub(super) struct McpStdioCleanupPacket {
    pub(super) child: Child,
    pub(super) stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    pub(super) stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
}

/// Reap one test-owned MCP stdio packet after the observer has returned.
pub(super) fn reap_mcp_stdio_packet(
    mut packet: McpStdioCleanupPacket,
) -> Result<(), Box<dyn Error>> {
    packet.child.stdin.take();
    if packet.child.try_wait()?.is_none() {
        packet.child.kill()?;
    }
    packet.child.wait()?;
    packet
        .stdout_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stdout cleanup reader panicked"))??;
    packet
        .stderr_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stderr cleanup reader panicked"))??;
    Ok(())
}

pub(super) fn run_mcp_stdio_with_env_and_test_delay(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env_and_test_delay_and_kill(
        executable,
        cwd,
        args,
        messages,
        environment,
        timeout,
        observer_delay,
        hold_stdin_until_observation,
        &mut |child| child.kill(),
    )
}

fn run_mcp_stdio_with_env_and_test_delay_and_kill(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
    kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
) -> Result<String, Box<dyn Error>> {
    run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
        executable,
        cwd,
        args,
        messages,
        environment,
        timeout,
        observer_delay,
        hold_stdin_until_observation,
        None,
        None,
        kill_child,
        None,
    )
}

/// Test-only variant that transfers a proven-live child and its readers to the
/// caller when injected termination cannot safely reap it here.
pub(super) fn run_mcp_stdio_with_env_and_test_delay_and_kill_and_handoff(
    executable: &std::path::Path,
    cwd: &std::path::Path,
    args: &[String],
    messages: &[impl AsRef<str>],
    environment: &[(&str, Option<&str>)],
    timeout: Duration,
    observer_delay: Option<Duration>,
    hold_stdin_until_observation: bool,
    exit_probe_error: Option<io::Error>,
    cleanup_probe_error: Option<io::Error>,
    kill_child: &mut impl FnMut(&mut Child) -> io::Result<()>,
    handoff_live_child: Option<
        &mut dyn FnMut(
            Child,
            thread::JoinHandle<io::Result<Vec<u8>>>,
            thread::JoinHandle<io::Result<Vec<u8>>>,
        ),
    >,
) -> Result<String, Box<dyn Error>> {
    let mut exit_probe_error = exit_probe_error;
    let mut cleanup_probe_error = cleanup_probe_error;
    let mut expected_responses = BTreeSet::new();
    for message in messages {
        let request: Value = serde_json::from_str(message.as_ref())?;
        if let Some(id) = request.get("id") {
            expected_responses.insert(id.to_string());
        }
    }
    let input = format!(
        "{}\n",
        messages
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut command = StdCommand::new(executable);
    command
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in environment {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    let mut child = command.spawn()?;
    let mut stdin = Some(
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("mcp stdin was not piped"))?,
    );
    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("mcp stdout was not piped"))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("mcp stderr was not piped"))?;
    let (response_sender, response_receiver) = mpsc::channel();
    let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut reader = BufReader::new(&mut stdout_pipe);
        let mut output = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            output.extend_from_slice(line.as_bytes());
            drop(response_sender.send(line));
        }
        Ok(output)
    });
    let stderr_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        stderr_pipe.read_to_end(&mut output)?;
        Ok(output)
    });

    let response_result = (|| -> Result<(), Box<dyn Error>> {
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("mcp stdin was closed before requests were sent"))?;
        stdin.write_all(input.as_bytes())?;
        stdin.flush()?;
        let mut response_deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| io::Error::other("MCP response deadline overflowed"))?;
        while !expected_responses.is_empty() {
            let remaining = response_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "projectatlas mcp did not answer request ids before shutdown: {expected_responses:?}"
                    ),
                )
                .into());
            }
            let line = response_receiver
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "projectatlas mcp response deadline elapsed with request ids pending: {expected_responses:?}"
                        ),
                    ),
                    mpsc::RecvTimeoutError::Disconnected => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "projectatlas mcp closed before answering every request",
                    ),
                })?;
            let response: Value = serde_json::from_str(line.trim())?;
            if response
                .get("id")
                .is_some_and(|id| expected_responses.remove(&id.to_string()))
            {
                response_deadline = Instant::now()
                    .checked_add(Duration::from_secs(10))
                    .ok_or_else(|| io::Error::other("MCP response deadline overflowed"))?;
            }
        }
        Ok(())
    })();
    if !hold_stdin_until_observation {
        stdin.take();
    }
    drop(response_receiver);

    if observer_delay.is_some()
        && (!hold_stdin_until_observation || exit_probe_error.is_some())
        && let Err(error) = synchronize_prompt_exit_before_delayed_observation(
            &mut child,
            "projectatlas mcp",
            exit_probe_error.take(),
        )
    {
        let kill_result = kill_child(&mut child);
        stdin.take();
        let status_after_kill = child.try_wait();
        if kill_result.is_err() && !matches!(&status_after_kill, Ok(Some(_))) {
            if let Some(handoff) = handoff_live_child {
                handoff(child, stdout_reader, stderr_reader);
            } else {
                drop(child);
                drop(stdout_reader);
                drop(stderr_reader);
            }
            let mut diagnostic = format!(
                "projectatlas mcp exit synchronization failed before delayed observation: {error}; cleanup incomplete: child/readers detached"
            );
            if let Some(kill_error) = kill_result.as_ref().err() {
                diagnostic.push_str("; termination failed: ");
                diagnostic.push_str(&kill_error.to_string());
            }
            if let Err(probe_error) = status_after_kill {
                diagnostic.push_str("; re-probe failed after termination attempt: ");
                diagnostic.push_str(&probe_error.to_string());
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
        }
        let status = child.wait()?;
        let stdout = stdout_reader
            .join()
            .map_err(|_panic| io::Error::other("mcp stdout reader panicked"))??;
        let stderr = stderr_reader
            .join()
            .map_err(|_panic| io::Error::other("mcp stderr reader panicked"))??;
        let diagnostic = format!(
            "projectatlas mcp exit synchronization failed before delayed observation: {error}; cleanup complete: child reaped and readers joined status={status}"
        );
        let _ = (stdout, stderr);
        return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
    }

    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("MCP process deadline overflowed"))?;
    if let Some(delay) = observer_delay {
        thread::sleep(delay);
    }
    let mut timeout_reason = None;
    let mut accepted_completion = false;
    let mut pre_termination_probe_error = None;
    let mut post_termination_probe_error = None;
    loop {
        if Instant::now() >= deadline {
            timeout_reason = Some("still running at deadline".to_string());
            break;
        }
        let (status, observed_at) = {
            let status = child.try_wait()?;
            let observed_at = Instant::now();
            (status, observed_at)
        };
        match status {
            Some(_) if observed_at < deadline => {
                accepted_completion = true;
                break;
            }
            Some(_) => {
                timeout_reason = Some(format!(
                    "completed after deadline (observed_at={observed_at:?})"
                ));
                break;
            }
            None => {
                let remaining = deadline.saturating_duration_since(observed_at);
                if remaining.is_zero() {
                    timeout_reason = Some("still running at deadline".to_string());
                    break;
                }
                thread::sleep(Duration::from_millis(100).min(remaining));
            }
        }
    }

    if timeout_reason.is_some() {
        let (status, observed_at) = {
            let status = match cleanup_probe_error.take() {
                Some(error) => {
                    pre_termination_probe_error = Some(error);
                    None
                }
                None => match child.try_wait() {
                    Ok(status) => status,
                    Err(error) => {
                        pre_termination_probe_error = Some(error);
                        None
                    }
                },
            };
            let observed_at = Instant::now();
            (status, observed_at)
        };
        if status.is_none() {
            let kill_result = kill_child(&mut child);
            let status_after_kill = match exit_probe_error.take() {
                Some(error) => Err(error),
                None => child.try_wait(),
            };
            let status_after_kill = match status_after_kill {
                Ok(status) => status,
                Err(error) if kill_result.is_ok() => {
                    post_termination_probe_error = Some(error);
                    None
                }
                Err(error) => {
                    stdin.take();
                    if let Some(handoff) = handoff_live_child {
                        handoff(child, stdout_reader, stderr_reader);
                    } else {
                        drop(child);
                        drop(stdout_reader);
                        drop(stderr_reader);
                    }
                    let mut diagnostic = format!(
                        "projectatlas mcp did not exit after stdin closed: {} status=unknown (re-probe failed after termination attempt: {error}; cleanup incomplete: child/readers detached)",
                        timeout_reason.as_deref().unwrap_or("timeout")
                    );
                    if let Some(kill_error) = kill_result.as_ref().err() {
                        diagnostic.push_str("; termination failed: ");
                        diagnostic.push_str(&kill_error.to_string());
                    }
                    return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
                }
            };
            if let Err(error) = kill_result
                && status_after_kill.is_none()
            {
                stdin.take();
                if let Some(handoff) = handoff_live_child {
                    handoff(child, stdout_reader, stderr_reader);
                } else {
                    drop(child);
                    drop(stdout_reader);
                    drop(stderr_reader);
                }
                let diagnostic = format!(
                    "projectatlas mcp did not exit after stdin closed: {} status=still-running at deadline (termination failed: {error}; cleanup incomplete: operating system refused termination; child was not reaped; child/readers detached)",
                    timeout_reason.as_deref().unwrap_or("timeout")
                );
                return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
            }
        } else if timeout_reason.as_deref() == Some("still running at deadline") {
            timeout_reason = Some(format!(
                "completed after deadline (observed_at={observed_at:?})"
            ));
        }
    }
    stdin.take();
    let wait_result = child.wait();
    let stdout = stdout_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_panic| io::Error::other("mcp stderr reader panicked"))??;
    if let Some(reason) = timeout_reason {
        let mut diagnostic = format!("projectatlas mcp did not exit after stdin closed: {reason}");
        if let Some(error) = pre_termination_probe_error {
            diagnostic.push_str(" status=unknown (re-probe failed before termination attempt: ");
            diagnostic.push_str(&error.to_string());
            diagnostic.push(')');
        }
        if let Some(error) = post_termination_probe_error {
            diagnostic.push_str(" status=unknown (re-probe failed after successful termination: ");
            diagnostic.push_str(&error.to_string());
            diagnostic.push(')');
        }
        if let Ok(status) = &wait_result {
            diagnostic.push_str(" status=");
            diagnostic.push_str(&status.to_string());
        }
        if !stderr.is_empty() {
            diagnostic.push_str(" stderr=");
            diagnostic.push_str(&String::from_utf8_lossy(&stderr));
        }
        return Err(io::Error::new(io::ErrorKind::TimedOut, diagnostic).into());
    }
    response_result?;
    let status = wait_result?;
    if !accepted_completion || !status.success() {
        return Err(io::Error::other(format!(
            "projectatlas mcp failed: {}",
            String::from_utf8_lossy(&stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(stdout)?)
}

/// Return the text payload for one MCP tool-call response id.
pub(super) fn mcp_tool_text(stdout: &str, id: i64) -> Result<String, Box<dyn Error>> {
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let response: Value = serde_json::from_str(line)?;
        if response.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        return response
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| io::Error::other(format!("MCP tool response {id} has no text")).into());
    }
    Err(io::Error::other(format!("MCP tool response {id} is missing")).into())
}

/// Return the community metadata findings from one analysis response.
pub(super) fn json_community_values(value: &Value) -> Result<Vec<&Value>, Box<dyn Error>> {
    let findings = value
        .pointer("/symbol_relations/findings")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("analysis response omitted findings"))?;
    let communities = findings
        .iter()
        .filter(|finding| finding.get("kind").and_then(Value::as_str) == Some("community"))
        .filter_map(|finding| finding.get("community"))
        .collect::<Vec<_>>();
    if communities.is_empty() {
        return Err(io::Error::other("analysis response omitted community metadata").into());
    }
    Ok(communities)
}

/// Verify stable IDs/order, containment exclusion, and the planted partition.
pub(super) fn assert_planted_community_values(
    communities: &[&Value],
    adapter: &str,
) -> Result<(), Box<dyn Error>> {
    let expected_group_a = BTreeSet::from([
        "group_a_one".to_string(),
        "group_a_root".to_string(),
        "group_a_two".to_string(),
    ]);
    let expected_group_b = BTreeSet::from([
        "group_b_one".to_string(),
        "group_b_root".to_string(),
        "group_b_two".to_string(),
    ]);
    let mut community_ids = BTreeSet::new();
    let mut member_sets = Vec::new();
    for community in communities {
        let id = community.get("id").and_then(Value::as_str).ok_or_else(|| {
            io::Error::other(format!("{adapter} community omitted its stable ID"))
        })?;
        if !id.starts_with("community-v1-") || !community_ids.insert(id.to_string()) {
            return Err(io::Error::other(format!(
                "{adapter} community IDs were missing or duplicated: {community_ids:?}"
            ))
            .into());
        }
        if community.get("coverage").and_then(Value::as_str) != Some("complete")
            || community.get("convergence").and_then(Value::as_str) != Some("converged")
            || community.get("truncated").and_then(Value::as_bool) != Some(false)
        {
            return Err(io::Error::other(format!(
                "{adapter} planted community did not report complete converged coverage: {community:?}"
            ))
            .into());
        }
        let members = community
            .get("members")
            .and_then(Value::as_array)
            .ok_or_else(|| io::Error::other(format!("{adapter} community omitted members")))?;
        let member_keys = members
            .iter()
            .map(|member| {
                member
                    .pointer("/node/entity/key/stable/canonical_identity")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        io::Error::other(format!("{adapter} community member lacked a stable key"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if member_keys.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(io::Error::other(format!(
                "{adapter} community members were not in stable key order: {member_keys:?}"
            ))
            .into());
        }
        if community
            .get("evidence")
            .and_then(Value::as_array)
            .is_some_and(|evidence| {
                evidence.iter().any(|edge| {
                    edge.get("source")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                        || edge
                            .get("target")
                            .and_then(Value::as_str)
                            .is_none_or(str::is_empty)
                        || edge.get("weight").and_then(Value::as_u64) == Some(0)
                        || edge.pointer("/relation/value").and_then(Value::as_str)
                            == Some("contains")
                })
            })
        {
            return Err(io::Error::other(format!(
                "{adapter} community evidence emitted a containment relation"
            ))
            .into());
        }
        let member_names = members
            .iter()
            .map(|member| {
                member
                    .pointer("/node/entity/selector/symbol/name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        io::Error::other(format!(
                            "{adapter} community member lacked a stable symbol name"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        member_sets.push(member_names.into_iter().collect::<BTreeSet<_>>());
    }
    let group_a_partition = member_sets
        .iter()
        .any(|members| expected_group_a.is_subset(members));
    let group_b_partition = member_sets
        .iter()
        .any(|members| expected_group_b.is_subset(members));
    let groups_are_separate = member_sets.iter().all(|members| {
        !(members.iter().any(|name| expected_group_a.contains(name))
            && members.iter().any(|name| expected_group_b.contains(name)))
    });
    if !group_a_partition || !group_b_partition || !groups_are_separate {
        return Err(io::Error::other(format!(
            "{adapter} community projection did not preserve planted groups: {member_sets:?}"
        ))
        .into());
    }
    Ok(())
}

/// Hash every bounded user-table row through one read-only `SQLite` connection.
pub(super) fn sqlite_table_digests(
    connection: &Connection,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    const MAX_TABLE_ROWS: usize = 16_384;
    const MAX_TABLE_BYTES: usize = 8 * 1024 * 1024;

    let table_names = {
        let mut statement = connection.prepare(
            "SELECT name
             FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut tables = BTreeMap::new();
    for table_name in table_names {
        let quoted_name = format!("\"{}\"", table_name.replace('"', "\"\""));
        let column_count = {
            let statement = connection.prepare(&format!("SELECT * FROM {quoted_name} LIMIT 0"))?;
            statement.column_count()
        };
        if column_count == 0 {
            return Err(io::Error::other(format!(
                "MCP contract table {table_name} has no columns"
            ))
            .into());
        }
        let ordering = (1..=column_count)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let mut statement =
            connection.prepare(&format!("SELECT * FROM {quoted_name} ORDER BY {ordering}"))?;
        let mut rows = statement.query([])?;
        let mut encoded = Vec::new();
        let mut row_count = 0usize;
        while let Some(row) = rows.next()? {
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| io::Error::other("MCP contract row count overflowed"))?;
            if row_count > MAX_TABLE_ROWS {
                return Err(io::Error::other(format!(
                    "MCP contract table {table_name} exceeded {MAX_TABLE_ROWS} rows"
                ))
                .into());
            }
            for index in 0..column_count {
                match row.get_ref(index)? {
                    ValueRef::Null => encoded.push(0),
                    ValueRef::Integer(value) => {
                        encoded.push(1);
                        encoded.extend_from_slice(&value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        encoded.push(2);
                        encoded.extend_from_slice(&value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) => {
                        encoded.push(3);
                        encoded.extend_from_slice(&u64::try_from(value.len())?.to_le_bytes());
                        encoded.extend_from_slice(value);
                    }
                    ValueRef::Blob(value) => {
                        encoded.push(4);
                        encoded.extend_from_slice(&u64::try_from(value.len())?.to_le_bytes());
                        encoded.extend_from_slice(value);
                    }
                }
            }
            encoded.push(0xff);
            if encoded.len() > MAX_TABLE_BYTES {
                return Err(io::Error::other(format!(
                    "MCP contract table {table_name} exceeded {MAX_TABLE_BYTES} encoded bytes"
                ))
                .into());
            }
        }
        let digest = format!("{row_count}:{}", sha256_hex(&encoded));
        tables.insert(table_name, digest);
    }
    Ok(tables)
}

/// Capture bounded logical rows so WAL/page-layout changes do not masquerade as product state.
pub(super) fn mcp_database_snapshot(
    database: &Path,
) -> Result<McpDatabaseSnapshot, Box<dyn Error>> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tables = sqlite_table_digests(&connection)?;
    let (usage, authoritative) = tables
        .into_iter()
        .partition(|(table_name, _)| table_name.starts_with("usage_"));
    drop(connection);

    let store = AtlasStore::open_read_only(database)?;
    let publication = store
        .index_publication()?
        .ok_or_else(|| io::Error::other("MCP contract database has no publication"))?;
    let authored_purposes = {
        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut statement = connection.prepare(
            "SELECT n.path, COALESCE(p.purpose, ''), p.source, p.status
             FROM purposes AS p
             JOIN nodes AS n ON n.id = p.node_id
             WHERE p.source IN ('imported', 'agent')
             ORDER BY n.path",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    format!(
                        "{}\0{}\0{}",
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?
                    ),
                ))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?
    };
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let metadata_canary = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [MCP_CONTRACT_METADATA_CANARY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let sealed_mcp_instances = usize::try_from(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM usage_instances WHERE owner = 'mcp_process' AND state = 'sealed'",
        [],
        |row| row.get(0),
    )?)?;
    let usage_events = store
        .usage_events(None)?
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let retention = store.telemetry_retention_state()?;
    Ok(McpDatabaseSnapshot {
        authoritative,
        usage,
        authored_purposes,
        metadata_canary,
        project_instance_id: store
            .project_instance_id()?
            .map(projectatlas_core::graph::ProjectInstanceId::as_hex),
        usage_calls: store.token_overview(None)?.calls,
        usage_events,
        active_usage_instances: retention.active_instance_rows,
        sealed_mcp_instances,
        generation: publication.generation.get(),
        purpose_revision: store.authored_purpose_revision()?,
        publication_state: format!("{:?}", publication.state).to_ascii_lowercase(),
    })
}

/// Require a nested JSON string value.
pub(super) fn require_json_string(
    value: &Value,
    path: &[&str],
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_str() == Some(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected:?}, found {current:?}"
        ))
        .into())
    }
}

/// Require a nested JSON string to contain a substring.
pub(super) fn require_json_contains(
    value: &Value,
    path: &[&str],
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let text = current
        .as_str()
        .ok_or_else(|| io::Error::other(format!("expected string at {path:?}")))?;
    if text.contains(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to contain {expected:?}, found {text:?}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value.
pub(super) fn require_json_usize(
    value: &Value,
    path: &[&str],
    expected: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_u64() == Some(u64::try_from(expected)?) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected}, found {current:?}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value to be at least a threshold.
pub(super) fn require_json_usize_at_least(
    value: &Value,
    path: &[&str],
    expected_minimum: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_u64()
        .ok_or_else(|| io::Error::other(format!("expected integer at {path:?}")))?;
    if actual >= u64::try_from(expected_minimum)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to be at least {expected_minimum}, found {actual}"
        ))
        .into())
    }
}

/// Require a nested JSON integer value to be greater than a threshold.
pub(super) fn require_json_usize_greater_than(
    value: &Value,
    path: &[&str],
    threshold: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let actual = current
        .as_u64()
        .ok_or_else(|| io::Error::other(format!("expected integer at {path:?}")))?;
    if actual > u64::try_from(threshold)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to be greater than {threshold}, found {actual}"
        ))
        .into())
    }
}

/// Require a nested JSON array length.
pub(super) fn require_json_array_len(
    value: &Value,
    path: &[&str],
    expected: usize,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    let length = current
        .as_array()
        .ok_or_else(|| io::Error::other(format!("expected array at {path:?}")))?
        .len();
    if length == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} length {expected}, found {length}"
        ))
        .into())
    }
}

/// Require a nested JSON boolean value.
pub(super) fn require_json_bool(
    value: &Value,
    path: &[&str],
    expected: bool,
) -> Result<(), Box<dyn Error>> {
    let current = json_at(value, path)?;
    if current.as_bool() == Some(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "expected {path:?} to equal {expected}, found {current:?}"
        ))
        .into())
    }
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut rendered = String::with_capacity(digest.len() * 2);
    for byte in digest {
        rendered.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}

/// Navigate a JSON value by object keys and decimal array indexes.
pub(super) fn json_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value, Box<dyn Error>> {
    let mut current = value;
    for segment in path {
        current = if let Some(array) = current.as_array() {
            let index = segment.parse::<usize>()?;
            array
                .get(index)
                .ok_or_else(|| io::Error::other(format!("missing json array index {segment}")))?
        } else {
            current
                .get(segment)
                .ok_or_else(|| io::Error::other(format!("missing json segment {segment}")))?
        };
    }
    Ok(current)
}

/// Return the explicitly selected packaged runtime or the local test binary.
pub(super) fn mcp_contract_executable() -> PathBuf {
    std::env::var_os(MCP_CONTRACT_EXECUTABLE_ENV).map_or_else(
        || assert_cmd::cargo::cargo_bin("projectatlas"),
        PathBuf::from,
    )
}

/// Run a JSON summary command for one indexed path.
pub(super) fn json_summary_command(
    repo: &Path,
    db: &Path,
    file: &str,
) -> Result<Value, Box<dyn Error>> {
    let output = StdCommand::new(mcp_contract_executable())
        .current_dir(repo)
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(db)
        .args(["summary", file, "--limit", "10"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "summary command failed for {file}: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

pub(super) fn git_command_for_root(root: &Path) -> StdCommand {
    let mut command = StdCommand::new("git");
    command.current_dir(root);
    for variable in GIT_REPOSITORY_ENVIRONMENT_VARIABLES {
        command.env_remove(variable);
    }
    command
}

/// Return the repository workspace root for fixture access.
pub(super) fn workspace_root() -> Result<std::path::PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("workspace root not found").into())
}

pub(super) fn complete_mcp_test_after_shutdown<T>(
    operation_result: Result<T, Box<dyn Error>>,
    shutdown: impl FnOnce() -> Result<(), Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    let shutdown_result = shutdown();
    let value = operation_result?;
    shutdown_result?;
    Ok(value)
}
