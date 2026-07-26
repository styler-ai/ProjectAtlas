//! Serve the closed optional-parser protocol from a separately packaged worker.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_linux_authority;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod parser_worker_containment;

use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::io::{Seek as _, SeekFrom};
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use libloading::Library;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use serde::Deserialize;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use sha2::{Digest as _, Sha256};
use thiserror::Error;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use tree_sitter::Language;
use tree_sitter::{Parser, Tree};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use tree_sitter_language::LanguageFn;
use tree_sitter_language_pack::LanguageRegistry;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use parser_linux_authority::{
    ACCEPTED_FD_ARGUMENT, ARTIFACT_FD_ARGUMENT, GRAMMAR_FD_ARGUMENT, POLICY_FD_ARGUMENT,
    SERVE_ARGUMENT,
};
use projectatlas_core::optional_parser_pack::{
    AcceptedGrammar, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES, OptionalParserPackManifest,
};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME, OPTIONAL_PARSER_PACK_MAX_FILE_BYTES,
    OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_SCHEMA_VERSION, OptionalParserPackArtifactManifest,
    PackPlatform, ParserPackPayloadRole,
};
use projectatlas_core::optional_parser_protocol::{
    PARSER_FRAME_HEADER_BYTES, PARSER_MAX_STDERR_BYTES, ParserArtifactIdentity, ParserCompletion,
    ParserCompletionEvidence, ParserContainmentKind, ParserControl, ParserFailure,
    ParserFailureCode, ParserFrame, ParserFrameHeader, ParserLanguageIdentity, ParserProgress,
    ParserProgressStage, ParserProtocolError, ParserReady, ParserRequest, ParserResponseIdentity,
    ParserSyntaxKind, decode_parser_completion_for_request, decode_parser_failure_for_request,
    decode_parser_progress_for_request, decode_parser_ready_for_launch,
    decode_parser_request_for_session, decode_parser_session_open, encode_parser_control,
};

/// Directory containing the platform grammar libraries beside the worker.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const GRAMMAR_LIBRARY_DIRECTORY: &str = "lib";
/// Validated logical manifest packaged beside the worker.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
/// Immutable artifact manifest whose exact bytes identify the running pack.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
/// Maximum immutable native-import policy bytes admitted before containment.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const NATIVE_IMPORT_POLICY_MAX_BYTES: usize = 1024 * 1024;
/// Required offline build flag for the optional grammar dependency.
const OFFLINE_BUILD_VARIABLE: &str = "TSLP_OFFLINE";
/// Required dynamic-link build flag for the optional grammar dependency.
const LINK_MODE_BUILD_VARIABLE: &str = "TSLP_LINK_MODE";
/// Build selector that must stay absent because platform libraries are repackaged.
const LANGUAGE_FILTER_BUILD_VARIABLE: &str = "TSLP_LANGUAGES";
/// Build escape hatch that must stay absent so failed grammars fail the build.
const ALLOW_FAILED_GRAMMARS_BUILD_VARIABLE: &str = "TSLP_ALLOW_FAILED_GRAMMARS";
/// Release-tool probe that validates the worker without opening a parser pack.
const VERIFY_BUILD_CONTRACT_ARGUMENT: &str = "--verify-build-contract";
/// Only production worker invocation accepted from either platform adapter.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const SERVE_ARGUMENT: &str = "--serve";

/// Compile-time values that governed this worker and its optional dependency.
const COMPILED_BUILD_ENVIRONMENT: ParserPackBuildEnvironment<'static> =
    ParserPackBuildEnvironment {
        offline: option_env!("TSLP_OFFLINE"),
        link_mode: option_env!("TSLP_LINK_MODE"),
        language_filter: option_env!("TSLP_LANGUAGES"),
        allow_failed_grammars: option_env!("TSLP_ALLOW_FAILED_GRAMMARS"),
    };

/// Build-time environment accepted for a releasable optional parser worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParserPackBuildEnvironment<'a> {
    /// Whether dependency construction was forced offline.
    offline: Option<&'a str>,
    /// Whether grammars were built as independently loaded libraries.
    link_mode: Option<&'a str>,
    /// Optional upstream source-build selector, which `ProjectAtlas` forbids.
    language_filter: Option<&'a str>,
    /// Optional upstream partial-success escape hatch, which `ProjectAtlas` forbids.
    allow_failed_grammars: Option<&'a str>,
}

/// Closed startup operations accepted by the separately packaged worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerInvocation {
    /// Validate the compiled worker contract without opening pack files.
    VerifyBuildContract,
    /// Enter the contained resident protocol loop.
    Serve {
        /// Exact inherited Linux launch descriptors.
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        descriptors: LinuxAuthorityDescriptors,
    },
}

/// Dynamic inherited descriptor identities for one Linux resident launch.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxAuthorityDescriptors {
    /// Sealed artifact-manifest descriptor.
    artifact: i32,
    /// Sealed accepted-manifest descriptor.
    accepted: i32,
    /// Sealed native-policy descriptor.
    policy: i32,
    /// Sealed selected-grammar descriptor.
    grammar: i32,
}

/// Bounded projection of the immutable native-import policy used before containment.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Deserialize)]
struct RuntimeMappingPolicy {
    /// Exact policy schema understood by the worker.
    schema_version: u32,
    /// Closed target rows from which the current Linux row is selected.
    platforms: Vec<RuntimeMappingPlatformPolicy>,
}

/// Target-scoped projection of exact eager worker mappings.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Deserialize)]
struct RuntimeMappingPlatformPolicy {
    /// Artifact target owning this row.
    platform: PackPlatform,
    /// Exact direct runtime libraries that must already be mapped before `main`.
    worker_preloaded_libraries: Vec<String>,
}

