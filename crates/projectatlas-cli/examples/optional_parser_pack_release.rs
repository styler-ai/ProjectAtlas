//! Seal, verify, and aggregate release evidence for the optional parser pack.

use projectatlas_cli::optional_parser_lifecycle::{
    OptionalParserPackLifecycleError, TemporaryParserArtifactProfile,
};
use projectatlas_cli::parser_supervisor::{
    OptionalParserSupervisor, admit_optional_parser_artifact, probe_optional_parser_memory_boundary,
};
#[cfg(test)]
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES,
    OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES, ParserPackMemoryControl,
    ParserPackMemoryProbe,
};
use projectatlas_core::optional_parser_pack::{
    OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES, OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES,
    OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES, OPTIONAL_PARSER_PACK_MAX_FILE_BYTES,
    OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES, OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY, OptionalParserPackArtifactManifest,
    OptionalParserPackManifest, OptionalParserPackPlatformProof, OptionalParserPackProofAggregate,
    PackPlatform, PackRelativePath, ParserPackFreshRunner, ParserPackGrammarProbe,
    ParserPackNetworkDenial, ParserPackNetworkIsolation, ParserPackPayloadRole,
    ParserPackVerifiedControl, Sha256Digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::{Command, Stdio};
use std::str;
#[cfg(test)]
use std::thread;
#[cfg(test)]
use std::time::{Duration, Instant};
use tar::{Builder as TarBuilder, EntryType, Header};
use tempfile::NamedTempFile;
use thiserror::Error;

/// Canonical directory enclosing every completed archive payload.
const ARCHIVE_ROOT: &str = "projectatlas-broad-parser";
/// Canonical immutable artifact-manifest filename.
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
/// Canonical accepted logical-manifest filename.
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
/// Canonical retained fixture-corpus filename.
const FIXTURE_CORPUS_FILE_NAME: &str = "optional-parser-pack-corpus.json";
/// Canonical normalized native-audit report filename.
const NATIVE_AUDIT_REPORT_FILE_NAME: &str = "native-audit-report.json";
/// Canonical native-library directory.
const LIB_DIRECTORY_NAME: &str = "lib";
/// Deterministic zstd compression level used on every platform.
const ZSTD_COMPRESSION_LEVEL: i32 = 19;
/// Poll interval for the release tool's isolated child-process tests.
#[cfg(test)]
const CHILD_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Maximum JSON bytes accepted for one fresh-runner context.
const MAX_RUNNER_CONTEXT_BYTES: u64 = 16 * 1024;
/// Maximum JSON bytes accepted for one platform proof.
const MAX_PLATFORM_PROOF_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum imported symbols admitted by one normalized audit row.
const MAX_IMPORTED_SYMBOLS_PER_NATIVE_BINARY: usize = 65_536;
/// Maximum direct native dependencies admitted by one normalized binary audit row.
const MAX_NATIVE_DEPENDENCIES_PER_NATIVE_BINARY: usize = 64;
/// Maximum exported symbols admitted by one worker audit row.
const MAX_EXPORTS_PER_WORKER: usize = 1_024;
/// Maximum named definitions admitted when worker definition evidence is available.
const MAX_DEFINED_SYMBOLS_PER_WORKER: usize = 262_144;
/// Tar header and terminator allowance beyond admitted file payload bytes.
const TAR_FRAMING_ALLOWANCE_BYTES: u64 = 1024 * 1024;
/// Regular payload mode.
const PAYLOAD_MODE: u32 = 0o644;
/// Executable worker mode.
const WORKER_MODE: u32 = 0o755;

/// Outer release-tool result boundary.
type ToolResult<T> = Result<T, Box<dyn Error>>;

/// A release verification failed and mandatory temporary-profile cleanup also failed.
#[derive(Debug, Error)]
#[error(
    "optional parser-pack release verification failed and temporary profile cleanup also failed: operation: {operation}; cleanup: {cleanup}"
)]
struct ReleaseOperationAndCleanupError {
    /// Original release verification failure.
    #[source]
    operation: Box<dyn Error>,
    /// Typed artifact-profile cleanup failure.
    cleanup: OptionalParserPackLifecycleError,
}

/// Preserve both release verification and temporary-profile cleanup failures.
fn finish_release_cleanup<T>(
    operation: ToolResult<T>,
    cleanup: Result<(), OptionalParserPackLifecycleError>,
) -> ToolResult<T> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(operation), Ok(())) => Err(operation),
        (Ok(_), Err(cleanup)) => Err(Box::new(cleanup)),
        (Err(operation), Err(cleanup)) => Err(Box::new(ReleaseOperationAndCleanupError {
            operation,
            cleanup,
        })),
    }
}

/// Closed release operation selected at the process boundary.
enum ReleaseCommand {
    /// Create one deterministic completed archive from a validated staging directory.
    Create {
        /// Staged pack payload directory.
        staged_directory: PathBuf,
        /// New archive path.
        archive: PathBuf,
    },
    /// Verify one completed archive on a fresh runner and write its platform proof.
    Verify {
        /// Completed platform archive.
        archive: PathBuf,
        /// Externally attested fresh-runner controls.
        runner_context: PathBuf,
        /// New platform-proof path.
        proof: PathBuf,
    },
    /// Aggregate the complete validated platform proof set from one candidate.
    Aggregate {
        /// Accepted logical manifest shared by all platform proofs.
        accepted_manifest: PathBuf,
        /// Complete accepted platform-proof path set.
        platform_proofs: Vec<PathBuf>,
        /// New aggregate-proof path.
        aggregate: PathBuf,
    },
}

/// Strict fresh-runner context supplied by the isolated verification job.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FreshRunnerWire {
    /// Verification ran in a fresh host job or machine image.
    fresh_host: ParserPackVerifiedControl,
    /// Repository source and build output were unavailable to verification.
    repository_inputs_absent: ParserPackVerifiedControl,
    /// Verification invoked neither Cargo nor a compiler.
    build_tools_not_invoked: ParserPackVerifiedControl,
    /// Verification current directory was outside the extracted pack.
    working_directory_outside_pack: ParserPackVerifiedControl,
    /// Ambient dynamic-library search paths were cleared.
    ambient_library_paths_cleared: ParserPackVerifiedControl,
    /// Exact physical egress-denial outcome.
    network_denial: NetworkDenialWire,
}

/// Strict nested network-denial evidence.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkDenialWire {
    /// Platform-specific physical containment mechanism.
    mechanism: ParserPackNetworkIsolation,
    /// DNS resolution or query attempt was denied.
    dns_denied: bool,
    /// Direct TCP connection attempt was denied.
    direct_tcp_denied: bool,
    /// HTTPS connection attempt was denied.
    https_denied: bool,
}

impl From<FreshRunnerWire> for ParserPackFreshRunner {
    fn from(wire: FreshRunnerWire) -> Self {
        Self {
            fresh_host: wire.fresh_host,
            repository_inputs_absent: wire.repository_inputs_absent,
            build_tools_not_invoked: wire.build_tools_not_invoked,
            working_directory_outside_pack: wire.working_directory_outside_pack,
            ambient_library_paths_cleared: wire.ambient_library_paths_cleared,
            network_denial: ParserPackNetworkDenial {
                mechanism: wire.network_denial.mechanism,
                dns_denied: wire.network_denial.dns_denied,
                direct_tcp_denied: wire.network_denial.direct_tcp_denied,
                https_denied: wire.network_denial.https_denied,
            },
        }
    }
}

/// Exact file facts retained in one normalized native-audit row.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuditedFileWire {
    /// Artifact-relative path.
    path: String,
    /// Exact file digest.
    sha256: String,
    /// Exact file bytes.
    byte_length: u64,
}

/// One strict normalized native-audit row packaged by construction.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeAuditRowWire {
    /// Accepted logical language identity.
    language_id: String,
    /// Exact packaged library facts.
    file: AuditedFileWire,
    /// Required constructor export.
    export_symbol: String,
    /// Expected Tree-sitter ABI.
    expected_abi: u32,
    /// Observed native binary format.
    binary_format: String,
    /// Observed native architecture.
    architecture: String,
    /// Observed direct native dependencies.
    native_libraries: Vec<String>,
    /// Number of distinct normalized imported symbols.
    imported_symbol_count: usize,
    /// Digest of the normalized imported-symbol set.
    imported_symbols_sha256: String,
}

/// Strict versioned native-audit report packaged with one platform artifact.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeAuditReportWire {
    /// Exact report schema understood by this verifier.
    schema_version: u32,
    /// Native evidence for the exact packaged parser worker.
    worker: WorkerAuditWire,
    /// Required nullable platform-containment broker evidence.
    containment_broker: ContainmentBrokerAuditPresence,
    /// Accepted grammar audit rows in logical manifest order.
    grammars: Vec<NativeAuditRowWire>,
}

/// Required nullable wrapper that distinguishes a missing field from an explicit absence.
#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct ContainmentBrokerAuditPresence(Option<ContainmentBrokerAuditWire>);

