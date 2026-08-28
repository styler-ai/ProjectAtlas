//! Select the optional parser worker implementation for the current target.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_linux_authority;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_worker_containment;

#[cfg(not(any(
    test,
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
use std::env;
#[cfg(not(any(
    test,
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
use std::io::{self, Write};
use std::process::ExitCode;

#[cfg(any(
    test,
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
))]
#[path = "parser_worker.rs"]
mod worker_impl;

#[cfg(any(
    test,
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
))]
fn main() -> ExitCode {
    worker_impl::main()
}

#[cfg(not(any(
    test,
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn main() -> ExitCode {
    let mut standard_error = io::stderr().lock();
    if writeln!(
        standard_error,
        "optional parser containment is unsupported on {}-{}",
        env::consts::OS,
        env::consts::ARCH
    )
    .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::FAILURE
}