/// Prepared dynamic parser runtime retained for one grammar-affined session.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
struct PreparedParserRuntime {
    /// Executable-relative pack authority retained on path-based platforms.
    pack_root: PathBuf,
    /// Manifest-scoped dynamic grammar registry.
    registry: LanguageRegistry,
    /// Reused parser with no language until the first authenticated request.
    parser: Parser,
    /// Exact first-request language retained for session affinity.
    language: Option<ParserLanguageIdentity>,
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl PreparedParserRuntime {
    /// Prepare the existing path-based runtime used by Windows containment.
    fn new(pack_root: &Path) -> Self {
        Self {
            pack_root: pack_root.to_path_buf(),
            registry: LanguageRegistry::with_libs_dir(pack_root.join(GRAMMAR_LIBRARY_DIRECTORY)),
            parser: Parser::new(),
            language: None,
        }
    }

    /// Load the first accepted grammar and reject every later grammar change.
    fn admit_language(
        &mut self,
        requested: &ParserLanguageIdentity,
    ) -> Result<(), ParserFailureCode> {
        if let Some(language) = &self.language {
            return if language == requested {
                Ok(())
            } else {
                Err(ParserFailureCode::LanguageMismatch)
            };
        }

        let grammar = approve_grammar_selection(&self.pack_root, requested.as_str())
            .map_err(|error| grammar_admission_failure(&error))?;
        let language = self
            .registry
            .get_language(&grammar.language_id)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        let actual_abi = language.abi_version();
        let expected_abi = usize::try_from(grammar.abi_export.expected_abi)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        if actual_abi != expected_abi {
            return Err(ParserFailureCode::ArtifactMismatch);
        }
        self.parser
            .set_language(&language)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        self.language = Some(requested.clone());
        Ok(())
    }
}

/// Linux runtime bound to one selected sealed grammar for its full lifetime.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct PreparedParserRuntime {
    /// Reused parser, dropped before the library that owns its language tables.
    parser: Parser,
    /// Resident library retained longer than `parser` and every derived language.
    library: Option<Library>,
    /// Read-only sealed grammar descriptor retained as the launch authority.
    grammar_file: File,
    /// Exact accepted grammar identity selected by the artifact inventory.
    grammar: AcceptedGrammar,
    /// Exact first-request language retained for session affinity.
    language: Option<ParserLanguageIdentity>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl PreparedParserRuntime {
    /// Retain one selected sealed grammar without loading it before authentication.
    fn new(grammar_file: File, grammar: AcceptedGrammar) -> Self {
        Self {
            parser: Parser::new(),
            library: None,
            grammar_file,
            grammar,
            language: None,
        }
    }

    /// Load the selected grammar only for its exact first authenticated request.
    fn admit_language(
        &mut self,
        requested: &ParserLanguageIdentity,
    ) -> Result<(), ParserFailureCode> {
        if let Some(language) = &self.language {
            return if language == requested {
                Ok(())
            } else {
                Err(ParserFailureCode::LanguageMismatch)
            };
        }
        if self.grammar.language_id != requested.as_str() {
            return Err(ParserFailureCode::LanguageMismatch);
        }

        let (library, language) = load_sealed_grammar(&self.grammar_file, &self.grammar)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        let actual_abi = language.abi_version();
        let expected_abi = usize::try_from(self.grammar.abi_export.expected_abi)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        if actual_abi != expected_abi {
            return Err(ParserFailureCode::ArtifactMismatch);
        }
        self.parser
            .set_language(&language)
            .map_err(|_source| ParserFailureCode::ArtifactMismatch)?;
        self.library = Some(library);
        self.language = Some(requested.clone());
        Ok(())
    }

    /// Borrow the selected grammar descriptor for containment admission.
    fn grammar_file(&self) -> &File {
        &self.grammar_file
    }
}

/// Load one audited Tree-sitter constructor from the exact selected grammar descriptor.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[expect(
    unsafe_code,
    reason = "the fully sealed artifact validates the exact audited library and C export before this bounded loader call"
)]
fn load_sealed_grammar(
    grammar_file: &File,
    grammar: &AcceptedGrammar,
) -> Result<(Library, Language), libloading::Error> {
    use std::os::fd::AsRawFd as _;

    let path = PathBuf::from(format!("/proc/self/fd/{}", grammar_file.as_raw_fd()));
    // SAFETY: `path` names the retained read-only fully sealed memfd whose
    // bytes, role, and digest were matched to this unique accepted grammar.
    let library = unsafe { Library::new(path) }?;
    // SAFETY: the validated logical row supplies a C identifier and the
    // artifact's native audit proves that it is the grammar constructor export.
    let constructor = unsafe {
        library.get::<unsafe extern "C" fn() -> *const ()>(
            grammar.abi_export.export_symbol.as_str().as_bytes(),
        )?
    };
    // SAFETY: the audited Tree-sitter constructor is retained by `library`;
    // `PreparedParserRuntime` drops its parser before that library.
    let language = Language::new(unsafe { LanguageFn::from_raw(*constructor) });
    Ok((library, language))
}

/// Fail-closed startup or protocol-boundary error.
#[derive(Debug, Error)]
enum WorkerStartupError {
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
    /// The operating system could not identify the running worker executable.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    #[error("cannot resolve the running worker executable: {0}")]
    CurrentExecutable(String),
    /// The running executable path was not absolute.
    #[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
    #[error("refusing non-absolute worker executable path: {}", .0.display())]
    RelativeExecutable(PathBuf),
    /// The running executable did not have a containing directory.
    #[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
    #[error("worker executable has no containing pack directory: {}", .0.display())]
    MissingExecutableParent(PathBuf),
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
        /// Rust target operating-system identity.
        os: &'static str,
        /// Rust target architecture identity.
        architecture: &'static str,
    },
    /// An executable-relative pack file could not be read within its hard bound.
    #[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
    #[error("cannot read optional parser {role} {}: {message}", path.display())]
    PackFileRead {
        /// Stable file responsibility.
        role: &'static str,
        /// Exact executable-relative path.
        path: PathBuf,
        /// Bounded later at the stderr process boundary.
        message: String,
    },
    /// Reading one already admitted sealed Linux authority failed.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("cannot read sealed optional parser {role}: {source}")]
    AuthorityRead {
        /// Stable launch-payload responsibility.
        role: &'static str,
        /// Bounded descriptor read failure.
        #[source]
        source: io::Error,
    },
    /// The logical manifest failed typed validation.
    #[error("optional parser manifest validation failed: {0}")]
    InvalidManifest(String),
    /// The sealed Linux documents or selected grammar did not form one exact artifact.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("optional parser sealed Linux authority is invalid: {reason}")]
    InvalidLinuxAuthority {
        /// Stable content-free rejection reason.
        reason: &'static str,
    },
    /// The artifact manifest could not provide a valid Linux runtime-mapping policy.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("optional parser Linux runtime-mapping policy is invalid: {reason}")]
    InvalidRuntimeMappingPolicy {
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// One strict artifact or native-import policy document could not be decoded.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("optional parser {role} JSON is invalid: {source}")]
    RuntimeMappingPolicyJson {
        /// Stable document responsibility.
        role: &'static str,
        /// Strict JSON decoding failure.
        #[source]
        source: serde_json::Error,
    },
    /// The requested grammar was not one of the accepted logical rows.
    #[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
    #[error("optional parser grammar {0:?} is not accepted by the packaged manifest")]
    UnknownGrammar(String),
    /// The contained process ended before a required protocol frame.
    #[error("optional parser protocol ended before {phase}")]
    UnexpectedEnd {
        /// Stable expected-frame phase.
        phase: &'static str,
    },
    /// Reading one exact protocol frame failed.
    #[error("could not read optional parser {phase}: {source}")]
    StreamRead {
        /// Stable read phase.
        phase: &'static str,
        /// Pipe read failure.
        #[source]
        source: io::Error,
    },
    /// Writing one exact protocol frame failed.
    #[error("could not write optional parser {phase}: {source}")]
    StreamWrite {
        /// Stable write phase.
        phase: &'static str,
        /// Pipe write failure.
        #[source]
        source: io::Error,
    },
    /// A framed or typed protocol invariant failed.
    #[error("optional parser protocol violation: {0}")]
    Protocol(#[from] ParserProtocolError),
    /// Linux containment failed before any protocol input was read.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[error("optional parser Linux containment failed at {stage:?}: {source}")]
    LinuxContainment {
        /// Closed containment stage.
        stage: parser_worker_containment::ParserWorkerContainmentStage,
        /// Typed containment failure.
        #[source]
        source: parser_worker_containment::ParserWorkerContainmentError,
    },
}

/// Validate the compile-time dependency build policy.
fn validate_build_environment(
    environment: ParserPackBuildEnvironment<'_>,
) -> Result<(), WorkerStartupError> {
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
fn validate_worker_build_contract() -> Result<(), WorkerStartupError> {
    validate_build_environment(COMPILED_BUILD_ENVIRONMENT)?;
    validate_empty_compiled_registry(&LanguageRegistry::new().available_languages())
}

/// Require the upstream registry to expose no built-in grammar identities.
fn validate_empty_compiled_registry(languages: &[String]) -> Result<(), WorkerStartupError> {
    if languages.is_empty() {
        return Ok(());
    }
    Err(WorkerStartupError::CompiledGrammarRegistryNotEmpty {
        count: languages.len(),
    })
}

/// Require one exact build-time value.
fn require_build_value(
    actual: Option<&str>,
    name: &'static str,
    expected: &'static str,
) -> Result<(), WorkerStartupError> {
    if actual == Some(expected) {
        return Ok(());
    }
    Err(WorkerStartupError::InvalidBuildVariable {
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
) -> Result<(), WorkerStartupError> {
    if actual.is_none() {
        return Ok(());
    }
    Err(WorkerStartupError::InvalidBuildVariable {
        name,
        expected: "absent",
        observed: "present",
    })
}

/// Resolve the pack root exclusively from an absolute running executable path.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
fn executable_pack_root(executable: &Path) -> Result<PathBuf, WorkerStartupError> {
    if !executable.is_absolute() {
        return Err(WorkerStartupError::RelativeExecutable(
            executable.to_path_buf(),
        ));
    }
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| WorkerStartupError::MissingExecutableParent(executable.to_path_buf()))
}

/// Require the broker-canonicalized executable-relative Windows pack directory.
///
/// The Windows broker resolves the exact worker path before launch. Reopening
/// the worker's parent with `canonicalize` after `AppContainer` admission fails
/// even though the artifact-scoped package ACE authorizes direct pack reads.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn validated_pack_root(pack_root: &Path) -> Result<PathBuf, WorkerStartupError> {
    let metadata = pack_root
        .metadata()
        .map_err(|error| WorkerStartupError::PackFileRead {
            role: "pack root",
            path: pack_root.to_path_buf(),
            message: error.to_string(),
        })?;
    if !metadata.is_dir() {
        return Err(WorkerStartupError::PackFileRead {
            role: "pack root",
            path: pack_root.to_path_buf(),
            message: "expected a directory".to_string(),
        });
    }
    Ok(pack_root.to_path_buf())
}

/// Parse exactly one closed worker operation without accepting grammar or path input.
fn parse_worker_invocation(
    mut arguments: impl Iterator<Item = OsString>,
) -> Result<WorkerInvocation, WorkerStartupError> {
    let argument = arguments
        .next()
        .ok_or(WorkerStartupError::MissingInvocation)?;
    if argument == VERIFY_BUILD_CONTRACT_ARGUMENT {
        return if arguments.next().is_none() {
            Ok(WorkerInvocation::VerifyBuildContract)
        } else {
            Err(WorkerStartupError::UnexpectedArguments)
        };
    }
    if argument != SERVE_ARGUMENT {
        return Err(WorkerStartupError::UnexpectedArguments);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let artifact = parse_linux_descriptor_argument(&mut arguments, ARTIFACT_FD_ARGUMENT)?;
        let accepted = parse_linux_descriptor_argument(&mut arguments, ACCEPTED_FD_ARGUMENT)?;
        let policy = parse_linux_descriptor_argument(&mut arguments, POLICY_FD_ARGUMENT)?;
        let grammar = parse_linux_descriptor_argument(&mut arguments, GRAMMAR_FD_ARGUMENT)?;
        if arguments.next().is_some() {
            return Err(WorkerStartupError::UnexpectedArguments);
        }
        let descriptors = [artifact, accepted, policy, grammar];
        if descriptors
            .iter()
            .enumerate()
            .any(|(index, descriptor)| descriptors[index + 1..].contains(descriptor))
        {
            return Err(WorkerStartupError::UnexpectedArguments);
        }
        Ok(WorkerInvocation::Serve {
            descriptors: LinuxAuthorityDescriptors {
                artifact,
                accepted,
                policy,
                grammar,
            },
        })
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        if arguments.next().is_some() {
            return Err(WorkerStartupError::UnexpectedArguments);
        }
        Ok(WorkerInvocation::Serve {})
    }
}

/// Consume one exact Linux descriptor flag and canonical decimal value.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn parse_linux_descriptor_argument(
    arguments: &mut impl Iterator<Item = OsString>,
    expected_flag: &str,
) -> Result<i32, WorkerStartupError> {
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(expected_flag)) {
        return Err(WorkerStartupError::UnexpectedArguments);
    }
    let value = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or(WorkerStartupError::UnexpectedArguments)?;
    let descriptor = value
        .parse::<i32>()
        .map_err(|_source| WorkerStartupError::UnexpectedArguments)?;
    if descriptor <= 2 || value != descriptor.to_string() {
        return Err(WorkerStartupError::UnexpectedArguments);
    }
    Ok(descriptor)
}