/// Strict normalized native evidence for the artifact-bound containment broker.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContainmentBrokerAuditWire {
    /// Exact packaged broker file facts.
    file: AuditedFileWire,
    /// External runtime family required to start the broker.
    runtime_family: String,
    /// Observed native binary format.
    binary_format: String,
    /// Observed native architecture.
    architecture: String,
    /// Observed native object kind.
    object_kind: String,
    /// Canonical native PE entry point, which is zero for the managed broker.
    entry_point: String,
    /// RVA of the validated CLR runtime header.
    clr_runtime_header_rva: u32,
    /// Exact byte length of the validated CLR runtime header.
    clr_runtime_header_size: u32,
    /// Sorted normalized PE-loader dependencies.
    pe_loader_libraries: Vec<String>,
    /// Number of distinct normalized PE imported symbols.
    pe_imported_symbol_count: usize,
    /// Digest of the normalized PE imported-symbol set.
    pe_imported_symbols_sha256: String,
    /// Sorted normalized managed P/Invoke module set.
    managed_modules: Vec<String>,
    /// Number of compiled managed P/Invoke method imports.
    managed_import_count: usize,
    /// Digest of sorted normalized managed P/Invoke method imports.
    managed_imports_sha256: String,
    /// Number of normalized native exports.
    export_count: usize,
    /// Digest of the sorted normalized export sequence.
    exports_sha256: String,
}

/// Strict normalized native evidence for the packaged parser worker.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerAuditWire {
    /// Exact packaged worker file facts.
    file: AuditedFileWire,
    /// Observed native binary format.
    binary_format: String,
    /// Observed native architecture.
    architecture: String,
    /// Observed native object kind.
    object_kind: String,
    /// Canonical non-zero native entry point.
    entry_point: String,
    /// Sorted normalized direct native dependencies.
    native_libraries: Vec<String>,
    /// Number of distinct normalized imported symbols.
    imported_symbol_count: usize,
    /// Digest of the normalized imported-symbol set.
    imported_symbols_sha256: String,
    /// Number of normalized native exports.
    export_count: usize,
    /// Digest of the sorted normalized export sequence.
    exports_sha256: String,
    /// Whether a named-definition table was available to the constructor.
    defined_symbol_evidence_available: bool,
    /// Exact named-definition count when evidence was available.
    defined_symbol_count: Option<usize>,
    /// Digest of sorted normalized definitions when evidence was available.
    defined_symbols_sha256: Option<String>,
}

/// Exact digest and size observed for one extracted archive file.
struct ObservedFile {
    /// Exact file bytes.
    bytes: u64,
    /// SHA-256 of the exact file bytes.
    sha256: Sha256Digest,
}

/// Validated extracted archive retained until worker probing completes.
struct ExtractedArchive {
    /// Temporary extraction directory owning every extracted file.
    _directory: tempfile::TempDir,
    /// Canonical extracted pack root.
    pack_root: PathBuf,
    /// Exact expanded file payload bytes, including the artifact manifest.
    expanded_bytes: u64,
    /// Exact observed files keyed by canonical artifact-relative path.
    observed: BTreeMap<String, ObservedFile>,
}

/// Reader that rejects decompression beyond one fixed bound.
struct BoundedReader<R> {
    /// Wrapped decompressor.
    inner: R,
    /// Maximum bytes the reader may return.
    maximum: u64,
    /// Bytes returned so far.
    consumed: u64,
}

/// Writer that rejects a completed archive as soon as compressed bytes exceed the ceiling.
struct BoundedWriter<W> {
    /// Wrapped archive destination.
    inner: W,
    /// Maximum bytes the writer may accept.
    maximum: u64,
    /// Bytes written so far.
    written: u64,
}

impl<W> BoundedWriter<W> {
    /// Wrap a writer with a hard accepted-byte ceiling.
    const fn new(inner: W, maximum: u64) -> Self {
        Self {
            inner,
            maximum,
            written: 0,
        }
    }
}

impl<R> BoundedReader<R> {
    /// Wrap a reader with a hard returned-byte ceiling.
    const fn new(inner: R, maximum: u64) -> Self {
        Self {
            inner,
            maximum,
            consumed: 0,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.consumed >= self.maximum {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expanded archive exceeded its hard byte ceiling",
                )),
            };
        }
        let remaining = self.maximum.saturating_sub(self.consumed);
        let allowed = usize::try_from(remaining.min(buffer.len() as u64)).map_err(|source| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("archive read bound cannot be represented: {source}"),
            )
        })?;
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.consumed = self
            .consumed
            .checked_add(u64::try_from(read).map_err(|source| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("archive read count cannot be represented: {source}"),
                )
            })?)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "archive read count overflowed")
            })?;
        Ok(read)
    }
}

impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.written >= self.maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "compressed archive exceeded its hard byte ceiling",
            ));
        }
        let remaining = self.maximum.saturating_sub(self.written);
        let allowed = usize::try_from(remaining.min(buffer.len() as u64)).map_err(|source| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("archive write bound cannot be represented: {source}"),
            )
        })?;
        let written = self.inner.write(&buffer[..allowed])?;
        self.written = self
            .written
            .checked_add(u64::try_from(written).map_err(|source| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("archive write count cannot be represented: {source}"),
                )
            })?)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "archive write count overflowed")
            })?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Parse and execute exactly one closed release operation.
fn main() -> ToolResult<()> {
    match parse_command(env::args_os().skip(1))? {
        ReleaseCommand::Create {
            staged_directory,
            archive,
        } => create_archive(&staged_directory, &archive),
        ReleaseCommand::Verify {
            archive,
            runner_context,
            proof,
        } => verify_archive(&archive, &runner_context, &proof),
        ReleaseCommand::Aggregate {
            accepted_manifest,
            platform_proofs,
            aggregate,
        } => aggregate_proofs(&accepted_manifest, &platform_proofs, &aggregate),
    }
}

/// Parse one exact closed command shape without accepting optional behavior.
fn parse_command(arguments: impl Iterator<Item = OsString>) -> ToolResult<ReleaseCommand> {
    let arguments = arguments.collect::<Vec<_>>();
    let Some(command) = arguments.first().and_then(|value| value.to_str()) else {
        return Err(invalid(
            "usage: optional_parser_pack_release <create|verify|aggregate> ...",
        ));
    };
    match command {
        "create" if arguments.len() == 3 => Ok(ReleaseCommand::Create {
            staged_directory: PathBuf::from(&arguments[1]),
            archive: PathBuf::from(&arguments[2]),
        }),
        "verify" if arguments.len() == 4 => Ok(ReleaseCommand::Verify {
            archive: PathBuf::from(&arguments[1]),
            runner_context: PathBuf::from(&arguments[2]),
            proof: PathBuf::from(&arguments[3]),
        }),
        "aggregate" if arguments.len() == PackPlatform::ALL.len() + 3 => {
            let aggregate = arguments
                .last()
                .ok_or_else(|| invalid("aggregate output path is missing"))?;
            Ok(ReleaseCommand::Aggregate {
                accepted_manifest: PathBuf::from(&arguments[1]),
                platform_proofs: arguments[2..arguments.len() - 1]
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
                aggregate: PathBuf::from(aggregate),
            })
        }
        _ => Err(invalid(
            "create requires <staged-directory> <new-archive>; verify requires <archive> \
             <runner-context-json> <new-platform-proof>; aggregate requires \
             <accepted-manifest> <platform-proofs...> <new-aggregate-proof>",
        )),
    }
}

/// Create one deterministic canonical archive from an exact staged payload.
fn create_archive(staged_directory: &Path, archive: &Path) -> ToolResult<()> {
    require_directory(staged_directory, "staged artifact directory")?;
    let (logical, artifact, observed) = validate_staged_artifact(staged_directory)?;
    artifact.validate(&logical)?;
    validate_native_audit_report(staged_directory, &logical, &artifact, &observed)?;
    require_archive_name(archive, artifact.platform)?;

    let mut paths = artifact
        .files
        .iter()
        .map(|file| file.path.as_str().to_owned())
        .collect::<Vec<_>>();
    paths.push(ARTIFACT_MANIFEST_FILE_NAME.to_owned());
    paths.sort();
    write_deterministic_archive(staged_directory, &paths, archive, artifact.platform)
}

