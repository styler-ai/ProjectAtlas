//! Thin evidence adapter over `processkit` process-tree supervision.

use processkit::{Command, OutputBufferPolicy};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;

/// Failures at the process-tree supervision boundary.
#[derive(Debug, Error)]
pub(super) enum SupervisionError {
    /// A zero-byte capture ceiling cannot retain useful evidence.
    #[error("process output limit must be greater than zero")]
    ZeroOutputLimit,
    /// `processkit` could not launch, supervise, capture, or tear down the tree.
    #[error(transparent)]
    Process(#[from] processkit::Error),
}

/// Bounded retained bytes and their digest for one process stream.
#[derive(Clone, Debug, Serialize)]
pub(super) struct CapturedStream {
    /// Bytes retained by `processkit` after applying the configured ceiling.
    #[serde(skip)]
    pub(super) retained: Vec<u8>,
    /// Number of retained bytes.
    pub(super) retained_bytes: usize,
    /// Digest over the retained bytes.
    pub(super) retained_sha256: String,
}

impl CapturedStream {
    /// Build stable evidence without serializing stream contents.
    fn new(retained: Vec<u8>) -> Self {
        let retained_sha256 = format!("{:x}", Sha256::digest(&retained));
        Self {
            retained_bytes: retained.len(),
            retained,
            retained_sha256,
        }
    }
}

/// Result returned after `processkit` completed supervision of one process tree.
#[derive(Debug, Serialize)]
pub(super) struct SupervisedCommandOutput {
    /// Exit code when the leader returned one.
    pub(super) exit_code: Option<i32>,
    /// Whether the configured deadline terminated the tree.
    pub(super) timed_out: bool,
    /// Wall-clock lifetime observed by `processkit`.
    pub(super) duration_ns: u64,
    /// Whether either retained stream crossed the configured ceiling.
    pub(super) output_truncated: bool,
    /// Bounded standard output evidence.
    pub(super) stdout: CapturedStream,
    /// Bounded standard error evidence.
    pub(super) stderr: CapturedStream,
}

impl SupervisedCommandOutput {
    /// Return whether the leader exited successfully without timeout or truncation.
    pub(super) fn is_success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.output_truncated
    }
}