/// Read one executable-relative pack file without trusting its on-disk size.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
fn read_bounded_pack_file(
    path: &Path,
    role: &'static str,
    maximum: usize,
) -> Result<Vec<u8>, WorkerStartupError> {
    let file = File::open(path).map_err(|error| WorkerStartupError::PackFileRead {
        role,
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let maximum_with_sentinel = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(maximum_with_sentinel)
        .read_to_end(&mut bytes)
        .map_err(|error| WorkerStartupError::PackFileRead {
            role,
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if bytes.len() > maximum {
        return Err(WorkerStartupError::PackFileRead {
            role,
            path: path.to_path_buf(),
            message: format!("file exceeds {maximum} bytes"),
        });
    }
    Ok(bytes)
}

/// Parsed sealed Linux authority retained through containment and the session.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxWorkerAuthority {
    /// Exact artifact-manifest byte identity used by READY and every request.
    artifact_identity: ParserArtifactIdentity,
    /// Exact eager runtime mapping set admitted before containment.
    runtime_mappings: Vec<String>,
    /// Unique accepted grammar row matching the selected grammar descriptor.
    grammar: AcceptedGrammar,
    /// Read-only sealed selected grammar descriptor.
    grammar_file: File,
}

/// Read one already bounded sealed document from its beginning.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_sealed_document(
    mut file: File,
    role: &'static str,
    maximum: usize,
) -> Result<Vec<u8>, WorkerStartupError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| WorkerStartupError::AuthorityRead { role, source })?;
    let maximum_with_sentinel = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(maximum_with_sentinel)
        .read_to_end(&mut bytes)
        .map_err(|source| WorkerStartupError::AuthorityRead { role, source })?;
    if bytes.len() > maximum {
        return Err(WorkerStartupError::InvalidLinuxAuthority {
            reason: "a sealed authority document exceeds its byte ceiling",
        });
    }
    Ok(bytes)
}

/// Hash one sealed grammar without materializing its native bytes.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn hash_sealed_grammar(file: &mut File) -> Result<(u64, String), WorkerStartupError> {
    let expected_bytes = file
        .metadata()
        .map_err(|source| WorkerStartupError::AuthorityRead {
            role: "selected grammar",
            source,
        })?
        .len();
    file.seek(SeekFrom::Start(0))
        .map_err(|source| WorkerStartupError::AuthorityRead {
            role: "selected grammar",
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut observed_bytes = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| WorkerStartupError::AuthorityRead {
                role: "selected grammar",
                source,
            })?;
        if read == 0 {
            break;
        }
        observed_bytes = observed_bytes
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or(WorkerStartupError::InvalidLinuxAuthority {
                reason: "selected grammar byte count overflowed",
            })?;
        hasher.update(&buffer[..read]);
    }
    if observed_bytes != expected_bytes {
        return Err(WorkerStartupError::InvalidLinuxAuthority {
            reason: "selected grammar bytes differ from descriptor metadata",
        });
    }
    Ok((observed_bytes, lower_hex(&hasher.finalize())))
}

/// Select the unique accepted grammar row matching exact sealed native bytes.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn select_sealed_grammar(
    files: &[projectatlas_core::optional_parser_pack::ParserPackPayloadFile],
    logical: &OptionalParserPackManifest,
    grammar_bytes: u64,
    grammar_sha256: &str,
) -> Result<AcceptedGrammar, WorkerStartupError> {
    let mut matching = files.iter().filter(|file| {
        matches!(&file.role, ParserPackPayloadRole::GrammarLibrary { .. })
            && file.bytes == grammar_bytes
            && file.sha256.as_str() == grammar_sha256
    });
    let selected = matching
        .next()
        .ok_or(WorkerStartupError::InvalidLinuxAuthority {
            reason: "selected grammar descriptor matches no artifact grammar row",
        })?;
    if matching.next().is_some() {
        return Err(WorkerStartupError::InvalidLinuxAuthority {
            reason: "selected grammar descriptor matches multiple artifact grammar rows",
        });
    }
    let ParserPackPayloadRole::GrammarLibrary { language_id } = &selected.role else {
        return Err(WorkerStartupError::InvalidLinuxAuthority {
            reason: "selected grammar artifact row has another payload role",
        });
    };
    logical
        .grammars()
        .binary_search_by(|grammar| grammar.language_id.as_str().cmp(language_id.as_str()))
        .ok()
        .and_then(|index| logical.grammars().get(index))
        .cloned()
        .ok_or(WorkerStartupError::InvalidLinuxAuthority {
            reason: "selected artifact grammar has no accepted logical row",
        })
}