/// Verify one completed archive and seal its fresh-runner platform proof.
fn verify_archive(archive: &Path, runner_context: &Path, proof: &Path) -> ToolResult<()> {
    require_new_output(proof, "platform proof")?;
    let (archive_sha256, archive_bytes) =
        sha256_file(archive, OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES)?;
    let archive_name = archive
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| invalid("archive path has no UTF-8 basename"))?
        .to_owned();
    let extracted = extract_archive(archive)?;
    let (revalidated_sha256, revalidated_bytes) =
        sha256_file(archive, OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES)?;
    if revalidated_sha256 != archive_sha256 || revalidated_bytes != archive_bytes {
        return Err(invalid("completed archive changed during verification"));
    }
    let accepted_path = extracted.pack_root.join(ACCEPTED_MANIFEST_FILE_NAME);
    let accepted_bytes = read_bounded_file(
        &accepted_path,
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
    )?;
    let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
    let artifact_path = extracted.pack_root.join(ARTIFACT_MANIFEST_FILE_NAME);
    let artifact_bytes = read_bounded_file(
        &artifact_path,
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
    )?;
    let artifact: OptionalParserPackArtifactManifest = serde_json::from_slice(&artifact_bytes)?;
    artifact.validate(&logical)?;
    require_archive_name(archive, artifact.platform)?;
    if artifact.platform != current_platform()? {
        return Err(invalid(format!(
            "archive target {} does not match this fresh runner",
            artifact.platform.as_str()
        )));
    }
    validate_extracted_inventory(&extracted, &artifact)?;
    validate_native_audit_report(
        &extracted.pack_root,
        &logical,
        &artifact,
        &extracted.observed,
    )?;

    let runner_wire: FreshRunnerWire = serde_json::from_slice(&read_bounded_file(
        runner_context,
        MAX_RUNNER_CONTEXT_BYTES,
    )?)?;
    let runner = ParserPackFreshRunner::from(runner_wire);
    let platform = artifact.platform;
    // Reject a dirty or incompletely isolated runner before executing packaged code.
    runner.validate(platform)?;
    let supervisor = OptionalParserSupervisor::open(&extracted.pack_root)?;
    let temporary_profile = TemporaryParserArtifactProfile::for_verified_supervisor(&supervisor);
    let verification = (|| -> ToolResult<OptionalParserPackPlatformProof> {
        admit_optional_parser_artifact(supervisor, &logical)?;
        let memory = probe_optional_parser_memory_boundary(&extracted.pack_root, &logical)?;
        let grammars = logical
            .grammars()
            .iter()
            .map(|grammar| ParserPackGrammarProbe {
                language_id: grammar.language_id.clone(),
                worker_probe_passed: true,
            })
            .collect();

        let artifact_observed = require_observed(&extracted.observed, ARTIFACT_MANIFEST_FILE_NAME)?;
        let accepted_observed = require_observed(&extracted.observed, ACCEPTED_MANIFEST_FILE_NAME)?;
        let fixture_observed = require_observed(&extracted.observed, FIXTURE_CORPUS_FILE_NAME)?;
        let audit_observed = require_observed(&extracted.observed, NATIVE_AUDIT_REPORT_FILE_NAME)?;
        let platform_proof = OptionalParserPackPlatformProof {
            schema_version: OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION,
            pack_id: logical.pack_id().to_owned(),
            platform,
            candidate: artifact.candidate,
            archive_name,
            archive_sha256,
            archive_bytes,
            expanded_bytes: extracted.expanded_bytes,
            artifact_manifest_sha256: artifact_observed.sha256.clone(),
            accepted_manifest_sha256: accepted_observed.sha256.clone(),
            capability_set_digest: logical.capability_set_digest().clone(),
            fixture_corpus_sha256: fixture_observed.sha256.clone(),
            native_audit_report_sha256: audit_observed.sha256.clone(),
            runner,
            grammars,
            memory,
        };
        // Bind every successful exact-host probe before mandatory cleanup.
        platform_proof.validate(&logical)?;
        Ok(platform_proof)
    })();
    let platform_proof = finish_release_cleanup(verification, temporary_profile.cleanup())?;
    // Publish no proof unless mandatory temporary-profile cleanup also passed.
    write_new_json(proof, &platform_proof)
}

/// Aggregate the complete platform proof set after independent logical validation.
fn aggregate_proofs(
    accepted_manifest: &Path,
    platform_proofs: &[PathBuf],
    output: &Path,
) -> ToolResult<()> {
    require_new_output(output, "aggregate proof")?;
    if platform_proofs.len() != PackPlatform::ALL.len() {
        return Err(invalid(
            "the complete optional-pack platform proof set is required",
        ));
    }
    let accepted_bytes = read_bounded_file(
        accepted_manifest,
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
    )?;
    let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
    let accepted_sha256 = Sha256Digest::new(sha256_bytes(&accepted_bytes))?;
    let mut proofs = platform_proofs
        .iter()
        .map(|path| {
            let bytes = read_bounded_file(path, MAX_PLATFORM_PROOF_BYTES)?;
            let proof: OptionalParserPackPlatformProof = serde_json::from_slice(&bytes)?;
            proof.validate(&logical)?;
            if proof.accepted_manifest_sha256 != accepted_sha256 {
                return Err(invalid(format!(
                    "platform proof {} does not bind the supplied accepted manifest",
                    proof.platform.as_str()
                )));
            }
            Ok(proof)
        })
        .collect::<ToolResult<Vec<_>>>()?;
    proofs.sort_by_key(|proof| platform_ordinal(proof.platform));
    let first = proofs
        .first()
        .ok_or_else(|| invalid("platform proof set is empty"))?;
    let aggregate = OptionalParserPackProofAggregate {
        schema_version: OPTIONAL_PARSER_PACK_PROOF_AGGREGATE_SCHEMA_VERSION,
        pack_id: logical.pack_id().to_owned(),
        projectatlas_version: logical.runtime().projectatlas_version.clone(),
        accepted_manifest_sha256: accepted_sha256,
        capability_set_digest: logical.capability_set_digest().clone(),
        fixture_corpus_sha256: first.fixture_corpus_sha256.clone(),
        platforms: proofs,
    };
    aggregate.validate(&logical)?;
    write_new_json(output, &aggregate)
}

/// Validate an exact staged artifact and return its observed inventory.
fn validate_staged_artifact(
    staged_directory: &Path,
) -> ToolResult<(
    OptionalParserPackManifest,
    OptionalParserPackArtifactManifest,
    BTreeMap<String, ObservedFile>,
)> {
    let accepted_bytes = read_bounded_file(
        &staged_directory.join(ACCEPTED_MANIFEST_FILE_NAME),
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
    )?;
    let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
    let artifact_bytes = read_bounded_file(
        &staged_directory.join(ARTIFACT_MANIFEST_FILE_NAME),
        u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
    )?;
    let artifact: OptionalParserPackArtifactManifest = serde_json::from_slice(&artifact_bytes)?;
    artifact.validate(&logical)?;
    let observed = enumerate_staged_files(staged_directory)?;
    validate_observed_inventory(&observed, &artifact)?;
    Ok((logical, artifact, observed))
}

/// Enumerate only the fixed root files and flat native-library directory.
fn enumerate_staged_files(root: &Path) -> ToolResult<BTreeMap<String, ObservedFile>> {
    let mut observed = BTreeMap::new();
    let mut root_entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    root_entries.sort_by_key(fs::DirEntry::file_name);
    for entry in root_entries {
        let name = entry.file_name().into_string().map_err(|name| {
            invalid(format!(
                "staged artifact contains a non-UTF-8 root name {}",
                name.display()
            ))
        })?;
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            insert_observed_file(&mut observed, &entry.path(), &name)?;
        } else if file_type.is_dir() && name == LIB_DIRECTORY_NAME {
            let mut libraries = fs::read_dir(entry.path())?.collect::<Result<Vec<_>, _>>()?;
            libraries.sort_by_key(fs::DirEntry::file_name);
            for library in libraries {
                if !library.file_type()?.is_file() {
                    return Err(invalid(
                        "native-library directory contains a non-regular entry",
                    ));
                }
                let library_name = library.file_name().into_string().map_err(|name| {
                    invalid(format!(
                        "native library has a non-UTF-8 name {}",
                        name.display()
                    ))
                })?;
                insert_observed_file(
                    &mut observed,
                    &library.path(),
                    &format!("{LIB_DIRECTORY_NAME}/{library_name}"),
                )?;
            }
        } else {
            return Err(invalid(
                "staged artifact contains an unexpected directory or non-regular entry",
            ));
        }
        if observed.len() > OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES.saturating_add(1) {
            return Err(invalid("staged artifact exceeded its file-entry ceiling"));
        }
    }
    Ok(observed)
}

/// Hash and insert one exact regular staged file.
fn insert_observed_file(
    observed: &mut BTreeMap<String, ObservedFile>,
    path: &Path,
    relative: &str,
) -> ToolResult<()> {
    let relative = PackRelativePath::new(relative)?;
    let (sha256, bytes) = sha256_file(path, OPTIONAL_PARSER_PACK_MAX_FILE_BYTES)?;
    let key = relative.as_str().to_owned();
    if observed
        .insert(key.clone(), ObservedFile { bytes, sha256 })
        .is_some()
    {
        return Err(invalid(format!("duplicate staged payload path {key:?}")));
    }
    Ok(())
}

/// Require exact manifest-listed files plus the one self-excluded artifact manifest.
fn validate_observed_inventory(
    observed: &BTreeMap<String, ObservedFile>,
    artifact: &OptionalParserPackArtifactManifest,
) -> ToolResult<()> {
    let expected_count = artifact
        .files
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("expected artifact file count overflowed"))?;
    if observed.len() != expected_count {
        return Err(invalid(format!(
            "artifact contains {} files; expected {expected_count}",
            observed.len()
        )));
    }
    for file in &artifact.files {
        let actual = require_observed(observed, file.path.as_str())?;
        if actual.bytes != file.bytes || actual.sha256 != file.sha256 {
            return Err(invalid(format!(
                "payload {} differs from its artifact manifest",
                file.path.as_str()
            )));
        }
    }
    require_observed(observed, ARTIFACT_MANIFEST_FILE_NAME)?;
    Ok(())
}

/// Validate exact extracted files and the manifest-listed payload bindings.
fn validate_extracted_inventory(
    extracted: &ExtractedArchive,
    artifact: &OptionalParserPackArtifactManifest,
) -> ToolResult<()> {
    validate_observed_inventory(&extracted.observed, artifact)
}

