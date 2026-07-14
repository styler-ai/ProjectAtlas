//! Exercise the public lint binary contract that unit tests cannot observe.

use std::process::Command;

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