/// Parse the hash-bound exact eager Linux DSO set.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn load_runtime_mapping_policy(
    platform: PackPlatform,
    expected_policy_sha256: &str,
    policy_bytes: &[u8],
) -> Result<Vec<String>, WorkerStartupError> {
    if platform != PackPlatform::LinuxX86_64 {
        return Err(WorkerStartupError::InvalidRuntimeMappingPolicy {
            reason: "artifact target is not Linux x86-64",
        });
    }

    if sha256_hex(policy_bytes) != expected_policy_sha256 {
        return Err(WorkerStartupError::InvalidRuntimeMappingPolicy {
            reason: "native-import policy digest differs from the artifact manifest",
        });
    }
    let policy =
        serde_json::from_slice::<RuntimeMappingPolicy>(policy_bytes).map_err(|source| {
            WorkerStartupError::RuntimeMappingPolicyJson {
                role: "native-import policy",
                source,
            }
        })?;
    if policy.schema_version != OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_SCHEMA_VERSION {
        return Err(WorkerStartupError::InvalidRuntimeMappingPolicy {
            reason: "native-import policy schema is unsupported",
        });
    }
    let mut selected = None;
    for row in policy.platforms {
        if row.platform != PackPlatform::LinuxX86_64 {
            continue;
        }
        if selected.replace(row.worker_preloaded_libraries).is_some() {
            return Err(WorkerStartupError::InvalidRuntimeMappingPolicy {
                reason: "native-import policy contains duplicate Linux rows",
            });
        }
    }
    let mut libraries = selected.ok_or(WorkerStartupError::InvalidRuntimeMappingPolicy {
        reason: "native-import policy has no Linux row",
    })?;
    if libraries.is_empty()
        || libraries.windows(2).any(|pair| pair[0] >= pair[1])
        || libraries
            .iter()
            .any(|library| !is_runtime_library_basename(library))
        || libraries
            .iter()
            .any(|library| library == OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME)
    {
        return Err(WorkerStartupError::InvalidRuntimeMappingPolicy {
            reason: "Linux eager runtime libraries are not a sorted unique basename set",
        });
    }
    libraries.push(OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME.to_owned());
    libraries.sort_unstable();
    Ok(libraries)
}

/// Require the sealed accepted-manifest bytes to match their artifact binding.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn validate_accepted_manifest_digest(
    expected_sha256: &str,
    accepted_bytes: &[u8],
) -> Result<(), WorkerStartupError> {
    if sha256_hex(accepted_bytes) != expected_sha256 {
        return Err(WorkerStartupError::InvalidLinuxAuthority {
            reason: "accepted manifest digest differs from the artifact manifest",
        });
    }
    Ok(())
}

/// Consume and cross-validate the complete sealed Linux launch authority.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn load_linux_worker_authority(
    descriptors: LinuxAuthorityDescriptors,
) -> Result<LinuxWorkerAuthority, WorkerStartupError> {
    use parser_worker_containment::take_sealed_authority_descriptor;

    let artifact_file = take_sealed_authority_descriptor(
        descriptors.artifact,
        "artifact manifest",
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
    )
    .map_err(|source| WorkerStartupError::LinuxContainment {
        stage: source.stage(),
        source,
    })?;
    let accepted_file = take_sealed_authority_descriptor(
        descriptors.accepted,
        "accepted capability manifest",
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES).unwrap_or(u64::MAX),
    )
    .map_err(|source| WorkerStartupError::LinuxContainment {
        stage: source.stage(),
        source,
    })?;
    let policy_file = take_sealed_authority_descriptor(
        descriptors.policy,
        "native-import policy",
        u64::try_from(NATIVE_IMPORT_POLICY_MAX_BYTES).unwrap_or(u64::MAX),
    )
    .map_err(|source| WorkerStartupError::LinuxContainment {
        stage: source.stage(),
        source,
    })?;
    let mut grammar_file = take_sealed_authority_descriptor(
        descriptors.grammar,
        "selected grammar",
        OPTIONAL_PARSER_PACK_MAX_FILE_BYTES,
    )
    .map_err(|source| WorkerStartupError::LinuxContainment {
        stage: source.stage(),
        source,
    })?;

    let artifact_bytes = read_sealed_document(
        artifact_file,
        "artifact manifest",
        OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    )?;
    let accepted_bytes = read_sealed_document(
        accepted_file,
        "accepted capability manifest",
        OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    )?;
    let policy_bytes = read_sealed_document(
        policy_file,
        "native-import policy",
        NATIVE_IMPORT_POLICY_MAX_BYTES,
    )?;
    let logical = OptionalParserPackManifest::from_json(&accepted_bytes)
        .map_err(|error| WorkerStartupError::InvalidManifest(error.to_string()))?;
    let artifact = serde_json::from_slice::<OptionalParserPackArtifactManifest>(&artifact_bytes)
        .map_err(|source| WorkerStartupError::RuntimeMappingPolicyJson {
            role: "artifact manifest",
            source,
        })?;
    artifact
        .validate(&logical)
        .map_err(|error| WorkerStartupError::InvalidManifest(error.to_string()))?;
    validate_accepted_manifest_digest(artifact.accepted_manifest_sha256.as_str(), &accepted_bytes)?;
    let runtime_mappings = load_runtime_mapping_policy(
        artifact.platform,
        artifact.native_audit.policy_sha256.as_str(),
        &policy_bytes,
    )?;
    let (grammar_bytes, grammar_sha256) = hash_sealed_grammar(&mut grammar_file)?;
    let grammar = select_sealed_grammar(&artifact.files, &logical, grammar_bytes, &grammar_sha256)?;
    Ok(LinuxWorkerAuthority {
        artifact_identity: ParserArtifactIdentity::for_bytes(&artifact_bytes),
        runtime_mappings,
        grammar,
        grammar_file,
    })
}

/// Return whether one runtime identity is a bounded platform-neutral basename.
#[cfg(any(test, all(target_os = "linux", target_arch = "x86_64")))]
fn is_runtime_library_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

/// Render one exact lowercase SHA-256 digest.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    lower_hex(&digest)
}

/// Render exact bytes as lowercase hexadecimal.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn lower_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Select one exact grammar from the validated executable-relative logical manifest.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
fn approve_grammar_selection(
    pack_root: &Path,
    grammar_id: &str,
) -> Result<AcceptedGrammar, WorkerStartupError> {
    let manifest_path = pack_root.join(ACCEPTED_MANIFEST_FILE_NAME);
    let manifest_bytes = read_bounded_pack_file(
        &manifest_path,
        "accepted capability manifest",
        OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    )?;
    let manifest = OptionalParserPackManifest::from_json(&manifest_bytes)
        .map_err(|error| WorkerStartupError::InvalidManifest(error.to_string()))?;
    manifest
        .grammars()
        .binary_search_by(|candidate| candidate.language_id.as_str().cmp(grammar_id))
        .ok()
        .and_then(|index| manifest.grammars().get(index))
        .cloned()
        .ok_or_else(|| WorkerStartupError::UnknownGrammar(grammar_id.to_string()))
}

/// Convert manifest or payload admission failure to a closed worker response.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
const fn grammar_admission_failure(error: &WorkerStartupError) -> ParserFailureCode {
    match error {
        WorkerStartupError::UnknownGrammar(_) => ParserFailureCode::LanguageMismatch,
        WorkerStartupError::PackFileRead { .. } | WorkerStartupError::InvalidManifest(_) => {
            ParserFailureCode::ArtifactMismatch
        }
        _ => ParserFailureCode::InternalFailure,
    }
}

/// Hash the exact immutable artifact manifest independently inside the worker.
#[cfg(any(test, not(all(target_os = "linux", target_arch = "x86_64"))))]
fn observe_artifact_identity(
    pack_root: &Path,
) -> Result<ParserArtifactIdentity, WorkerStartupError> {
    let path = pack_root.join(ARTIFACT_MANIFEST_FILE_NAME);
    let bytes = read_bounded_pack_file(
        &path,
        "artifact manifest",
        OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    )?;
    Ok(ParserArtifactIdentity::for_bytes(&bytes))
}