/// Parse and cross-check the normalized packaged native-audit report.
fn validate_native_audit_report(
    pack_root: &Path,
    logical: &OptionalParserPackManifest,
    artifact: &OptionalParserPackArtifactManifest,
    observed: &BTreeMap<String, ObservedFile>,
) -> ToolResult<()> {
    let report_observed = require_observed(observed, NATIVE_AUDIT_REPORT_FILE_NAME)?;
    if report_observed.sha256 != artifact.native_audit.report_sha256 {
        return Err(invalid(
            "native-audit report digest differs from the artifact audit summary",
        ));
    }
    let bytes = read_bounded_file(
        &pack_root.join(NATIVE_AUDIT_REPORT_FILE_NAME),
        OPTIONAL_PARSER_PACK_MAX_FILE_BYTES,
    )?;
    let report: NativeAuditReportWire = serde_json::from_slice(&bytes)?;
    if report.schema_version != OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION {
        return Err(invalid(format!(
            "native-audit report schema is {}; expected {OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION}",
            report.schema_version
        )));
    }
    validate_worker_audit(&report.worker, artifact)?;
    validate_containment_broker_audit(&report.containment_broker, artifact)?;
    if report.grammars.len() != logical.grammars().len() {
        return Err(invalid(
            "native-audit report does not cover every accepted grammar exactly once",
        ));
    }
    let grammar_files = artifact
        .files
        .iter()
        .filter_map(|file| match &file.role {
            ParserPackPayloadRole::GrammarLibrary { language_id } => {
                Some((language_id.as_str(), file))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    for (row, grammar) in report.grammars.iter().zip(logical.grammars()) {
        if row.language_id != grammar.language_id
            || row.export_symbol != grammar.abi_export.export_symbol.as_str()
            || row.expected_abi != grammar.abi_export.expected_abi
        {
            return Err(invalid(format!(
                "native-audit row {:?} differs from its accepted grammar",
                row.language_id
            )));
        }
        let file = grammar_files
            .get(row.language_id.as_str())
            .ok_or_else(|| invalid("native-audit row has no manifest-listed library"))?;
        let row_path = PackRelativePath::new(&row.file.path)?;
        let row_sha256 = Sha256Digest::new(&row.file.sha256)?;
        if row_path != file.path || row.file.byte_length != file.bytes || row_sha256 != file.sha256
        {
            return Err(invalid(format!(
                "native-audit file facts drifted for {:?}",
                row.language_id
            )));
        }
        validate_audit_vocabulary(row, artifact.platform)?;
    }
    Ok(())
}

/// Cross-check conditional broker evidence against the target and exact payload manifest.
fn validate_containment_broker_audit(
    presence: &ContainmentBrokerAuditPresence,
    artifact: &OptionalParserPackArtifactManifest,
) -> ToolResult<()> {
    let manifest_broker = artifact
        .files
        .iter()
        .find(|file| matches!(file.role, ParserPackPayloadRole::ContainmentBroker));
    match (artifact.platform, presence.0.as_ref(), manifest_broker) {
        (PackPlatform::LinuxX86_64, None, None) => Ok(()),
        (PackPlatform::WindowsX86_64, Some(broker), Some(manifest_broker)) => {
            let broker_path = PackRelativePath::new(&broker.file.path)?;
            let broker_sha256 = Sha256Digest::new(&broker.file.sha256)?;
            if broker_path != manifest_broker.path
                || broker.file.byte_length != manifest_broker.bytes
                || broker_sha256 != manifest_broker.sha256
            {
                return Err(invalid(
                    "native-audit containment-broker facts differ from the artifact manifest",
                ));
            }
            let (expected_format, expected_architecture) =
                audit_target_vocabulary(artifact.platform);
            if broker.runtime_family != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY
                || broker.binary_format != expected_format
                || broker.architecture != expected_architecture
                || broker.object_kind != "executable"
                || broker.entry_point != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT
                || broker.clr_runtime_header_rva == 0
                || broker.clr_runtime_header_size
                    != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE
            {
                return Err(invalid(
                    "native-audit containment-broker runtime or target identity is invalid",
                ));
            }
            let empty_surface_sha256 = sha256_bytes(&[]);
            if broker.pe_loader_libraries.len() > MAX_NATIVE_DEPENDENCIES_PER_NATIVE_BINARY
                || !strictly_sorted_unique(&broker.pe_loader_libraries)
                || broker.pe_imported_symbol_count != 0
                || broker.pe_imported_symbols_sha256 != empty_surface_sha256.as_str()
                || broker.managed_modules.is_empty()
                || broker.managed_modules.len() > MAX_NATIVE_DEPENDENCIES_PER_NATIVE_BINARY
                || !strictly_sorted_unique(&broker.managed_modules)
                || broker.managed_import_count == 0
                || broker.managed_import_count > MAX_IMPORTED_SYMBOLS_PER_NATIVE_BINARY
                || broker.export_count != 0
                || broker.exports_sha256 != empty_surface_sha256.as_str()
                || broker.pe_loader_libraries
                    != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES
                || broker.managed_modules != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES
            {
                return Err(invalid(
                    "native-audit containment-broker collections or counts exceeded their closed bounds",
                ));
            }
            Sha256Digest::new(&broker.pe_imported_symbols_sha256)?;
            Sha256Digest::new(&broker.managed_imports_sha256)?;
            Sha256Digest::new(&broker.exports_sha256)?;
            Ok(())
        }
        (PackPlatform::LinuxX86_64 | PackPlatform::WindowsX86_64, _, _) => Err(invalid(
            "native-audit containment-broker presence does not match the platform artifact",
        )),
    }
}

/// Cross-check strict worker evidence against the exact manifest-listed worker payload.
fn validate_worker_audit(
    worker: &WorkerAuditWire,
    artifact: &OptionalParserPackArtifactManifest,
) -> ToolResult<()> {
    let manifest_worker = artifact
        .files
        .iter()
        .find(|file| matches!(file.role, ParserPackPayloadRole::Worker))
        .ok_or_else(|| invalid("artifact manifest has no parser worker"))?;
    let worker_path = PackRelativePath::new(&worker.file.path)?;
    let worker_sha256 = Sha256Digest::new(&worker.file.sha256)?;
    if worker_path != manifest_worker.path
        || worker.file.byte_length != manifest_worker.bytes
        || worker_sha256 != manifest_worker.sha256
    {
        return Err(invalid(
            "native-audit worker file facts differ from the artifact manifest",
        ));
    }
    let (expected_format, expected_architecture) = audit_target_vocabulary(artifact.platform);
    if worker.binary_format != expected_format
        || worker.architecture != expected_architecture
        || !accepted_worker_object_kind(artifact.platform, &worker.object_kind)
        || !canonical_nonzero_entry_point(&worker.entry_point)
    {
        return Err(invalid(
            "native-audit worker target, object kind, or entry point is invalid",
        ));
    }
    if worker.native_libraries.len() > MAX_NATIVE_DEPENDENCIES_PER_NATIVE_BINARY
        || !strictly_sorted_unique(&worker.native_libraries)
        || worker.imported_symbol_count > MAX_IMPORTED_SYMBOLS_PER_NATIVE_BINARY
        || worker.export_count > MAX_EXPORTS_PER_WORKER
    {
        return Err(invalid(
            "native-audit worker collections or counts exceeded their closed bounds",
        ));
    }
    Sha256Digest::new(&worker.imported_symbols_sha256)?;
    Sha256Digest::new(&worker.exports_sha256)?;
    validate_defined_symbol_evidence(worker)
}

/// Require definition evidence fields to agree without treating unavailable evidence as proof.
fn validate_defined_symbol_evidence(worker: &WorkerAuditWire) -> ToolResult<()> {
    match (
        worker.defined_symbol_evidence_available,
        worker.defined_symbol_count,
        worker.defined_symbols_sha256.as_deref(),
    ) {
        (false, None, None) => Ok(()),
        (true, Some(count), Some(digest)) if count <= MAX_DEFINED_SYMBOLS_PER_WORKER => {
            Sha256Digest::new(digest)?;
            Ok(())
        }
        _ => Err(invalid(
            "native-audit worker definition evidence has inconsistent availability, count, or digest",
        )),
    }
}

/// Return whether one worker object kind is executable for its native target.
fn accepted_worker_object_kind(platform: PackPlatform, object_kind: &str) -> bool {
    match platform {
        PackPlatform::LinuxX86_64 => matches!(object_kind, "executable" | "dynamic"),
        PackPlatform::WindowsX86_64 => matches!(object_kind, "executable"),
    }
}

/// Return whether an entry point is canonical `0x` plus 16 lowercase hex digits and non-zero.
fn canonical_nonzero_entry_point(entry_point: &str) -> bool {
    let Some(hexadecimal) = entry_point.strip_prefix("0x") else {
        return false;
    };
    hexadecimal.len() == 16
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && u64::from_str_radix(hexadecimal, 16).is_ok_and(|value| value != 0)
}

/// Validate bounded normalized vocabulary not otherwise represented by core types.
fn validate_audit_vocabulary(row: &NativeAuditRowWire, platform: PackPlatform) -> ToolResult<()> {
    let (expected_format, expected_architecture) = audit_target_vocabulary(platform);
    if row.binary_format != expected_format || row.architecture != expected_architecture {
        return Err(invalid(format!(
            "native-audit target vocabulary drifted for {:?}",
            row.language_id
        )));
    }
    if row.imported_symbol_count > MAX_IMPORTED_SYMBOLS_PER_NATIVE_BINARY {
        return Err(invalid(
            "native-audit imported-symbol count exceeded its bound",
        ));
    }
    Sha256Digest::new(&row.imported_symbols_sha256)?;
    if row.native_libraries.len() > MAX_NATIVE_DEPENDENCIES_PER_NATIVE_BINARY
        || !strictly_sorted_unique(&row.native_libraries)
    {
        return Err(invalid(
            "native-audit dependencies exceeded their bound or are not sorted and unique",
        ));
    }
    Ok(())
}

/// Return the normalized native-audit format and architecture for one target.
const fn audit_target_vocabulary(platform: PackPlatform) -> (&'static str, &'static str) {
    match platform {
        PackPlatform::LinuxX86_64 => ("elf", "x86_64"),
        PackPlatform::WindowsX86_64 => ("pe", "x86_64"),
    }
}

/// Write one canonical tar.zst archive with no host-owned metadata.
fn write_deterministic_archive(
    staged_directory: &Path,
    relative_paths: &[String],
    output: &Path,
    platform: PackPlatform,
) -> ToolResult<()> {
    if output.exists() {
        return Err(invalid(format!(
            "refusing to overwrite archive {}",
            output.display()
        )));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    require_directory(parent, "archive parent directory")?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    let compressed_bytes = {
        let bounded = BoundedWriter::new(
            temporary.as_file_mut(),
            OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES,
        );
        let mut encoder = zstd::Encoder::new(bounded, ZSTD_COMPRESSION_LEVEL)?;
        encoder.include_checksum(true)?;
        let mut archive = TarBuilder::new(encoder);
        for relative in relative_paths {
            let relative = PackRelativePath::new(relative)?;
            let source = staged_directory.join(Path::new(relative.as_str()));
            let metadata = fs::symlink_metadata(&source)?;
            if !metadata.file_type().is_file()
                || metadata.len() > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES
            {
                return Err(invalid(format!(
                    "archive input {} is not a bounded regular file",
                    source.display()
                )));
            }
            let archive_path = format!("{ARCHIVE_ROOT}/{}", relative.as_str());
            let mut header = Header::new_ustar();
            header.set_entry_type(EntryType::Regular);
            header.set_size(metadata.len());
            header.set_mode(if relative.as_str() == platform.worker_file_name() {
                WORKER_MODE
            } else {
                PAYLOAD_MODE
            });
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            archive.append_data(
                &mut header,
                archive_path,
                BufReader::new(File::open(source)?),
            )?;
        }
        archive.finish()?;
        let encoder = archive.into_inner()?;
        encoder.finish()?.written
    };
    temporary.as_file().sync_all()?;
    let bytes = temporary.as_file().metadata()?.len();
    if bytes == 0 || bytes != compressed_bytes || bytes > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES {
        return Err(invalid(format!(
            "completed archive is {bytes} bytes; maximum is {OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES}"
        )));
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| Box::<dyn Error>::from(error.error))?;
    Ok(())
}

/// Stream, validate, and extract one canonical completed archive.
fn extract_archive(path: &Path) -> ToolResult<ExtractedArchive> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > OPTIONAL_PARSER_PACK_MAX_ARCHIVE_BYTES
    {
        return Err(invalid("archive is not a bounded non-empty regular file"));
    }
    let directory = tempfile::tempdir()?;
    let pack_root = directory.path().join(ARCHIVE_ROOT);
    fs::create_dir(&pack_root)?;
    let decoder = zstd::Decoder::new(BufReader::new(File::open(path)?))?;
    let maximum_tar_bytes = OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES
        .checked_add(TAR_FRAMING_ALLOWANCE_BYTES)
        .ok_or_else(|| invalid("tar expansion bound overflowed"))?;
    let bounded = BoundedReader::new(decoder, maximum_tar_bytes);
    let mut archive = tar::Archive::new(bounded);
    let mut observed = BTreeMap::new();
    let mut previous_path: Option<String> = None;
    let mut expanded_bytes = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        if observed.len() >= OPTIONAL_PARSER_PACK_MAX_FILE_ENTRIES.saturating_add(1) {
            return Err(invalid("archive exceeded its file-entry ceiling"));
        }
        if !entry.header().entry_type().is_file() {
            return Err(invalid("archive contains a non-regular entry"));
        }
        let raw_path = entry.path_bytes();
        let archive_path = str::from_utf8(raw_path.as_ref())?;
        let relative = archive_path
            .strip_prefix(&format!("{ARCHIVE_ROOT}/"))
            .ok_or_else(|| invalid("archive entry is outside the canonical pack root"))?;
        let relative = PackRelativePath::new(relative)?;
        if previous_path
            .as_ref()
            .is_some_and(|previous| previous.as_str() >= relative.as_str())
        {
            return Err(invalid(
                "archive entries are not strictly path-sorted and unique",
            ));
        }
        previous_path = Some(relative.as_str().to_owned());
        let bytes = entry.header().size()?;
        if bytes == 0 || bytes > OPTIONAL_PARSER_PACK_MAX_FILE_BYTES {
            return Err(invalid("archive entry is empty or exceeds its file bound"));
        }
        let expected_mode = if matches!(
            relative.as_str(),
            "projectatlas-parser-worker" | "projectatlas-parser-worker.exe"
        ) {
            WORKER_MODE
        } else {
            PAYLOAD_MODE
        };
        if entry.header().uid()? != 0
            || entry.header().gid()? != 0
            || entry.header().mtime()? != 0
            || entry.header().mode()? != expected_mode
        {
            return Err(invalid("archive entry metadata is not canonical"));
        }
        expanded_bytes = expanded_bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid("expanded payload byte count overflowed"))?;
        if expanded_bytes > OPTIONAL_PARSER_PACK_MAX_EXPANDED_BYTES {
            return Err(invalid("archive exceeded its expanded payload ceiling"));
        }
        let destination = pack_root.join(Path::new(relative.as_str()));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let mut hasher = Sha256::new();
        let copied = copy_and_hash(&mut entry, &mut output, &mut hasher)?;
        if copied != bytes {
            return Err(invalid("archive entry size differs from its tar header"));
        }
        output.sync_all()?;
        #[cfg(unix)]
        set_extracted_mode(&destination, expected_mode)?;
        let key = relative.as_str().to_owned();
        observed.insert(
            key,
            ObservedFile {
                bytes,
                sha256: Sha256Digest::new(lowercase_hex(hasher.finalize().as_ref()))?,
            },
        );
    }
    let mut bounded = archive.into_inner();
    require_zero_tar_padding(&mut bounded)?;
    Ok(ExtractedArchive {
        _directory: directory,
        pack_root,
        expanded_bytes,
        observed,
    })
}

/// Require all decompressed bytes after the tar terminator to be canonical zero padding.
fn require_zero_tar_padding(input: &mut impl Read) -> ToolResult<()> {
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        if buffer[..read].iter().any(|byte| *byte != 0) {
            return Err(invalid(
                "archive contains non-zero data after its canonical tar terminator",
            ));
        }
    }
}

