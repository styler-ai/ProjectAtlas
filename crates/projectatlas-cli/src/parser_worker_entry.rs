//! Select the optional parser worker implementation for the current target.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_linux_authority;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_worker_containment;
mod parser_worker_contract;

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
    match parser_worker_contract::unsupported_host_startup(env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut standard_error = io::stderr().lock();
            if writeln!(standard_error, "{error}").is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}