/// Read one complete frame after validating its fixed header before allocation.
fn read_exact_frame(
    input: &mut impl Read,
    phase: &'static str,
) -> Result<Option<Vec<u8>>, WorkerStartupError> {
    let mut header_bytes = [0_u8; PARSER_FRAME_HEADER_BYTES];
    loop {
        match input.read(&mut header_bytes[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => return Err(WorkerStartupError::StreamRead { phase, source }),
        }
    }
    input
        .read_exact(&mut header_bytes[1..])
        .map_err(|source| WorkerStartupError::StreamRead { phase, source })?;
    let header = ParserFrameHeader::decode(&header_bytes)?;
    let payload_len = header.payload_len() as usize;
    let frame_len = PARSER_FRAME_HEADER_BYTES.checked_add(payload_len).ok_or(
        ParserProtocolError::FramePayloadTooLarge {
            kind: header.kind(),
            actual: u32::MAX,
            maximum: header.kind().maximum_payload_bytes(),
        },
    )?;
    let mut frame = vec![0_u8; frame_len];
    frame[..PARSER_FRAME_HEADER_BYTES].copy_from_slice(&header_bytes);
    input
        .read_exact(&mut frame[PARSER_FRAME_HEADER_BYTES..])
        .map_err(|source| WorkerStartupError::StreamRead { phase, source })?;
    Ok(Some(frame))
}

/// Read one required frame or return a typed premature-end failure.
fn read_required_frame(
    input: &mut impl Read,
    phase: &'static str,
) -> Result<Vec<u8>, WorkerStartupError> {
    read_exact_frame(input, phase)?.ok_or(WorkerStartupError::UnexpectedEnd { phase })
}

/// Flush one already validated protocol frame to the supervisor.
fn write_frame(
    output: &mut impl Write,
    phase: &'static str,
    bytes: &[u8],
) -> Result<(), WorkerStartupError> {
    output
        .write_all(bytes)
        .and_then(|()| output.flush())
        .map_err(|source| WorkerStartupError::StreamWrite { phase, source })
}

/// Emit READY only after locally validating its complete launch identity.
fn write_ready(
    output: &mut impl Write,
    session: &projectatlas_core::optional_parser_protocol::ParserSessionIdentity,
    artifact: &ParserArtifactIdentity,
    containment: ParserContainmentKind,
) -> Result<(), WorkerStartupError> {
    let bytes = encode_parser_control(&ParserControl::Ready(ParserReady::new(
        session.clone(),
        artifact.clone(),
        containment,
    )))?;
    let frame = ParserFrame::decode_exact(&bytes)?;
    decode_parser_ready_for_launch(frame, session, artifact, containment)?;
    write_frame(output, "READY", &bytes)
}

/// Emit one monotonic progress observation after validating it as the supervisor will.
fn write_progress(
    output: &mut impl Write,
    request: &ParserRequest,
    previous: Option<&ParserProgress>,
    sequence: u32,
    stage: ParserProgressStage,
) -> Result<ParserProgress, WorkerStartupError> {
    let progress = ParserProgress::new(
        ParserResponseIdentity::for_request(request),
        sequence,
        stage,
        0,
        None,
    )?;
    let bytes = encode_parser_control(&ParserControl::Progress(progress))?;
    let frame = ParserFrame::decode_exact(&bytes)?;
    let (validated, _disposition) = decode_parser_progress_for_request(frame, request, previous)?;
    write_frame(output, "progress", &bytes)?;
    Ok(validated)
}

/// Emit one identity-bound closed request failure.
fn write_failure(
    output: &mut impl Write,
    request: &ParserRequest,
    code: ParserFailureCode,
) -> Result<(), WorkerStartupError> {
    let failure = ParserFailure::new(ParserResponseIdentity::for_request(request), code);
    let bytes = encode_parser_control(&ParserControl::Failure(failure))?;
    let frame = ParserFrame::decode_exact(&bytes)?;
    decode_parser_failure_for_request(frame, request)?;
    write_frame(output, "failure", &bytes)
}

/// Emit one bounded completion after applying the same output checks as the supervisor.
fn write_completion(
    output: &mut impl Write,
    request: &ParserRequest,
    evidence: ParserCompletionEvidence,
) -> Result<(), WorkerStartupError> {
    let completion = ParserCompletion::new(ParserResponseIdentity::for_request(request), evidence);
    let bytes = encode_parser_control(&ParserControl::Completion(completion))?;
    let frame = ParserFrame::decode_exact(&bytes)?;
    decode_parser_completion_for_request(frame, request)?;
    write_frame(output, "completion", &bytes)
}

/// Increment one bounded structural counter or classify the request limit.
fn increment_bounded(value: &mut u32, maximum: u32) -> Result<(), ParserFailureCode> {
    *value = value
        .checked_add(1)
        .ok_or(ParserFailureCode::LimitExceeded)?;
    if *value > maximum {
        return Err(ParserFailureCode::LimitExceeded);
    }
    Ok(())
}

/// Traverse one syntax tree without recursion or materializing node collections.
fn collect_completion_evidence(
    tree: &Tree,
    request: &ParserRequest,
) -> Result<ParserCompletionEvidence, ParserFailureCode> {
    let root = tree.root_node();
    let mut cursor = tree.walk();
    let mut depth = 0_u32;
    let mut maximum_depth = 0_u32;
    let mut visited_node_count = 0_u32;
    let mut named_node_count = 0_u32;
    let mut error_node_count = 0_u32;
    let mut missing_node_count = 0_u32;
    let limits = request.limits();

    loop {
        let node = cursor.node();
        increment_bounded(&mut visited_node_count, limits.node_count())?;
        if node.is_named() {
            increment_bounded(&mut named_node_count, limits.node_count())?;
        }
        if node.is_error() {
            increment_bounded(&mut error_node_count, limits.node_count())?;
        }
        if node.is_missing() {
            increment_bounded(&mut missing_node_count, limits.node_count())?;
        }
        maximum_depth = maximum_depth.max(depth);
        if maximum_depth > limits.tree_depth() {
            return Err(ParserFailureCode::LimitExceeded);
        }

        if cursor.goto_first_child() {
            depth = depth
                .checked_add(1)
                .ok_or(ParserFailureCode::LimitExceeded)?;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                let root_start = u32::try_from(root.start_byte())
                    .map_err(|_source| ParserFailureCode::LimitExceeded)?;
                let root_end = u32::try_from(root.end_byte())
                    .map_err(|_source| ParserFailureCode::LimitExceeded)?;
                let root_kind = ParserSyntaxKind::new(root.kind())
                    .map_err(|_source| ParserFailureCode::ParseRejected)?;
                let evidence = ParserCompletionEvidence::new(
                    root_kind,
                    root_start,
                    root_end,
                    root.has_error(),
                    named_node_count,
                    error_node_count,
                    missing_node_count,
                    maximum_depth,
                )
                .map_err(|_source| ParserFailureCode::LimitExceeded)?;
                evidence
                    .validate_for(request)
                    .map_err(|_source| ParserFailureCode::LimitExceeded)?;
                return Ok(evidence);
            }
            depth = depth.saturating_sub(1);
        }
    }
}

/// Process one request/source pair and return whether this session remains healthy.
fn process_request(
    runtime: &mut PreparedParserRuntime,
    input: &mut impl Read,
    output: &mut impl Write,
    request: &ParserRequest,
) -> Result<bool, WorkerStartupError> {
    let source_bytes = read_required_frame(input, "raw source")?;
    let source_frame = ParserFrame::decode_exact(&source_bytes)?;
    if request.validate_source_frame(source_frame).is_err() {
        write_failure(output, request, ParserFailureCode::InvalidSource)?;
        return Ok(false);
    }
    if let Err(code) = runtime.admit_language(request.language()) {
        write_failure(output, request, code)?;
        return Ok(false);
    }

    let accepted = write_progress(output, request, None, 1, ParserProgressStage::Accepted)?;
    let parsing = write_progress(
        output,
        request,
        Some(&accepted),
        2,
        ParserProgressStage::Parsing,
    )?;
    runtime.parser.reset();
    let Some(tree) = runtime.parser.parse(source_frame.payload(), None) else {
        write_failure(output, request, ParserFailureCode::ParseRejected)?;
        return Ok(false);
    };
    let _collecting = write_progress(
        output,
        request,
        Some(&parsing),
        3,
        ParserProgressStage::CollectingEvidence,
    )?;
    match collect_completion_evidence(&tree, request) {
        Ok(evidence) => match write_completion(output, request, evidence) {
            Ok(()) => Ok(true),
            Err(WorkerStartupError::Protocol(ParserProtocolError::RequestLimitExceeded {
                ..
            })) => {
                write_failure(output, request, ParserFailureCode::LimitExceeded)?;
                Ok(true)
            }
            Err(error) => Err(error),
        },
        Err(code) => {
            write_failure(output, request, code)?;
            Ok(matches!(code, ParserFailureCode::LimitExceeded))
        }
    }
}