/// Copy one bounded archive entry while computing its exact digest.
fn copy_and_hash(input: &mut impl Read, output: &mut File, hasher: &mut Sha256) -> ToolResult<u64> {
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut total = 0u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(u64::try_from(read)?)
            .ok_or_else(|| invalid("archive entry byte count overflowed"))?;
    }
    Ok(total)
}

/// Supervise one isolated child until success, failure, or timeout.
#[cfg(test)]
fn run_process(
    executable: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    timeout: Duration,
) -> ToolResult<()> {
    if !executable.is_absolute() || !working_directory.is_absolute() {
        return Err(invalid(
            "fresh-runner executable and working directory must be absolute",
        ));
    }
    let mut child = Command::new(executable)
        .args(arguments)
        .env_clear()
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(invalid(format!(
                    "packaged worker exited unsuccessfully with {status}"
                )))
            };
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let status = child.wait()?;
            return Err(invalid(format!(
                "packaged worker exceeded {} ms and was terminated with {status}",
                timeout.as_millis()
            )));
        }
        thread::sleep(CHILD_PROCESS_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
    }
}

/// Hash one exact regular file under a hard byte ceiling.
fn sha256_file(path: &Path, maximum: u64) -> ToolResult<(Sha256Digest, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(invalid(format!(
            "{} is not a bounded non-empty regular file",
            path.display()
        )));
    }
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut total = 0u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(u64::try_from(read)?)
            .ok_or_else(|| invalid("file byte count overflowed"))?;
        if total > maximum {
            return Err(invalid(format!(
                "{} exceeded its file byte ceiling",
                path.display()
            )));
        }
    }
    if total != metadata.len() {
        return Err(invalid(format!(
            "{} changed while it was being hashed",
            path.display()
        )));
    }
    Ok((
        Sha256Digest::new(lowercase_hex(hasher.finalize().as_ref()))?,
        total,
    ))
}

/// Read one exact regular file without materializing beyond its hard limit.
fn read_bounded_file(path: &Path, maximum: u64) -> ToolResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(invalid(format!(
            "{} is not a bounded non-empty regular file",
            path.display()
        )));
    }
    let capacity = usize::try_from(metadata.len())?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() != capacity {
        return Err(invalid(format!(
            "{} changed while it was being read",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Serialize one proof to a new file through an atomic no-clobber move.
fn write_new_json(path: &Path, value: &impl serde::Serialize) -> ToolResult<()> {
    if path.exists() {
        return Err(invalid(format!(
            "refusing to overwrite proof {}",
            path.display()
        )));
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    require_directory(parent, "proof parent directory")?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| Box::<dyn Error>::from(error.error))?;
    Ok(())
}

/// Require one observed artifact file by canonical path.
fn require_observed<'a>(
    observed: &'a BTreeMap<String, ObservedFile>,
    path: &str,
) -> ToolResult<&'a ObservedFile> {
    observed
        .get(path)
        .ok_or_else(|| invalid(format!("artifact is missing required file {path:?}")))
}

/// Require the canonical archive basename for one platform.
fn require_archive_name(path: &Path, platform: PackPlatform) -> ToolResult<()> {
    let expected = format!("{ARCHIVE_ROOT}-{}.tar.zst", platform.as_str());
    if path.file_name().and_then(OsStr::to_str) != Some(expected.as_str()) {
        return Err(invalid(format!(
            "archive basename must be {expected:?} for {}",
            platform.as_str()
        )));
    }
    Ok(())
}

/// Require one existing non-symlink directory.
fn require_directory(path: &Path, owner: &str) -> ToolResult<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(invalid(format!("{owner} is not a directory")));
    }
    Ok(())
}

