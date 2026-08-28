//! Target-neutral startup contract shared by the parser worker entrypoints.

use std::ffi::{OsStr, OsString};

use thiserror::Error;
use tree_sitter_language_pack::LanguageRegistry;

/// Release-tool probe that validates the worker without opening a parser pack.
pub(super) const VERIFY_BUILD_CONTRACT_ARGUMENT: &str = "--verify-build-contract";
/// Only production worker invocation accepted from either platform adapter.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub(super) const SERVE_ARGUMENT: &str = "--serve";
/// Required offline build flag for the optional grammar dependency.
const OFFLINE_BUILD_VARIABLE: &str = "TSLP_OFFLINE";
/// Required dynamic-link build flag for the optional grammar dependency.
const LINK_MODE_BUILD_VARIABLE: &str = "TSLP_LINK_MODE";
/// Build selector that must stay absent because platform libraries are repackaged.
const LANGUAGE_FILTER_BUILD_VARIABLE: &str = "TSLP_LANGUAGES";
/// Build escape hatch that must stay absent so failed grammars fail the build.
const ALLOW_FAILED_GRAMMARS_BUILD_VARIABLE: &str = "TSLP_ALLOW_FAILED_GRAMMARS";

/// Compile-time values that governed this worker and its optional dependency.
pub(super) const COMPILED_BUILD_ENVIRONMENT: ParserPackBuildEnvironment<'static> =
    ParserPackBuildEnvironment {
        offline: option_env!("TSLP_OFFLINE"),
        link_mode: option_env!("TSLP_LINK_MODE"),
        language_filter: option_env!("TSLP_LANGUAGES"),
        allow_failed_grammars: option_env!("TSLP_ALLOW_FAILED_GRAMMARS"),
    };

/// Build-time environment accepted for a releasable optional parser worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ParserPackBuildEnvironment<'a> {
    /// Whether dependency construction was forced offline.
    pub(super) offline: Option<&'a str>,
    /// Whether grammars were built as independently loaded libraries.
    pub(super) link_mode: Option<&'a str>,
    /// Optional upstream source-build selector, which `ProjectAtlas` forbids.
    pub(super) language_filter: Option<&'a str>,
    /// Optional upstream partial-success escape hatch, which `ProjectAtlas` forbids.
    pub(super) allow_failed_grammars: Option<&'a str>,
}

/// Target-neutral startup contract failure.
#[derive(Clone, Copy, Debug, Error)]
pub(super) enum WorkerContractError {
    /// A required build flag was absent or had an unsafe value.
    #[error(
        "optional parser worker build contract requires {name}={expected}; compiled value was {observed}"
    )]
    InvalidBuildVariable {
        /// Environment variable that violated the build contract.
        name: &'static str,
        /// Required value, or `absent` when the variable is forbidden.
        expected: &'static str,
        /// Content-free state of the compile-time value.
        observed: &'static str,
    },
    /// The upstream registry exposed one or more compiled grammar identities.
    #[error(
        "optional parser worker must contain zero embedded grammars; compiled registry exposed {count} identities"
    )]
    CompiledGrammarRegistryNotEmpty {
        /// Number of language identities visible without a parser pack.
        count: usize,
    },
    /// The caller omitted the one required closed startup operation.
    #[error("optional parser worker requires exactly --serve or --verify-build-contract")]
    MissingInvocation,
    /// The caller supplied an unknown or additional startup argument.
    #[error("optional parser worker accepts only --serve or --verify-build-contract")]
    UnexpectedArguments,
    /// The current platform cannot establish an accepted optional-parser boundary.
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    #[error("optional parser containment is unsupported on {os}-{architecture}")]
    UnsupportedTarget {
        /// Rust target-operating-system identity.
        os: &'static str,
        /// Rust target architecture identity.
        architecture: &'static str,
    },
}

/// Closed target-neutral startup operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkerOperation {
    /// Validate the compiled worker contract without opening pack files.
    VerifyBuildContract,
    /// Enter the platform-specific resident protocol loop.
    Serve,
}

/// Validate the compile-time dependency build policy.
pub(super) fn validate_build_environment(
    environment: ParserPackBuildEnvironment<'_>,
) -> Result<(), WorkerContractError> {
    require_build_value(environment.offline, OFFLINE_BUILD_VARIABLE, "1")?;
    require_build_value(environment.link_mode, LINK_MODE_BUILD_VARIABLE, "dynamic")?;
    require_absent_build_value(environment.language_filter, LANGUAGE_FILTER_BUILD_VARIABLE)?;
    require_absent_build_value(
        environment.allow_failed_grammars,
        ALLOW_FAILED_GRAMMARS_BUILD_VARIABLE,
    )?;
    Ok(())
}

/// Validate every build property that must hold without a parser pack.
pub(super) fn validate_worker_build_contract() -> Result<(), WorkerContractError> {
    validate_build_environment(COMPILED_BUILD_ENVIRONMENT)?;
    validate_empty_compiled_registry(&LanguageRegistry::new().available_languages())
}

/// Require the upstream registry to expose no built-in grammar identities.
pub(super) fn validate_empty_compiled_registry(
    languages: &[String],
) -> Result<(), WorkerContractError> {
    if languages.is_empty() {
        return Ok(());
    }
    Err(WorkerContractError::CompiledGrammarRegistryNotEmpty {
        count: languages.len(),
    })
}

/// Require one exact build-time value.
fn require_build_value(
    actual: Option<&str>,
    name: &'static str,
    expected: &'static str,
) -> Result<(), WorkerContractError> {
    if actual == Some(expected) {
        return Ok(());
    }
    Err(WorkerContractError::InvalidBuildVariable {
        name,
        expected,
        observed: if actual.is_some() {
            "invalid"
        } else {
            "absent"
        },
    })
}

/// Reject a build-time variable even when Cargo supplied an empty value.
fn require_absent_build_value(
    actual: Option<&str>,
    name: &'static str,
) -> Result<(), WorkerContractError> {
    if actual.is_none() {
        return Ok(());
    }
    Err(WorkerContractError::InvalidBuildVariable {
        name,
        expected: "absent",
        observed: "present",
    })
}

/// Consume the first target-neutral operation without interpreting platform authority.
pub(super) fn parse_worker_operation(
    arguments: &mut impl Iterator<Item = OsString>,
    serve_argument: &str,
) -> Result<WorkerOperation, WorkerContractError> {
    match arguments
        .next()
        .ok_or(WorkerContractError::MissingInvocation)?
        .as_os_str()
    {
        argument if argument == OsStr::new(VERIFY_BUILD_CONTRACT_ARGUMENT) => {
            Ok(WorkerOperation::VerifyBuildContract)
        }
        argument if argument == OsStr::new(serve_argument) => Ok(WorkerOperation::Serve),
        _ => Err(WorkerContractError::UnexpectedArguments),
    }
}

/// Run the unsupported-host entrypoint without opening a parser pack.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
pub(super) fn unsupported_host_startup(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), WorkerContractError> {
    validate_worker_build_contract()?;
    let mut arguments = arguments;
    match parse_worker_operation(&mut arguments, SERVE_ARGUMENT)? {
        WorkerOperation::VerifyBuildContract => {
            if arguments.next().is_some() {
                return Err(WorkerContractError::UnexpectedArguments);
            }
            Ok(())
        }
        WorkerOperation::Serve => {
            if arguments.next().is_some() {
                return Err(WorkerContractError::UnexpectedArguments);
            }
            Err(WorkerContractError::UnsupportedTarget {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
            })
        }
    }
}