/// Serve one contained protocol session until clean EOF or a closed unhealthy result.
fn serve_session(
    runtime: &mut PreparedParserRuntime,
    input: &mut impl Read,
    output: &mut impl Write,
    artifact: &ParserArtifactIdentity,
    containment: ParserContainmentKind,
) -> Result<(), WorkerStartupError> {
    let session_bytes = read_required_frame(input, "SessionOpen")?;
    let session_frame = ParserFrame::decode_exact(&session_bytes)?;
    let session_open = decode_parser_session_open(session_frame)?;
    write_ready(output, session_open.session(), artifact, containment)?;

    while let Some(request_bytes) = read_exact_frame(input, "request")? {
        let request_frame = ParserFrame::decode_exact(&request_bytes)?;
        let request =
            decode_parser_request_for_session(request_frame, session_open.session(), artifact)?;
        if !process_request(runtime, input, output, &request)? {
            break;
        }
    }
    Ok(())
}

/// Lock the process standard streams and run the contained protocol loop.
fn serve_standard_streams(
    runtime: &mut PreparedParserRuntime,
    artifact: &ParserArtifactIdentity,
    containment: ParserContainmentKind,
) -> Result<(), WorkerStartupError> {
    let standard_input = io::stdin();
    let standard_output = io::stdout();
    let mut input = standard_input.lock();
    let mut output = standard_output.lock();
    serve_session(runtime, &mut input, &mut output, artifact, containment)
}

/// Prepare, contain, and serve the accepted Linux x86-64 worker.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn serve_worker(descriptors: LinuxAuthorityDescriptors) -> Result<(), WorkerStartupError> {
    use parser_worker_containment::{
        enforce_parser_worker_containment, observe_parser_worker_preconditions,
    };

    let authority = load_linux_worker_authority(descriptors)?;
    let mut runtime = PreparedParserRuntime::new(authority.grammar_file, authority.grammar);
    let preconditions =
        observe_parser_worker_preconditions(runtime.grammar_file(), &authority.runtime_mappings)
            .map_err(|source| WorkerStartupError::LinuxContainment {
                stage: source.stage(),
                source,
            })?;
    enforce_parser_worker_containment(runtime.grammar_file(), preconditions).map_err(|source| {
        WorkerStartupError::LinuxContainment {
            stage: source.stage(),
            source,
        }
    })?;
    serve_standard_streams(
        &mut runtime,
        &authority.artifact_identity,
        ParserContainmentKind::LinuxLandlockSeccomp,
    )
}

/// Serve the already admitted Windows x86-64 `AppContainer` worker.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn serve_worker(pack_root: &Path) -> Result<(), WorkerStartupError> {
    let pack_root = validated_pack_root(pack_root)?;
    let mut runtime = PreparedParserRuntime::new(&pack_root);
    let artifact = observe_artifact_identity(&pack_root)?;
    serve_standard_streams(
        &mut runtime,
        &artifact,
        ParserContainmentKind::WindowsAppContainerJob,
    )
}

/// Reject unshipped optional-parser targets before reading protocol input.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn serve_worker(_pack_root: &Path) -> Result<(), WorkerStartupError> {
    Err(WorkerStartupError::UnsupportedTarget {
        os: env::consts::OS,
        architecture: env::consts::ARCH,
    })
}

/// Run the fail-closed optional worker startup sequence.
fn run() -> Result<(), WorkerStartupError> {
    validate_worker_build_contract()?;
    match parse_worker_invocation(env::args_os().skip(1))? {
        WorkerInvocation::VerifyBuildContract => Ok(()),
        WorkerInvocation::Serve {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            descriptors,
        } => {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            {
                return serve_worker(descriptors);
            }
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
            {
                let executable = env::current_exe()
                    .map_err(|error| WorkerStartupError::CurrentExecutable(error.to_string()))?;
                let pack_root = executable_pack_root(&executable)?;
                serve_worker(&pack_root)
            }
        }
    }
}

/// Return the largest UTF-8 prefix allowed at the diagnostic boundary.
fn bounded_diagnostic(error: &WorkerStartupError) -> String {
    let mut message = error.to_string();
    let mut end = message.len().min(PARSER_MAX_STDERR_BYTES.saturating_sub(1));
    while !message.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    message.truncate(end);
    message
}

/// Emit one bounded worker diagnostic without formatting macros.
fn write_startup_error(error: &WorkerStartupError) -> io::Result<()> {
    let mut standard_error = io::stderr().lock();
    let diagnostic = bounded_diagnostic(error);
    standard_error.write_all(diagnostic.as_bytes())?;
    standard_error.write_all(b"\n")
}