/// Refuse an already occupied release-output path before expensive verification work.
fn require_new_output(path: &Path, owner: &str) -> ToolResult<()> {
    if path.exists() {
        return Err(invalid(format!(
            "refusing to overwrite {owner} {}",
            path.display()
        )));
    }
    Ok(())
}

/// Return whether a string slice is strictly sorted and unique.
fn strictly_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| {
        pair.first()
            .is_some_and(|left| pair.get(1).is_some_and(|right| left < right))
    })
}

/// Return one platform's fixed aggregate order.
fn platform_ordinal(platform: PackPlatform) -> usize {
    PackPlatform::ALL
        .iter()
        .position(|candidate| *candidate == platform)
        .unwrap_or(PackPlatform::ALL.len())
}

/// Hash exact bytes through SHA-256.
fn sha256_bytes(bytes: &[u8]) -> String {
    lowercase_hex(Sha256::digest(bytes).as_ref())
}

/// Encode bytes as canonical lowercase hexadecimal without another dependency.
fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Construct one data-boundary error with preserved human context.
fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

/// Identify the contained optional-pack target or fail closed before verification.
fn current_platform() -> ToolResult<PackPlatform> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(PackPlatform::LinuxX86_64),
        ("windows", "x86_64") => Ok(PackPlatform::WindowsX86_64),
        _ => Err(invalid(
            "unsupported_containment: this host has no accepted optional parser-pack runtime adapter",
        )),
    }
}

