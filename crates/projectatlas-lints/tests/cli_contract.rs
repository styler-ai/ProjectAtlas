//! Exercise the public lint binary contract that unit tests cannot observe.

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

const LOCK: &[u8] = include_bytes!("../../../registry/language-registry.json");
const ACCEPTED: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
const HISTORICAL: &[u8] =
    include_bytes!("../../../fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon");
const CORE_OUTPUT_PATH: &str = "crates/projectatlas-core/src/language_detection_registry.rs";

fn run_cli(args: &[&str], root: &Path) -> io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_cargo-projectatlas-lints"))
        .args(args)
        .current_dir(root)
        .output()
}

fn seed_registry_workspace(root: &Path) -> Result<(), Box<dyn Error>> {
    for (relative, bytes) in [
        ("registry/language-registry.json", LOCK),
        (
            "docs/benchmarks/projectatlas-v0.4-capability-registry.json",
            ACCEPTED,
        ),
        (
            "fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon",
            HISTORICAL,
        ),
    ] {
        let path = root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("registry CLI fixture has no parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, bytes)?;
    }
    for relative in [
        CORE_OUTPUT_PATH,
        "crates/projectatlas-symbols/src/language_parser_registry.rs",
        "crates/projectatlas-cli/src/language_capability_settings.rs",
        "docs/benchmarks/projectatlas-v0.4-language-capability-state.json",
        "docs/language-capabilities.json",
    ] {
        let output = root.join(relative);
        let parent = output
            .parent()
            .ok_or_else(|| io::Error::other("registry CLI output has no parent"))?;
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn verify_language_registry_cli_contract() -> Result<(), Box<dyn Error>> {
    let repository = tempfile::tempdir()?;
    seed_registry_workspace(repository.path())?;

    let write = run_cli(&["language-registry", "write"], repository.path())?;
    if !write.status.success() {
        return Err(io::Error::other(format!(
            "language-registry write failed: {}",
            String::from_utf8_lossy(&write.stderr)
        ))
        .into());
    }

    let check = run_cli(&["language-registry", "check"], repository.path())?;
    if !check.status.success()
        || !String::from_utf8_lossy(&check.stdout)
            .contains("language registry is valid and current")
    {
        return Err(io::Error::other(format!(
            "repository-root language-registry check failed: stdout={} stderr={}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        ))
        .into());
    }

    let core_output = repository.path().join(CORE_OUTPUT_PATH);
    fs::write(&core_output, b"deliberate-cli-drift")?;
    let before = fs::read(&core_output)?;
    let drift = run_cli(&["language-registry", "check"], repository.path())?;
    if drift.status.code() != Some(1)
        || !String::from_utf8_lossy(&drift.stderr).contains(CORE_OUTPUT_PATH)
        || fs::read(core_output)? != before
    {
        return Err(io::Error::other(format!(
            "drift check contract failed: status={:?} stderr={}",
            drift.status.code(),
            String::from_utf8_lossy(&drift.stderr)
        ))
        .into());
    }
    Ok(())
}

#[test]
fn test_quality_help_describes_coverage_enforcement_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-projectatlas-lints"))
        .args(["test-quality", "--help"])
        .output();
    assert!(output.is_ok(), "test-quality help could not be launched");
    let Ok(output) = output else { return };
    assert!(
        output.status.success(),
        "test-quality help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout);
    assert!(stdout.is_ok(), "test-quality help was not UTF-8");
    let Ok(stdout) = stdout else { return };
    for required in [
        "Usage: cargo projectatlas-lints test-quality <COMMAND> [OPTIONS]",
        "coverage           Validate one platform LLVM coverage export",
        "--enforcement defaults to release-quality",
    ] {
        assert!(
            stdout.contains(required),
            "test-quality help omitted {required:?}: {stdout}"
        );
    }
}

#[test]
fn top_level_and_language_registry_help_are_stable() {
    let current = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (args, required) in [
        (
            &["--help"][..],
            &[
                "Usage: cargo projectatlas-lints <COMMAND>",
                "language-registry  Validate or generate the typed language registry.",
            ][..],
        ),
        (
            &["language-registry", "--help"][..],
            &[
                "Usage: cargo projectatlas-lints language-registry <COMMAND>",
                "check  Validate inputs and fail on generated-output drift without writing.",
                "write  Validate inputs and replace only changed fixed outputs.",
            ][..],
        ),
    ] {
        let output = run_cli(args, current);
        assert!(output.is_ok(), "help command could not be launched");
        let Ok(output) = output else { return };
        assert!(
            output.status.success(),
            "help command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        for fragment in required {
            assert!(
                stdout.contains(fragment),
                "help output omitted {fragment:?}: {stdout}"
            );
        }
    }
}

#[test]
fn unknown_commands_exit_with_usage_status() {
    let current = Path::new(env!("CARGO_MANIFEST_DIR"));
    for args in [
        &["unknown-command"][..],
        &["language-registry", "unknown-command"][..],
    ] {
        let output = run_cli(args, current);
        assert!(output.is_ok(), "unknown command could not be launched");
        let Ok(output) = output else { return };
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn language_registry_check_reports_drift_without_mutation() {
    let result = verify_language_registry_cli_contract();
    assert!(result.is_ok(), "{result:?}");
}