/// Start the optional parser worker and map failure to the process boundary.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if write_startup_error(&error).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    //! Protect the worker's closed invocation, framing, and READY contract.

    use std::io::Cursor;

    use super::*;
    use projectatlas_core::optional_parser_protocol::{
        PARSER_PROTOCOL_VERSION, ParserContentDigest, ParserFrameKind, ParserLanguageIdentity,
        ParserRequestIdentity, ParserRequestLimits, ParserSessionIdentity, ParserSourceIdentity,
        decode_parser_failure_for_request, decode_parser_ready_for_launch, encode_parser_frame,
    };

    /// Return the only accepted dependency build environment.
    fn valid_environment() -> ParserPackBuildEnvironment<'static> {
        ParserPackBuildEnvironment {
            offline: Some("1"),
            link_mode: Some("dynamic"),
            language_filter: None,
            allow_failed_grammars: None,
        }
    }

    /// Return a typed test failure instead of panicking in a fallible test.
    fn require(condition: bool, message: &'static str) -> std::io::Result<()> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message))
        }
    }

    /// Build a runtime that will not reach grammar loading in protocol-only tests.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn protocol_test_runtime(pack_root: &Path) -> PreparedParserRuntime {
        PreparedParserRuntime::new(pack_root)
    }

    /// Build a Linux runtime with a retained dummy descriptor for protocol-only tests.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn protocol_test_runtime(
        _pack_root: &Path,
    ) -> Result<PreparedParserRuntime, Box<dyn std::error::Error>> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("CLI crate is not inside the workspace")?;
        let manifest = OptionalParserPackManifest::from_json(&std::fs::read(
            workspace_root.join("packaging/parser-pack/accepted-capabilities.json"),
        )?)?;
        let grammar = manifest
            .grammars()
            .first()
            .cloned()
            .ok_or("accepted grammar manifest is empty")?;
        let grammar_file =
            File::open(workspace_root.join("packaging/parser-pack/native-import-policy.json"))?;
        Ok(PreparedParserRuntime::new(grammar_file, grammar))
    }

    /// Accept only an offline dynamic worker build with repackaged libraries.
    #[test]
    fn build_environment_is_exact_and_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        validate_build_environment(valid_environment())?;

        for environment in [
            ParserPackBuildEnvironment {
                offline: None,
                ..valid_environment()
            },
            ParserPackBuildEnvironment {
                link_mode: Some("static"),
                ..valid_environment()
            },
            ParserPackBuildEnvironment {
                language_filter: Some("all"),
                ..valid_environment()
            },
            ParserPackBuildEnvironment {
                allow_failed_grammars: Some("1"),
                ..valid_environment()
            },
        ] {
            require(
                validate_build_environment(environment).is_err(),
                "invalid worker build environment was accepted",
            )?;
        }
        Ok(())
    }

    /// Prove the concrete worker dependency registry contains no built-in grammar.
    #[test]
    fn compiled_worker_registry_is_empty() -> Result<(), WorkerStartupError> {
        validate_worker_build_contract()
    }

    /// Keep accepted ELF SONAMEs bounded while rejecting paths and unsafe bytes.
    #[test]
    fn runtime_library_basename_accepts_sonames_and_rejects_paths() {
        for accepted in ["libc.so.6", "libgcc_s.so.1", "libm.so.6", "libstdc++.so.6"] {
            assert!(is_runtime_library_basename(accepted), "{accepted}");
        }
        for rejected in [
            "",
            "../libc.so.6",
            "lib/libc.so.6",
            r"lib\libc.so.6",
            "lib c.so.6",
            "libc.so.6\n",
            "libc.sø.6",
        ] {
            assert!(!is_runtime_library_basename(rejected), "{rejected:?}");
        }
        assert!(!is_runtime_library_basename(&"a".repeat(256)));
    }

    /// Prove the committed Linux policy is consumable by the packaged worker.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn packaged_linux_runtime_policy_matches_worker_vocabulary()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("CLI crate is not inside the workspace")?;
        let bytes =
            std::fs::read(workspace_root.join("packaging/parser-pack/native-import-policy.json"))?;
        let policy = serde_json::from_slice::<RuntimeMappingPolicy>(&bytes)?;
        let linux_rows = policy
            .platforms
            .into_iter()
            .filter(|row| row.platform == PackPlatform::LinuxX86_64)
            .collect::<Vec<_>>();
        require(linux_rows.len() == 1, "policy must contain one Linux row")?;
        let libraries = &linux_rows[0].worker_preloaded_libraries;
        require(!libraries.is_empty(), "Linux runtime policy is empty")?;
        require(
            libraries.windows(2).all(|pair| pair[0] < pair[1]),
            "Linux runtime policy is not sorted and unique",
        )?;
        require(
            libraries
                .iter()
                .all(|library| is_runtime_library_basename(library)),
            "Linux runtime policy contains an unsafe basename",
        )?;
        require(
            libraries
                .iter()
                .all(|library| library != OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME),
            "Linux runtime policy includes the separately admitted loader",
        )?;
        let mut completed = libraries.clone();
        completed.push(OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME.to_owned());
        completed.sort_unstable();
        completed.dedup();
        require(
            completed.len() == libraries.len() + 1,
            "Linux runtime loader was not added exactly once",
        )?;
        Ok(())
    }

    /// Reject mismatched sealed documents, ambiguous grammar bytes, and request drift.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn sealed_launch_authority_is_content_and_language_bound()
    -> Result<(), Box<dyn std::error::Error>> {
        use projectatlas_core::optional_parser_pack::{
            PackRelativePath, ParserPackPayloadFile, Sha256Digest,
        };

        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("CLI crate is not inside the workspace")?;
        let accepted_bytes =
            std::fs::read(workspace_root.join("packaging/parser-pack/accepted-capabilities.json"))?;
        let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
        let selected = logical
            .grammars()
            .first()
            .cloned()
            .ok_or("accepted grammar manifest is empty")?;
        let other = logical
            .grammars()
            .get(1)
            .ok_or("accepted grammar manifest needs two rows for ambiguity coverage")?;
        let grammar_bytes = b"sealed grammar";
        let grammar_sha256 = sha256_hex(grammar_bytes);
        let payload = ParserPackPayloadFile {
            path: PackRelativePath::new("lib/selected.so")?,
            role: ParserPackPayloadRole::GrammarLibrary {
                language_id: selected.language_id.clone(),
            },
            bytes: u64::try_from(grammar_bytes.len())?,
            sha256: Sha256Digest::new(grammar_sha256.clone())?,
        };

        let bound = select_sealed_grammar(
            std::slice::from_ref(&payload),
            &logical,
            payload.bytes,
            &grammar_sha256,
        )?;
        require(
            bound.language_id == selected.language_id,
            "selected grammar bytes did not bind their accepted row",
        )?;
        require(
            matches!(
                select_sealed_grammar(
                    std::slice::from_ref(&payload),
                    &logical,
                    payload.bytes,
                    &"0".repeat(64),
                ),
                Err(WorkerStartupError::InvalidLinuxAuthority {
                    reason: "selected grammar descriptor matches no artifact grammar row"
                })
            ),
            "unknown sealed grammar bytes were accepted",
        )?;

        let mut ambiguous = payload.clone();
        ambiguous.path = PackRelativePath::new("lib/other.so")?;
        ambiguous.role = ParserPackPayloadRole::GrammarLibrary {
            language_id: other.language_id.clone(),
        };
        require(
            matches!(
                select_sealed_grammar(
                    &[payload.clone(), ambiguous],
                    &logical,
                    payload.bytes,
                    &grammar_sha256,
                ),
                Err(WorkerStartupError::InvalidLinuxAuthority {
                    reason: "selected grammar descriptor matches multiple artifact grammar rows"
                })
            ),
            "ambiguous sealed grammar bytes were accepted",
        )?;

        validate_accepted_manifest_digest(&sha256_hex(&accepted_bytes), &accepted_bytes)?;
        require(
            validate_accepted_manifest_digest(&"0".repeat(64), &accepted_bytes).is_err(),
            "accepted manifest bytes ignored their artifact digest",
        )?;
        let policy_bytes =
            std::fs::read(workspace_root.join("packaging/parser-pack/native-import-policy.json"))?;
        load_runtime_mapping_policy(
            PackPlatform::LinuxX86_64,
            &sha256_hex(&policy_bytes),
            &policy_bytes,
        )?;
        require(
            load_runtime_mapping_policy(PackPlatform::LinuxX86_64, &"0".repeat(64), &policy_bytes)
                .is_err(),
            "native policy bytes ignored their artifact digest",
        )?;

        let grammar_file =
            File::open(workspace_root.join("packaging/parser-pack/native-import-policy.json"))?;
        let mut runtime = PreparedParserRuntime::new(grammar_file, selected.clone());
        let requested = logical
            .grammars()
            .iter()
            .find(|grammar| grammar.language_id != selected.language_id)
            .ok_or("accepted grammar manifest needs another language")?;
        require(
            runtime.admit_language(&ParserLanguageIdentity::new(
                requested.language_id.as_str(),
            )?) == Err(ParserFailureCode::LanguageMismatch),
            "first request selected a language other than its sealed grammar",
        )?;
        require(
            runtime.library.is_none() && runtime.language.is_none(),
            "language mismatch loaded or retained untrusted grammar state",
        )?;
        Ok(())
    }

    /// Reject any worker build whose dependency exposes a grammar identity.
    #[test]
    fn embedded_grammar_identity_fails_the_build_contract() {
        assert!(matches!(
            validate_empty_compiled_registry(&["ada".to_owned()]),
            Err(WorkerStartupError::CompiledGrammarRegistryNotEmpty { count: 1 })
        ));
    }

    /// Derive the pack root from the executable rather than the current directory.
    #[test]
    fn pack_root_is_executable_relative() -> Result<(), Box<dyn std::error::Error>> {
        let executable = if cfg!(windows) {
            Path::new(r"C:\ProjectAtlas\pack\projectatlas-parser-worker.exe")
        } else {
            Path::new("/opt/projectatlas/pack/projectatlas-parser-worker")
        };
        let expected = executable
            .parent()
            .ok_or_else(|| WorkerStartupError::MissingExecutableParent(executable.into()))?;
        let actual = executable_pack_root(executable)?;
        require(actual == expected, "pack root was not executable-relative")?;
        Ok(())
    }

    /// Accept only the exact build probe and platform serve authority.
    #[test]
    fn invocation_is_closed_and_contains_no_grammar_argument()
    -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let serve_arguments = [
            OsString::from(SERVE_ARGUMENT),
            OsString::from(ARTIFACT_FD_ARGUMENT),
            OsString::from("10"),
            OsString::from(ACCEPTED_FD_ARGUMENT),
            OsString::from("11"),
            OsString::from(POLICY_FD_ARGUMENT),
            OsString::from("12"),
            OsString::from(GRAMMAR_FD_ARGUMENT),
            OsString::from("13"),
        ];
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let serve_arguments = [OsString::from(SERVE_ARGUMENT)];
        let serve = parse_worker_invocation(serve_arguments.into_iter())?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        require(
            serve
                == WorkerInvocation::Serve {
                    descriptors: LinuxAuthorityDescriptors {
                        artifact: 10,
                        accepted: 11,
                        policy: 12,
                        grammar: 13,
                    },
                },
            "serve invocation was not accepted",
        )?;
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        require(
            serve == WorkerInvocation::Serve {},
            "serve invocation was not accepted",
        )?;
        require(
            parse_worker_invocation([OsString::from(VERIFY_BUILD_CONTRACT_ARGUMENT)].into_iter())?
                == WorkerInvocation::VerifyBuildContract,
            "build-contract verification invocation was not accepted",
        )?;
        require(
            parse_worker_invocation(Vec::<OsString>::new().into_iter()).is_err(),
            "empty worker invocation was accepted",
        )?;
        require(
            parse_worker_invocation([OsString::from("ada")].into_iter()).is_err(),
            "grammar argument was accepted as a worker invocation",
        )?;
        require(
            parse_worker_invocation(
                [OsString::from(SERVE_ARGUMENT), OsString::from("repo/path")].into_iter(),
            )
            .is_err(),
            "worker invocation accepted an extra repository path",
        )?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            require(
                parse_worker_invocation(
                    [
                        SERVE_ARGUMENT,
                        ARTIFACT_FD_ARGUMENT,
                        "10",
                        ACCEPTED_FD_ARGUMENT,
                        "10",
                        POLICY_FD_ARGUMENT,
                        "12",
                        GRAMMAR_FD_ARGUMENT,
                        "13",
                    ]
                    .map(OsString::from)
                    .into_iter(),
                )
                .is_err(),
                "worker invocation accepted duplicate authority descriptors",
            )?;
            require(
                parse_worker_invocation(
                    [
                        SERVE_ARGUMENT,
                        ACCEPTED_FD_ARGUMENT,
                        "11",
                        ARTIFACT_FD_ARGUMENT,
                        "10",
                        POLICY_FD_ARGUMENT,
                        "12",
                        GRAMMAR_FD_ARGUMENT,
                        "13",
                    ]
                    .map(OsString::from)
                    .into_iter(),
                )
                .is_err(),
                "worker invocation accepted reordered authority flags",
            )?;
            require(
                parse_worker_invocation(
                    [
                        SERVE_ARGUMENT,
                        ARTIFACT_FD_ARGUMENT,
                        "010",
                        ACCEPTED_FD_ARGUMENT,
                        "11",
                        POLICY_FD_ARGUMENT,
                        "12",
                        GRAMMAR_FD_ARGUMENT,
                        "13",
                    ]
                    .map(OsString::from)
                    .into_iter(),
                )
                .is_err(),
                "worker invocation accepted a noncanonical descriptor identity",
            )?;
        }
        Ok(())
    }

    /// Refuse a grammar when the executable-relative accepted manifest is absent.
    #[test]
    fn grammar_loading_requires_packaged_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let pack = tempfile::tempdir()?;
        let result = approve_grammar_selection(pack.path(), "ada");
        require(
            matches!(result, Err(WorkerStartupError::PackFileRead { .. })),
            "grammar selection succeeded without the packaged manifest",
        )?;
        Ok(())
    }

    /// Approve only exact grammar identities admitted by the validated manifest.
    #[test]
    fn grammar_selection_is_bound_to_validated_manifest() -> Result<(), Box<dyn std::error::Error>>
    {
        let pack = tempfile::tempdir()?;
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("CLI crate is not inside the workspace")?;
        std::fs::copy(
            workspace_root.join("packaging/parser-pack/accepted-capabilities.json"),
            pack.path().join(ACCEPTED_MANIFEST_FILE_NAME),
        )?;

        let approved = approve_grammar_selection(pack.path(), "abl")?;
        require(
            approved.language_id == "abl",
            "approved grammar identity did not match the manifest",
        )?;
        require(
            matches!(
                approve_grammar_selection(pack.path(), "rust"),
                Err(WorkerStartupError::UnknownGrammar(language_id)) if language_id == "rust"
            ),
            "grammar absent from the manifest was accepted",
        )?;
        Ok(())
    }

    /// Validate the fixed header before allocating or reading an oversized payload.
    #[test]
    fn framed_reads_are_fixed_and_bounded_after_header() -> Result<(), Box<dyn std::error::Error>> {
        let encoded = encode_parser_frame(ParserFrameKind::RawSource, b"source")?;
        let mut input = Cursor::new(encoded.clone());
        require(
            read_exact_frame(&mut input, "test")? == Some(encoded),
            "bounded frame read did not preserve the encoded frame",
        )?;

        let oversized = [
            b'P',
            b'A',
            PARSER_PROTOCOL_VERSION,
            ParserFrameKind::RawSource.as_u8(),
            0xff,
            0xff,
            0xff,
            0xff,
        ];
        require(
            matches!(
                read_exact_frame(&mut Cursor::new(oversized), "test"),
                Err(WorkerStartupError::Protocol(
                    ParserProtocolError::FramePayloadTooLarge { .. }
                ))
            ),
            "oversized frame payload was not rejected from its header",
        )?;
        Ok(())
    }

    /// Bind READY to the exact session, artifact bytes, and containment kind.
    #[test]
    fn session_open_emits_exact_artifact_bound_ready() -> Result<(), Box<dyn std::error::Error>> {
        let pack = tempfile::tempdir()?;
        let artifact_bytes = br#"{"schema_version":2}"#;
        std::fs::write(
            pack.path().join(ARTIFACT_MANIFEST_FILE_NAME),
            artifact_bytes,
        )?;
        let session = ParserSessionIdentity::for_entropy(b"worker-session");
        let opening = encode_parser_control(&ParserControl::SessionOpen(
            projectatlas_core::optional_parser_protocol::ParserSessionOpen::new(session.clone()),
        ))?;
        let mut input = Cursor::new(opening);
        let mut output = Vec::new();
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let mut runtime = protocol_test_runtime(pack.path());
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let mut runtime = protocol_test_runtime(pack.path())?;
        let artifact = ParserArtifactIdentity::new(ParserContentDigest::for_bytes(artifact_bytes));
        serve_session(
            &mut runtime,
            &mut input,
            &mut output,
            &artifact,
            ParserContainmentKind::WindowsAppContainerJob,
        )?;

        let frame = ParserFrame::decode_exact(&output)?;
        decode_parser_ready_for_launch(
            frame,
            &session,
            &artifact,
            ParserContainmentKind::WindowsAppContainerJob,
        )?;
        Ok(())
    }

    /// Return a typed identity-bound failure for forged raw source bytes.
    #[test]
    fn forged_raw_source_emits_invalid_source_failure() -> Result<(), Box<dyn std::error::Error>> {
        let pack = tempfile::tempdir()?;
        let session = ParserSessionIdentity::for_entropy(b"worker-session");
        let artifact = ParserArtifactIdentity::for_bytes(b"artifact");
        let request = ParserRequest::new(
            session,
            ParserRequestIdentity::new(1)?,
            artifact,
            ParserLanguageIdentity::new("abl")?,
            ParserSourceIdentity::for_bytes(b"expected")?,
            ParserRequestLimits::new(1024, 1024, 128)?,
        );
        let raw_source = encode_parser_frame(ParserFrameKind::RawSource, b"forged")?;
        let mut input = Cursor::new(raw_source);
        let mut output = Vec::new();
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        let mut runtime = protocol_test_runtime(pack.path());
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let mut runtime = protocol_test_runtime(pack.path())?;

        require(
            !process_request(&mut runtime, &mut input, &mut output, &request)?,
            "forged source was accepted",
        )?;
        let failure =
            decode_parser_failure_for_request(ParserFrame::decode_exact(&output)?, &request)?;
        require(
            failure.code() == ParserFailureCode::InvalidSource,
            "forged source returned the wrong typed failure",
        )?;
        Ok(())
    }

    /// Keep diagnostics bounded without splitting multibyte UTF-8.
    #[test]
    fn startup_diagnostic_is_bounded() {
        let error = WorkerStartupError::InvalidManifest("x".repeat(PARSER_MAX_STDERR_BYTES * 2));
        assert!(bounded_diagnostic(&error).len().saturating_add(1) <= PARSER_MAX_STDERR_BYTES);
    }
}
