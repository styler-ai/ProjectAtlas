//! Exercise the unsupported-host parser-worker entrypoint as a real process.

#![cfg(target_os = "macos")]

use std::{io, process::Command};

use assert_cmd::cargo::cargo_bin;

#[test]
fn build_contract_probe_is_portable_but_serving_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let empty_directory = tempfile::tempdir()?;
    let executable = cargo_bin("projectatlas-parser-worker");

    let verified = Command::new(&executable)
        .current_dir(empty_directory.path())
        .arg("--verify-build-contract")
        .output()?;
    if !verified.status.success() {
        return Err(io::Error::other(format!(
            "build-contract probe failed: status={:?}, stdout={:?}, stderr={:?}",
            verified.status,
            String::from_utf8_lossy(&verified.stdout),
            String::from_utf8_lossy(&verified.stderr),
        ))
        .into());
    }
    if !verified.stdout.is_empty() {
        return Err(io::Error::other(format!(
            "build-contract probe wrote stdout: status={:?}, stdout={:?}, stderr={:?}",
            verified.status,
            String::from_utf8_lossy(&verified.stdout),
            String::from_utf8_lossy(&verified.stderr),
        ))
        .into());
    }
    if !verified.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "build-contract probe wrote stderr: status={:?}, stdout={:?}, stderr={:?}",
            verified.status,
            String::from_utf8_lossy(&verified.stdout),
            String::from_utf8_lossy(&verified.stderr),
        ))
        .into());
    }

    let served = Command::new(&executable)
        .current_dir(empty_directory.path())
        .arg("--serve")
        .output()?;
    if served.status.success() {
        return Err(io::Error::other(format!(
            "unsupported serving probe unexpectedly succeeded: status={:?}, stdout={:?}, stderr={:?}",
            served.status,
            String::from_utf8_lossy(&served.stdout),
            String::from_utf8_lossy(&served.stderr),
        ))
        .into());
    }
    if !served.stdout.is_empty() {
        return Err(io::Error::other(format!(
            "unsupported serving probe wrote stdout: status={:?}, stdout={:?}, stderr={:?}",
            served.status,
            String::from_utf8_lossy(&served.stdout),
            String::from_utf8_lossy(&served.stderr),
        ))
        .into());
    }
    let expected_stderr = format!(
        "optional parser containment is unsupported on {}-{}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .into_bytes();
    if served.stderr != expected_stderr {
        return Err(io::Error::other(format!(
            "unsupported serving diagnostic mismatch: status={:?}, stdout={:?}, stderr={:?}, expected_stderr={:?}",
            served.status,
            String::from_utf8_lossy(&served.stdout),
            String::from_utf8_lossy(&served.stderr),
            String::from_utf8_lossy(&expected_stderr),
        ))
        .into());
    }
    Ok(())
}