/// Restore canonical executable permissions after manual extraction.
#[cfg(unix)]
fn set_extracted_mode(path: &Path, mode: u32) -> ToolResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Protect deterministic archive mechanics, process bounds, and aggregate validation.

    use super::*;
    use projectatlas_core::optional_parser_pack::{
        Blake3Digest, OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
        OptionalParserPackPlatformProof, ParserPackCandidateIdentity,
        ParserPackCandidateSourceState, ParserPackNativeAudit, ParserPackOfflineConstruction,
        ParserPackPayloadFile, ParserPackPayloadMeasurements, ParserPackSourceAsset,
        SourceRevision,
    };

    /// Platform-neutral fixture target for archive and manifest unit tests.
    const TEST_PLATFORM: PackPlatform = PackPlatform::LinuxX86_64;

    /// Ensure one failed predicate becomes a normal test error.
    fn require(condition: bool, message: &str) -> ToolResult<()> {
        if condition {
            Ok(())
        } else {
            Err(invalid(message))
        }
    }

    #[test]
    fn release_operation_and_profile_cleanup_failures_are_both_retained() -> ToolResult<()> {
        let result = finish_release_cleanup::<()>(
            Err(invalid("verification failed")),
            Err(OptionalParserPackLifecycleError::InvalidData {
                reason: "cleanup failed".to_owned(),
            }),
        );
        let error = match result {
            Ok(()) => return Err(invalid("dual release failure was accepted")),
            Err(error) => error,
        };
        let combined = error
            .downcast_ref::<ReleaseOperationAndCleanupError>()
            .ok_or_else(|| invalid("dual release failure lost its typed wrapper"))?;
        require(
            combined
                .operation
                .to_string()
                .contains("verification failed")
                && combined.cleanup.to_string().contains("cleanup failed"),
            "dual release failure lost one typed cause",
        )
    }

    /// Return the construction egress-denial mechanism required by one platform.
    fn test_construction_network_denial(platform: PackPlatform) -> ParserPackNetworkDenial {
        let mechanism = match platform {
            PackPlatform::LinuxX86_64 => ParserPackNetworkIsolation::LinuxNetworkNamespace,
            PackPlatform::WindowsX86_64 => ParserPackNetworkIsolation::WindowsPrincipalFirewall,
        };
        ParserPackNetworkDenial {
            mechanism,
            dns_denied: true,
            direct_tcp_denied: true,
            https_denied: true,
        }
    }

    /// Return the fresh-verification egress-denial mechanism required by one platform.
    fn test_fresh_runner_network_denial(platform: PackPlatform) -> ParserPackNetworkDenial {
        let mechanism = match platform {
            PackPlatform::LinuxX86_64 => ParserPackNetworkIsolation::LinuxNetworkNamespace,
            PackPlatform::WindowsX86_64 => ParserPackNetworkIsolation::WindowsAppContainer,
        };
        ParserPackNetworkDenial {
            mechanism,
            dns_denied: true,
            direct_tcp_denied: true,
            https_denied: true,
        }
    }

    /// Build one clean exact candidate shared by aggregate test proofs.
    fn test_candidate() -> ToolResult<ParserPackCandidateIdentity> {
        Ok(ParserPackCandidateIdentity {
            projectatlas_revision: SourceRevision::new("1".repeat(40))?,
            cargo_package_version: "0.4.0".to_owned(),
            intended_release_version: "0.4.0".to_owned(),
            cargo_lock_sha256: Sha256Digest::new("2".repeat(64))?,
            rustc_release: "1.93.0".to_owned(),
            rustc_commit_hash: "3".repeat(40),
            source_state: ParserPackCandidateSourceState::Clean,
        })
    }

    /// Build the smallest artifact value needed to test exact observed inventory.
    fn inventory_artifact(payload: &[u8]) -> ToolResult<OptionalParserPackArtifactManifest> {
        let file = ParserPackPayloadFile {
            path: PackRelativePath::new("payload.bin")?,
            role: ParserPackPayloadRole::ProjectLicense,
            bytes: u64::try_from(payload.len())?,
            sha256: Sha256Digest::new(sha256_bytes(payload))?,
        };
        let files = vec![file];
        Ok(OptionalParserPackArtifactManifest {
            schema_version: OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
            pack_id: "projectatlas-broad-parser".to_owned(),
            projectatlas_version: "0.4.0".to_owned(),
            platform: TEST_PLATFORM,
            candidate: test_candidate()?,
            accepted_manifest_sha256: Sha256Digest::new("4".repeat(64))?,
            capability_set_digest: Blake3Digest::for_bytes(b"capability"),
            fixture_corpus_sha256: Sha256Digest::new("5".repeat(64))?,
            source_asset: ParserPackSourceAsset {
                release_tag: "v1.13.2".to_owned(),
                release_revision: SourceRevision::new("6".repeat(40))?,
                name: "source.tar.zst".to_owned(),
                sha256: Sha256Digest::new("7".repeat(64))?,
                bytes: 1024,
                parsers_manifest_sha256: Sha256Digest::new("8".repeat(64))?,
            },
            construction: ParserPackOfflineConstruction {
                cargo_frozen: ParserPackVerifiedControl::Verified,
                cargo_offline: ParserPackVerifiedControl::Verified,
                dependency_offline: ParserPackVerifiedControl::Verified,
                zero_embedded_grammars: ParserPackVerifiedControl::Verified,
                language_selector_absent: ParserPackVerifiedControl::Verified,
                failed_grammar_override_absent: ParserPackVerifiedControl::Verified,
                network_denial: test_construction_network_denial(TEST_PLATFORM),
            },
            native_audit: ParserPackNativeAudit {
                policy_sha256: Sha256Digest::new("9".repeat(64))?,
                report_sha256: Sha256Digest::new("a".repeat(64))?,
                audited_libraries: 1,
                forbidden_imports: 0,
                unexpected_dependencies: 0,
                missing_exports: 0,
                unexpected_exports: 0,
            },
            measurements: ParserPackPayloadMeasurements::from_files(&files)?,
            files,
        })
    }

    /// Write one test archive from an explicitly selected staged inventory.
    fn write_inventory_archive(staged: &Path, paths: &[&str], archive: &Path) -> ToolResult<()> {
        let mut paths = paths
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>();
        paths.sort();
        write_deterministic_archive(staged, &paths, archive, TEST_PLATFORM)
    }

    /// Return the checked-in accepted manifest used by aggregate tests.
    fn accepted_manifest_path() -> ToolResult<PathBuf> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("core crate is not inside the workspace"))?;
        Ok(workspace.join("packaging/parser-pack/accepted-capabilities.json"))
    }

    /// Build one valid platform proof from the checked-in accepted manifest.
    fn test_platform_proof(
        logical: &OptionalParserPackManifest,
        accepted_sha256: &Sha256Digest,
        platform: PackPlatform,
        ordinal: usize,
    ) -> ToolResult<OptionalParserPackPlatformProof> {
        Ok(OptionalParserPackPlatformProof {
            schema_version: OPTIONAL_PARSER_PACK_PLATFORM_PROOF_SCHEMA_VERSION,
            pack_id: logical.pack_id().to_owned(),
            platform,
            candidate: test_candidate()?,
            archive_name: format!("{ARCHIVE_ROOT}-{}.tar.zst", platform.as_str()),
            archive_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 1))?,
            archive_bytes: 1024,
            expanded_bytes: 4096,
            artifact_manifest_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 11))?,
            accepted_manifest_sha256: accepted_sha256.clone(),
            capability_set_digest: logical.capability_set_digest().clone(),
            fixture_corpus_sha256: Sha256Digest::new("b".repeat(64))?,
            native_audit_report_sha256: Sha256Digest::new(format!("{:064x}", ordinal + 21))?,
            runner: ParserPackFreshRunner {
                fresh_host: ParserPackVerifiedControl::Verified,
                repository_inputs_absent: ParserPackVerifiedControl::Verified,
                build_tools_not_invoked: ParserPackVerifiedControl::Verified,
                working_directory_outside_pack: ParserPackVerifiedControl::Verified,
                ambient_library_paths_cleared: ParserPackVerifiedControl::Verified,
                network_denial: test_fresh_runner_network_denial(platform),
            },
            grammars: logical
                .grammars()
                .iter()
                .map(|grammar| ParserPackGrammarProbe {
                    language_id: grammar.language_id.clone(),
                    worker_probe_passed: true,
                })
                .collect(),
            memory: match platform {
                PackPlatform::LinuxX86_64 => ParserPackMemoryProbe {
                    control: ParserPackMemoryControl::LinuxProcStatus,
                    process_limit_bytes: OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES,
                    process_tree_limit_bytes: OPTIONAL_PARSER_PACK_LINUX_MEMORY_PROBE_BYTES,
                    observation_interval_millis: Some(20),
                    peak_observed_bytes: Some(1024 * 1024 + 4096),
                    maximum_observed_overshoot_bytes: Some(4096),
                    limit_enforced: ParserPackVerifiedControl::Verified,
                    process_tree_cleaned: ParserPackVerifiedControl::Verified,
                },
                PackPlatform::WindowsX86_64 => ParserPackMemoryProbe {
                    control: ParserPackMemoryControl::WindowsJobObject,
                    process_limit_bytes: OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES,
                    process_tree_limit_bytes:
                        OPTIONAL_PARSER_PACK_WINDOWS_MINIMUM_MEMORY_PROBE_BYTES,
                    observation_interval_millis: None,
                    peak_observed_bytes: None,
                    maximum_observed_overshoot_bytes: None,
                    limit_enforced: ParserPackVerifiedControl::Verified,
                    process_tree_cleaned: ParserPackVerifiedControl::Verified,
                },
            },
        })
    }

    /// Create identical canonical archive bytes from identical staged bytes.
    #[test]
    fn deterministic_archive_bytes_are_reproducible() -> ToolResult<()> {
        let staging = tempfile::tempdir()?;
        fs::write(staging.path().join("a.txt"), b"alpha")?;
        fs::write(staging.path().join("b.txt"), b"beta")?;
        let output = tempfile::tempdir()?;
        let first = output.path().join("first.tar.zst");
        let second = output.path().join("second.tar.zst");
        let paths = vec!["a.txt".to_owned(), "b.txt".to_owned()];
        write_deterministic_archive(staging.path(), &paths, &first, TEST_PLATFORM)?;
        write_deterministic_archive(staging.path(), &paths, &second, TEST_PLATFORM)?;
        require(
            fs::read(first)? == fs::read(second)?,
            "identical staged bytes did not produce identical archive bytes",
        )
    }

    /// Reject an archive that carries a file absent from its artifact manifest.
    #[test]
    fn archive_rejects_extra_files() -> ToolResult<()> {
        let staging = tempfile::tempdir()?;
        let payload = b"expected payload";
        fs::write(staging.path().join("payload.bin"), payload)?;
        fs::write(staging.path().join("extra.bin"), b"not manifested")?;
        fs::write(
            staging.path().join(ARTIFACT_MANIFEST_FILE_NAME),
            serde_json::to_vec(&inventory_artifact(payload)?)?,
        )?;
        let output = tempfile::tempdir()?;
        let archive_path = output.path().join("extra.tar.zst");
        write_inventory_archive(
            staging.path(),
            &[ARTIFACT_MANIFEST_FILE_NAME, "extra.bin", "payload.bin"],
            &archive_path,
        )?;
        let extracted = extract_archive(&archive_path)?;
        let artifact: OptionalParserPackArtifactManifest =
            serde_json::from_slice(&read_bounded_file(
                &extracted.pack_root.join(ARTIFACT_MANIFEST_FILE_NAME),
                u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
            )?)?;
        require(
            validate_extracted_inventory(&extracted, &artifact).is_err(),
            "archive accepted an unmanifested extra file",
        )
    }

    /// Reject an archive whose payload bytes differ from its artifact manifest.
    #[test]
    fn archive_rejects_tampered_payloads() -> ToolResult<()> {
        let staging = tempfile::tempdir()?;
        let artifact = inventory_artifact(b"expected payload")?;
        fs::write(staging.path().join("payload.bin"), b"tampered payload")?;
        fs::write(
            staging.path().join(ARTIFACT_MANIFEST_FILE_NAME),
            serde_json::to_vec(&artifact)?,
        )?;
        let output = tempfile::tempdir()?;
        let archive_path = output.path().join("tampered.tar.zst");
        write_inventory_archive(
            staging.path(),
            &[ARTIFACT_MANIFEST_FILE_NAME, "payload.bin"],
            &archive_path,
        )?;
        let extracted = extract_archive(&archive_path)?;
        require(
            validate_extracted_inventory(&extracted, &artifact).is_err(),
            "archive accepted payload bytes that differed from its manifest",
        )
    }

    /// Bind the packaged native-audit report digest and every accepted grammar identity.
    #[test]
    fn native_audit_report_is_digest_and_identity_bound() -> ToolResult<()> {
        let accepted_path = accepted_manifest_path()?;
        let logical = OptionalParserPackManifest::from_json(&read_bounded_file(
            &accepted_path,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
        )?)?;
        let platform = TEST_PLATFORM;
        let (binary_format, architecture) = audit_target_vocabulary(platform);
        let mut artifact = inventory_artifact(b"placeholder")?;
        let grammar_files = logical
            .grammars()
            .iter()
            .enumerate()
            .map(|(ordinal, grammar)| {
                Ok(ParserPackPayloadFile {
                    path: PackRelativePath::new(format!(
                        "{LIB_DIRECTORY_NAME}/{}",
                        platform.grammar_library_file_name(&grammar.abi_export.library_stem)
                    ))?,
                    role: ParserPackPayloadRole::GrammarLibrary {
                        language_id: grammar.language_id.clone(),
                    },
                    bytes: 1,
                    sha256: Sha256Digest::new(format!("{:064x}", ordinal + 1))?,
                })
            })
            .collect::<ToolResult<Vec<_>>>()?;
        let worker_file = ParserPackPayloadFile {
            path: PackRelativePath::new(platform.worker_file_name())?,
            role: ParserPackPayloadRole::Worker,
            bytes: 1,
            sha256: Sha256Digest::new("e".repeat(64))?,
        };
        artifact.files = grammar_files.clone();
        artifact.files.push(worker_file.clone());
        artifact
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        artifact.measurements = ParserPackPayloadMeasurements::from_files(&artifact.files)?;
        let rows = logical
            .grammars()
            .iter()
            .zip(&grammar_files)
            .map(|(grammar, file)| NativeAuditRowWire {
                language_id: grammar.language_id.clone(),
                file: AuditedFileWire {
                    path: file.path.as_str().to_owned(),
                    sha256: file.sha256.as_str().to_owned(),
                    byte_length: file.bytes,
                },
                export_symbol: grammar.abi_export.export_symbol.as_str().to_owned(),
                expected_abi: grammar.abi_export.expected_abi,
                binary_format: binary_format.to_owned(),
                architecture: architecture.to_owned(),
                native_libraries: Vec::new(),
                imported_symbol_count: 0,
                imported_symbols_sha256: "d".repeat(64),
            })
            .collect::<Vec<_>>();
        let mut report = NativeAuditReportWire {
            schema_version: OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION,
            worker: WorkerAuditWire {
                file: AuditedFileWire {
                    path: worker_file.path.as_str().to_owned(),
                    sha256: worker_file.sha256.as_str().to_owned(),
                    byte_length: worker_file.bytes,
                },
                binary_format: binary_format.to_owned(),
                architecture: architecture.to_owned(),
                object_kind: "executable".to_owned(),
                entry_point: "0x0000000000000001".to_owned(),
                native_libraries: Vec::new(),
                imported_symbol_count: 0,
                imported_symbols_sha256: "f".repeat(64),
                export_count: 0,
                exports_sha256: "0".repeat(64),
                defined_symbol_evidence_available: false,
                defined_symbol_count: None,
                defined_symbols_sha256: None,
            },
            containment_broker: ContainmentBrokerAuditPresence(None),
            grammars: rows,
        };
        let pack = tempfile::tempdir()?;
        let report_path = pack.path().join(NATIVE_AUDIT_REPORT_FILE_NAME);
        let report_bytes = serde_json::to_vec(&report)?;
        fs::write(&report_path, &report_bytes)?;
        artifact.native_audit.report_sha256 = Sha256Digest::new(sha256_bytes(&report_bytes))?;
        let observed = enumerate_staged_files(pack.path())?;
        validate_native_audit_report(pack.path(), &logical, &artifact, &observed)?;

        let worker_sha256 = report.worker.file.sha256.clone();
        report.worker.file.sha256 = "1".repeat(64);
        let worker_drift_bytes = serde_json::to_vec(&report)?;
        fs::write(&report_path, &worker_drift_bytes)?;
        artifact.native_audit.report_sha256 = Sha256Digest::new(sha256_bytes(&worker_drift_bytes))?;
        let worker_drift_observed = enumerate_staged_files(pack.path())?;
        require(
            validate_native_audit_report(pack.path(), &logical, &artifact, &worker_drift_observed)
                .is_err(),
            "native-audit report accepted drifted worker file evidence",
        )?;
        report.worker.file.sha256 = worker_sha256;

        report.worker.defined_symbol_count = Some(1);
        let inconsistent_bytes = serde_json::to_vec(&report)?;
        fs::write(&report_path, &inconsistent_bytes)?;
        artifact.native_audit.report_sha256 = Sha256Digest::new(sha256_bytes(&inconsistent_bytes))?;
        let inconsistent_observed = enumerate_staged_files(pack.path())?;
        require(
            validate_native_audit_report(pack.path(), &logical, &artifact, &inconsistent_observed)
                .is_err(),
            "native-audit report accepted inconsistent worker definition evidence",
        )?;
        report.worker.defined_symbol_count = None;

        let first = report
            .grammars
            .first_mut()
            .ok_or_else(|| invalid("native-audit fixture rows are empty"))?;
        first.language_id.push_str("-drift");
        let drifted_bytes = serde_json::to_vec(&report)?;
        fs::write(&report_path, &drifted_bytes)?;
        artifact.native_audit.report_sha256 = Sha256Digest::new(sha256_bytes(&drifted_bytes))?;
        let drifted_observed = enumerate_staged_files(pack.path())?;
        require(
            validate_native_audit_report(pack.path(), &logical, &artifact, &drifted_observed)
                .is_err(),
            "native-audit report accepted a drifted grammar identity",
        )
    }

    /// Bind Windows broker evidence to its exact payload, target, and native surfaces.
    #[test]
    fn containment_broker_audit_is_platform_and_payload_bound() -> ToolResult<()> {
        let mut artifact = inventory_artifact(b"placeholder")?;
        artifact.platform = PackPlatform::WindowsX86_64;
        let broker_file = ParserPackPayloadFile {
            path: PackRelativePath::new("projectatlas-parser-containment.exe")?,
            role: ParserPackPayloadRole::ContainmentBroker,
            bytes: 33_280,
            sha256: Sha256Digest::new("b".repeat(64))?,
        };
        artifact.files.push(broker_file.clone());
        artifact
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        artifact.measurements = ParserPackPayloadMeasurements::from_files(&artifact.files)?;
        let mut presence = ContainmentBrokerAuditPresence(Some(ContainmentBrokerAuditWire {
            file: AuditedFileWire {
                path: broker_file.path.as_str().to_owned(),
                sha256: broker_file.sha256.as_str().to_owned(),
                byte_length: broker_file.bytes,
            },
            runtime_family: OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY.to_owned(),
            binary_format: "pe".to_owned(),
            architecture: "x86_64".to_owned(),
            object_kind: "executable".to_owned(),
            entry_point: OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT.to_owned(),
            clr_runtime_header_rva: 0x2000,
            clr_runtime_header_size: OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE,
            pe_loader_libraries: OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES
                .iter()
                .map(|library| (*library).to_owned())
                .collect(),
            pe_imported_symbol_count: 0,
            pe_imported_symbols_sha256: sha256_bytes(&[]),
            managed_modules: OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES
                .iter()
                .map(|module| (*module).to_owned())
                .collect(),
            managed_import_count: 30,
            managed_imports_sha256: "d".repeat(64),
            export_count: 0,
            exports_sha256: sha256_bytes(&[]),
        }));
        validate_containment_broker_audit(&presence, &artifact)?;
        assert!(
            validate_containment_broker_audit(&ContainmentBrokerAuditPresence(None), &artifact)
                .is_err()
        );

        let broker = presence.0.as_mut().expect("broker fixture");
        broker.clr_runtime_header_rva = 0;
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        let broker = presence.0.as_mut().expect("broker fixture");
        broker.clr_runtime_header_rva = 0x2000;
        broker.entry_point = "0x0000000000000001".to_owned();
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        let broker = presence.0.as_mut().expect("broker fixture");
        broker.entry_point = OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT.to_owned();
        broker.pe_loader_libraries.push("mscoree.dll".to_owned());
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        presence
            .0
            .as_mut()
            .expect("broker fixture")
            .pe_loader_libraries
            .clear();
        presence.0.as_mut().expect("broker fixture").file.sha256 = "f".repeat(64);
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        let broker = presence.0.as_mut().expect("broker fixture");
        broker.file.sha256 = broker_file.sha256.as_str().to_owned();
        broker.managed_modules.pop();
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        presence
            .0
            .as_mut()
            .expect("broker fixture")
            .managed_modules
            .push("userenv.dll".to_owned());
        artifact.platform = PackPlatform::LinuxX86_64;
        assert!(validate_containment_broker_audit(&presence, &artifact).is_err());
        Ok(())
    }

    /// Reject non-canonical worker entry points and platform-incompatible object kinds.
    #[test]
    fn worker_audit_vocabulary_is_fail_closed() -> ToolResult<()> {
        require(
            canonical_nonzero_entry_point("0x0000000000000001"),
            "canonical non-zero entry point was rejected",
        )?;
        for invalid_entry_point in [
            "0x0000000000000000",
            "0000000000000001",
            "0x000000000000000A",
            "0x1",
        ] {
            require(
                !canonical_nonzero_entry_point(invalid_entry_point),
                "non-canonical entry point was accepted",
            )?;
        }
        require(
            accepted_worker_object_kind(PackPlatform::LinuxX86_64, "dynamic")
                && accepted_worker_object_kind(PackPlatform::LinuxX86_64, "executable")
                && !accepted_worker_object_kind(PackPlatform::WindowsX86_64, "dynamic")
                && accepted_worker_object_kind(PackPlatform::WindowsX86_64, "executable"),
            "worker object-kind policy drifted",
        )
    }

    /// Reject a non-regular entry before extraction can create it.
    #[test]
    fn archive_rejects_non_regular_entries() -> ToolResult<()> {
        let output = tempfile::tempdir()?;
        let archive_path = output.path().join("unsafe.tar.zst");
        let file = File::create(&archive_path)?;
        let encoder = zstd::Encoder::new(file, ZSTD_COMPRESSION_LEVEL)?;
        let mut archive = TarBuilder::new(encoder);
        let mut header = Header::new_ustar();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(PAYLOAD_MODE);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_link_name("../outside")?;
        header.set_cksum();
        archive.append_data(&mut header, format!("{ARCHIVE_ROOT}/unsafe"), io::empty())?;
        archive.finish()?;
        archive.into_inner()?.finish()?;
        require(
            extract_archive(&archive_path).is_err(),
            "archive accepted a non-regular traversal entry",
        )
    }

    /// Exercise the child timeout path without a shell or external fixture executable.
    #[test]
    fn child_process_timeout_is_enforced() -> ToolResult<()> {
        let executable = env::current_exe()?;
        let working_directory = tempfile::tempdir()?;
        let arguments = [
            OsString::from("--exact"),
            OsString::from("tests::timeout_child_fixture"),
        ];
        require(
            run_process(
                &executable,
                &arguments,
                working_directory.path(),
                Duration::from_millis(20),
            )
            .is_err(),
            "long-running child escaped its timeout",
        )
    }

    /// Sleep only when selected exactly by the timeout supervisor subprocess.
    #[test]
    fn timeout_child_fixture() {
        if env::args_os().any(|argument| argument == "--exact") {
            thread::sleep(Duration::from_secs(1));
        }
    }

    /// Reject an unsuccessful child before a proof can claim a passed grammar.
    #[test]
    fn child_process_failure_is_enforced() -> ToolResult<()> {
        let executable = env::current_exe()?;
        let working_directory = tempfile::tempdir()?;
        let arguments = [OsString::from("--definitely-not-a-test-harness-option")];
        require(
            run_process(
                &executable,
                &arguments,
                working_directory.path(),
                Duration::from_secs(5),
            )
            .is_err(),
            "unsuccessful child was accepted",
        )
    }

    /// Aggregate the complete matching proof set and reject one divergent candidate.
    #[test]
    fn aggregate_requires_complete_proofs_from_one_candidate() -> ToolResult<()> {
        let accepted_path = accepted_manifest_path()?;
        let accepted_bytes = read_bounded_file(
            &accepted_path,
            u64::try_from(OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?,
        )?;
        let logical = OptionalParserPackManifest::from_json(&accepted_bytes)?;
        let accepted_sha256 = Sha256Digest::new(sha256_bytes(&accepted_bytes))?;
        let proofs = PackPlatform::ALL
            .iter()
            .enumerate()
            .map(|(ordinal, platform)| {
                test_platform_proof(&logical, &accepted_sha256, *platform, ordinal)
            })
            .collect::<ToolResult<Vec<_>>>()?;
        let directory = tempfile::tempdir()?;
        let proof_paths = proofs
            .iter()
            .enumerate()
            .map(|(ordinal, proof)| {
                let path = directory.path().join(format!("proof-{ordinal}.json"));
                write_new_json(&path, proof)?;
                Ok(path)
            })
            .collect::<ToolResult<Vec<_>>>()?;
        let aggregate_path = directory.path().join("aggregate.json");
        aggregate_proofs(&accepted_path, &proof_paths, &aggregate_path)?;
        let aggregate: OptionalParserPackProofAggregate = serde_json::from_slice(
            &read_bounded_file(&aggregate_path, MAX_PLATFORM_PROOF_BYTES)?,
        )?;
        aggregate.validate(&logical)?;

        let mut divergent = proofs;
        let last = divergent
            .last_mut()
            .ok_or_else(|| invalid("proof fixture set is empty"))?;
        last.candidate.projectatlas_revision = SourceRevision::new("c".repeat(40))?;
        let divergent_paths = divergent
            .iter()
            .enumerate()
            .map(|(ordinal, proof)| {
                let path = directory.path().join(format!("divergent-{ordinal}.json"));
                write_new_json(&path, proof)?;
                Ok(path)
            })
            .collect::<ToolResult<Vec<_>>>()?;
        require(
            aggregate_proofs(
                &accepted_path,
                &divergent_paths,
                &directory.path().join("divergent-aggregate.json"),
            )
            .is_err(),
            "aggregate accepted platform proofs from divergent candidates",
        )
    }
}
