//! Exercise the unsupported-host parser-worker entrypoint as a real process.

#![cfg(target_os = "macos")]

use std::process::Command;

use assert_cmd::cargo::cargo_bin;

#[test]
fn build_contract_probe_is_portable_but_serving_fails_closed() {
    let empty_directory =
        tempfile::tempdir().expect("create an empty directory for the worker probes");
    let executable = cargo_bin("projectatlas-parser-worker");

    let verified = Command::new(&executable)
        .current_dir(empty_directory.path())
        .arg("--verify-build-contract")
        .output()
        .expect("run the worker build-contract probe");
    assert!(
        verified.status.success(),
        "build-contract probe failed: stderr={:?}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(verified.stdout.is_empty());
    assert!(verified.stderr.is_empty());

    let served = Command::new(&executable)
        .current_dir(empty_directory.path())
        .arg("--serve")
        .output()
        .expect("run the unsupported worker serving probe");
    assert!(!served.status.success());
    assert!(served.stdout.is_empty());
    assert_eq!(
        served.stderr,
        format!(
            "optional parser containment is unsupported on {}-{}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
        .into_bytes()
    );
}