/// Run one command with native `processkit` timeout, tree teardown, and bounded capture.
pub(super) async fn run_supervised(
    command: Command,
    timeout: Duration,
    output_limit: usize,
) -> Result<SupervisedCommandOutput, SupervisionError> {
    if output_limit == 0 {
        return Err(SupervisionError::ZeroOutputLimit);
    }
    let result = command
        .timeout(timeout)
        .kill_on_parent_death()
        .output_buffer(OutputBufferPolicy::unbounded().with_max_bytes(output_limit))
        .output_bytes()
        .await?;
    let truncated = result.truncated();
    Ok(SupervisedCommandOutput {
        exit_code: result.code(),
        timed_out: result.timed_out(),
        duration_ns: u64::try_from(result.duration().as_nanos()).unwrap_or(u64::MAX),
        output_truncated: truncated,
        stdout: CapturedStream::new(result.stdout().clone()),
        stderr: CapturedStream::new(result.stderr().as_bytes().to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::{self, Write};
    use std::process::{Command as StdCommand, Stdio};
    use std::thread;

    /// Select the subprocess probe role.
    const PROBE_ROLE_ENV: &str = "PROJECTATLAS_SUPERVISION_PROBE";
    /// Destination written only by an escaped descendant.
    const PROBE_MARKER_ENV: &str = "PROJECTATLAS_SUPERVISION_MARKER";
    /// Destination proving that the leader spawned its descendant.
    const PROBE_READY_ENV: &str = "PROJECTATLAS_SUPERVISION_READY";
    /// Exact test-harness child entry.
    const PROBE_TEST_NAME: &str = "bounded_process_supervisor::tests::supervision_probe";

    /// Convert a failed process invariant into a test error without panicking.
    fn check(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    /// Zero capture cannot accidentally become unbounded.
    #[tokio::test(flavor = "current_thread")]
    async fn zero_capture_limit_fails_before_spawn() {
        let result = run_supervised(Command::new("not-started"), Duration::from_secs(1), 0).await;
        assert!(matches!(result, Err(SupervisionError::ZeroOutputLimit)));
    }

    /// Exercise output and descendant behavior in real subprocesses.
    #[test]
    fn supervision_probe() -> Result<(), Box<dyn std::error::Error>> {
        match env::var(PROBE_ROLE_ENV).as_deref() {
            Ok("output") => {
                io::stdout().write_all(&vec![b'o'; 8 * 1024])?;
                io::stderr().write_all(&vec![b'e'; 8 * 1024])?;
            }
            Ok("leader") => {
                let executable = env::current_exe()?;
                let marker = env::var(PROBE_MARKER_ENV)?;
                StdCommand::new(executable)
                    .args(["--exact", PROBE_TEST_NAME, "--nocapture"])
                    .env(PROBE_ROLE_ENV, "descendant")
                    .env(PROBE_MARKER_ENV, marker)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                fs::write(env::var(PROBE_READY_ENV)?, b"ready")?;
                thread::sleep(Duration::from_secs(5));
            }
            Ok("descendant") => {
                thread::sleep(Duration::from_secs(1));
                fs::write(env::var(PROBE_MARKER_ENV)?, b"orphaned")?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The process adapter bounds both retained streams and reports overflow.
    #[tokio::test(flavor = "current_thread")]
    async fn output_capture_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let output = run_supervised(
            Command::new(env::current_exe()?)
                .args(["--exact", PROBE_TEST_NAME, "--nocapture"])
                .env_clear()
                .env(PROBE_ROLE_ENV, "output"),
            Duration::from_secs(5),
            128,
        )
        .await?;
        check(output.output_truncated, "overflow was not reported")?;
        check(output.stdout.retained_bytes <= 128, "stdout exceeded cap")?;
        check(output.stderr.retained_bytes <= 128, "stderr exceeded cap")?;
        check(!output.is_success(), "overflow was classified as success")?;
        Ok(())
    }

    /// A timed-out leader cannot leave its descendant running.
    #[tokio::test(flavor = "current_thread")]
    async fn timeout_terminates_descendants() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let ready = directory.path().join("ready");
        let marker = directory.path().join("descendant-finished");
        let output = run_supervised(
            Command::new(env::current_exe()?)
                .args(["--exact", PROBE_TEST_NAME, "--nocapture"])
                .env_clear()
                .env(PROBE_ROLE_ENV, "leader")
                .env(PROBE_READY_ENV, &ready)
                .env(PROBE_MARKER_ENV, &marker),
            Duration::from_millis(500),
            8 * 1024,
        )
        .await?;
        check(output.timed_out, "deadline did not time out")?;
        check(ready.is_file(), "leader never started descendant")?;
        thread::sleep(Duration::from_millis(1_100));
        check(!marker.exists(), "descendant survived process-tree timeout")?;
        Ok(())
    }

    /// Serialized supervision evidence must not claim unobserved tree emptiness.
    #[tokio::test(flavor = "current_thread")]
    async fn serialization_omits_unobserved_tree_state() -> Result<(), Box<dyn std::error::Error>> {
        let output = run_supervised(
            Command::new(env::current_exe()?)
                .args(["--exact", PROBE_TEST_NAME, "--nocapture"])
                .env_clear(),
            Duration::from_secs(5),
            8 * 1024,
        )
        .await?;
        let serialized = serde_json::to_value(output)?;
        check(
            serialized.get("tree_terminated").is_none() && serialized.get("tree_empty").is_none(),
            "supervision evidence claimed an unobserved tree postcondition",
        )
    }
}
