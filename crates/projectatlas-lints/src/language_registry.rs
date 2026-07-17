//! Typed validation and deterministic generation for the `ProjectAtlas` language registry.

#[cfg(not(windows))]
use cap_fs_ext::DirExt as _;
#[cfg(windows)]
use cap_fs_ext::OpenOptionsMaybeDirExt as _;
use cap_fs_ext::{FollowSymlinks, MetadataExt as _, OpenOptionsFollowExt as _};
use cap_std::ambient_authority;
use cap_std::fs::{
    Dir as CapabilityDir, File as CapabilityFile, Metadata as CapabilityMetadata,
    OpenOptions as CapabilityOpenOptions,
};
use processkit::{Command as ProcessCommand, OutputBufferPolicy, ProcessResult};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::{self, Write as FmtWrite};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;

/// Language-registry command that validates tracked output without writing.
const COMMAND_CHECK: &str = "check";
/// Language-registry command that validates and replaces tracked output.
const COMMAND_WRITE: &str = "write";
/// Repository-relative composite language-registry lock path.
const LOCK_PATH: &str = "registry/language-registry.json";
/// Repository-relative accepted v0.4 capability target path.
const ACCEPTED_TARGET_PATH: &str = "docs/benchmarks/projectatlas-v0.4-capability-registry.json";
/// Repository-relative historical runtime contract path.
const HISTORICAL_CONTRACT_PATH: &str =
    "fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon";
/// Repository-relative release-owned parser-pack trust manifest.
const PARSER_PACK_TRUST_PATH: &str = "registry/parser-pack-trust.json";
/// Repository-relative authoritative optional-pack budget contract.
const REPOSITORY_INTELLIGENCE_CONTRACTS_PATH: &str =
    "docs/benchmarks/projectatlas-v0.4-repository-intelligence-contracts.json";
/// Generated core language projection path.
const CORE_OUTPUT_PATH: &str = "crates/projectatlas-core/src/language_detection_registry.rs";
/// Generated symbols language projection path.
const SYMBOLS_OUTPUT_PATH: &str = "crates/projectatlas-symbols/src/language_parser_registry.rs";
/// Generated CLI language projection path.
const CLI_OUTPUT_PATH: &str = "crates/projectatlas-cli/src/language_capability_settings.rs";
/// Generated structured capability-state path.
const EVIDENCE_OUTPUT_PATH: &str =
    "docs/benchmarks/projectatlas-v0.4-language-capability-state.json";
/// Generated documentation and release language-support matrix.
const DOCUMENTATION_OUTPUT_PATH: &str = "docs/language-capabilities.json";
/// Semantic language-registry digest domain.
const REGISTRY_DIGEST_DOMAIN: &str = "projectatlas.language-registry.contract";
/// Semantic language-registry digest encoding version.
const REGISTRY_DIGEST_VERSION: u64 = 1;
/// Exact composite language-registry schema supported by this implementation.
const LANGUAGE_REGISTRY_SCHEMA_VERSION: u32 = 2;
/// Exact parser-pack trust schema supported by this implementation.
const PARSER_PACK_TRUST_SCHEMA_VERSION: u32 = 1;
/// Exact tracked parser-pack release-manifest schema supported by this implementation.
const PARSER_PACK_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Exact tracked parser-pack provenance schema supported by this implementation.
const PARSER_PACK_PROVENANCE_SCHEMA_VERSION: u32 = 1;
/// Exact tracked parser-pack advisory schema supported by this implementation.
const PARSER_PACK_ADVISORY_SCHEMA_VERSION: u32 = 1;
/// Exact tracked parser-pack WASM validation schema supported by this implementation.
const PARSER_PACK_WASM_VALIDATION_SCHEMA_VERSION: u32 = 1;
/// External parser ABI family carried by the tracked WebAssembly candidate.
const TREE_SITTER_WASM_ABI_ID: &str = "abi.tree-sitter-wasm";
/// Exact Tree-sitter grammar ABI carried by the tracked WebAssembly candidate.
const TREE_SITTER_WASM_ABI_VERSION: u32 = 15;
/// Parser-pack lifecycle ABI accepted by the existing `ProjectAtlas` lifecycle owner.
const PROJECTATLAS_PACK_ABI_ID: &str = "abi.projectatlas-parser-pack";
/// Exact parser-pack lifecycle ABI version accepted by the lifecycle owner.
const PROJECTATLAS_PACK_ABI_VERSION: u32 = 1;
/// Packaged platform carried by the tracked WebAssembly candidate.
const TREE_SITTER_WASM_PLATFORM: &str = "wasm32-unknown-emscripten";
/// Required Tree-sitter grammar function export.
const TREE_SITTER_WASM_REQUIRED_EXPORT: &str = "tree_sitter_javascript";
/// Exact WebAssembly module version admitted by the evaluation validator.
const TREE_SITTER_WASM_MODULE_VERSION: u32 = 1;
/// Exact accepted-capability schema supported by this implementation.
const ACCEPTED_CAPABILITY_SCHEMA_VERSION: u32 = 2;
/// Exact frozen runtime-compatibility fixture schema supported by this implementation.
const HISTORICAL_RUNTIME_CONTRACT_SCHEMA_VERSION: u32 = 3;
/// Ordered language-support evidence tiers from the weakest to the strongest claim.
const CAPABILITY_TIER_ORDER: &[CapabilityTier] = &[
    CapabilityTier::Detected,
    CapabilityTier::Parsed,
    CapabilityTier::Symbols,
    CapabilityTier::Semantic,
    CapabilityTier::Benchmarked,
];
/// Version of the built-in compiled-parser ABI contract.
const CURRENT_COMPILED_PARSER_ABI_VERSION: u32 = 1;
/// Identity of the built-in compiled-parser ABI contract.
const CURRENT_COMPILED_PARSER_ABI_ID: &str = "abi.projectatlas-compiled-parser";
/// Stable identity of the required built-in feature pack.
const DEFAULT_CORE_PACK_ID: &str = "default-core";
/// Stable identity of the optional broad-language feature pack.
const BROAD_LANGUAGE_PACK_ID: &str = "broad-language-pack";
/// Stable identity of the optional semantic feature pack.
const SEMANTIC_PACK_ID: &str = "semantic-pack";
/// Accepted mode that owns the `ObjectScript` export-container transform.
const OBJECTSCRIPT_EXPORT_XML_MODE_ID: &str = "mode.objectscript-export-xml";
/// Accepted parser reused after the `ObjectScript` export container is transformed.
const OBJECTSCRIPT_UDL_PARSER_ID: &str = "parse.objectscript-udl";
/// Stable identity of the `ObjectScript` export-container transform.
const OBJECTSCRIPT_EXPORT_XML_TRANSFORM_ID: &str = "transform.objectscript-export-xml-to-udl";
/// Version of the accepted `ObjectScript` export-container transform contract.
const OBJECTSCRIPT_EXPORT_XML_TRANSFORM_VERSION: u32 = 1;
/// Maximum original export-container bytes admitted by the transform.
const OBJECTSCRIPT_EXPORT_XML_MAX_INPUT_BYTES: u64 = 2_000_000;
/// Maximum total UDL bytes derived from one export container.
const OBJECTSCRIPT_EXPORT_XML_MAX_DERIVED_OUTPUT_BYTES: u64 = 2_000_000;
/// Maximum UDL records derived from one export container.
const OBJECTSCRIPT_EXPORT_XML_MAX_RECORDS: u32 = 1_024;
/// Maximum XML nesting depth admitted by the transform.
const OBJECTSCRIPT_EXPORT_XML_MAX_NESTING_DEPTH: u32 = 256;
/// Maximum diagnostics retained from one transform attempt.
const OBJECTSCRIPT_EXPORT_XML_MAX_DIAGNOSTICS: u32 = 256;
/// Maximum transform wall-clock duration in milliseconds.
const OBJECTSCRIPT_EXPORT_XML_DEADLINE_MS: u64 = 300_000;
/// Cancellation polling interval in milliseconds.
const OBJECTSCRIPT_EXPORT_XML_CANCELLATION_POLL_MS: u64 = 25;
/// Cancellation grace period in milliseconds.
const OBJECTSCRIPT_EXPORT_XML_CANCELLATION_GRACE_MS: u64 = 1_000;
/// Exact compact-mode fields that the accepted target may override.
const ACCEPTED_MODE_OVERRIDE_FIELDS: &[&str] = &[
    "accepted_delivery_target",
    "alias_of",
    "detection_rule_id",
    "fixture_ids",
    "required_claims",
    "achieved_claims",
    "evidence_state",
    "advertisement",
    "owner",
    "required_platforms",
];
/// Exact compact-parser fields that the accepted target may override.
const ACCEPTED_PARSER_OVERRIDE_FIELDS: &[&str] = &[
    "kind",
    "grammar_symbol",
    "tree_sitter_abi",
    "asset_id",
    "query_pack_id",
    "evidence_state",
    "advertised",
    "owner",
    "required_platforms",
];
/// Maximum accepted identifier length.
const MAX_ID_BYTES: usize = 128;
/// Maximum portable registry-path component length.
const MAX_PATH_COMPONENT_BYTES: usize = 120;
/// Maximum complete portable registry-path length.
const MAX_REGISTRY_PATH_BYTES: usize = 512;
/// Maximum evaluation parser-pack candidates admitted by one trust manifest.
const MAX_PARSER_PACK_CANDIDATES: usize = 64;
/// Maximum tracked files admitted by one evaluation parser-pack candidate.
const MAX_PARSER_PACK_FILES: usize = 4096;
/// Maximum tracked directories admitted by one evaluation parser-pack candidate.
const MAX_PARSER_PACK_DIRECTORIES: usize = 4096;
/// Maximum sortable entries admitted from one candidate directory.
const MAX_PARSER_PACK_DIRECTORY_ENTRIES: usize =
    MAX_PARSER_PACK_FILES + MAX_PARSER_PACK_DIRECTORIES;
/// Maximum nested directory depth below one candidate payload root.
const MAX_PARSER_PACK_DEPTH: usize = 64;
/// Maximum bytes retained in memory for one candidate metadata record.
const MAX_PARSER_PACK_METADATA_BYTES: u64 = 4 * 1_024 * 1_024;
/// Bounded streaming buffer used for candidate digest verification.
const PARSER_PACK_STREAM_BUFFER_BYTES: usize = 64 * 1_024;
/// Exact optional-pack resource vocabulary needed before candidate traversal.
const OPTIONAL_PACK_REQUIRED_LIMIT_FIELDS: &[&str] = &[
    "input_bytes",
    "stage_time_ms",
    "memory_bytes",
    "output_bytes",
    "executable_bytes",
    "installed_bytes",
    "startup_p95_ms",
    "active_rss_bytes",
    "cancellation_grace_ms",
];
/// Exact pack-manifest vocabulary required by the authoritative budget owner.
const OPTIONAL_PACK_REQUIRED_MANIFEST_FIELDS: &[&str] = &[
    "pack_id",
    "version",
    "platform",
    "digest",
    "license",
    "limits",
    "enforcement",
];
/// Repository-owned quality policy compiled into the deterministic generator.
const TEST_QUALITY_POLICY: &str = include_str!("../../../test-quality.toml");
/// Maximum wall time for one generated Rust process.
const GENERATED_RUST_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum retained bytes for each generated Rust process output stream.
const GENERATED_RUST_PROCESS_STREAM_LIMIT_BYTES: usize = 64 * 1_024;

/// Run one language-registry command from the repository root.
pub(crate) fn run(args: &[String], root: &Path) -> Result<(), LanguageRegistryError> {
    match args {
        [] => write_help(),
        [argument] if matches!(argument.as_str(), "--help" | "-h") => write_help(),
        [command] if command == COMMAND_CHECK => check(root),
        [command] if command == COMMAND_WRITE => write(root),
        [command] => Err(LanguageRegistryError::Usage(format!(
            "unknown language-registry command {command:?}"
        ))),
        _ => Err(LanguageRegistryError::Usage(
            "language-registry accepts exactly one command".to_string(),
        )),
    }
}

/// Print language-registry command help.
fn write_help() -> Result<(), LanguageRegistryError> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Usage: cargo projectatlas-lints language-registry <COMMAND>\n\nCommands:\n  {COMMAND_CHECK}  Validate inputs and fail on generated-output drift without writing.\n  {COMMAND_WRITE}  Validate inputs and replace only changed fixed outputs."
    )
    .map_err(LanguageRegistryError::Io)
}

/// Validate all fixed inputs and compare every generated output without writing.
fn check(root: &Path) -> Result<(), LanguageRegistryError> {
    let workspace = RegistryWorkspace::new(root)?;
    let inputs = workspace.read_inputs()?;
    let fixed = inputs.fixed();
    let generated = validate_and_generate(&inputs.lock, &fixed)?;
    let mut drift = Vec::new();
    for artifact in generated.entries() {
        let inspected = workspace.inspect_output(artifact.path)?;
        match inspected.snapshot {
            OutputSnapshot::File { bytes, .. } if bytes == artifact.bytes => {}
            OutputSnapshot::File { .. } => drift.push(format!("{} differs", artifact.path)),
            OutputSnapshot::Missing => drift.push(format!("{} is missing", artifact.path)),
        }
    }
    workspace.ensure_inputs_unchanged(&inputs)?;
    if drift.is_empty() {
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "projectatlas-lints: language registry is valid and current"
        )
        .map_err(LanguageRegistryError::Io)
    } else {
        Err(LanguageRegistryError::Drift(drift))
    }
}

/// Validate all fixed inputs, prepare every changed output, then replace fixed files.
fn write(root: &Path) -> Result<(), LanguageRegistryError> {
    let workspace = RegistryWorkspace::new(root)?;
    let inputs = workspace.read_inputs()?;
    let fixed = inputs.fixed();
    let generated = validate_and_generate(&inputs.lock, &fixed)?;
    let inspected = generated
        .entries()
        .into_iter()
        .map(|artifact| {
            workspace
                .inspect_output(artifact.path)
                .map(|output| (artifact, output))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut prepared = Vec::new();

    for (artifact, inspected) in inspected {
        if inspected.snapshot.matches_bytes(artifact.bytes) {
            continue;
        }
        prepared.push(PreparedOutput::new(inspected, artifact.bytes)?);
    }

    let changed = prepared.len();
    commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)
    })?;
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "projectatlas-lints: wrote {changed} changed language-registry output(s)"
    )
    .map_err(LanguageRegistryError::Io)
}

/// Errors raised by language-registry validation and generation.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LanguageRegistryError {
    /// Invalid command-line usage.
    #[error("{0}")]
    Usage(String),
    /// Generic stream IO error.
    #[error("io error: {0}")]
    Io(io::Error),
    /// Fixed input or output read failure.
    #[error("failed to read {path}: {source}")]
    ReadFile {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying IO error.
        source: io::Error,
    },
    /// JSON duplicate-key or typed-decoding failure.
    #[error("failed to decode {label}: {source}")]
    JsonDecode {
        /// Input identity used in the diagnostic.
        label: &'static str,
        /// Underlying JSON decoding error.
        source: serde_json::Error,
    },
    /// Historical TOON decoding failure.
    #[error("failed to decode historical runtime contract: {0}")]
    HistoricalDecode(String),
    /// Invalid typed registry state.
    #[error("language registry validation failed: {0}")]
    Validation(String),
    /// Generated Rust could not be normalized by the pinned workspace formatter.
    #[error("failed to format generated Rust for {owner}: {detail}")]
    FormatRust {
        /// Generated projection owner.
        owner: &'static str,
        /// Formatter launch, IO, status, or output diagnostic.
        detail: String,
    },
    /// A bounded generated Rust process could not complete successfully.
    #[error("generated Rust process failed for {owner}: {detail}")]
    GeneratedRustProcess {
        /// Generated Rust operation owner.
        owner: &'static str,
        /// Launch, timeout, capture, status, or output diagnostic.
        detail: String,
    },
    /// One or more generated outputs differ or are absent.
    #[error("language registry output drift: {0:?}")]
    Drift(Vec<String>),
    /// Temporary output preparation failure.
    #[error("failed to prepare generated output {path}: {source}")]
    PrepareOutput {
        /// Final output path.
        path: PathBuf,
        /// Underlying IO error.
        source: io::Error,
    },
    /// Prepared output replacement failure.
    #[error(
        "failed to replace generated output {path}: {source}; rollback failures: {rollback_failures:?}"
    )]
    PersistOutput {
        /// Final output path.
        path: PathBuf,
        /// Underlying IO error.
        source: io::Error,
        /// Any compensating rollback failures after the reported replacement failed.
        rollback_failures: Vec<String>,
    },
}

impl LanguageRegistryError {
    /// Return the process exit code owned by this command group.
    pub(crate) const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            _ => 1,
        }
    }
}

/// Fixed byte inputs consumed by the pure generator.
struct FixedInputBytes<'a> {
    /// Complete accepted capability-registry bytes.
    accepted_capability_registry: &'a [u8],
    /// Complete historical runtime-contract bytes.
    historical_runtime_contract: &'a [u8],
    /// Complete parser-pack trust-manifest bytes.
    parser_pack_trust: &'a [u8],
    /// Complete authoritative repository-intelligence contract bytes.
    repository_intelligence_contracts: &'a [u8],
    /// Streamed snapshots of every candidate payload root.
    parser_pack_payloads: &'a [CandidatePayloadSnapshot],
}

/// Owned fixed inputs captured for validation and publication guards.
struct OwnedInputBytes {
    /// Composite registry-lock bytes.
    lock: Vec<u8>,
    /// Accepted target bytes.
    accepted: Vec<u8>,
    /// Historical runtime-contract bytes.
    historical: Vec<u8>,
    /// Release-owned parser-pack trust-manifest bytes.
    parser_pack_trust: Vec<u8>,
    /// Authoritative repository-intelligence contract bytes.
    repository_intelligence_contracts: Vec<u8>,
    /// Streamed and bounded candidate payload snapshots.
    parser_pack_payloads: Vec<CandidatePayloadSnapshot>,
}

impl OwnedInputBytes {
    /// Borrow the fixed external inputs for pure validation.
    fn fixed(&self) -> FixedInputBytes<'_> {
        FixedInputBytes {
            accepted_capability_registry: &self.accepted,
            historical_runtime_contract: &self.historical,
            parser_pack_trust: &self.parser_pack_trust,
            repository_intelligence_contracts: &self.repository_intelligence_contracts,
            parser_pack_payloads: &self.parser_pack_payloads,
        }
    }

    /// Return whether all fixed input bytes are unchanged.
    fn same_state(&self, other: &Self) -> bool {
        self.lock == other.lock
            && self.accepted == other.accepted
            && self.historical == other.historical
            && self.parser_pack_trust == other.parser_pack_trust
            && self.repository_intelligence_contracts == other.repository_intelligence_contracts
            && self.parser_pack_payloads == other.parser_pack_payloads
    }
}

/// Complete streamed snapshot of one candidate payload root.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CandidatePayloadSnapshot {
    /// Stable trust-record identity.
    candidate_id: ParserPackCandidateId,
    /// Repository-relative payload root.
    payload_root: RegistryPath,
    /// Exact non-root directory inventory.
    directories: BTreeSet<RegistryPath>,
    /// Exact file inventory keyed by payload-relative path.
    files: BTreeMap<RegistryPath, CandidateFileSnapshot>,
}

/// Streamed identity and optionally retained bounded contents for one candidate file.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CandidateFileSnapshot {
    /// Exact file byte count observed through the opened stream.
    bytes: u64,
    /// SHA-256 of the exact opened file bytes.
    sha256: Sha256Digest,
    /// Bounded bytes retained only for typed metadata validation.
    metadata_bytes: Option<Vec<u8>>,
}

/// Stable identity derived only from an already-open capability handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CapabilityPathIdentity {
    /// Filesystem device or volume identity.
    device: u64,
    /// Filesystem object identity.
    inode: u64,
}

/// Bounded mutable state for one candidate payload traversal.
struct CandidateCaptureState {
    /// Exact non-root directory inventory.
    directories: BTreeSet<RegistryPath>,
    /// Exact declared file snapshots.
    files: BTreeMap<RegistryPath, CandidateFileSnapshot>,
    /// Total child entries observed during the initial traversal pass.
    entries: usize,
    /// Total streamed file bytes.
    captured_bytes: u64,
}

impl CandidateCaptureState {
    /// Create empty state for one authorized candidate.
    fn new() -> Self {
        Self {
            directories: BTreeSet::new(),
            files: BTreeMap::new(),
            entries: 0,
            captured_bytes: 0,
        }
    }

    /// Account for one child entry before classifying or opening it.
    fn record_entry(&mut self) -> Result<(), LanguageRegistryError> {
        if self.entries >= MAX_PARSER_PACK_DIRECTORY_ENTRIES {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload exceeds {MAX_PARSER_PACK_DIRECTORY_ENTRIES} total entries"
            )));
        }
        self.entries += 1;
        Ok(())
    }

    /// Account for one non-root child directory.
    fn record_directory(&mut self, path: RegistryPath) -> Result<(), LanguageRegistryError> {
        if self.directories.len() >= MAX_PARSER_PACK_DIRECTORIES {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload exceeds {MAX_PARSER_PACK_DIRECTORIES} directories"
            )));
        }
        self.directories.insert(path);
        Ok(())
    }

    /// Reserve one declared file slot before opening or streaming its bytes.
    fn reserve_file(&self) -> Result<(), LanguageRegistryError> {
        if self.files.len() >= MAX_PARSER_PACK_FILES {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload exceeds {MAX_PARSER_PACK_FILES} files"
            )));
        }
        Ok(())
    }
}

/// Fixed generated artifacts returned by the pure generator.
struct GeneratedArtifacts {
    /// Core detection and language-spec projection.
    core: Vec<u8>,
    /// Symbols parser and augmenter projection.
    symbols: Vec<u8>,
    /// CLI summary and symbol-policy projection.
    cli: Vec<u8>,
    /// Structured current/accepted capability state.
    evidence: Vec<u8>,
    /// Documentation and release language-support matrix.
    documentation: Vec<u8>,
}

impl GeneratedArtifacts {
    /// Return every fixed output in deterministic replacement order.
    fn entries(&self) -> [GeneratedArtifact<'_>; 5] {
        [
            GeneratedArtifact {
                path: CORE_OUTPUT_PATH,
                bytes: &self.core,
            },
            GeneratedArtifact {
                path: SYMBOLS_OUTPUT_PATH,
                bytes: &self.symbols,
            },
            GeneratedArtifact {
                path: CLI_OUTPUT_PATH,
                bytes: &self.cli,
            },
            GeneratedArtifact {
                path: EVIDENCE_OUTPUT_PATH,
                bytes: &self.evidence,
            },
            GeneratedArtifact {
                path: DOCUMENTATION_OUTPUT_PATH,
                bytes: &self.documentation,
            },
        ]
    }
}

/// One borrowed fixed generated artifact.
struct GeneratedArtifact<'a> {
    /// Repository-relative fixed output path.
    path: &'static str,
    /// Exact generated output bytes.
    bytes: &'a [u8],
}

/// Validated prior state of one fixed generated output.
enum OutputSnapshot {
    /// The exact final leaf is absent below a validated parent.
    Missing,
    /// The exact final leaf is a regular, non-reparse file.
    File {
        /// Exact prior bytes.
        bytes: Vec<u8>,
        /// Portable filesystem permissions retained for replacement and rollback.
        permissions: fs::Permissions,
    },
}

impl OutputSnapshot {
    /// Return whether this snapshot already contains the generated bytes.
    fn matches_bytes(&self, expected: &[u8]) -> bool {
        matches!(self, Self::File { bytes, .. } if bytes == expected)
    }

    /// Return whether two validated snapshots have identical portable state.
    fn same_state(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Missing, Self::Missing) => true,
            (
                Self::File {
                    bytes: left_bytes,
                    permissions: left_permissions,
                },
                Self::File {
                    bytes: right_bytes,
                    permissions: right_permissions,
                },
            ) => {
                left_bytes == right_bytes && permissions_equal(left_permissions, right_permissions)
            }
            (Self::Missing, Self::File { .. }) | (Self::File { .. }, Self::Missing) => false,
        }
    }

    /// Borrow existing permissions when the output already exists.
    fn permissions(&self) -> Option<&fs::Permissions> {
        match self {
            Self::Missing => None,
            Self::File { permissions, .. } => Some(permissions),
        }
    }
}

/// One output inspected through the exact path boundary.
struct InspectedOutput {
    /// Fixed repository-relative output identity.
    relative: &'static str,
    /// Exact final path below the trusted root.
    path: PathBuf,
    /// Validated prior state.
    snapshot: OutputSnapshot,
}

/// One changed output with forward and compensating rollback files prepared up front.
struct PreparedOutput {
    /// Fixed repository-relative output identity.
    relative: &'static str,
    /// Exact final path.
    path: PathBuf,
    /// Validated state captured before any mutation.
    prior: OutputSnapshot,
    /// Expected generated bytes used for post-replacement verification.
    expected: Vec<u8>,
    /// Same-directory forward replacement.
    forward: Option<NamedTempFile>,
    /// Same-directory rollback replacement for a previously existing file.
    rollback: Option<NamedTempFile>,
    /// Exact state installed by this operation before any later failure.
    committed: Option<OutputSnapshot>,
}

impl PreparedOutput {
    /// Prepare both forward and rollback artifacts before the first replacement.
    fn new(inspected: InspectedOutput, expected: &[u8]) -> Result<Self, LanguageRegistryError> {
        let forward =
            prepare_temporary_output(&inspected.path, expected, inspected.snapshot.permissions())?;
        let rollback = match &inspected.snapshot {
            OutputSnapshot::Missing => None,
            OutputSnapshot::File { bytes, permissions } => Some(prepare_temporary_output(
                &inspected.path,
                bytes,
                Some(permissions),
            )?),
        };
        Ok(Self {
            relative: inspected.relative,
            path: inspected.path,
            prior: inspected.snapshot,
            expected: expected.to_vec(),
            forward: Some(forward),
            rollback,
            committed: None,
        })
    }
}

/// Prepare and sync one same-directory replacement file.
fn prepare_temporary_output(
    path: &Path,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<NamedTempFile, LanguageRegistryError> {
    let parent = path.parent().ok_or_else(|| {
        LanguageRegistryError::Validation(format!(
            "generated output {} has no parent",
            path.display()
        ))
    })?;
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|source| LanguageRegistryError::PrepareOutput {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(bytes)
        .map_err(|source| LanguageRegistryError::PrepareOutput {
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(|source| LanguageRegistryError::PrepareOutput {
                path: path.to_path_buf(),
                source,
            })?;
    }
    #[cfg(unix)]
    if permissions.is_none() {
        use std::os::unix::fs::PermissionsExt;

        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|source| LanguageRegistryError::PrepareOutput {
                path: path.to_path_buf(),
                source,
            })?;
    }
    temporary
        .flush()
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| LanguageRegistryError::PrepareOutput {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(temporary)
}

/// Replace every prepared output and compensate earlier replacements after a reported failure.
fn commit_prepared<F>(
    workspace: &RegistryWorkspace,
    expected_inputs: &OwnedInputBytes,
    mut prepared: Vec<PreparedOutput>,
    mut replace: F,
) -> Result<(), LanguageRegistryError>
where
    F: FnMut(NamedTempFile, &Path) -> io::Result<()>,
{
    let mut committed = Vec::new();
    for index in 0..prepared.len() {
        if let Err(error) = workspace.ensure_inputs_unchanged(expected_inputs) {
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                io::Error::other(error.to_string()),
            );
        }
        let current = match workspace.inspect_output(prepared[index].relative) {
            Ok(current) => current,
            Err(error) => {
                return rollback_after_failure(
                    workspace,
                    &mut prepared,
                    &committed,
                    &mut replace,
                    index,
                    io::Error::other(error.to_string()),
                );
            }
        };
        if !current.snapshot.same_state(&prepared[index].prior) {
            let source = io::Error::other("output changed after validation and preparation");
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                source,
            );
        }
        let forward = prepared[index].forward.take().ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "forward replacement for {} was already consumed",
                prepared[index].path.display()
            ))
        })?;
        if let Err(source) = replace(forward, &prepared[index].path) {
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                source,
            );
        }
        committed.push(index);
        let installed = match workspace.inspect_output(prepared[index].relative) {
            Ok(installed) => installed,
            Err(error) => {
                return rollback_after_failure(
                    workspace,
                    &mut prepared,
                    &committed,
                    &mut replace,
                    index,
                    io::Error::other(error.to_string()),
                );
            }
        };
        if !installed.snapshot.matches_bytes(&prepared[index].expected) {
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                io::Error::other("replacement bytes differ after persistence"),
            );
        }
        prepared[index].committed = Some(installed.snapshot);
        if let Some(permissions) = prepared[index].prior.permissions()
            && let Err(source) = fs::set_permissions(&prepared[index].path, permissions.clone())
        {
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                source,
            );
        }
        let actual = match workspace.inspect_output(prepared[index].relative) {
            Ok(actual) => actual,
            Err(error) => {
                return rollback_after_failure(
                    workspace,
                    &mut prepared,
                    &committed,
                    &mut replace,
                    index,
                    io::Error::other(error.to_string()),
                );
            }
        };
        if !actual.snapshot.matches_bytes(&prepared[index].expected) {
            return rollback_after_failure(
                workspace,
                &mut prepared,
                &committed,
                &mut replace,
                index,
                io::Error::other("replacement bytes differ after persistence"),
            );
        }
        prepared[index].committed = Some(actual.snapshot);
    }
    if let Err(error) = workspace.ensure_inputs_unchanged(expected_inputs) {
        let Some(failed_index) = committed.last().copied() else {
            return Err(error);
        };
        return rollback_after_failure(
            workspace,
            &mut prepared,
            &committed,
            &mut replace,
            failed_index,
            io::Error::other(error.to_string()),
        );
    }
    Ok(())
}

/// Roll back committed outputs in reverse order and retain every compensation failure.
fn rollback_after_failure<F>(
    workspace: &RegistryWorkspace,
    prepared: &mut [PreparedOutput],
    committed: &[usize],
    replace: &mut F,
    failed_index: usize,
    source: io::Error,
) -> Result<(), LanguageRegistryError>
where
    F: FnMut(NamedTempFile, &Path) -> io::Result<()>,
{
    let mut rollback_failures = Vec::new();
    for index in committed.iter().rev().copied() {
        let output = &mut prepared[index];
        let Some(committed_state) = output.committed.as_ref() else {
            rollback_failures.push(format!(
                "{} has no recorded committed state",
                output.path.display()
            ));
            continue;
        };
        let current = match workspace.inspect_output(output.relative) {
            Ok(current) => current,
            Err(error) => {
                rollback_failures.push(error.to_string());
                continue;
            }
        };
        if !current.snapshot.same_state(committed_state) {
            rollback_failures.push(format!(
                "{} changed after replacement; refusing compensating rollback",
                output.path.display()
            ));
            continue;
        }
        let result = match &output.prior {
            OutputSnapshot::Missing => {
                workspace
                    .inspect_output(output.relative)
                    .and_then(|current| match current.snapshot {
                        OutputSnapshot::Missing => Ok(()),
                        OutputSnapshot::File { .. } => {
                            fs::remove_file(&output.path).map_err(|source| {
                                LanguageRegistryError::ReadFile {
                                    path: output.path.clone(),
                                    source,
                                }
                            })
                        }
                    })
            }
            OutputSnapshot::File { permissions, .. } => {
                let Some(rollback) = output.rollback.take() else {
                    rollback_failures.push(format!(
                        "{} has no prepared rollback artifact",
                        output.path.display()
                    ));
                    continue;
                };
                replace(rollback, &output.path)
                    .map_err(|source| LanguageRegistryError::PersistOutput {
                        path: output.path.clone(),
                        source,
                        rollback_failures: Vec::new(),
                    })
                    .and_then(|()| {
                        fs::set_permissions(&output.path, permissions.clone()).map_err(|source| {
                            LanguageRegistryError::PersistOutput {
                                path: output.path.clone(),
                                source,
                                rollback_failures: Vec::new(),
                            }
                        })
                    })
            }
        };
        if let Err(error) = result {
            rollback_failures.push(error.to_string());
            continue;
        }
        match workspace.inspect_output(output.relative) {
            Ok(restored) if restored.snapshot.same_state(&output.prior) => {}
            Ok(_) => {
                rollback_failures.push(format!("{} differs after rollback", output.path.display()));
            }
            Err(error) => rollback_failures.push(error.to_string()),
        }
    }
    Err(LanguageRegistryError::PersistOutput {
        path: prepared[failed_index].path.clone(),
        source,
        rollback_failures,
    })
}

/// Compare portable permissions without depending on platform-private ACL state.
#[cfg(unix)]
fn permissions_equal(left: &fs::Permissions, right: &fs::Permissions) -> bool {
    use std::os::unix::fs::PermissionsExt;

    left.mode() == right.mode()
}

/// Compare the portable read-only permission state on non-Unix hosts.
#[cfg(not(unix))]
fn permissions_equal(left: &fs::Permissions, right: &fs::Permissions) -> bool {
    left.readonly() == right.readonly()
}

/// Filesystem boundary for fixed registry inputs and outputs.
struct RegistryWorkspace {
    /// Canonical trusted repository root.
    root: PathBuf,
    /// Retained no-follow capability for all candidate payload traversal.
    root_dir: CapabilityDir,
}

impl RegistryWorkspace {
    /// Create a fixed workspace rooted below one canonical directory.
    fn new(root: &Path) -> Result<Self, LanguageRegistryError> {
        let root_metadata =
            fs::symlink_metadata(root).map_err(|source| LanguageRegistryError::ReadFile {
                path: root.to_path_buf(),
                source,
            })?;
        if root_metadata.file_type().is_symlink()
            || metadata_is_reparse_point(&root_metadata)
            || !root_metadata.is_dir()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "registry root {} is not a regular non-reparse directory",
                root.display()
            )));
        }
        let root = root
            .canonicalize()
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: root.to_path_buf(),
                source,
            })?;
        let root_dir = open_registry_root_capability(&root)?;
        let metadata =
            root_dir
                .dir_metadata()
                .map_err(|source| LanguageRegistryError::ReadFile {
                    path: root.clone(),
                    source,
                })?;
        if !metadata.is_dir() {
            return Err(LanguageRegistryError::Validation(format!(
                "registry root capability {} is not a directory",
                root.display()
            )));
        }
        Ok(Self { root, root_dir })
    }

    /// Read all fixed inputs after link and containment checks.
    fn read_inputs(&self) -> Result<OwnedInputBytes, LanguageRegistryError> {
        let lock = self.read_input(LOCK_PATH)?;
        let decoded_lock = decode_language_registry_lock(&lock)?;
        let repository_intelligence_contracts =
            self.read_input(REPOSITORY_INTELLIGENCE_CONTRACTS_PATH)?;
        let parser_pack_trust = self.read_input(PARSER_PACK_TRUST_PATH)?;
        verify_raw_digest(
            &parser_pack_trust,
            &decoded_lock.parser_pack_trust.raw_sha256,
            "parser-pack trust manifest",
        )?;
        let decoded_trust = decode_parser_pack_trust(&parser_pack_trust)?;
        let parser_pack_installed_byte_limit = validate_parser_pack_capture_authority(
            &decoded_lock,
            &decoded_trust,
            &repository_intelligence_contracts,
        )?;
        let parser_pack_payloads = decoded_trust
            .candidates
            .iter()
            .map(|candidate| {
                self.capture_candidate_payload(candidate, parser_pack_installed_byte_limit)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(OwnedInputBytes {
            lock,
            accepted: self.read_input(ACCEPTED_TARGET_PATH)?,
            historical: self.read_input(HISTORICAL_CONTRACT_PATH)?,
            parser_pack_trust,
            repository_intelligence_contracts,
            parser_pack_payloads,
        })
    }

    /// Re-read every fixed input and reject publication from a stale capture.
    fn ensure_inputs_unchanged(
        &self,
        expected: &OwnedInputBytes,
    ) -> Result<(), LanguageRegistryError> {
        let current = self.read_inputs()?;
        if current.same_state(expected) {
            Ok(())
        } else {
            Err(LanguageRegistryError::Validation(
                "language-registry inputs changed after validation".to_string(),
            ))
        }
    }

    /// Read one fixed input after validating every repository component.
    fn read_input(&self, relative: &'static str) -> Result<Vec<u8>, LanguageRegistryError> {
        let relative = RegistryPath::try_from(relative.to_string())?;
        let inspection = self.inspect_relative(&relative, false)?.ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "required registry input {} is absent",
                relative.as_str()
            ))
        })?;
        let path = inspection.path;
        fs::read(&path).map_err(|source| LanguageRegistryError::ReadFile { path, source })
    }

    /// Capture one declared candidate payload with streamed digests and exact inventories.
    fn capture_candidate_payload(
        &self,
        candidate: &ParserPackCandidateTrust,
        installed_byte_limit: u64,
    ) -> Result<CandidatePayloadSnapshot, LanguageRegistryError> {
        let (root, root_identity, root_diagnostic) =
            self.open_candidate_payload_root(&candidate.payload_root)?;
        let declared_files = candidate
            .inventory
            .iter()
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let mut state = CandidateCaptureState::new();
        Self::capture_candidate_directory(
            &root,
            &root_diagnostic,
            "",
            0,
            &declared_files,
            candidate.installed_bytes,
            installed_byte_limit,
            &mut state,
        )?;
        let (reopened_root, reopened_identity, _) =
            self.open_candidate_payload_root(&candidate.payload_root)?;
        let reopened_metadata =
            reopened_root
                .dir_metadata()
                .map_err(|source| LanguageRegistryError::ReadFile {
                    path: root_diagnostic.clone(),
                    source,
                })?;
        require_candidate_directory_metadata(&root_diagnostic, &reopened_metadata)?;
        if reopened_identity != root_identity {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload root {} changed identity while it was being verified",
                root_diagnostic.display()
            )));
        }
        if state.captured_bytes != candidate.installed_bytes
            || state.files.len() != declared_files.len()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload {} does not match its declared file count or installed bytes",
                candidate.candidate_id.as_str()
            )));
        }
        Ok(CandidatePayloadSnapshot {
            candidate_id: candidate.candidate_id.clone(),
            payload_root: candidate.payload_root.clone(),
            directories: state.directories,
            files: state.files,
        })
    }

    /// Walk one retained candidate directory without following links or accepting unbounded input.
    fn capture_candidate_directory(
        directory: &CapabilityDir,
        diagnostic: &Path,
        prefix: &str,
        depth: usize,
        declared_files: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
        declared_candidate_bytes: u64,
        installed_byte_limit: u64,
        state: &mut CandidateCaptureState,
    ) -> Result<(), LanguageRegistryError> {
        validate_candidate_depth(diagnostic, depth)?;
        let before =
            directory
                .dir_metadata()
                .map_err(|source| LanguageRegistryError::ReadFile {
                    path: diagnostic.to_path_buf(),
                    source,
                })?;
        require_candidate_directory_metadata(diagnostic, &before)?;
        let identity = capability_path_identity(&before);
        let initial_names = candidate_directory_names(directory, diagnostic)?;
        for name in &initial_names {
            state.record_entry()?;
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let relative = RegistryPath::try_from(relative)?;
            let child_diagnostic = diagnostic.join(name);
            let metadata = directory.symlink_metadata(name).map_err(|source| {
                LanguageRegistryError::ReadFile {
                    path: child_diagnostic.clone(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() {
                return Err(LanguageRegistryError::Validation(format!(
                    "candidate payload path {} is a link or reparse point",
                    child_diagnostic.display()
                )));
            }
            if metadata.is_dir() {
                let child = open_candidate_directory(directory, name, &child_diagnostic)?;
                let child_metadata =
                    child
                        .dir_metadata()
                        .map_err(|source| LanguageRegistryError::ReadFile {
                            path: child_diagnostic.clone(),
                            source,
                        })?;
                require_candidate_directory_metadata(&child_diagnostic, &child_metadata)?;
                let child_identity = capability_path_identity(&child_metadata);
                state.record_directory(relative.clone())?;
                Self::capture_candidate_directory(
                    &child,
                    &child_diagnostic,
                    relative.as_str(),
                    depth + 1,
                    declared_files,
                    declared_candidate_bytes,
                    installed_byte_limit,
                    state,
                )?;
                let reopened = open_candidate_directory(directory, name, &child_diagnostic)?;
                let reopened_metadata =
                    reopened
                        .dir_metadata()
                        .map_err(|source| LanguageRegistryError::ReadFile {
                            path: child_diagnostic.clone(),
                            source,
                        })?;
                require_candidate_directory_metadata(&child_diagnostic, &reopened_metadata)?;
                if capability_path_identity(&reopened_metadata) != child_identity {
                    return Err(LanguageRegistryError::Validation(format!(
                        "candidate directory {} changed identity while it was being verified",
                        child_diagnostic.display()
                    )));
                }
            } else if metadata.is_file() {
                state.reserve_file()?;
                let declared = declared_files.get(&relative).ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "candidate payload contains undeclared file {}",
                        relative.as_str()
                    ))
                })?;
                let remaining_candidate_bytes = declared_candidate_bytes
                    .min(installed_byte_limit)
                    .checked_sub(state.captured_bytes)
                    .ok_or_else(|| {
                        LanguageRegistryError::Validation(
                            "candidate payload exceeded its authorized installed-byte ceiling"
                                .to_string(),
                        )
                    })?;
                let snapshot = capture_candidate_file(
                    directory,
                    name,
                    &child_diagnostic,
                    declared.role.requires_metadata_bytes(),
                    declared.bytes,
                    remaining_candidate_bytes,
                    declared_candidate_bytes.min(installed_byte_limit),
                    state,
                )?;
                state.files.insert(relative, snapshot);
            } else {
                return Err(LanguageRegistryError::Validation(format!(
                    "candidate payload path {} is not a regular file or directory",
                    child_diagnostic.display()
                )));
            }
        }
        let final_names = candidate_directory_names(directory, diagnostic)?;
        let after = directory
            .dir_metadata()
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
        require_candidate_directory_metadata(diagnostic, &after)?;
        require_candidate_directory_stable(
            diagnostic,
            &initial_names,
            &final_names,
            identity,
            capability_path_identity(&after),
        )
    }

    /// Resolve one exact candidate root while retaining every opened directory capability.
    fn open_candidate_payload_root(
        &self,
        relative: &RegistryPath,
    ) -> Result<(CapabilityDir, CapabilityPathIdentity, PathBuf), LanguageRegistryError> {
        let mut opened = None::<CapabilityDir>;
        let mut diagnostic = self.root.clone();
        for expected in relative.as_str().split('/') {
            let parent = opened.as_ref().unwrap_or(&self.root_dir);
            let actual = exact_candidate_child_name(parent, expected, &diagnostic)?;
            diagnostic.push(&actual);
            let child = open_candidate_directory(parent, &actual, &diagnostic)?;
            let metadata =
                child
                    .dir_metadata()
                    .map_err(|source| LanguageRegistryError::ReadFile {
                        path: diagnostic.clone(),
                        source,
                    })?;
            require_candidate_directory_metadata(&diagnostic, &metadata)?;
            opened = Some(child);
        }
        let directory = opened.ok_or_else(|| {
            LanguageRegistryError::Validation(
                "candidate payload root has no components".to_string(),
            )
        })?;
        let metadata =
            directory
                .dir_metadata()
                .map_err(|source| LanguageRegistryError::ReadFile {
                    path: diagnostic.clone(),
                    source,
                })?;
        require_candidate_directory_metadata(&diagnostic, &metadata)?;
        Ok((directory, capability_path_identity(&metadata), diagnostic))
    }

    /// Inspect one fixed output before any read, comparison, or mutation.
    fn inspect_output(
        &self,
        relative: &'static str,
    ) -> Result<InspectedOutput, LanguageRegistryError> {
        let validated = RegistryPath::try_from(relative.to_string())?;
        match self.inspect_relative(&validated, true)? {
            None => Ok(InspectedOutput {
                relative,
                path: self.root.join(validated.as_str()),
                snapshot: OutputSnapshot::Missing,
            }),
            Some(inspection) => {
                let bytes = fs::read(&inspection.path).map_err(|source| {
                    LanguageRegistryError::ReadFile {
                        path: inspection.path.clone(),
                        source,
                    }
                })?;
                Ok(InspectedOutput {
                    relative,
                    path: inspection.path,
                    snapshot: OutputSnapshot::File {
                        bytes,
                        permissions: inspection.metadata.permissions(),
                    },
                })
            }
        }
    }

    /// Walk one portable relative path by exact directory-entry spelling.
    fn inspect_relative(
        &self,
        relative: &RegistryPath,
        allow_missing_leaf: bool,
    ) -> Result<Option<PathInspection>, LanguageRegistryError> {
        let mut current = self.root.clone();
        let components = relative.as_str().split('/').collect::<Vec<_>>();
        for (index, expected) in components.iter().enumerate() {
            let final_component = index + 1 == components.len();
            let exact = Self::exact_child(&current, expected)?;
            let Some(path) = exact else {
                if final_component && allow_missing_leaf {
                    return Ok(None);
                }
                let missing = current.join(expected);
                return Err(LanguageRegistryError::ReadFile {
                    path: missing,
                    source: io::Error::new(
                        io::ErrorKind::NotFound,
                        "exact path component is absent",
                    ),
                });
            };
            current = path;
            let metadata = fs::symlink_metadata(&current).map_err(|source| {
                LanguageRegistryError::ReadFile {
                    path: current.clone(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
                return Err(LanguageRegistryError::Validation(format!(
                    "registry path {} traverses a link or reparse point",
                    current.display()
                )));
            }
            if !final_component && !metadata.is_dir() {
                return Err(LanguageRegistryError::Validation(format!(
                    "registry path component {} is not a directory",
                    current.display()
                )));
            }
            if final_component {
                if !metadata.is_file() {
                    return Err(LanguageRegistryError::Validation(format!(
                        "registry path {} is not a regular file",
                        current.display()
                    )));
                }
                return Ok(Some(PathInspection {
                    path: current,
                    metadata,
                }));
            }
        }
        Err(LanguageRegistryError::Validation(
            "registry path has no components".to_string(),
        ))
    }

    /// Find one exact child and reject a case-insensitive near match.
    fn exact_child(
        parent: &Path,
        expected: &str,
    ) -> Result<Option<PathBuf>, LanguageRegistryError> {
        let entries = fs::read_dir(parent).map_err(|source| LanguageRegistryError::ReadFile {
            path: parent.to_path_buf(),
            source,
        })?;
        let mut wrong_case = None;
        for entry in entries {
            let entry = entry.map_err(|source| LanguageRegistryError::ReadFile {
                path: parent.to_path_buf(),
                source,
            })?;
            let name = entry.file_name();
            if name == std::ffi::OsStr::new(expected) {
                return Ok(Some(entry.path()));
            }
            if name
                .to_str()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            {
                wrong_case = name.to_str().map(ToOwned::to_owned);
            }
        }
        if let Some(actual) = wrong_case {
            Err(LanguageRegistryError::Validation(format!(
                "registry path component {expected:?} has wrong filesystem spelling {actual:?} below {}",
                parent.display()
            )))
        } else {
            Ok(None)
        }
    }
}

/// Open the canonical repository root once without following its final component.
fn open_registry_root_capability(root: &Path) -> Result<CapabilityDir, LanguageRegistryError> {
    let mut options = CapabilityOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(windows)]
    options.maybe_dir(true);
    let file = CapabilityFile::open_ambient_with(root, &options, ambient_authority()).map_err(
        |source| LanguageRegistryError::ReadFile {
            path: root.to_path_buf(),
            source,
        },
    )?;
    let metadata = file
        .metadata()
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: root.to_path_buf(),
            source,
        })?;
    require_candidate_directory_metadata(root, &metadata)?;
    Ok(CapabilityDir::from_std_file(file.into_std()))
}

/// Enumerate one directory before choosing an exact candidate-root component.
fn exact_candidate_child_name(
    parent: &CapabilityDir,
    expected: &str,
    diagnostic: &Path,
) -> Result<String, LanguageRegistryError> {
    let entries = parent
        .entries()
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
    let mut names = Vec::<OsString>::new();
    for entry in entries {
        if names.len() >= MAX_PARSER_PACK_DIRECTORY_ENTRIES {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate root parent {} exceeds {MAX_PARSER_PACK_DIRECTORY_ENTRIES} entries",
                diagnostic.display()
            )));
        }
        let entry = entry.map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
        names.push(entry.file_name());
    }
    let matches = names
        .iter()
        .filter_map(|name| name.to_str())
        .filter(|actual| actual.eq_ignore_ascii_case(expected))
        .collect::<Vec<_>>();
    let [actual] = matches.as_slice() else {
        return Err(LanguageRegistryError::Validation(format!(
            "required candidate payload component {expected:?} has {} ASCII-case-insensitive matches below {}",
            matches.len(),
            diagnostic.display()
        )));
    };
    if *actual != expected {
        return Err(LanguageRegistryError::Validation(format!(
            "candidate payload component {expected:?} has wrong filesystem spelling {actual:?} below {}",
            diagnostic.display()
        )));
    }
    Ok((*actual).to_string())
}

/// Open one candidate child directory relative to a retained parent without following links.
#[cfg(not(windows))]
fn open_candidate_directory(
    parent: &CapabilityDir,
    name: &str,
    diagnostic: &Path,
) -> Result<CapabilityDir, LanguageRegistryError> {
    parent
        .open_dir_nofollow(name)
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })
}

/// Open one Windows candidate child directory through a no-follow file capability.
#[cfg(windows)]
fn open_candidate_directory(
    parent: &CapabilityDir,
    name: &str,
    diagnostic: &Path,
) -> Result<CapabilityDir, LanguageRegistryError> {
    let mut options = CapabilityOpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No);
    let file =
        parent
            .open_with(name, &options)
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
    Ok(CapabilityDir::from_std_file(file.into_std()))
}

/// Collect one exact, portable, bounded candidate child-name set in sorted order.
fn candidate_directory_names(
    directory: &CapabilityDir,
    diagnostic: &Path,
) -> Result<Vec<String>, LanguageRegistryError> {
    let entries = directory
        .entries()
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
    let mut names = Vec::new();
    for entry in entries {
        if names.len() >= MAX_PARSER_PACK_DIRECTORY_ENTRIES {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate directory {} exceeds {MAX_PARSER_PACK_DIRECTORY_ENTRIES} entries",
                diagnostic.display()
            )));
        }
        let entry = entry.map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
        let name = entry.file_name().into_string().map_err(|value| {
            LanguageRegistryError::Validation(format!(
                "candidate payload contains a non-UTF-8 path component {} below {}",
                Path::new(&value).display(),
                diagnostic.display()
            ))
        })?;
        RegistryPath::try_from(name.clone())?;
        names.push(name);
    }
    names.sort_unstable();
    for pair in names.windows(2) {
        if pair[0].eq_ignore_ascii_case(&pair[1]) {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate directory {} contains ASCII-case-colliding entries {:?} and {:?}",
                diagnostic.display(),
                pair[0],
                pair[1]
            )));
        }
    }
    Ok(names)
}

/// Require one candidate traversal depth to remain within the hard recursion ceiling.
fn validate_candidate_depth(diagnostic: &Path, depth: usize) -> Result<(), LanguageRegistryError> {
    if depth > MAX_PARSER_PACK_DEPTH {
        Err(LanguageRegistryError::Validation(format!(
            "candidate directory {} exceeds maximum depth {MAX_PARSER_PACK_DEPTH}",
            diagnostic.display()
        )))
    } else {
        Ok(())
    }
}

/// Require a retained directory handle to preserve its identity and exact child-name set.
fn require_candidate_directory_stable(
    diagnostic: &Path,
    initial_names: &[String],
    final_names: &[String],
    initial_identity: CapabilityPathIdentity,
    final_identity: CapabilityPathIdentity,
) -> Result<(), LanguageRegistryError> {
    if initial_names == final_names && initial_identity == final_identity {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "candidate directory {} changed while it was being verified",
            diagnostic.display()
        )))
    }
}

/// Derive a portable identity only from metadata returned by an opened capability handle.
fn capability_path_identity(metadata: &CapabilityMetadata) -> CapabilityPathIdentity {
    CapabilityPathIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// Require one opened candidate directory handle to identify a directory, not a link.
fn require_candidate_directory_metadata(
    diagnostic: &Path,
    metadata: &CapabilityMetadata,
) -> Result<(), LanguageRegistryError> {
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "candidate payload path {} is not an opened non-link directory",
            diagnostic.display()
        )))
    }
}

/// Require one opened candidate file handle to retain the trusted type, link count, and length.
fn require_candidate_file_metadata(
    diagnostic: &Path,
    metadata: &CapabilityMetadata,
    declared_bytes: u64,
) -> Result<(), LanguageRegistryError> {
    if metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.nlink() == 1
        && metadata.len() == declared_bytes
    {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "candidate payload file {} is not a single-link regular file of declared length {declared_bytes}",
            diagnostic.display()
        )))
    }
}

/// Advance exact streamed-byte counts without crossing file or candidate ceilings.
fn record_candidate_stream_bytes(
    diagnostic: &Path,
    file_bytes: u64,
    read_bytes: u64,
    declared_bytes: u64,
    remaining_candidate_bytes: u64,
    authorized_candidate_bytes: u64,
    state: &mut CandidateCaptureState,
) -> Result<u64, LanguageRegistryError> {
    let next_file_bytes = file_bytes.checked_add(read_bytes).ok_or_else(|| {
        LanguageRegistryError::Validation(format!(
            "candidate file byte count overflowed for {}",
            diagnostic.display()
        ))
    })?;
    let next_candidate_bytes = state
        .captured_bytes
        .checked_add(read_bytes)
        .ok_or_else(|| {
            LanguageRegistryError::Validation(
                "candidate payload captured-byte count overflowed".to_string(),
            )
        })?;
    if next_file_bytes > declared_bytes
        || next_file_bytes > remaining_candidate_bytes
        || next_candidate_bytes > authorized_candidate_bytes
    {
        return Err(LanguageRegistryError::Validation(format!(
            "candidate file {} exceeded its declared streaming byte ceiling {declared_bytes}",
            diagnostic.display()
        )));
    }
    state.captured_bytes = next_candidate_bytes;
    Ok(next_file_bytes)
}

/// Require a completed file stream and both capability identities to remain exact.
fn require_candidate_file_stable(
    diagnostic: &Path,
    streamed_bytes: u64,
    declared_bytes: u64,
    initial_identity: CapabilityPathIdentity,
    final_identity: CapabilityPathIdentity,
    reopened_identity: CapabilityPathIdentity,
) -> Result<(), LanguageRegistryError> {
    if streamed_bytes == declared_bytes
        && final_identity == initial_identity
        && reopened_identity == initial_identity
    {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "candidate file {} changed while it was being verified",
            diagnostic.display()
        )))
    }
}

/// Stream one capability-opened candidate file while guarding its exact identity and ceilings.
fn capture_candidate_file(
    parent: &CapabilityDir,
    name: &str,
    diagnostic: &Path,
    retain_metadata: bool,
    declared_bytes: u64,
    remaining_candidate_bytes: u64,
    authorized_candidate_bytes: u64,
    state: &mut CandidateCaptureState,
) -> Result<CandidateFileSnapshot, LanguageRegistryError> {
    if declared_bytes > remaining_candidate_bytes {
        return Err(LanguageRegistryError::Validation(format!(
            "candidate file {} exceeds or differs from its declared byte ceiling {declared_bytes}",
            diagnostic.display()
        )));
    }
    if retain_metadata && declared_bytes > MAX_PARSER_PACK_METADATA_BYTES {
        return Err(LanguageRegistryError::Validation(format!(
            "candidate metadata file {} exceeds {MAX_PARSER_PACK_METADATA_BYTES} bytes",
            diagnostic.display()
        )));
    }
    let mut options = CapabilityOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file =
        parent
            .open_with(name, &options)
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
    let before = file
        .metadata()
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
    require_candidate_file_metadata(diagnostic, &before, declared_bytes)?;
    let identity = capability_path_identity(&before);
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut retained = if retain_metadata {
        Some(Vec::with_capacity(
            usize::try_from(declared_bytes).map_err(|source| {
                LanguageRegistryError::Validation(format!(
                    "candidate metadata length does not fit memory index for {}: {source}",
                    diagnostic.display()
                ))
            })?,
        ))
    } else {
        None
    };
    let mut buffer = vec![0_u8; PARSER_PACK_STREAM_BUFFER_BYTES].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        let read = u64::try_from(read).map_err(|source| {
            LanguageRegistryError::Validation(format!(
                "candidate read length does not fit u64 for {}: {source}",
                diagnostic.display()
            ))
        })?;
        total = record_candidate_stream_bytes(
            diagnostic,
            total,
            read,
            declared_bytes,
            remaining_candidate_bytes,
            authorized_candidate_bytes,
            state,
        )?;
        let read = usize::try_from(read).map_err(|source| {
            LanguageRegistryError::Validation(format!(
                "candidate read length does not fit memory index for {}: {source}",
                diagnostic.display()
            ))
        })?;
        hasher.update(&buffer[..read]);
        if let Some(bytes) = &mut retained {
            bytes.extend_from_slice(&buffer[..read]);
        }
    }
    let after = file
        .metadata()
        .map_err(|source| LanguageRegistryError::ReadFile {
            path: diagnostic.to_path_buf(),
            source,
        })?;
    require_candidate_file_metadata(diagnostic, &after, declared_bytes)?;
    let reopened =
        parent
            .open_with(name, &options)
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
    let reopened_metadata =
        reopened
            .metadata()
            .map_err(|source| LanguageRegistryError::ReadFile {
                path: diagnostic.to_path_buf(),
                source,
            })?;
    require_candidate_file_metadata(diagnostic, &reopened_metadata, declared_bytes)?;
    require_candidate_file_stable(
        diagnostic,
        total,
        declared_bytes,
        identity,
        capability_path_identity(&after),
        capability_path_identity(&reopened_metadata),
    )?;
    Ok(CandidateFileSnapshot {
        bytes: total,
        sha256: Sha256Digest(format!("{:x}", hasher.finalize())),
        metadata_bytes: retained,
    })
}

/// Exact path and metadata returned by the filesystem boundary.
struct PathInspection {
    /// Exact path built from directory entries.
    path: PathBuf,
    /// Link-aware metadata for the final regular file.
    metadata: fs::Metadata,
}

/// Return whether metadata identifies a Windows reparse point.
#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Return false for reparse points on platforms without that file attribute.
#[cfg(not(windows))]
const fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

/// Portable ASCII-only registry artifact path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct RegistryPath(String);

impl RegistryPath {
    /// Borrow the validated repository-relative spelling.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RegistryPath {
    type Error = LanguageRegistryError;

    /// Validate one portable repository-relative artifact path.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_registry_path(&value)?;
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for RegistryPath {
    /// Deserialize and validate a portable path without host path semantics.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(de::Error::custom)
    }
}

/// Validate one portable ASCII-only registry path.
fn validate_registry_path(value: &str) -> Result<(), LanguageRegistryError> {
    if value.is_empty()
        || value.len() > MAX_REGISTRY_PATH_BYTES
        || !value.is_ascii()
        || value.starts_with(['/', '\\'])
        || value.contains('\\')
        || value.contains("//")
        || value.ends_with('/')
    {
        return Err(LanguageRegistryError::Validation(format!(
            "nonportable registry path {value:?}"
        )));
    }
    let invalid = |character: char| {
        character.is_ascii_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
    };
    for component in value.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.len() > MAX_PATH_COMPONENT_BYTES
            || component.ends_with(['.', ' '])
            || component.chars().any(invalid)
            || is_windows_device_name(component)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "nonportable registry path component {component:?} in {value:?}"
            )));
        }
    }
    Ok(())
}

/// Return whether a path component is a reserved Windows device basename.
fn is_windows_device_name(component: &str) -> bool {
    let basename = component
        .split_once('.')
        .map_or(component, |(basename, _)| basename)
        .to_ascii_uppercase();
    matches!(
        basename.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || basename
        .strip_prefix("COM")
        .or_else(|| basename.strip_prefix("LPT"))
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

/// Define a validated responsibility-prefixed identifier newtype.
macro_rules! validated_id {
    ($name:ident, $prefix:literal, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        struct $name(String);

        impl $name {
            #[doc = concat!("Borrow the validated `", $prefix, "` identifier.")]
            fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = LanguageRegistryError;

            #[doc = concat!("Validate one `", $prefix, "` identifier.")]
            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_identifier(&value, $prefix)?;
                Ok(Self(value))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            #[doc = concat!("Deserialize and validate one `", $prefix, "` identifier.")]
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_from(value).map_err(de::Error::custom)
            }
        }
    };
}

validated_id!(
    RegistryId,
    "registry.",
    "Composite language-registry identity."
);
validated_id!(ModeId, "mode.", "Language mode identity.");
validated_id!(ParserId, "parser.", "Built-in parser component identity.");
validated_id!(DetectionRuleId, "detect.", "Detection-rule identity.");
validated_id!(FixtureId, "fixture.", "Language fixture identity.");
validated_id!(AssetId, "asset.", "Parser asset identity.");
validated_id!(ParserAbiId, "abi.", "Versioned parser ABI identity.");
validated_id!(
    EmbeddedAdapterId,
    "embedded.",
    "Embedded-language adapter identity."
);
validated_id!(QueryPackId, "queries.", "Extraction query-pack identity.");
validated_id!(
    SemanticProviderId,
    "provider.",
    "Semantic-provider identity."
);
validated_id!(EvidenceId, "evidence.", "Verification evidence identity.");

/// Stable parser-pack candidate identity shared with optional-pack readiness.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ParserPackCandidateId(String);

impl ParserPackCandidateId {
    /// Borrow the validated shared candidate identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ParserPackCandidateId {
    type Error = LanguageRegistryError;

    /// Validate one lowercase, responsibility-named candidate slug.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid = !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && !value.as_bytes().windows(2).any(|pair| pair == b"--")
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(LanguageRegistryError::Validation(format!(
                "invalid parser-pack candidate identity {value:?}"
            )))
        }
    }
}

impl<'de> Deserialize<'de> for ParserPackCandidateId {
    /// Deserialize and validate one shared parser-pack candidate identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(de::Error::custom)
    }
}

/// Validate a bounded lowercase ASCII responsibility-prefixed identifier.
fn validate_identifier(value: &str, prefix: &str) -> Result<(), LanguageRegistryError> {
    let suffix = value.strip_prefix(prefix).unwrap_or_default();
    let valid = !suffix.is_empty()
        && value.len() <= MAX_ID_BYTES
        && suffix
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && suffix
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !suffix
            .as_bytes()
            .windows(2)
            .any(|pair| matches!(pair, [b'.' | b'_' | b'-', b'.' | b'_' | b'-']))
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "invalid {prefix} identifier {value:?}"
        )))
    }
}

/// Public language mode spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct PublicMode(String);

impl PublicMode {
    /// Borrow the validated public mode spelling.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublicMode {
    /// Deserialize and validate one public mode spelling.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let valid = !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'#')
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(de::Error::custom(format!("invalid public mode {value:?}")))
        }
    }
}

/// Lowercase hexadecimal SHA-256 digest.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct Sha256Digest(String);

impl Sha256Digest {
    /// Borrow the validated lowercase hexadecimal digest.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    /// Deserialize and validate one SHA-256 digest.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("expected 64 lowercase hexadecimal bytes"))
        }
    }
}

/// Exact historical Git object identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct RevisionId(String);

impl RevisionId {
    /// Borrow the validated revision identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RevisionId {
    /// Deserialize and validate one 40-character Git object identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() == 40
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("expected a 40-character Git object ID"))
        }
    }
}

/// Exact historical release identifier.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ReleaseId(String);

impl ReleaseId {
    /// Borrow the validated release identifier.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ReleaseId {
    /// Deserialize a bounded printable release identifier.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value
            .strip_prefix('v')
            .and_then(|suffix| suffix.as_bytes().first())
            .is_some_and(u8::is_ascii_digit)
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("invalid release identifier"))
        }
    }
}

/// Bounded upstream source identity for a parser asset.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ParserAssetSource(String);

impl ParserAssetSource {
    /// Borrow the validated upstream source identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserAssetSource {
    /// Deserialize a nonempty, bounded, control-free source identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if metadata_text_is_valid(&value, MAX_REGISTRY_PATH_BYTES) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("invalid parser asset source"))
        }
    }
}

/// Bounded upstream version identity for a parser asset.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ParserAssetVersion(String);

impl ParserAssetVersion {
    /// Borrow the validated upstream version identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserAssetVersion {
    /// Deserialize a nonempty, bounded, control-free version identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if metadata_text_is_valid(&value, MAX_ID_BYTES) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("invalid parser asset version"))
        }
    }
}

/// Bounded declared license identity for a parser asset.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ParserAssetLicense(String);

impl ParserAssetLicense {
    /// Borrow the validated license identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserAssetLicense {
    /// Deserialize a nonempty, bounded, trimmed, control-free license identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if metadata_text_is_valid(&value, MAX_ID_BYTES) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("invalid parser asset license"))
        }
    }
}

/// Bounded external parser ABI version declared by the accepted target.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct ParserAbiVersion(String);

impl ParserAbiVersion {
    /// Borrow the validated external parser ABI version.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ParserAbiVersion {
    /// Deserialize a nonempty, bounded, control-free ABI version.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if metadata_text_is_valid(&value, MAX_ID_BYTES) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom("invalid parser ABI version"))
        }
    }
}

/// Return whether one metadata value is nonempty, bounded, trimmed, and control-free.
fn metadata_text_is_valid(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// Accepted target registry identity, which follows its external contract spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct AcceptedRegistryId(String);

impl AcceptedRegistryId {
    /// Borrow the accepted external registry identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AcceptedRegistryId {
    /// Deserialize a bounded accepted-registry identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let valid = !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(de::Error::custom(format!(
                "invalid accepted-registry identity {value:?}"
            )))
        }
    }
}

/// Feature-pack identity with externally stable spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct PackId(String);

impl PackId {
    /// Borrow the feature-pack identifier.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackId {
    /// Deserialize one closed `ProjectAtlas` feature-pack identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if matches!(
            value.as_str(),
            "default-core" | "broad-language-pack" | "semantic-pack"
        ) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom(format!(
                "unsupported feature-pack identifier {value:?}"
            )))
        }
    }
}

/// Closed lock-file format identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum RegistryFormat {
    /// Composite `ProjectAtlas` language-runtime lock.
    #[serde(rename = "projectatlas.language-registry-lock")]
    LanguageRegistryLock,
}

/// Current feature-pack ownership state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PackOwnership {
    /// Required built-in runtime behavior.
    DefaultCore,
    /// Explicitly installed optional behavior.
    Optional,
}

impl PackOwnership {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::DefaultCore => "default-core",
            Self::Optional => "optional",
        }
    }
}

/// Runtime boundary used by a feature pack.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PackRuntime {
    /// Runs in the default `ProjectAtlas` process.
    InProcess,
    /// Runs in a supervised worker process.
    SupervisedWorker,
}

impl PackRuntime {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::InProcess => "in-process",
            Self::SupervisedWorker => "supervised-worker",
        }
    }
}

/// One pack boundary declared by the composite lock.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryPack {
    /// Stable pack identity.
    pack_id: PackId,
    /// Required or optional ownership.
    ownership: PackOwnership,
    /// Runtime process boundary.
    runtime: PackRuntime,
}

/// Case policy for a detection rule.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CasePolicy {
    /// Compare the spelling exactly.
    Sensitive,
    /// Compare ASCII letters without case.
    AsciiInsensitive,
}

impl CasePolicy {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Sensitive => "sensitive",
            Self::AsciiInsensitive => "ascii-insensitive",
        }
    }
}

/// Closed precedence class for a built-in content detector.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ContentDetectionKind {
    /// Interpreter declaration at the start of a text file.
    Shebang,
    /// Bounded deterministic signature in file content.
    ContentSignature,
    /// Bounded repository-context discriminator.
    ProjectContext,
}

impl ContentDetectionKind {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Shebang => "shebang",
            Self::ContentSignature => "content-signature",
            Self::ProjectContext => "project-context",
        }
    }
}

/// Closed built-in content detector with an executable default-core matcher.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum BuiltInContentDetector {
    /// Python-family shebang.
    #[serde(rename = "content.shebang.python")]
    ShebangPython,
    /// Shell-family shebang.
    #[serde(rename = "content.shebang.shell")]
    ShebangShell,
    /// JavaScript-family shebang.
    #[serde(rename = "content.shebang.javascript")]
    ShebangJavascript,
    /// Ruby-family shebang.
    #[serde(rename = "content.shebang.ruby")]
    ShebangRuby,
    /// Perl-family shebang.
    #[serde(rename = "content.shebang.perl")]
    ShebangPerl,
    /// PHP opening-tag signature.
    #[serde(rename = "content.signature.php")]
    SignaturePhp,
    /// XML declaration signature.
    #[serde(rename = "content.signature.xml")]
    SignatureXml,
    /// Docker build-definition context.
    #[serde(rename = "content.context.docker-build")]
    ContextDockerBuild,
}

/// Complete closed matcher inventory consumed by registry validation and generation.
const BUILT_IN_CONTENT_DETECTORS: [BuiltInContentDetector; 8] = [
    BuiltInContentDetector::ShebangPython,
    BuiltInContentDetector::ShebangShell,
    BuiltInContentDetector::ShebangJavascript,
    BuiltInContentDetector::ShebangRuby,
    BuiltInContentDetector::ShebangPerl,
    BuiltInContentDetector::SignaturePhp,
    BuiltInContentDetector::SignatureXml,
    BuiltInContentDetector::ContextDockerBuild,
];

impl BuiltInContentDetector {
    /// Return the externally stable detector identity.
    const fn contract_id(self) -> &'static str {
        match self {
            Self::ShebangPython => "content.shebang.python",
            Self::ShebangShell => "content.shebang.shell",
            Self::ShebangJavascript => "content.shebang.javascript",
            Self::ShebangRuby => "content.shebang.ruby",
            Self::ShebangPerl => "content.shebang.perl",
            Self::SignaturePhp => "content.signature.php",
            Self::SignatureXml => "content.signature.xml",
            Self::ContextDockerBuild => "content.context.docker-build",
        }
    }

    /// Return the only valid precedence stage for this matcher.
    const fn detection_kind(self) -> ContentDetectionKind {
        match self {
            Self::ShebangPython
            | Self::ShebangShell
            | Self::ShebangJavascript
            | Self::ShebangRuby
            | Self::ShebangPerl => ContentDetectionKind::Shebang,
            Self::SignaturePhp | Self::SignatureXml => ContentDetectionKind::ContentSignature,
            Self::ContextDockerBuild => ContentDetectionKind::ProjectContext,
        }
    }

    /// Return the only valid current base-language mode for this matcher.
    const fn mode_id(self) -> &'static str {
        match self {
            Self::ShebangPython => "mode.python",
            Self::ShebangShell => "mode.shell",
            Self::ShebangJavascript => "mode.javascript",
            Self::ShebangRuby => "mode.ruby",
            Self::ShebangPerl => "mode.perl",
            Self::SignaturePhp => "mode.php",
            Self::SignatureXml => "mode.xml",
            Self::ContextDockerBuild => "mode.dockerfile",
        }
    }

    /// Return the generated Rust enum variant.
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::ShebangPython => "ShebangPython",
            Self::ShebangShell => "ShebangShell",
            Self::ShebangJavascript => "ShebangJavascript",
            Self::ShebangRuby => "ShebangRuby",
            Self::ShebangPerl => "ShebangPerl",
            Self::SignaturePhp => "SignaturePhp",
            Self::SignatureXml => "SignatureXml",
            Self::ContextDockerBuild => "ContextDockerBuild",
        }
    }
}

/// Closed semantic refinement supported by the current generated detector.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SemanticMode {
    /// Kubernetes resource semantics.
    Kubernetes,
    /// Kustomize configuration semantics.
    Kustomize,
}

/// Complete closed semantic-refinement inventory consumed by registry validation.
const SEMANTIC_MODES: [SemanticMode; 2] = [SemanticMode::Kubernetes, SemanticMode::Kustomize];

impl SemanticMode {
    /// Return the generated Rust enum variant.
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::Kubernetes => "Kubernetes",
            Self::Kustomize => "Kustomize",
        }
    }

    /// Return the public semantic-mode spelling.
    const fn public_mode(self) -> &'static str {
        match self {
            Self::Kubernetes => "kubernetes",
            Self::Kustomize => "kustomize",
        }
    }

    /// Return the only compatible current base-language mode.
    const fn base_mode_id(self) -> &'static str {
        match self {
            Self::Kubernetes | Self::Kustomize => "mode.yaml",
        }
    }
}

/// One semantic-mode/base-language compatibility row.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticModeRule {
    /// Closed semantic refinement.
    mode: SemanticMode,
    /// Compatible current base-language mode.
    base_mode_id: ModeId,
}

/// One closed current-runtime language detection rule.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "layer", rename_all = "kebab-case", deny_unknown_fields)]
enum DetectionRule {
    /// Match one exact repository basename.
    ExactFilename {
        /// Stable rule identity.
        id: DetectionRuleId,
        /// Exact filename spelling.
        file_name: String,
        /// Filename case policy.
        case: CasePolicy,
        /// Whether the broad scanner sees this rule directly.
        scanner_visible: bool,
        /// Selected current-runtime mode.
        mode_id: ModeId,
    },
    /// Match a multi-suffix extension before ordinary extensions.
    CompoundExtension {
        /// Stable rule identity.
        id: DetectionRuleId,
        /// Compound extension including its leading dot.
        extension: String,
        /// Extension case policy.
        case: CasePolicy,
        /// Case policy used while recognizing the compound suffix in a path.
        path_suffix_case: CasePolicy,
        /// Whether the broad scanner sees this rule directly.
        scanner_visible: bool,
        /// Selected current-runtime mode.
        mode_id: ModeId,
    },
    /// Match one ordinary extension.
    Extension {
        /// Stable rule identity.
        id: DetectionRuleId,
        /// Extension including its leading dot.
        extension: String,
        /// Extension case policy.
        case: CasePolicy,
        /// Whether the broad scanner sees this rule directly.
        scanner_visible: bool,
        /// Selected current-runtime mode.
        mode_id: ModeId,
    },
    /// Select a mode through one bounded built-in content detector.
    Content {
        /// Stable rule identity.
        id: DetectionRuleId,
        /// Stable built-in detector identity; this is not a plugin or regex payload.
        detector_id: BuiltInContentDetector,
        /// Detector precedence class.
        detector_kind: ContentDetectionKind,
        /// Whether the broad scanner sees this rule directly.
        scanner_visible: bool,
        /// Selected current-runtime or dialect mode.
        mode_id: ModeId,
    },
}

impl DetectionRule {
    /// Borrow the stable rule identity.
    fn id(&self) -> &DetectionRuleId {
        match self {
            Self::ExactFilename { id, .. }
            | Self::CompoundExtension { id, .. }
            | Self::Extension { id, .. }
            | Self::Content { id, .. } => id,
        }
    }

    /// Borrow the selected mode identity.
    fn mode_id(&self) -> &ModeId {
        match self {
            Self::ExactFilename { mode_id, .. }
            | Self::CompoundExtension { mode_id, .. }
            | Self::Extension { mode_id, .. }
            | Self::Content { mode_id, .. } => mode_id,
        }
    }

    /// Return whether the broad scanner owns this rule.
    const fn scanner_visible(&self) -> bool {
        match self {
            Self::ExactFilename {
                scanner_visible, ..
            }
            | Self::CompoundExtension {
                scanner_visible, ..
            }
            | Self::Extension {
                scanner_visible, ..
            }
            | Self::Content {
                scanner_visible, ..
            } => *scanner_visible,
        }
    }

    /// Return the stable layer tag.
    const fn layer_tag(&self) -> &'static str {
        match self {
            Self::ExactFilename { .. } => "exact-filename",
            Self::CompoundExtension { .. } => "compound-extension",
            Self::Extension { .. } => "extension",
            Self::Content { .. } => "content",
        }
    }

    /// Borrow the matched spelling.
    fn pattern(&self) -> &str {
        match self {
            Self::ExactFilename { file_name, .. } => file_name,
            Self::CompoundExtension { extension, .. } | Self::Extension { extension, .. } => {
                extension
            }
            Self::Content { detector_id, .. } => detector_id.contract_id(),
        }
    }

    /// Return the case policy.
    const fn case_policy(&self) -> CasePolicy {
        match self {
            Self::ExactFilename { case, .. }
            | Self::CompoundExtension { case, .. }
            | Self::Extension { case, .. } => *case,
            Self::Content { .. } => CasePolicy::Sensitive,
        }
    }

    /// Return the path-level matching policy for exact and compound rules.
    const fn path_case_policy(&self) -> CasePolicy {
        match self {
            Self::CompoundExtension {
                path_suffix_case, ..
            } => *path_suffix_case,
            Self::ExactFilename { case, .. } | Self::Extension { case, .. } => *case,
            Self::Content { .. } => CasePolicy::Sensitive,
        }
    }
}

/// Current parser-support class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserSupport {
    /// Compiled specialized parser.
    Native,
    /// Structured manifest adapter.
    Manifest,
    /// `ProjectAtlas` structural adapter.
    Structural,
    /// Bounded fallback parser.
    Fallback,
}

impl ParserSupport {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Manifest => "manifest",
            Self::Structural => "structural",
            Self::Fallback => "fallback",
        }
    }
}

/// Closed summary adapter selected by current CLI behavior.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SummaryAdapterId {
    /// No specialized summary adapter.
    None,
    /// Markdown summary adapter.
    Markdown,
    /// JSON summary adapter.
    Json,
    /// YAML summary adapter.
    Yaml,
    /// CSS summary adapter.
    Css,
    /// HTML summary adapter.
    Html,
    /// TOON summary adapter.
    Toon,
    /// Generic configuration-text summary adapter.
    ConfigText,
    /// TOML summary adapter.
    Toml,
    /// XML summary adapter.
    Xml,
    /// `PowerShell` summary adapter.
    Powershell,
}

impl SummaryAdapterId {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Css => "css",
            Self::Html => "html",
            Self::Toon => "toon",
            Self::ConfigText => "config-text",
            Self::Toml => "toml",
            Self::Xml => "xml",
            Self::Powershell => "powershell",
        }
    }
}

/// Closed compiled tree-sitter parser selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum BuiltInParserId {
    /// Rust grammar.
    Rust,
    /// Python grammar.
    Python,
    /// JavaScript grammar.
    Javascript,
    /// TypeScript grammar.
    Typescript,
    /// TSX grammar.
    Tsx,
    /// Java grammar.
    Java,
    /// Kotlin grammar.
    Kotlin,
    /// C-sharp grammar.
    Csharp,
    /// Go grammar.
    Go,
    /// Objective-C grammar.
    ObjectiveC,
    /// Zig grammar.
    Zig,
    /// C grammar.
    C,
    /// C++ grammar.
    Cpp,
}

impl BuiltInParserId {
    /// Return the stable parser spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Javascript => "javascript",
            Self::Typescript => "typescript",
            Self::Tsx => "tsx",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Csharp => "csharp",
            Self::Go => "go",
            Self::ObjectiveC => "objective-c",
            Self::Zig => "zig",
            Self::C => "c",
            Self::Cpp => "cpp",
        }
    }
}

/// Closed language-specific symbol augmenter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AugmenterId {
    /// Kotlin structural enrichment.
    Kotlin,
    /// Gradle Kotlin DSL enrichment.
    GradleKotlin,
    /// Objective-C structural enrichment.
    ObjectiveC,
    /// Zig structural enrichment.
    Zig,
    /// Gradle Groovy DSL enrichment.
    GradleGroovy,
}

impl AugmenterId {
    /// Return the stable augmenter spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Kotlin => "kotlin",
            Self::GradleKotlin => "gradle-kotlin",
            Self::ObjectiveC => "objective-c",
            Self::Zig => "zig",
            Self::GradleGroovy => "gradle-groovy",
        }
    }
}

/// Closed manifest symbol adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ManifestAdapterId {
    /// Cargo package/workspace manifest adapter.
    CargoManifest,
    /// Cargo resolved-package lockfile adapter.
    CargoLock,
}

impl ManifestAdapterId {
    /// Return the stable manifest-adapter spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::CargoManifest => "cargo-manifest",
            Self::CargoLock => "cargo-lock",
        }
    }
}

/// Closed structural symbol adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SymbolAdapterId {
    /// Vue component structural adapter.
    Vue,
    /// `PowerShell` structural adapter.
    Powershell,
}

impl SymbolAdapterId {
    /// Return the stable structural-adapter spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Vue => "vue",
            Self::Powershell => "powershell",
        }
    }
}

/// Closed current symbol-routing pipeline.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SymbolPipeline {
    /// Skip symbol extraction for this structured mode.
    Skip,
    /// Run a compiled parser and optional deterministic augmenters.
    BuiltIn {
        /// Compiled parser identity.
        parser: BuiltInParserId,
        /// Ordered post-parser augmenters.
        augmenters: Vec<AugmenterId>,
    },
    /// Run a typed manifest adapter.
    Manifest {
        /// Manifest adapter identity.
        adapter: ManifestAdapterId,
    },
    /// Run a `ProjectAtlas` structural adapter.
    Structural {
        /// Structural adapter identity.
        adapter: SymbolAdapterId,
    },
    /// Run fallback extraction and optional augmenters.
    Fallback {
        /// Ordered fallback augmenters.
        augmenters: Vec<AugmenterId>,
    },
}

impl SymbolPipeline {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(&self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::BuiltIn { .. } => "built-in",
            Self::Manifest { .. } => "manifest",
            Self::Structural { .. } => "structural",
            Self::Fallback { .. } => "fallback",
        }
    }
}

/// One current public language and its exact runtime behavior.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentLanguageMode {
    /// Stable current-mode identity.
    mode_id: ModeId,
    /// Existing public runtime spelling.
    public_mode: PublicMode,
    /// Corresponding accepted-target mode identity.
    accepted_mode_id: ModeId,
    /// Optional current-mode alias target.
    alias_of: Option<ModeId>,
    /// Current parser-support class.
    parser_support: ParserSupport,
    /// Pack that owns current behavior, independent of future delivery.
    current_pack_id: PackId,
    /// Current CLI summary adapter.
    summary_adapter: SummaryAdapterId,
    /// Current symbol-routing pipeline.
    symbols: SymbolPipeline,
}

/// Current parser implementation form.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserImplementation {
    /// A grammar compiled into default core.
    CompiledTreeSitter,
}

impl ParserImplementation {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::CompiledTreeSitter => "compiled-tree-sitter",
        }
    }
}

/// Current parser ABI verification state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AbiState {
    /// Bound to the compiled runtime contract rather than an external ABI.
    CurrentCompiledContract,
    /// Declared for a pack candidate but not yet accepted as achieved behavior.
    PendingPackVerification,
}

impl AbiState {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::CurrentCompiledContract => "current-compiled-contract",
            Self::PendingPackVerification => "pending-pack-verification",
        }
    }
}

/// ABI state nested under one parser component.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserAbi {
    /// Stable ABI contract identity.
    abi_id: ParserAbiId,
    /// Positive ABI contract version.
    version: u32,
    /// Current ABI verification state.
    state: AbiState,
}

/// One compiled parser component used by current runtime routes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserComponent {
    /// Stable component identity.
    parser_id: ParserId,
    /// Closed compiled parser choice.
    built_in_parser: BuiltInParserId,
    /// Implementation form.
    implementation: ParserImplementation,
    /// Pack that currently owns the component.
    current_pack_id: PackId,
    /// Current ABI state.
    abi: ParserAbi,
    /// Optional external parser asset.
    asset_id: Option<AssetId>,
    /// Optional extraction-query pack.
    query_pack_id: Option<QueryPackId>,
    /// Fixture identities that bind the component.
    fixture_ids: Vec<FixtureId>,
    /// Evidence identities that bind provenance.
    provenance_evidence_ids: Vec<EvidenceId>,
}

/// One external parser artifact declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserAsset {
    /// Stable parser-asset identity.
    asset_id: AssetId,
    /// Repository-relative lock or packaged-artifact path.
    path: RegistryPath,
    /// Owning feature pack.
    pack_id: PackId,
    /// Upstream source identity.
    source: ParserAssetSource,
    /// Exact upstream version identity.
    version: ParserAssetVersion,
    /// Versioned parser ABI required by the artifact.
    abi: ParserAbi,
    /// Exact artifact digest.
    digest_sha256: Sha256Digest,
    /// Declared license identifier for later SPDX inventory validation.
    license: ParserAssetLicense,
    /// Ordered reviewed patch paths.
    patches: Vec<RegistryPath>,
}

/// One bounded host-to-embedded language adapter declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedAdapter {
    /// Stable embedded-adapter identity.
    adapter_id: EmbeddedAdapterId,
    /// Host language mode.
    host_mode_id: ModeId,
    /// Embedded language or dialect mode.
    embedded_mode_id: ModeId,
    /// Feature pack that owns adapter execution.
    pack_id: PackId,
    /// Optional extraction-query pack used by the adapter.
    query_pack_id: Option<QueryPackId>,
    /// Fixtures that bind host-to-embedded reconciliation behavior.
    fixture_ids: Vec<FixtureId>,
}

/// One extraction query-pack declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QueryPack {
    /// Stable query-pack identity.
    #[serde(rename = "query_pack_id")]
    id: QueryPackId,
    /// Repository-relative query artifact path.
    path: RegistryPath,
    /// Owning feature pack.
    pack_id: PackId,
    /// Exact query artifact digest.
    digest_sha256: Sha256Digest,
}

/// One semantic-provider declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticProvider {
    /// Stable provider identity.
    provider_id: SemanticProviderId,
    /// Owning feature pack.
    pack_id: PackId,
    /// Current modes served by this provider.
    mode_ids: Vec<ModeId>,
    /// Required fixture identities.
    fixture_ids: Vec<FixtureId>,
}

/// Verification state for a current fixture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum VerificationState {
    /// Evidence has been verified against the bound input.
    Verified,
    /// Evidence remains pending.
    Pending,
}

impl VerificationState {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Pending => "pending",
        }
    }
}

/// Fixture verification state and its evidence references.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FixtureVerification {
    /// Current verification state.
    state: VerificationState,
    /// Evidence identities supporting the state.
    evidence_ids: Vec<EvidenceId>,
}

/// One registry fixture declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryFixture {
    /// Stable fixture identity.
    fixture_id: FixtureId,
    /// Repository-relative fixture path.
    path: RegistryPath,
    /// Verification state and evidence.
    verification: FixtureVerification,
}

/// Current evidence artifact kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum EvidenceKind {
    /// Frozen prior-runtime behavior contract.
    FrozenRuntimeContract,
}

impl EvidenceKind {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::FrozenRuntimeContract => "frozen-runtime-contract",
        }
    }
}

/// One digest-bound evidence artifact.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryEvidence {
    /// Stable evidence identity.
    evidence_id: EvidenceId,
    /// Evidence artifact kind.
    kind: EvidenceKind,
    /// Repository-relative evidence path.
    path: RegistryPath,
    /// Exact artifact digest.
    digest_sha256: Sha256Digest,
}

/// Accepted-target binding stored in the composite lock.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedTargetBinding {
    /// Fixed repository-relative accepted-target path.
    path: RegistryPath,
    /// Exact accepted-target registry identity.
    registry_id: AcceptedRegistryId,
    /// Existing accepted-set semantic digest.
    accepted_set_sha256: Sha256Digest,
    /// Exact accepted-target raw-byte digest.
    raw_sha256: Sha256Digest,
}

/// Historical-runtime binding stored in the composite lock.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoricalContractBinding {
    /// Fixed repository-relative historical fixture path.
    path: RegistryPath,
    /// Exact historical release.
    release: ReleaseId,
    /// Exact historical commit.
    commit: RevisionId,
    /// Exact historical fixture raw-byte digest.
    raw_sha256: Sha256Digest,
}

/// Release-owned parser-pack trust binding stored in the composite lock.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackTrustBinding {
    /// Fixed repository-relative trust-manifest path.
    path: RegistryPath,
    /// Exact trust-manifest raw-byte digest.
    raw_sha256: Sha256Digest,
}

/// Closed parser-pack trust-manifest format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ParserPackTrustFormat {
    /// Release-owned evaluation trust projection.
    #[serde(rename = "projectatlas.parser-pack-trust")]
    ParserPackTrust,
}

/// Closed release eligibility for a parser-pack candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackCandidateEligibility {
    /// Candidate may be evaluated but is neither selected nor runtime-reachable.
    EvaluationOnlyUnselected,
}

impl ParserPackCandidateEligibility {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::EvaluationOnlyUnselected => "evaluation-only-unselected",
        }
    }
}

/// Closed evidence state for an external parser ABI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackAbiState {
    /// ABI evidence exists but runtime verification and selection remain pending.
    PendingPackVerification,
}

impl ParserPackAbiState {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::PendingPackVerification => "pending-pack-verification",
        }
    }
}

/// `ProjectAtlas` parser-pack lifecycle ABI carried by an evaluation candidate.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackLifecycleAbi {
    /// Parser-pack lifecycle ABI family identity.
    abi_id: ParserAbiId,
    /// Parser-pack lifecycle ABI version.
    version: u32,
}

/// External grammar ABI carried by an evaluation candidate.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackGrammarAbi {
    /// External grammar ABI family identity.
    abi_id: ParserAbiId,
    /// External grammar ABI version.
    version: u32,
    /// Evidence state for the external ABI.
    state: ParserPackAbiState,
}

/// Closed role for one exact trusted candidate file.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackTrustedFileRole {
    /// Candidate release manifest.
    Manifest,
    /// Executable parser module retained only below the strict validation ceiling.
    ParserModule,
    /// Typed static and local-probe validation for the parser module.
    WasmValidation,
    /// Retained script used to produce the local WASM probe result.
    WasmProbeScript,
    /// Reproducible-build provenance record.
    Provenance,
    /// License text.
    License,
    /// Advisory scan record.
    AdvisoryReport,
    /// Software bill of materials.
    Sbom,
    /// Tracked source patch applied before the candidate build.
    Patch,
}

impl ParserPackTrustedFileRole {
    /// Return whether typed validation needs the bounded file contents retained.
    const fn requires_metadata_bytes(self) -> bool {
        match self {
            Self::Manifest
            | Self::ParserModule
            | Self::WasmValidation
            | Self::WasmProbeScript
            | Self::Provenance
            | Self::License
            | Self::AdvisoryReport
            | Self::Sbom
            | Self::Patch => true,
        }
    }

    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::ParserModule => "parser-module",
            Self::WasmValidation => "wasm-validation",
            Self::WasmProbeScript => "wasm-probe-script",
            Self::Provenance => "provenance",
            Self::License => "license",
            Self::AdvisoryReport => "advisory-report",
            Self::Sbom => "sbom",
            Self::Patch => "patch",
        }
    }
}

/// Closed local output-identity evidence available for an evaluation candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackLocalOutputIdentity {
    /// Two clean local builds produced byte-identical output artifacts.
    TwoByteIdenticalCleanOutputs,
}

impl ParserPackLocalOutputIdentity {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::TwoByteIdenticalCleanOutputs => "two-byte-identical-clean-outputs",
        }
    }
}

/// Closed hosted reproduction state for an evaluation candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackHostedReproductionState {
    /// Hosted reproduction has not yet run.
    Pending,
}

impl ParserPackHostedReproductionState {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

/// Closed retention state for raw historical build logs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackRawBuildLogRetention {
    /// The historical raw process logs were not retained.
    NotRetained,
}

impl ParserPackRawBuildLogRetention {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::NotRetained => "not-retained",
        }
    }
}

/// Packaged Windows support state for filesystem-identity evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ParserPackWindowsIdentityEvidence {
    /// `ReFS` exposes 128-bit file identities while the current capability API reports 64 bits.
    PendingRefs128BitIdentityEvidence,
}

impl ParserPackWindowsIdentityEvidence {
    /// Return the stable semantic-digest tag.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::PendingRefs128BitIdentityEvidence => "pending-refs128-bit-identity-evidence",
        }
    }
}

/// Honest reproduction evidence carried consistently by trust, manifest, and provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackReproductionEvidenceState {
    /// Locally observed output identity, not a hosted reproducibility claim.
    local_output_identity: ParserPackLocalOutputIdentity,
    /// Hosted reproduction remains a later release gate.
    hosted_reproduction: ParserPackHostedReproductionState,
    /// Raw historical process-log retention state.
    raw_build_logs: ParserPackRawBuildLogRetention,
    /// Packaged Windows identity evidence that remains a release caveat.
    packaged_windows_identity: ParserPackWindowsIdentityEvidence,
}

/// Exact trusted file inventory row.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackTrustedFile {
    /// Candidate-root-relative portable path.
    path: RegistryPath,
    /// File responsibility.
    role: ParserPackTrustedFileRole,
    /// Exact nonzero byte count.
    bytes: u64,
    /// Exact file digest.
    sha256: Sha256Digest,
}

/// Trust-manifest provenance binding for one candidate.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackProvenanceBinding {
    /// HTTPS upstream repository.
    source: String,
    /// Exact upstream source revision.
    revision: RevisionId,
    /// Exact downloaded source-package digest.
    source_package_sha256: Sha256Digest,
    /// Fully resolved build-toolchain identity.
    toolchain: String,
    /// Honest local, hosted, and raw-log evidence state.
    evidence_state: ParserPackReproductionEvidenceState,
    /// Candidate-root-relative provenance record.
    record_path: RegistryPath,
    /// Candidate-root-relative tracked patches, in application order.
    patch_paths: Vec<RegistryPath>,
}

/// One component license bound to an exact candidate file.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackLicenseBinding {
    /// Component covered by the license.
    component: String,
    /// SPDX license expression.
    expression: String,
    /// Candidate-root-relative license record.
    record_path: RegistryPath,
}

/// One release-owned, evaluation-only parser-pack candidate.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackCandidateTrust {
    /// Stable candidate identity.
    candidate_id: ParserPackCandidateId,
    /// Closed evaluation lifecycle.
    eligibility: ParserPackCandidateEligibility,
    /// Candidate is never public support while selection remains blocked.
    advertised: bool,
    /// Optional pack that would own the candidate if later selected.
    pack_id: PackId,
    /// Versioned candidate release identity.
    release_version: String,
    /// `ProjectAtlas` lifecycle ABI needed by the existing pack lifecycle owner.
    pack_abi: ParserPackLifecycleAbi,
    /// External Tree-sitter grammar ABI provenance.
    grammar_abi: ParserPackGrammarAbi,
    /// Platform of the tracked candidate payload.
    packaged_platform: PlatformId,
    /// Repository-relative candidate root.
    payload_root: RegistryPath,
    /// Candidate-root-relative manifest path.
    manifest_path: RegistryPath,
    /// Exact installed bytes represented by the complete inventory.
    installed_bytes: u64,
    /// Complete nonzero candidate file inventory.
    inventory: Vec<ParserPackTrustedFile>,
    /// Bound source and build provenance.
    provenance: ParserPackProvenanceBinding,
    /// Complete component-license records.
    licenses: Vec<ParserPackLicenseBinding>,
    /// Candidate-root-relative advisory record.
    advisory_record_path: RegistryPath,
    /// Candidate-root-relative WASM validation record.
    wasm_validation_record_path: RegistryPath,
    /// Candidate-root-relative SPDX record.
    sbom_record_path: RegistryPath,
}

/// Complete release-owned parser-pack trust manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParserPackTrustManifest {
    /// Trust-manifest schema version.
    schema_version: u32,
    /// Closed trust-manifest format.
    format: ParserPackTrustFormat,
    /// Bounded evaluation-only candidate inventory.
    candidates: Vec<ParserPackCandidateTrust>,
}

/// One ABI identity stored by the candidate's own release manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackManifestAbi {
    /// ABI family identity.
    abi_id: ParserAbiId,
    /// ABI version.
    version: u32,
}

/// Candidate source fields repeated in its release manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackManifestProvenance {
    /// HTTPS upstream repository.
    source: String,
    /// Exact source revision.
    revision: RevisionId,
    /// Fully resolved build-toolchain identity.
    toolchain: String,
    /// Honest local, hosted, and raw-log evidence state.
    evidence_state: ParserPackReproductionEvidenceState,
}

/// Candidate license fields repeated in its release manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackManifestLicense {
    /// Licensed component.
    component: String,
    /// SPDX license expression.
    expression: String,
}

/// One non-manifest artifact declared by the candidate release manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackManifestArtifact {
    /// Candidate-root-relative artifact path.
    path: RegistryPath,
    /// Artifact role.
    role: ParserPackTrustedFileRole,
    /// Exact artifact digest.
    sha256: Sha256Digest,
    /// Exact artifact bytes.
    bytes: u64,
}

/// Candidate-owned release manifest reconciled against release-owned trust.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackReleaseManifest {
    /// Release-manifest schema version.
    schema_version: u32,
    /// Owning optional pack.
    pack_id: PackId,
    /// Candidate release version.
    version: String,
    /// `ProjectAtlas` parser-pack lifecycle ABI.
    pack_abi: ParserPackManifestAbi,
    /// External grammar ABI.
    grammar_abi: ParserPackManifestAbi,
    /// Required runtime boundary.
    runtime: PackRuntime,
    /// Packaged platform.
    platform: PlatformId,
    /// Source and build identity.
    provenance: ParserPackManifestProvenance,
    /// Component licenses.
    licenses: Vec<ParserPackManifestLicense>,
    /// Exact non-manifest artifact inventory.
    artifacts: Vec<ParserPackManifestArtifact>,
}

/// Closed candidate provenance-record format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum ParserPackProvenanceFormat {
    /// `ProjectAtlas` parser-pack build provenance.
    #[serde(rename = "projectatlas.parser-pack-provenance")]
    ParserPackProvenance,
}

/// Exact source package used by a candidate build.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackSourceProvenance {
    /// HTTPS upstream repository.
    repository: String,
    /// Exact source revision.
    revision: RevisionId,
    /// HTTPS immutable package URL.
    package_url: String,
    /// Exact package digest.
    package_sha256: Sha256Digest,
}

/// Resolved Tree-sitter CLI toolchain component.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackTreeSitterToolchain {
    /// Tool version.
    version: String,
    /// Exact source revision.
    revision: RevisionId,
    /// HTTPS package URL.
    package_url: String,
    /// Registry-provided SHA-512 integrity payload.
    package_integrity_sha512: String,
    /// Exact executable digest.
    binary_sha256: Sha256Digest,
}

/// Resolved emsdk toolchain component.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackEmsdkToolchain {
    /// Tool version.
    version: String,
    /// Exact tag revision.
    revision: RevisionId,
    /// HTTPS source repository.
    source: String,
    /// Exact release revision resolved by emsdk.
    resolved_release_revision: RevisionId,
}

/// Resolved Emscripten toolchain component.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackEmscriptenToolchain {
    /// Tool version.
    version: String,
    /// Exact source revision.
    revision: RevisionId,
    /// Exact compiler-driver digest.
    emcc_sha256: Sha256Digest,
}

/// Resolved LLVM toolchain component.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackLlvmToolchain {
    /// Tool version.
    version: String,
    /// Exact source revision.
    revision: RevisionId,
    /// Exact Clang executable digest.
    clang_sha256: Sha256Digest,
    /// Exact WebAssembly linker digest.
    wasm_ld_sha256: Sha256Digest,
}

/// Resolved binary-only toolchain component.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBinaryToolchain {
    /// Tool version.
    version: String,
    /// Exact executable digest.
    binary_sha256: Sha256Digest,
}

/// Fully resolved candidate build toolchain.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackResolvedToolchain {
    /// Tree-sitter grammar compiler.
    tree_sitter_cli: ParserPackTreeSitterToolchain,
    /// Emscripten SDK selector.
    emsdk: ParserPackEmsdkToolchain,
    /// Emscripten compiler.
    emscripten: ParserPackEmscriptenToolchain,
    /// LLVM and WebAssembly linker.
    llvm: ParserPackLlvmToolchain,
    /// JavaScript runtime used by the compiler.
    node: ParserPackBinaryToolchain,
    /// Python runtime used by the compiler.
    python: ParserPackBinaryToolchain,
}

/// One exact source input to a reproducible candidate build.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBuildSourceFile {
    /// Source-root-relative path.
    path: RegistryPath,
    /// Exact source-file digest.
    sha256: Sha256Digest,
}

/// Candidate parser output from the reproducible build.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBuildOutput {
    /// Candidate-root-relative output path.
    path: RegistryPath,
    /// Exact output bytes.
    bytes: u64,
    /// Exact output digest.
    sha256: Sha256Digest,
}

/// Fully resolved candidate build invocation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBuildProvenance {
    /// Exact clean source directory for the first retained local output.
    working_directory: RegistryPath,
    /// Exact relative build command without unresolved placeholders.
    command: String,
    /// Exact assumptions required to interpret the local build evidence.
    environment_assumptions: Vec<String>,
    /// Resolved compiler flags in order.
    resolved_flags: Vec<String>,
    /// Exact build source inventory.
    source_files: Vec<ParserPackBuildSourceFile>,
    /// Exact candidate output.
    output: ParserPackBuildOutput,
}

/// One independent clean reproduction result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackReproduction {
    /// Stable run identity.
    run_id: String,
    /// Exact clean source directory used by this run.
    working_directory: RegistryPath,
    /// Exact relative command used by this run.
    command: String,
    /// Raw historical process-log retention state.
    raw_log_retention: ParserPackRawBuildLogRetention,
    /// Exact output bytes.
    bytes: u64,
    /// Exact output digest.
    sha256: Sha256Digest,
}

/// Complete typed candidate build-provenance record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackProvenanceRecord {
    /// Provenance schema version.
    schema_version: u32,
    /// Closed provenance format.
    format: ParserPackProvenanceFormat,
    /// Bound trust candidate.
    candidate_id: ParserPackCandidateId,
    /// Exact upstream source package.
    source: ParserPackSourceProvenance,
    /// Honest local, hosted, and raw-log evidence state.
    evidence_state: ParserPackReproductionEvidenceState,
    /// Fully resolved toolchain.
    resolved_toolchain: ParserPackResolvedToolchain,
    /// Fully resolved build.
    build: ParserPackBuildProvenance,
    /// Local clean-build output identity records.
    local_output_runs: Vec<ParserPackReproduction>,
}

/// Closed advisory-record format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum ParserPackAdvisoryFormat {
    /// `ProjectAtlas` parser-pack advisory projection.
    #[serde(rename = "projectatlas.parser-pack-advisories")]
    ParserPackAdvisories,
}

/// Pinned advisory database identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryDatabase {
    /// HTTPS advisory database source.
    source: String,
    /// Exact advisory database revision.
    revision: RevisionId,
}

/// Advisory scanner identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryTool {
    /// Scanner name.
    name: String,
    /// Scanner version.
    version: String,
    /// Offline scan command.
    command: String,
    /// Exact scanner process exit code.
    exit_code: i32,
}

/// Candidate source component covered by the advisory record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryComponent {
    /// Package ecosystem.
    ecosystem: String,
    /// Package name.
    name: String,
    /// Package version.
    version: String,
    /// Exact source-package digest.
    checksum_sha256: Sha256Digest,
}

/// One exact Cargo advisory-scan input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryInput {
    /// Source-package-relative input path.
    path: RegistryPath,
    /// Exact input bytes.
    bytes: u64,
    /// Exact input digest.
    sha256: Sha256Digest,
}

/// Parsed summary repeated outside the retained raw advisory result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryResult {
    /// Dependency count parsed by the scanner.
    dependency_count: u64,
    /// Whether the scanner found any vulnerability.
    vulnerabilities_found: bool,
    /// Exact vulnerability count.
    vulnerability_count: u64,
}

/// Exact advisory-database section emitted by `cargo audit`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoAuditRawDatabase {
    /// Advisory count at the pinned database revision.
    #[serde(rename = "advisory-count")]
    advisory_count: u64,
    /// Optional database commit emitted by the offline scanner.
    #[serde(rename = "last-commit")]
    last_commit: Option<String>,
    /// Optional database update time emitted by the offline scanner.
    #[serde(rename = "last-updated")]
    last_updated: Option<String>,
}

/// Exact lockfile summary emitted by `cargo audit`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoAuditRawLockfile {
    /// Parsed dependency count.
    #[serde(rename = "dependency-count")]
    dependency_count: u64,
}

/// Exact scanner settings emitted by `cargo audit`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoAuditRawSettings {
    /// Requested target architectures.
    target_arch: Vec<String>,
    /// Requested target operating systems.
    target_os: Vec<String>,
    /// Optional severity threshold.
    severity: Option<String>,
    /// Ignored advisory identities.
    ignore: Vec<String>,
    /// Enabled informational warning classes.
    informational_warnings: Vec<String>,
}

/// Exact vulnerability section emitted by `cargo audit`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoAuditRawVulnerabilities {
    /// Whether any vulnerability was found.
    found: bool,
    /// Exact vulnerability count.
    count: u64,
    /// Raw vulnerability rows; empty for the trusted result.
    list: Vec<serde_json::Value>,
}

/// Typed retained raw `cargo audit` result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoAuditRawResult {
    /// Advisory database summary.
    database: CargoAuditRawDatabase,
    /// Lockfile summary.
    lockfile: CargoAuditRawLockfile,
    /// Scanner settings.
    settings: CargoAuditRawSettings,
    /// Vulnerability result.
    vulnerabilities: CargoAuditRawVulnerabilities,
    /// Warning rows keyed by category.
    warnings: BTreeMap<String, serde_json::Value>,
}

/// Complete typed advisory record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackAdvisoryRecord {
    /// Advisory-record schema version.
    schema_version: u32,
    /// Closed advisory-record format.
    format: ParserPackAdvisoryFormat,
    /// Bound trust candidate.
    candidate_id: ParserPackCandidateId,
    /// Pinned advisory database.
    database: ParserPackAdvisoryDatabase,
    /// Pinned scanner.
    tool: ParserPackAdvisoryTool,
    /// Scanned source component.
    component: ParserPackAdvisoryComponent,
    /// Exact source-package manifest and lock inputs.
    inputs: Vec<ParserPackAdvisoryInput>,
    /// Exact raw UTF-8 scanner result without a trailing newline.
    raw_result: String,
    /// Exact raw-result byte count.
    raw_result_bytes: u64,
    /// Exact raw-result digest.
    raw_result_sha256: Sha256Digest,
    /// Parsed clean-result summary.
    result: ParserPackAdvisoryResult,
}

/// Closed candidate WASM-validation record format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum ParserPackWasmValidationFormat {
    /// `ProjectAtlas` candidate WASM validation.
    #[serde(rename = "projectatlas.parser-pack-wasm-validation")]
    ParserPackWasmValidation,
}

/// Static module identity and ABI requirements.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmModuleValidation {
    /// Candidate-root-relative module path.
    path: RegistryPath,
    /// Exact module bytes.
    bytes: u64,
    /// Exact module digest.
    sha256: Sha256Digest,
    /// Exact WebAssembly magic bytes in lowercase hexadecimal.
    magic_hex: String,
    /// WebAssembly binary-format version.
    module_version: u32,
    /// Required exported grammar function.
    required_function_export: String,
    /// Bound external grammar ABI version.
    grammar_abi_version: u32,
}

/// Local probe lifecycle that does not claim hosted reproduction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ParserPackWasmProbeState {
    /// The retained local probe passed while hosted proof remains pending.
    LocalPassHostedPending,
}

/// Hosted probe lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ParserPackWasmHostedState {
    /// Hosted validation has not yet run.
    Pending,
}

/// Exact local runtime used by the retained WASM probe.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmProbeTool {
    /// Runtime name.
    name: String,
    /// Runtime version without a leading `v`.
    version: String,
    /// Exact local runtime path recorded by the probe environment.
    binary_path: String,
    /// Exact runtime executable digest.
    binary_sha256: Sha256Digest,
}

/// Exact bounded import environment supplied by the retained local probe.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmProbeEnvironment {
    /// Initial linear-memory pages.
    memory_initial_pages: u32,
    /// Maximum linear-memory pages.
    memory_maximum_pages: u32,
    /// Initial indirect-table elements.
    table_initial_elements: u32,
    /// Initial stack pointer.
    stack_pointer: u32,
    /// Relocatable module memory base.
    memory_base: u32,
    /// Relocatable module table base.
    table_base: u32,
}

/// Parsed retained WASM probe result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmProbeResult {
    /// Exact probed module digest.
    module_sha256: Sha256Digest,
    /// Returned language-structure pointer.
    pointer: u32,
    /// Grammar ABI version read from the returned structure.
    abi: u32,
    /// Exact exported-name inventory.
    exports: Vec<String>,
}

/// Retained local WASM probe evidence.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmProbe {
    /// Local/hosted probe lifecycle.
    state: ParserPackWasmProbeState,
    /// Exact local runtime identity.
    tool: ParserPackWasmProbeTool,
    /// Candidate-root-relative retained probe script.
    script_path: RegistryPath,
    /// Exact retained probe-script digest.
    script_sha256: Sha256Digest,
    /// Exact relative probe command.
    command: String,
    /// Exact bounded import environment.
    environment: ParserPackWasmProbeEnvironment,
    /// Exact raw result without a trailing newline.
    raw_result: String,
    /// Exact raw-result digest.
    raw_result_sha256: Sha256Digest,
    /// Hosted probe lifecycle.
    hosted_state: ParserPackWasmHostedState,
}

/// Complete typed candidate WASM-validation record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackWasmValidationRecord {
    /// WASM-validation schema version.
    schema_version: u32,
    /// Closed record format.
    format: ParserPackWasmValidationFormat,
    /// Bound trust candidate.
    candidate_id: ParserPackCandidateId,
    /// Static module identity and ABI contract.
    module: ParserPackWasmModuleValidation,
    /// Retained local probe evidence.
    probe: ParserPackWasmProbe,
}

/// SPDX document creation metadata.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxCreationInfo {
    /// Reproducible record timestamp.
    created: String,
    /// Document creators.
    creators: Vec<String>,
}

/// One SPDX checksum.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxChecksum {
    /// SPDX checksum algorithm.
    algorithm: String,
    /// Hexadecimal checksum value.
    checksum_value: String,
}

/// One SPDX external package reference.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackSpdxExternalReference {
    /// SPDX reference category.
    #[serde(rename = "referenceCategory")]
    category: String,
    /// SPDX reference type.
    #[serde(rename = "referenceType")]
    kind: String,
    /// Package reference locator.
    #[serde(rename = "referenceLocator")]
    locator: String,
}

/// One package in the candidate SPDX document.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxPackage {
    /// Component name.
    name: String,
    /// SPDX package identity.
    #[serde(rename = "SPDXID")]
    spdx_id: String,
    /// Component version.
    version_info: String,
    /// HTTPS source-package URL.
    download_location: String,
    /// Whether exact files were analyzed.
    files_analyzed: bool,
    /// Package verification code, absent when complete file analysis is not claimed.
    package_verification_code: Option<serde_json::Value>,
    /// Package checksums.
    checksums: Vec<ParserPackSpdxChecksum>,
    /// Concluded license expression.
    license_concluded: String,
    /// Declared license expression.
    license_declared: String,
    /// Copyright notice.
    copyright_text: String,
    /// Package references.
    external_refs: Vec<ParserPackSpdxExternalReference>,
}

/// One file in the candidate SPDX document.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxFile {
    /// Candidate-root-relative file prefixed with `./`.
    file_name: String,
    /// SPDX file identity.
    #[serde(rename = "SPDXID")]
    spdx_id: String,
    /// File checksums.
    checksums: Vec<ParserPackSpdxChecksum>,
    /// Concluded license expression.
    license_concluded: String,
    /// Licenses found in the file.
    license_info_in_files: Vec<String>,
    /// Copyright notice.
    copyright_text: String,
}

/// One SPDX relationship.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxRelationship {
    /// Relationship source.
    spdx_element_id: String,
    /// Relationship type.
    relationship_type: String,
    /// Relationship target.
    related_spdx_element: String,
}

/// Minimum exact SPDX 2.3 projection required for a parser-pack candidate.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParserPackSpdxDocument {
    /// SPDX format version.
    spdx_version: String,
    /// SPDX data license.
    data_license: String,
    /// SPDX document identity.
    #[serde(rename = "SPDXID")]
    spdx_id: String,
    /// Human-readable document name.
    name: String,
    /// Unique HTTPS document namespace.
    document_namespace: String,
    /// Creation metadata.
    creation_info: ParserPackSpdxCreationInfo,
    /// Package identities described by the document.
    document_describes: Vec<String>,
    /// Package inventory.
    packages: Vec<ParserPackSpdxPackage>,
    /// File inventory.
    files: Vec<ParserPackSpdxFile>,
    /// Exact relationship graph.
    relationships: Vec<ParserPackSpdxRelationship>,
}

/// Repository-intelligence contract projection that owns optional-pack budgets.
#[derive(Debug, Deserialize)]
struct ParserPackBudgetDocument {
    /// Resource budgets.
    budgets: ParserPackBudgets,
    /// Unowned repository-intelligence contract sections ignored by this projection.
    #[serde(flatten)]
    _unowned: BTreeMap<String, serde_json::Value>,
}

/// Optional-pack budget owner nested below the repository budget catalog.
#[derive(Debug, Deserialize)]
struct ParserPackBudgets {
    /// Optional-pack isolation and accepted ceilings.
    optional_pack_contract: ParserPackBudgetContract,
    /// Unowned default-core budget sections ignored by this projection.
    #[serde(flatten)]
    _unowned: BTreeMap<String, serde_json::Value>,
}

/// Authoritative optional-pack isolation contract.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBudgetContract {
    /// Packs are not runtime-enabled by default.
    disabled_by_default: bool,
    /// Packs cannot spend the default-core resource allowance.
    may_spend_default_core_allowance: bool,
    /// Exact required limit-field vocabulary.
    required_limit_fields: Vec<String>,
    /// Exact required pack-manifest field vocabulary.
    required_manifest_fields: Vec<String>,
    /// Accepted pack budgets.
    accepted_pack_budgets: Vec<ParserPackBudget>,
    /// Fail-closed pack-selection rule.
    selection_rule: String,
}

/// One authoritative optional-pack budget row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBudget {
    /// Owning pack.
    pack_id: PackId,
    /// Registration lifecycle.
    status: ParserPackBudgetStatus,
    /// Numeric limits.
    limits: ParserPackBudgetLimits,
    /// Enforcement lifecycle.
    enforcement: ParserPackBudgetEnforcement,
    /// Whether this pack may borrow from default core.
    may_borrow_default_core_allowance: bool,
    /// Whether this pack is disabled by default.
    disabled_by_default: bool,
    /// Whether the pack can be completely removed.
    removable: bool,
}

/// Closed registration lifecycle for an optional parser-pack budget.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ParserPackBudgetStatus {
    /// The budget is fixed before candidate measurements exist.
    PreregisteredNotMeasured,
}

/// Closed enforcement lifecycle for an optional parser-pack budget.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ParserPackBudgetEnforcement {
    /// Every candidate must remain bound to its validated pack manifest.
    PackManifestRequired,
}

/// Installed-byte ceiling needed by this trust projection.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserPackBudgetLimits {
    /// Maximum pack input bytes.
    input_bytes: u64,
    /// Maximum pack stage duration in milliseconds.
    stage_time_ms: u64,
    /// Maximum pack memory bytes.
    memory_bytes: u64,
    /// Maximum pack output bytes.
    output_bytes: u64,
    /// Maximum executable bytes.
    executable_bytes: u64,
    /// Maximum installed candidate bytes.
    installed_bytes: u64,
    /// Maximum startup p95 in milliseconds.
    startup_p95_ms: u64,
    /// Maximum active resident bytes.
    active_rss_bytes: u64,
    /// Maximum cancellation grace in milliseconds.
    cancellation_grace_ms: u64,
}

/// Complete typed composite language registry.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LanguageRegistryLock {
    /// Lock schema version.
    schema_version: u32,
    /// Closed lock format.
    format: RegistryFormat,
    /// Stable composite registry identity.
    registry_id: RegistryId,
    /// Bound accepted future target.
    accepted_target: AcceptedTargetBinding,
    /// Bound current-runtime historical contract.
    historical_contract: HistoricalContractBinding,
    /// Bound release-owned parser-pack evaluation trust.
    parser_pack_trust: ParserPackTrustBinding,
    /// Declared pack boundaries.
    packs: Vec<RegistryPack>,
    /// Closed capability-tier vocabulary in increasing evidence order.
    capability_tiers: Vec<CapabilityTier>,
    /// Ordered current-runtime detection rules.
    detection_rules: Vec<DetectionRule>,
    /// Closed semantic refinements and compatible base modes.
    semantic_modes: Vec<SemanticModeRule>,
    /// Ordered current public modes and behavior.
    current_modes: Vec<CurrentLanguageMode>,
    /// Closed current built-in parser components.
    parser_components: Vec<ParserComponent>,
    /// External parser assets.
    assets: Vec<ParserAsset>,
    /// Bounded embedded-language adapter declarations.
    embedded_adapters: Vec<EmbeddedAdapter>,
    /// Extraction query packs.
    query_packs: Vec<QueryPack>,
    /// Semantic providers.
    semantic_providers: Vec<SemanticProvider>,
    /// Registry fixtures.
    fixtures: Vec<RegistryFixture>,
    /// Registry evidence artifacts.
    evidence: Vec<RegistryEvidence>,
}

/// Recursive seed that validates JSON duplicate-key uniqueness without materializing values.
#[derive(Clone, Copy, Debug)]
struct UniqueJsonSeed;

impl<'de> DeserializeSeed<'de> for UniqueJsonSeed {
    type Value = ();

    /// Deserialize any JSON value while rejecting repeated object keys.
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

/// Visitor used by [`UniqueJsonSeed`] for recursive duplicate-key validation.
#[derive(Clone, Copy, Debug)]
struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = ();

    /// Describe the accepted JSON value domain.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    /// Accept a boolean scalar.
    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept a signed integer scalar.
    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept an unsigned integer scalar.
    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept a floating-point scalar.
    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept a borrowed string scalar.
    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept an owned string scalar.
    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Accept a null scalar.
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    /// Recursively validate every array element.
    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element_seed(UniqueJsonSeed)?.is_some() {}
        Ok(())
    }

    /// Recursively validate every object value and reject repeated keys.
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate JSON object key {key:?}"
                )));
            }
            map.next_value_seed(UniqueJsonSeed)?;
        }
        Ok(())
    }
}

/// Reject recursive JSON duplicate keys before typed deserialization.
fn reject_duplicate_json_keys(
    bytes: &[u8],
    label: &'static str,
) -> Result<(), LanguageRegistryError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    UniqueJsonSeed
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end())
        .map_err(|source| LanguageRegistryError::JsonDecode { label, source })
}

/// Decode release-owned parser-pack trust through the shared duplicate-key guard.
fn decode_language_registry_lock(
    bytes: &[u8],
) -> Result<LanguageRegistryLock, LanguageRegistryError> {
    reject_duplicate_json_keys(bytes, "language registry lock")?;
    serde_json::from_slice(bytes).map_err(|source| LanguageRegistryError::JsonDecode {
        label: "language registry lock",
        source,
    })
}

/// Decode release-owned parser-pack trust through the shared duplicate-key guard.
fn decode_parser_pack_trust(
    bytes: &[u8],
) -> Result<ParserPackTrustManifest, LanguageRegistryError> {
    reject_duplicate_json_keys(bytes, "parser-pack trust manifest")?;
    serde_json::from_slice(bytes).map_err(|source| LanguageRegistryError::JsonDecode {
        label: "parser-pack trust manifest",
        source,
    })
}

/// Decode one bounded candidate metadata file through the shared duplicate-key guard.
fn decode_parser_pack_metadata<T>(
    bytes: &[u8],
    label: &'static str,
) -> Result<T, LanguageRegistryError>
where
    for<'de> T: Deserialize<'de>,
{
    reject_duplicate_json_keys(bytes, label)?;
    serde_json::from_slice(bytes)
        .map_err(|source| LanguageRegistryError::JsonDecode { label, source })
}

/// Validate the complete evaluation-only trust projection and return its installed-byte ceiling.
fn validate_parser_pack_capture_authority(
    lock: &LanguageRegistryLock,
    trust: &ParserPackTrustManifest,
    budget_bytes: &[u8],
) -> Result<u64, LanguageRegistryError> {
    if lock.schema_version != LANGUAGE_REGISTRY_SCHEMA_VERSION
        || lock.format != RegistryFormat::LanguageRegistryLock
    {
        return Err(LanguageRegistryError::Validation(
            "unsupported language registry authority for parser-pack capture".to_string(),
        ));
    }
    require_equal(
        lock.parser_pack_trust.path.as_str(),
        PARSER_PACK_TRUST_PATH,
        "parser-pack trust path",
    )?;
    if trust.schema_version != PARSER_PACK_TRUST_SCHEMA_VERSION
        || trust.format != ParserPackTrustFormat::ParserPackTrust
        || trust.candidates.is_empty()
        || trust.candidates.len() > MAX_PARSER_PACK_CANDIDATES
    {
        return Err(LanguageRegistryError::Validation(
            "unsupported or empty parser-pack trust manifest".to_string(),
        ));
    }

    reject_duplicate_json_keys(budget_bytes, "repository-intelligence contracts")?;
    let budget_document = serde_json::from_slice::<ParserPackBudgetDocument>(budget_bytes)
        .map_err(|source| LanguageRegistryError::JsonDecode {
            label: "repository-intelligence contracts",
            source,
        })?;
    let contract = &budget_document.budgets.optional_pack_contract;
    let required_limit_fields = contract
        .required_limit_fields
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_manifest_fields = contract
        .required_manifest_fields
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !contract.disabled_by_default
        || contract.may_spend_default_core_allowance
        || required_limit_fields != OPTIONAL_PACK_REQUIRED_LIMIT_FIELDS
        || required_manifest_fields != OPTIONAL_PACK_REQUIRED_MANIFEST_FIELDS
        || !metadata_text_is_valid(&contract.selection_rule, 1024)
    {
        return Err(LanguageRegistryError::Validation(
            "optional parser packs must remain disabled and isolated from default-core allowances"
                .to_string(),
        ));
    }
    let mut budget_ids = BTreeSet::new();
    let mut broad_budget = None;
    for budget in &contract.accepted_pack_budgets {
        if !budget_ids.insert(budget.pack_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate optional-pack budget {}",
                budget.pack_id.as_str()
            )));
        }
        if budget.limits.input_bytes == 0
            || budget.limits.stage_time_ms == 0
            || budget.limits.memory_bytes == 0
            || budget.limits.output_bytes == 0
            || budget.limits.executable_bytes == 0
            || budget.limits.installed_bytes == 0
            || budget.limits.startup_p95_ms == 0
            || budget.limits.active_rss_bytes == 0
            || budget.limits.cancellation_grace_ms == 0
        {
            return Err(LanguageRegistryError::Validation(format!(
                "optional-pack budget {} has a zero hard ceiling",
                budget.pack_id.as_str()
            )));
        }
        if budget.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID {
            broad_budget = Some(budget);
        }
    }
    let broad_budget = broad_budget.ok_or_else(|| {
        LanguageRegistryError::Validation(
            "broad-language-pack has no authoritative optional-pack budget".to_string(),
        )
    })?;
    if broad_budget.status != ParserPackBudgetStatus::PreregisteredNotMeasured
        || broad_budget.enforcement != ParserPackBudgetEnforcement::PackManifestRequired
        || broad_budget.may_borrow_default_core_allowance
        || !broad_budget.disabled_by_default
        || !broad_budget.removable
    {
        return Err(LanguageRegistryError::Validation(
            "broad-language-pack budget does not preserve preregistered isolated-pack policy"
                .to_string(),
        ));
    }

    let mut candidate_ids = BTreeSet::new();
    let mut payload_roots = BTreeSet::new();
    for candidate in &trust.candidates {
        if !candidate_ids.insert(candidate.candidate_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate parser-pack candidate {}",
                candidate.candidate_id.as_str()
            )));
        }
        if !payload_roots.insert(candidate.payload_root.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate parser-pack payload root {}",
                candidate.payload_root.as_str()
            )));
        }
        validate_parser_pack_capture_declaration(candidate, broad_budget.limits.installed_bytes)?;
    }
    Ok(broad_budget.limits.installed_bytes)
}

/// Validate all declared byte ceilings before a candidate path can be traversed.
fn validate_parser_pack_capture_declaration(
    candidate: &ParserPackCandidateTrust,
    installed_byte_limit: u64,
) -> Result<(), LanguageRegistryError> {
    if candidate.eligibility != ParserPackCandidateEligibility::EvaluationOnlyUnselected
        || candidate.advertised
        || candidate.pack_id.as_str() != BROAD_LANGUAGE_PACK_ID
        || candidate.pack_abi.abi_id.as_str() != PROJECTATLAS_PACK_ABI_ID
        || candidate.pack_abi.version != PROJECTATLAS_PACK_ABI_VERSION
        || candidate.grammar_abi.abi_id.as_str() != TREE_SITTER_WASM_ABI_ID
        || candidate.grammar_abi.version != TREE_SITTER_WASM_ABI_VERSION
        || candidate.grammar_abi.state != ParserPackAbiState::PendingPackVerification
        || candidate.packaged_platform.as_str() != TREE_SITTER_WASM_PLATFORM
        || candidate.installed_bytes == 0
        || candidate.installed_bytes > installed_byte_limit
        || !metadata_text_is_valid(&candidate.release_version, MAX_ID_BYTES)
        || candidate.inventory.is_empty()
        || candidate.inventory.len() > MAX_PARSER_PACK_FILES
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} crosses its evaluation, ABI, platform, or resource contract",
            candidate.candidate_id.as_str()
        )));
    }
    let mut paths = BTreeSet::new();
    let mut declared_bytes = 0_u64;
    for file in &candidate.inventory {
        if file.bytes == 0
            || !paths.insert(file.path.clone())
            || (file.role.requires_metadata_bytes() && file.bytes > MAX_PARSER_PACK_METADATA_BYTES)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} has an invalid declared inventory row {}",
                candidate.candidate_id.as_str(),
                file.path.as_str()
            )));
        }
        declared_bytes = declared_bytes.checked_add(file.bytes).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} declared bytes overflow",
                candidate.candidate_id.as_str()
            ))
        })?;
        if declared_bytes > candidate.installed_bytes || declared_bytes > installed_byte_limit {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} declared bytes exceed the authorized ceiling",
                candidate.candidate_id.as_str()
            )));
        }
    }
    if declared_bytes != candidate.installed_bytes {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} declared inventory does not equal installed bytes",
            candidate.candidate_id.as_str()
        )));
    }
    Ok(())
}

/// Validate the fully captured evaluation trust projection.
fn validate_parser_pack_trust(
    lock: &LanguageRegistryLock,
    trust: &ParserPackTrustManifest,
    payloads: &[CandidatePayloadSnapshot],
    budget_bytes: &[u8],
) -> Result<u64, LanguageRegistryError> {
    let installed_byte_limit = validate_parser_pack_capture_authority(lock, trust, budget_bytes)?;
    if payloads.len() != trust.candidates.len() {
        return Err(LanguageRegistryError::Validation(
            "parser-pack trust and captured payload counts differ".to_string(),
        ));
    }
    for candidate in &trust.candidates {
        let snapshot = payloads
            .iter()
            .find(|payload| payload.candidate_id == candidate.candidate_id)
            .ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "parser-pack candidate {} has no captured payload",
                    candidate.candidate_id.as_str()
                ))
            })?;
        validate_parser_pack_candidate(candidate, snapshot, installed_byte_limit)?;
    }
    Ok(installed_byte_limit)
}

/// Validate one candidate's complete inventory, release manifest, and trust evidence.
fn validate_parser_pack_candidate(
    candidate: &ParserPackCandidateTrust,
    snapshot: &CandidatePayloadSnapshot,
    installed_byte_limit: u64,
) -> Result<(), LanguageRegistryError> {
    validate_parser_pack_capture_declaration(candidate, installed_byte_limit)?;
    if snapshot.payload_root != candidate.payload_root {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} payload root changed during capture",
            candidate.candidate_id.as_str()
        )));
    }
    validate_candidate_path_inventory(snapshot)?;

    if candidate.inventory.is_empty() || candidate.inventory.len() > MAX_PARSER_PACK_FILES {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} has an empty or oversized inventory",
            candidate.candidate_id.as_str()
        )));
    }
    let mut inventory = BTreeMap::new();
    let mut roles = BTreeMap::<ParserPackTrustedFileRole, usize>::new();
    let mut installed_bytes = 0_u64;
    for file in &candidate.inventory {
        if file.bytes == 0 || inventory.insert(file.path.clone(), file).is_some() {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} has a duplicate or zero-byte inventory row {}",
                candidate.candidate_id.as_str(),
                file.path.as_str()
            )));
        }
        *roles.entry(file.role).or_default() += 1;
        installed_bytes = installed_bytes.checked_add(file.bytes).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} installed bytes overflow",
                candidate.candidate_id.as_str()
            ))
        })?;
    }
    for (role, exact_count) in [
        (ParserPackTrustedFileRole::Manifest, 1),
        (ParserPackTrustedFileRole::ParserModule, 1),
        (ParserPackTrustedFileRole::WasmValidation, 1),
        (ParserPackTrustedFileRole::WasmProbeScript, 1),
        (ParserPackTrustedFileRole::Provenance, 1),
        (ParserPackTrustedFileRole::AdvisoryReport, 1),
        (ParserPackTrustedFileRole::Sbom, 1),
    ] {
        if roles.get(&role).copied() != Some(exact_count) {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} must contain exactly one {} file",
                candidate.candidate_id.as_str(),
                role.contract_tag()
            )));
        }
    }
    if roles
        .get(&ParserPackTrustedFileRole::License)
        .copied()
        .unwrap_or_default()
        == 0
        || roles
            .get(&ParserPackTrustedFileRole::Patch)
            .copied()
            .unwrap_or_default()
            != candidate.provenance.patch_paths.len()
        || installed_bytes != candidate.installed_bytes
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} license, patch, or installed-byte inventory is incomplete",
            candidate.candidate_id.as_str()
        )));
    }
    if inventory.len() != snapshot.files.len() {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} has undeclared or missing payload files",
            candidate.candidate_id.as_str()
        )));
    }
    for (path, trusted) in &inventory {
        let captured = snapshot.files.get(path).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} is missing trusted file {}",
                candidate.candidate_id.as_str(),
                path.as_str()
            ))
        })?;
        if trusted.bytes != captured.bytes || trusted.sha256 != captured.sha256 {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} file {} differs from its trusted size or digest",
                candidate.candidate_id.as_str(),
                path.as_str()
            )));
        }
    }

    validate_candidate_role_path(
        &inventory,
        &candidate.manifest_path,
        ParserPackTrustedFileRole::Manifest,
        "manifest",
    )?;
    validate_candidate_role_path(
        &inventory,
        &candidate.provenance.record_path,
        ParserPackTrustedFileRole::Provenance,
        "provenance",
    )?;
    validate_candidate_role_path(
        &inventory,
        &candidate.advisory_record_path,
        ParserPackTrustedFileRole::AdvisoryReport,
        "advisory",
    )?;
    validate_candidate_role_path(
        &inventory,
        &candidate.wasm_validation_record_path,
        ParserPackTrustedFileRole::WasmValidation,
        "WASM validation",
    )?;
    validate_candidate_role_path(
        &inventory,
        &candidate.sbom_record_path,
        ParserPackTrustedFileRole::Sbom,
        "SBOM",
    )?;
    for license in &candidate.licenses {
        validate_candidate_role_path(
            &inventory,
            &license.record_path,
            ParserPackTrustedFileRole::License,
            "license",
        )?;
    }
    for patch in &candidate.provenance.patch_paths {
        validate_candidate_role_path(&inventory, patch, ParserPackTrustedFileRole::Patch, "patch")?;
    }
    validate_candidate_release_manifest(candidate, snapshot, &inventory)?;
    validate_candidate_evidence(candidate, snapshot, &inventory)
}

/// Reject case-colliding paths and empty undeclared directories in one captured payload.
fn validate_candidate_path_inventory(
    snapshot: &CandidatePayloadSnapshot,
) -> Result<(), LanguageRegistryError> {
    let mut folded = BTreeSet::new();
    for path in snapshot.directories.iter().chain(snapshot.files.keys()) {
        if !folded.insert(path.as_str().to_ascii_lowercase()) {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload contains a case-insensitive path collision at {}",
                path.as_str()
            )));
        }
    }
    for directory in &snapshot.directories {
        let prefix = format!("{}/", directory.as_str());
        if !snapshot
            .files
            .keys()
            .any(|path| path.as_str().starts_with(&prefix))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "candidate payload contains an empty directory {}",
                directory.as_str()
            )));
        }
    }
    Ok(())
}

/// Require one trust path to exist with its exact closed role.
fn validate_candidate_role_path(
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
    path: &RegistryPath,
    role: ParserPackTrustedFileRole,
    label: &str,
) -> Result<(), LanguageRegistryError> {
    if inventory.get(path).is_some_and(|file| file.role == role) {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "parser-pack {label} path {} is absent or has the wrong role",
            path.as_str()
        )))
    }
}

/// Borrow retained metadata bytes for one exact candidate path.
fn candidate_metadata_bytes<'a>(
    snapshot: &'a CandidatePayloadSnapshot,
    path: &RegistryPath,
    label: &str,
) -> Result<&'a [u8], LanguageRegistryError> {
    snapshot
        .files
        .get(path)
        .and_then(|file| file.metadata_bytes.as_deref())
        .ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "parser-pack {label} metadata {} was not retained",
                path.as_str()
            ))
        })
}

/// Reconcile the candidate-owned release manifest with release-owned trust.
fn validate_candidate_release_manifest(
    candidate: &ParserPackCandidateTrust,
    snapshot: &CandidatePayloadSnapshot,
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
) -> Result<(), LanguageRegistryError> {
    let bytes = candidate_metadata_bytes(snapshot, &candidate.manifest_path, "manifest")?;
    let manifest = decode_parser_pack_metadata::<ParserPackReleaseManifest>(
        bytes,
        "parser-pack release manifest",
    )?;
    if manifest.schema_version != PARSER_PACK_MANIFEST_SCHEMA_VERSION
        || manifest.pack_id != candidate.pack_id
        || manifest.version != candidate.release_version
        || manifest.pack_abi.abi_id != candidate.pack_abi.abi_id
        || manifest.pack_abi.version != candidate.pack_abi.version
        || manifest.grammar_abi.abi_id != candidate.grammar_abi.abi_id
        || manifest.grammar_abi.version != candidate.grammar_abi.version
        || manifest.runtime != PackRuntime::SupervisedWorker
        || manifest.platform != candidate.packaged_platform
        || manifest.provenance.source != candidate.provenance.source
        || manifest.provenance.revision != candidate.provenance.revision
        || manifest.provenance.toolchain != candidate.provenance.toolchain
        || manifest.provenance.evidence_state != candidate.provenance.evidence_state
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} release manifest disagrees with release-owned trust",
            candidate.candidate_id.as_str()
        )));
    }
    let trusted_licenses = candidate
        .licenses
        .iter()
        .map(|license| (license.component.as_str(), license.expression.as_str()))
        .collect::<BTreeSet<_>>();
    let manifest_licenses = manifest
        .licenses
        .iter()
        .map(|license| (license.component.as_str(), license.expression.as_str()))
        .collect::<BTreeSet<_>>();
    if trusted_licenses.len() != candidate.licenses.len()
        || manifest_licenses.len() != manifest.licenses.len()
        || trusted_licenses != manifest_licenses
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} license projection differs from trust",
            candidate.candidate_id.as_str()
        )));
    }
    let trusted_artifacts = inventory
        .iter()
        .filter(|(_, file)| file.role != ParserPackTrustedFileRole::Manifest)
        .map(|(path, file)| (path.as_str(), (file.role, file.bytes, file.sha256.as_str())))
        .collect::<BTreeMap<_, _>>();
    let mut manifest_artifacts = BTreeMap::new();
    for artifact in &manifest.artifacts {
        if artifact.role == ParserPackTrustedFileRole::Manifest
            || manifest_artifacts
                .insert(
                    artifact.path.as_str(),
                    (artifact.role, artifact.bytes, artifact.sha256.as_str()),
                )
                .is_some()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} manifest has a duplicate or nested manifest artifact",
                candidate.candidate_id.as_str()
            )));
        }
    }
    if manifest_artifacts != trusted_artifacts {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} manifest artifact inventory is not exact",
            candidate.candidate_id.as_str()
        )));
    }
    Ok(())
}

/// Validate provenance, license, advisory, and SPDX evidence as one trust chain.
fn validate_candidate_evidence(
    candidate: &ParserPackCandidateTrust,
    snapshot: &CandidatePayloadSnapshot,
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
) -> Result<(), LanguageRegistryError> {
    let provenance_bytes =
        candidate_metadata_bytes(snapshot, &candidate.provenance.record_path, "provenance")?;
    let provenance = decode_parser_pack_metadata::<ParserPackProvenanceRecord>(
        provenance_bytes,
        "parser-pack provenance record",
    )?;
    let parser_file = inventory
        .iter()
        .find(|(_, file)| file.role == ParserPackTrustedFileRole::ParserModule)
        .map(|(path, file)| (path, *file))
        .ok_or_else(|| {
            LanguageRegistryError::Validation("parser-pack parser module is absent".to_string())
        })?;
    let wasm_validation_bytes = candidate_metadata_bytes(
        snapshot,
        &candidate.wasm_validation_record_path,
        "WASM validation record",
    )?;
    let wasm_validation = decode_parser_pack_metadata::<ParserPackWasmValidationRecord>(
        wasm_validation_bytes,
        "parser-pack WASM validation record",
    )?;
    validate_candidate_wasm(
        candidate,
        snapshot,
        inventory,
        parser_file,
        &wasm_validation,
    )?;
    validate_candidate_provenance(candidate, &provenance, parser_file)?;

    for license in &candidate.licenses {
        if !metadata_text_is_valid(&license.component, MAX_ID_BYTES)
            || !metadata_text_is_valid(&license.expression, MAX_ID_BYTES)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} has invalid license metadata",
                candidate.candidate_id.as_str()
            )));
        }
        let bytes = candidate_metadata_bytes(snapshot, &license.record_path, "license")?;
        let text = std::str::from_utf8(bytes).map_err(|source| {
            LanguageRegistryError::Validation(format!(
                "parser-pack license {} is not UTF-8: {source}",
                license.record_path.as_str()
            ))
        })?;
        if !text.contains(&license.expression) {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack license {} does not contain expression {}",
                license.record_path.as_str(),
                license.expression
            )));
        }
    }

    let advisory_bytes =
        candidate_metadata_bytes(snapshot, &candidate.advisory_record_path, "advisory record")?;
    let advisory = decode_parser_pack_metadata::<ParserPackAdvisoryRecord>(
        advisory_bytes,
        "parser-pack advisory record",
    )?;
    validate_candidate_advisory(candidate, &provenance, &advisory)?;

    let sbom_bytes =
        candidate_metadata_bytes(snapshot, &candidate.sbom_record_path, "SBOM record")?;
    let sbom = decode_parser_pack_metadata::<ParserPackSpdxDocument>(
        sbom_bytes,
        "parser-pack SPDX record",
    )?;
    validate_candidate_spdx(candidate, inventory, &provenance, &advisory, &sbom)
}

/// Validate the tracked module's binary header, export ABI, and retained local probe.
fn validate_candidate_wasm(
    candidate: &ParserPackCandidateTrust,
    snapshot: &CandidatePayloadSnapshot,
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
    parser_file: (&RegistryPath, &ParserPackTrustedFile),
    record: &ParserPackWasmValidationRecord,
) -> Result<(), LanguageRegistryError> {
    let module_bytes = candidate_metadata_bytes(snapshot, parser_file.0, "parser module")?;
    let exports = wasm_exports(module_bytes)?;
    if record.schema_version != PARSER_PACK_WASM_VALIDATION_SCHEMA_VERSION
        || record.format != ParserPackWasmValidationFormat::ParserPackWasmValidation
        || record.candidate_id != candidate.candidate_id
        || record.module.path != *parser_file.0
        || record.module.bytes != parser_file.1.bytes
        || record.module.sha256 != parser_file.1.sha256
        || record.module.magic_hex != "0061736d"
        || record.module.module_version != TREE_SITTER_WASM_MODULE_VERSION
        || record.module.required_function_export != TREE_SITTER_WASM_REQUIRED_EXPORT
        || record.module.grammar_abi_version != candidate.grammar_abi.version
        || !exports
            .iter()
            .any(|(name, kind)| name == TREE_SITTER_WASM_REQUIRED_EXPORT && *kind == 0)
        || record.probe.state != ParserPackWasmProbeState::LocalPassHostedPending
        || record.probe.hosted_state != ParserPackWasmHostedState::Pending
        || record.probe.tool.name != "node"
        || !metadata_text_is_valid(&record.probe.tool.version, MAX_ID_BYTES)
        || !metadata_text_is_valid(&record.probe.tool.binary_path, 1024)
        || !hex_digest_has_nonzero_digit(record.probe.tool.binary_sha256.as_str())
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} WASM validation identity is incomplete or stale",
            candidate.candidate_id.as_str()
        )));
    }
    let script = inventory.get(&record.probe.script_path).ok_or_else(|| {
        LanguageRegistryError::Validation(format!(
            "parser-pack WASM probe script {} is absent from trust",
            record.probe.script_path.as_str()
        ))
    })?;
    if script.role != ParserPackTrustedFileRole::WasmProbeScript
        || script.sha256 != record.probe.script_sha256
        || record.probe.command
            != format!(
                "node {} {}",
                record.probe.script_path.as_str(),
                record.module.path.as_str()
            )
        || record.probe.environment.memory_initial_pages == 0
        || record.probe.environment.memory_initial_pages
            != record.probe.environment.memory_maximum_pages
        || record.probe.environment.table_initial_elements == 0
        || record.probe.environment.stack_pointer == 0
        || record.probe.environment.memory_base == 0
        || record.probe.environment.table_base != 0
        || record.probe.raw_result.is_empty()
        || record.probe.raw_result.contains(['\r', '\n'])
        || sha256_hex(record.probe.raw_result.as_bytes()) != record.probe.raw_result_sha256.as_str()
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} WASM probe evidence is incomplete or not exactly bound",
            candidate.candidate_id.as_str()
        )));
    }
    reject_duplicate_json_keys(
        record.probe.raw_result.as_bytes(),
        "parser-pack WASM probe result",
    )?;
    let result = serde_json::from_str::<ParserPackWasmProbeResult>(&record.probe.raw_result)
        .map_err(|source| LanguageRegistryError::JsonDecode {
            label: "parser-pack WASM probe result",
            source,
        })?;
    let export_names = exports
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    if result.module_sha256 != parser_file.1.sha256
        || result.pointer == 0
        || result.abi != candidate.grammar_abi.version
        || result
            .exports
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != export_names
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} WASM probe result differs from the trusted module",
            candidate.candidate_id.as_str()
        )));
    }
    Ok(())
}

/// Return the exact export names and kinds from one bounded WebAssembly module.
fn wasm_exports(bytes: &[u8]) -> Result<Vec<(String, u8)>, LanguageRegistryError> {
    if bytes.len() < 8
        || bytes[..4] != [0x00, 0x61, 0x73, 0x6d]
        || u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
            != TREE_SITTER_WASM_MODULE_VERSION
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack module has invalid WebAssembly magic or version".to_string(),
        ));
    }
    let mut cursor = 8_usize;
    let mut exports = None::<Vec<(String, u8)>>;
    while cursor < bytes.len() {
        let section_id = bytes[cursor];
        cursor += 1;
        let section_bytes = usize::try_from(read_wasm_u32(bytes, &mut cursor, bytes.len())?)
            .map_err(|source| {
                LanguageRegistryError::Validation(format!(
                    "WebAssembly section length does not fit memory index: {source}"
                ))
            })?;
        let section_end = cursor.checked_add(section_bytes).ok_or_else(|| {
            LanguageRegistryError::Validation("WebAssembly section length overflowed".to_string())
        })?;
        if section_end > bytes.len() {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly section exceeds the candidate module".to_string(),
            ));
        }
        if section_id != 7 {
            cursor = section_end;
            continue;
        }
        if exports.is_some() {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly module contains duplicate export sections".to_string(),
            ));
        }
        let count =
            usize::try_from(read_wasm_u32(bytes, &mut cursor, section_end)?).map_err(|source| {
                LanguageRegistryError::Validation(format!(
                    "WebAssembly export count does not fit memory index: {source}"
                ))
            })?;
        if count > MAX_PARSER_PACK_FILES {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly export count exceeds the evaluation ceiling".to_string(),
            ));
        }
        let mut section_exports = Vec::with_capacity(count);
        for _ in 0..count {
            let name_bytes = usize::try_from(read_wasm_u32(bytes, &mut cursor, section_end)?)
                .map_err(|source| {
                    LanguageRegistryError::Validation(format!(
                        "WebAssembly export name length does not fit memory index: {source}"
                    ))
                })?;
            let name_end = cursor.checked_add(name_bytes).ok_or_else(|| {
                LanguageRegistryError::Validation(
                    "WebAssembly export name length overflowed".to_string(),
                )
            })?;
            if name_end >= section_end {
                return Err(LanguageRegistryError::Validation(
                    "WebAssembly export name exceeds its section".to_string(),
                ));
            }
            let name = std::str::from_utf8(&bytes[cursor..name_end])
                .map_err(|source| {
                    LanguageRegistryError::Validation(format!(
                        "WebAssembly export name is not UTF-8: {source}"
                    ))
                })?
                .to_string();
            cursor = name_end;
            let kind = bytes[cursor];
            cursor += 1;
            let _index = read_wasm_u32(bytes, &mut cursor, section_end)?;
            section_exports.push((name, kind));
        }
        if cursor != section_end {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly export section has trailing bytes".to_string(),
            ));
        }
        exports = Some(section_exports);
    }
    exports.ok_or_else(|| {
        LanguageRegistryError::Validation("WebAssembly module has no export section".to_string())
    })
}

/// Decode one bounded unsigned WebAssembly LEB128 value.
fn read_wasm_u32(
    bytes: &[u8],
    cursor: &mut usize,
    end: usize,
) -> Result<u32, LanguageRegistryError> {
    let mut value = 0_u32;
    for shift in (0..35).step_by(7) {
        if *cursor >= end {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly LEB128 value exceeds its section".to_string(),
            ));
        }
        let byte = bytes[*cursor];
        *cursor += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return Err(LanguageRegistryError::Validation(
                "WebAssembly LEB128 value exceeds u32".to_string(),
            ));
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(LanguageRegistryError::Validation(
        "WebAssembly LEB128 value is unterminated".to_string(),
    ))
}

/// Validate exact build inputs, toolchain identity, and honest local output identity.
fn validate_candidate_provenance(
    candidate: &ParserPackCandidateTrust,
    record: &ParserPackProvenanceRecord,
    parser_file: (&RegistryPath, &ParserPackTrustedFile),
) -> Result<(), LanguageRegistryError> {
    let toolchain = &record.resolved_toolchain;
    let expected_toolchain = format!(
        "tree-sitter-cli {} {}; emsdk {} {}; emscripten {} {}; llvm {}",
        toolchain.tree_sitter_cli.version,
        toolchain.tree_sitter_cli.revision.as_str(),
        toolchain.emsdk.version,
        toolchain.emsdk.revision.as_str(),
        toolchain.emscripten.version,
        toolchain.emscripten.revision.as_str(),
        toolchain.llvm.revision.as_str()
    );
    if record.schema_version != PARSER_PACK_PROVENANCE_SCHEMA_VERSION
        || record.format != ParserPackProvenanceFormat::ParserPackProvenance
        || record.candidate_id != candidate.candidate_id
        || record.source.repository != candidate.provenance.source
        || record.source.revision != candidate.provenance.revision
        || record.source.package_sha256 != candidate.provenance.source_package_sha256
        || record.evidence_state != candidate.provenance.evidence_state
        || candidate.provenance.evidence_state.local_output_identity
            != ParserPackLocalOutputIdentity::TwoByteIdenticalCleanOutputs
        || candidate.provenance.evidence_state.hosted_reproduction
            != ParserPackHostedReproductionState::Pending
        || candidate.provenance.evidence_state.raw_build_logs
            != ParserPackRawBuildLogRetention::NotRetained
        || candidate
            .provenance
            .evidence_state
            .packaged_windows_identity
            != ParserPackWindowsIdentityEvidence::PendingRefs128BitIdentityEvidence
        || candidate.provenance.toolchain != expected_toolchain
        || !is_https_url(&record.source.repository)
        || !is_https_url(&record.source.package_url)
        || !is_https_url(&toolchain.tree_sitter_cli.package_url)
        || !is_https_url(&toolchain.emsdk.source)
        || record.build.working_directory.as_str() != "build-a"
        || record.build.command != "tree-sitter build --wasm --output ../build-a.wasm ."
        || record.build.command.contains(['<', '>'])
        || record.build.output.path != *parser_file.0
        || record.build.output.bytes != parser_file.1.bytes
        || record.build.output.sha256 != parser_file.1.sha256
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} provenance identity or output differs from trust",
            candidate.candidate_id.as_str()
        )));
    }
    for value in [
        toolchain.tree_sitter_cli.version.as_str(),
        toolchain.tree_sitter_cli.package_integrity_sha512.as_str(),
        toolchain.emsdk.version.as_str(),
        toolchain.emscripten.version.as_str(),
        toolchain.llvm.version.as_str(),
        toolchain.node.version.as_str(),
        toolchain.python.version.as_str(),
    ] {
        if !metadata_text_is_valid(value, 512) {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack candidate {} has incomplete resolved toolchain metadata",
                candidate.candidate_id.as_str()
            )));
        }
    }
    if toolchain.tree_sitter_cli.binary_sha256.as_str().is_empty()
        || toolchain
            .emsdk
            .resolved_release_revision
            .as_str()
            .is_empty()
        || toolchain.emscripten.emcc_sha256.as_str().is_empty()
        || toolchain.llvm.clang_sha256.as_str().is_empty()
        || toolchain.llvm.wasm_ld_sha256.as_str().is_empty()
        || toolchain.node.binary_sha256.as_str().is_empty()
        || toolchain.python.binary_sha256.as_str().is_empty()
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack resolved toolchain omitted an exact digest or revision".to_string(),
        ));
    }
    if record.build.resolved_flags.is_empty()
        || record
            .build
            .resolved_flags
            .iter()
            .any(|flag| !metadata_text_is_valid(flag, 1024))
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack provenance has no complete resolved compiler flags".to_string(),
        ));
    }
    if record.build.environment_assumptions.is_empty()
        || record
            .build
            .environment_assumptions
            .iter()
            .any(|value| !metadata_text_is_valid(value, 1024))
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack provenance has no exact environment assumptions".to_string(),
        ));
    }
    let mut source_paths = BTreeSet::new();
    if record.build.source_files.is_empty()
        || record.build.source_files.iter().any(|source| {
            !source_paths.insert(source.path.clone())
                || !hex_digest_has_nonzero_digit(source.sha256.as_str())
        })
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack provenance source-file inventory is empty or duplicated".to_string(),
        ));
    }
    let expected_runs =
        BTreeMap::from([("clean-build-a", "build-a"), ("clean-build-b", "build-b")]);
    let mut run_ids = BTreeSet::new();
    if record.local_output_runs.len() != 2
        || record.local_output_runs.iter().any(|run| {
            !metadata_text_is_valid(&run.run_id, MAX_ID_BYTES)
                || !run_ids.insert(run.run_id.as_str())
                || expected_runs.get(run.run_id.as_str()).copied()
                    != Some(run.working_directory.as_str())
                || run.raw_log_retention != ParserPackRawBuildLogRetention::NotRetained
                || run.command.contains(['<', '>'])
                || run.command
                    != format!(
                        "tree-sitter build --wasm --output ../{}.wasm .",
                        run.working_directory.as_str()
                    )
                || run.bytes != record.build.output.bytes
                || run.sha256 != record.build.output.sha256
        })
    {
        return Err(LanguageRegistryError::Validation(
            "parser-pack provenance does not contain two distinct byte-identical clean builds"
                .to_string(),
        ));
    }
    Ok(())
}

/// Validate the pinned offline advisory projection for the exact source package.
fn validate_candidate_advisory(
    candidate: &ParserPackCandidateTrust,
    provenance: &ParserPackProvenanceRecord,
    advisory: &ParserPackAdvisoryRecord,
) -> Result<(), LanguageRegistryError> {
    let expected_inputs = BTreeMap::from([
        (
            "Cargo.lock",
            (
                5_164_u64,
                "a10d31bf8f75b7736a97cce7e8f8cce0347b6642489e63953a9ba542826f652e",
            ),
        ),
        (
            "Cargo.toml",
            (
                1_435_u64,
                "424f2a3176e64503609d6dfc53e5c7e81f0900255412c32b44b602b41a9eb949",
            ),
        ),
        (
            "Cargo.toml.orig",
            (
                792_u64,
                "00c047f354a97883c34e6336b1db56dc6fbae658740b49841d39d1d7129468a0",
            ),
        ),
    ]);
    let inputs = advisory
        .inputs
        .iter()
        .map(|input| (input.path.as_str(), (input.bytes, input.sha256.as_str())))
        .collect::<BTreeMap<_, _>>();
    if advisory.schema_version != PARSER_PACK_ADVISORY_SCHEMA_VERSION
        || advisory.format != ParserPackAdvisoryFormat::ParserPackAdvisories
        || advisory.candidate_id != candidate.candidate_id
        || !is_https_url(&advisory.database.source)
        || !hex_digest_has_nonzero_digit(advisory.database.revision.as_str())
        || advisory.tool.name != "cargo-audit"
        || advisory.tool.command != "cargo audit --json --no-fetch"
        || advisory.tool.exit_code != 0
        || !metadata_text_is_valid(&advisory.tool.version, MAX_ID_BYTES)
        || advisory.component.ecosystem != "crates.io"
        || advisory.component.checksum_sha256 != provenance.source.package_sha256
        || advisory.inputs.len() != expected_inputs.len()
        || inputs != expected_inputs
        || advisory.raw_result.len()
            != usize::try_from(advisory.raw_result_bytes).unwrap_or(usize::MAX)
        || advisory.raw_result.contains(['\r', '\n'])
        || sha256_hex(advisory.raw_result.as_bytes()) != advisory.raw_result_sha256.as_str()
        || candidate.release_version
            != format!(
                "{}-{}-wasm",
                advisory.component.name, advisory.component.version
            )
        || !candidate
            .licenses
            .iter()
            .any(|license| license.component == advisory.component.name)
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} advisory record is stale, incomplete, or non-clean",
            candidate.candidate_id.as_str()
        )));
    }
    reject_duplicate_json_keys(
        advisory.raw_result.as_bytes(),
        "parser-pack cargo-audit raw result",
    )?;
    let raw =
        serde_json::from_str::<CargoAuditRawResult>(&advisory.raw_result).map_err(|source| {
            LanguageRegistryError::JsonDecode {
                label: "parser-pack cargo-audit raw result",
                source,
            }
        })?;
    if raw.database.advisory_count != 1_160
        || raw.database.last_commit.is_some()
        || raw.database.last_updated.is_some()
        || raw.lockfile.dependency_count != 23
        || !raw.settings.target_arch.is_empty()
        || !raw.settings.target_os.is_empty()
        || raw.settings.severity.is_some()
        || !raw.settings.ignore.is_empty()
        || raw.settings.informational_warnings != ["unmaintained", "unsound", "notice"]
        || raw.vulnerabilities.found
        || raw.vulnerabilities.count != 0
        || !raw.vulnerabilities.list.is_empty()
        || !raw.warnings.is_empty()
        || advisory.result.dependency_count != raw.lockfile.dependency_count
        || advisory.result.vulnerabilities_found != raw.vulnerabilities.found
        || advisory.result.vulnerability_count != raw.vulnerabilities.count
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} raw advisory result does not prove the recorded clean scan",
            candidate.candidate_id.as_str()
        )));
    }
    Ok(())
}

/// Validate SPDX package/file checksums and relationships against the trusted inventory.
fn validate_candidate_spdx(
    candidate: &ParserPackCandidateTrust,
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
    provenance: &ParserPackProvenanceRecord,
    advisory: &ParserPackAdvisoryRecord,
    document: &ParserPackSpdxDocument,
) -> Result<(), LanguageRegistryError> {
    if document.spdx_version != "SPDX-2.3"
        || document.data_license != "CC0-1.0"
        || document.spdx_id != "SPDXRef-DOCUMENT"
        || !metadata_text_is_valid(&document.name, 512)
        || !is_https_url(&document.document_namespace)
        || !document
            .document_namespace
            .contains(candidate.candidate_id.as_str())
        || !metadata_text_is_valid(&document.creation_info.created, 128)
        || document.creation_info.creators.is_empty()
        || document.packages.len() != 2
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} SPDX document header is incomplete",
            candidate.candidate_id.as_str()
        )));
    }
    let candidate_package = document
        .packages
        .iter()
        .find(|package| package.spdx_id == "SPDXRef-Package-ProjectAtlas-parser-pack")
        .ok_or_else(|| {
            LanguageRegistryError::Validation(
                "parser-pack SPDX candidate package is absent".to_string(),
            )
        })?;
    let source_package = document
        .packages
        .iter()
        .find(|package| package.spdx_id == "SPDXRef-Package-tree-sitter-javascript-source")
        .ok_or_else(|| {
            LanguageRegistryError::Validation(
                "parser-pack SPDX upstream source package is absent".to_string(),
            )
        })?;
    let expected_license = candidate
        .licenses
        .iter()
        .find(|license| license.component == advisory.component.name)
        .ok_or_else(|| {
            LanguageRegistryError::Validation(
                "parser-pack SPDX package has no trusted license".to_string(),
            )
        })?;
    let parser_file = inventory
        .values()
        .find(|file| file.role == ParserPackTrustedFileRole::ParserModule)
        .ok_or_else(|| {
            LanguageRegistryError::Validation(
                "parser-pack SPDX has no trusted parser module".to_string(),
            )
        })?;
    if candidate_package.name != "ProjectAtlas tree-sitter WASM grammar pack"
        || candidate_package.version_info != candidate.release_version
        || candidate_package.download_location != "NOASSERTION"
        || candidate_package.files_analyzed
        || candidate_package.package_verification_code.is_some()
        || spdx_sha256(&candidate_package.checksums) != Some(parser_file.sha256.as_str())
        || candidate_package.license_declared != expected_license.expression
        || candidate_package.license_concluded != expected_license.expression
        || !metadata_text_is_valid(&candidate_package.copyright_text, 1024)
        || !candidate_package.external_refs.is_empty()
        || source_package.name != advisory.component.name
        || source_package.version_info != advisory.component.version
        || source_package.download_location != provenance.source.package_url
        || source_package.files_analyzed
        || source_package.package_verification_code.is_some()
        || spdx_sha256(&source_package.checksums) != Some(provenance.source.package_sha256.as_str())
        || source_package.license_declared != expected_license.expression
        || source_package.license_concluded != expected_license.expression
        || !metadata_text_is_valid(&source_package.copyright_text, 1024)
        || !source_package.external_refs.iter().any(|reference| {
            reference.category == "PACKAGE-MANAGER"
                && reference.kind == "purl"
                && reference.locator
                    == format!(
                        "pkg:cargo/{}@{}",
                        advisory.component.name, advisory.component.version
                    )
        })
        || document.document_describes.as_slice() != [candidate_package.spdx_id.as_str()]
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} SPDX package does not bind the exact source and license",
            candidate.candidate_id.as_str()
        )));
    }

    let expected_files = inventory
        .iter()
        .filter(|(_, file)| {
            matches!(
                file.role,
                ParserPackTrustedFileRole::ParserModule | ParserPackTrustedFileRole::License
            )
        })
        .map(|(path, file)| (format!("./{}", path.as_str()), *file))
        .collect::<BTreeMap<_, _>>();
    if document.files.len() != expected_files.len() {
        return Err(LanguageRegistryError::Validation(
            "parser-pack SPDX file inventory is incomplete".to_string(),
        ));
    }
    let mut spdx_ids = BTreeSet::new();
    let mut expected_relationships = BTreeSet::from([
        (
            document.spdx_id.as_str(),
            "DESCRIBES",
            candidate_package.spdx_id.as_str(),
        ),
        (
            candidate_package.spdx_id.as_str(),
            "GENERATED_FROM",
            source_package.spdx_id.as_str(),
        ),
    ]);
    for file in &document.files {
        let trusted = expected_files.get(&file.file_name).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "parser-pack SPDX contains untrusted file {}",
                file.file_name
            ))
        })?;
        if !spdx_ids.insert(file.spdx_id.as_str())
            || spdx_sha256(&file.checksums) != Some(trusted.sha256.as_str())
            || !metadata_text_is_valid(&file.copyright_text, 1024)
            || file.license_info_in_files.is_empty()
            || (trusted.role == ParserPackTrustedFileRole::ParserModule
                && file.license_concluded != expected_license.expression)
            || (trusted.role == ParserPackTrustedFileRole::License
                && file.license_concluded != "NOASSERTION")
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser-pack SPDX file {} is not exactly linked to trusted bytes",
                file.file_name
            )));
        }
        expected_relationships.insert((
            candidate_package.spdx_id.as_str(),
            "CONTAINS",
            file.spdx_id.as_str(),
        ));
    }
    let relationships = document
        .relationships
        .iter()
        .map(|relationship| {
            (
                relationship.spdx_element_id.as_str(),
                relationship.relationship_type.as_str(),
                relationship.related_spdx_element.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    if document.relationships.len() != expected_relationships.len()
        || relationships != expected_relationships
    {
        return Err(LanguageRegistryError::Validation(format!(
            "parser-pack candidate {} SPDX relationship graph is not exact",
            candidate.candidate_id.as_str()
        )));
    }
    Ok(())
}

/// Return an exact SHA-256 checksum from an SPDX checksum set.
fn spdx_sha256(checksums: &[ParserPackSpdxChecksum]) -> Option<&str> {
    checksums
        .iter()
        .find(|checksum| checksum.algorithm == "SHA256")
        .map(|checksum| checksum.checksum_value.as_str())
}

/// Return whether a validated hexadecimal identity is not the all-zero sentinel.
fn hex_digest_has_nonzero_digit(value: &str) -> bool {
    value.bytes().any(|byte| byte != b'0')
}

/// Return whether one metadata field is a bounded absolute HTTPS URL.
fn is_https_url(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() > "https://".len()
        && value.len() <= 2048
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
}

validated_id!(
    AcceptedParserId,
    "parse.",
    "Accepted-target parser-capability identity."
);
validated_id!(
    TransformId,
    "transform.",
    "Accepted pre-parse transform identity."
);
validated_id!(
    AcceptedNameId,
    "accepted.",
    "Accepted language crosswalk identity."
);

/// Required release-platform identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct PlatformId(String);

impl PlatformId {
    /// Borrow the validated platform identity.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PlatformId {
    /// Deserialize a bounded portable platform identity.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let valid = !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(de::Error::custom(format!(
                "invalid platform identity {value:?}"
            )))
        }
    }
}

/// Accepted external capability-registry format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum AcceptedRegistryFormat {
    /// `ProjectAtlas` capability-delivery inventory.
    #[serde(rename = "projectatlas.capability-registry")]
    CapabilityRegistry,
}

/// Digest algorithm used by the accepted target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum DigestAlgorithm {
    /// SHA-256.
    Sha256,
}

/// Accepted target capability tier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum CapabilityTier {
    /// File-family detection.
    Detected,
    /// Bounded parser execution.
    Parsed,
    /// Normalized symbol extraction.
    Symbols,
    /// Project-wide semantic resolution.
    Semantic,
    /// Eligible benchmark evidence.
    Benchmarked,
}

impl CapabilityTier {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Parsed => "parsed",
            Self::Symbols => "symbols",
            Self::Semantic => "semantic",
            Self::Benchmarked => "benchmarked",
        }
    }
}

/// Return whether claims form an ordered prefix of the supported evidence tiers.
fn capability_claims_are_ordered_prefix(claims: &[CapabilityTier]) -> bool {
    CAPABILITY_TIER_ORDER.starts_with(claims)
}

/// Accepted target parser kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedParserKind {
    /// Pending grammar or vetted parser capability.
    TreeSitterOrVettedParser,
    /// Existing built-in manifest adapter.
    BuiltinManifest,
}

impl AcceptedParserKind {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::TreeSitterOrVettedParser => "tree-sitter-or-vetted-parser",
            Self::BuiltinManifest => "builtin-manifest",
        }
    }
}

/// Crosswalk mapping class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum CrosswalkMapping {
    /// Standard name maps directly to one canonical mode.
    CanonicalMode,
    /// Standard name is one of the disclosed aliases.
    StandardNameAlias,
    /// Standard name selects one explicit dialect mode.
    DialectMode,
}

impl CrosswalkMapping {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::CanonicalMode => "canonical-mode",
            Self::StandardNameAlias => "standard-name-alias",
            Self::DialectMode => "dialect-mode",
        }
    }
}

/// Counts declared by the accepted capability target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCounts {
    /// Runnable mode count.
    modes: usize,
    /// Normalized parser-capability count.
    normalized_parser_capabilities: usize,
    /// General capability inventory count.
    capabilities: usize,
    /// Accepted-language crosswalk row count.
    accepted_language_crosswalk_entries: usize,
    /// Existing public mode count.
    current_public_modes: usize,
}

/// Closed detection stage declared by the accepted target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum AcceptedDetectionStage {
    /// Exact case-sensitive repository filename.
    ExactFilename,
    /// Longest registered compound extension.
    CompoundExtension,
    /// Ordinary registered extension.
    Extension,
    /// Bounded interpreter declaration.
    Shebang,
    /// Bounded deterministic content signature.
    ContentSignature,
    /// Bounded repository-context discriminator.
    ProjectContext,
}

/// Complete accepted automatic detection order.
const ACCEPTED_DETECTION_PRECEDENCE: [AcceptedDetectionStage; 6] = [
    AcceptedDetectionStage::ExactFilename,
    AcceptedDetectionStage::CompoundExtension,
    AcceptedDetectionStage::Extension,
    AcceptedDetectionStage::Shebang,
    AcceptedDetectionStage::ContentSignature,
    AcceptedDetectionStage::ProjectContext,
];

/// Detection policy declared by the accepted target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedDetectionPolicy {
    /// Ordered detection precedence.
    precedence: [AcceptedDetectionStage; 6],
    /// Whether arbitrary custom languages are accepted.
    custom_language: bool,
    /// Whether ambiguity requires a named resolver.
    ambiguity_requires_named_resolver: bool,
    /// Whether aliases must remain acyclic.
    aliases_acyclic: bool,
}

/// Deterministic operation owned by one accepted pre-parse transform.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedTransformBehavior {
    /// Extract `ObjectScript` UDL records from one export XML container in source order.
    ExportContainerToUdlRecords,
}

impl AcceptedTransformBehavior {
    /// Return the stable serialized contract value.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::ExportContainerToUdlRecords => "export-container-to-udl-records",
        }
    }
}

/// Detection ownership retained before a pre-parse transform runs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedTransformDetectionOwnership {
    /// The mode's named detection rule selects the mode before transformation.
    ModeDetectionRuleBeforeTransform,
}

impl AcceptedTransformDetectionOwnership {
    /// Return the stable serialized contract value.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::ModeDetectionRuleBeforeTransform => "mode-detection-rule-before-transform",
        }
    }
}

/// Closed denial policy for transform-side capabilities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedTransformCapabilityPolicy {
    /// The capability is denied before any transform parsing or execution.
    Denied,
}

impl AcceptedTransformCapabilityPolicy {
    /// Return the stable serialized contract value.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Denied => "denied",
        }
    }
}

/// Coverage state required after a rejected or incomplete transform.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedTransformFailureCoverage {
    /// The mode is unavailable for this input.
    Unavailable,
    /// The mode is explicitly partial or unavailable, never silently complete.
    PartialOrUnavailable,
}

impl AcceptedTransformFailureCoverage {
    /// Return the stable serialized contract value.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::PartialOrUnavailable => "partial-or-unavailable",
        }
    }
}

/// Deterministic behavior for an export containing several UDL records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedTransformMultiRecordBehavior {
    /// Parse every bounded record independently in original source order.
    ParseEachRecordInSourceOrder,
}

impl AcceptedTransformMultiRecordBehavior {
    /// Return the stable serialized contract value.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::ParseEachRecordInSourceOrder => "parse-each-record-in-source-order",
        }
    }
}

/// Hard resource limits for one accepted pre-parse transform.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedTransformLimits {
    /// Maximum bytes accepted from the original file.
    max_input_bytes: u64,
    /// Maximum combined bytes emitted for downstream parsing.
    max_derived_output_bytes: u64,
    /// Maximum records emitted from one container.
    max_records: u32,
    /// Maximum admitted container nesting depth.
    max_nesting_depth: u32,
    /// Maximum diagnostics retained from one attempt.
    max_diagnostics: u32,
    /// Hard deadline for the complete transform attempt.
    deadline_ms: u64,
}

/// Required cancellation behavior for one accepted pre-parse transform.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedTransformCancellation {
    /// Whether the transform must honor cancellation.
    enabled: bool,
    /// Maximum interval between cancellation checks.
    poll_interval_ms: u64,
    /// Maximum grace period after cancellation is observed.
    grace_period_ms: u64,
}

/// Required source mapping retained through one accepted transform.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the serialized contract exposes four independent fail-closed mapping requirements"
)]
struct AcceptedTransformSourceMapping {
    /// Preserve the original file identity for every derived record.
    original_file_identity: bool,
    /// Preserve the source range and identity of every derived record.
    per_record_provenance: bool,
    /// Map every derived fact back to the original file.
    every_derived_fact: bool,
    /// Map every diagnostic back to the original file.
    every_diagnostic: bool,
}

/// Fail-closed security policy for one XML-backed transform.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedTransformSecurityPolicy {
    /// DTD processing policy.
    dtd: AcceptedTransformCapabilityPolicy,
    /// Entity-expansion policy.
    entity_expansion: AcceptedTransformCapabilityPolicy,
    /// External-resource access policy.
    external_resources: AcceptedTransformCapabilityPolicy,
    /// Schema-fetch policy.
    schema_fetch: AcceptedTransformCapabilityPolicy,
    /// Embedded execution policy.
    execution: AcceptedTransformCapabilityPolicy,
}

/// Explicit failure behavior for every adversarial transform input class.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedTransformFailurePolicy {
    /// Coverage state for an empty container.
    empty_input: AcceptedTransformFailureCoverage,
    /// Coverage state for malformed XML or malformed records.
    malformed_input: AcceptedTransformFailureCoverage,
    /// Coverage state for an oversized container or derived output.
    oversized_input: AcceptedTransformFailureCoverage,
    /// Coverage state for excessive nesting.
    deeply_nested_input: AcceptedTransformFailureCoverage,
    /// Behavior for a valid multi-record export.
    multi_record_input: AcceptedTransformMultiRecordBehavior,
    /// Whether transform failure may fall through to an unrelated parser.
    unrelated_parser_fallback: bool,
    /// Whether symbols may be guessed after transform failure.
    guessed_symbols_after_failure: bool,
    /// Maximum support claim after any transform failure.
    coverage_after_failure: AcceptedTransformFailureCoverage,
}

/// Complete accepted contract for a deterministic pre-parse transform.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedPreParseTransform {
    /// Stable transform identity.
    transform_id: TransformId,
    /// Stable transform contract version.
    version: u32,
    /// Closed deterministic transform behavior.
    behavior: AcceptedTransformBehavior,
    /// Whether equal inputs always produce equal ordered outputs and diagnostics.
    deterministic: bool,
    /// Accepted mode that owns detection and the transformed parse result.
    target_mode_id: ModeId,
    /// Accepted parser that consumes each derived record.
    target_parser_id: AcceptedParserId,
    /// Explicit ownership boundary between detection and transformation.
    detection_ownership: AcceptedTransformDetectionOwnership,
    /// Named detection rule that selects the target mode before transformation.
    detection_rule_id: DetectionRuleId,
    /// Hard transform resource limits.
    limits: AcceptedTransformLimits,
    /// Required cancellation behavior.
    cancellation: AcceptedTransformCancellation,
    /// Required original-file and per-record source mapping.
    source_mapping: AcceptedTransformSourceMapping,
    /// Fail-closed XML and execution policy.
    security: AcceptedTransformSecurityPolicy,
    /// Explicit behavior for every failure class.
    failure_policy: AcceptedTransformFailurePolicy,
}

/// Default fields materialized for every compact accepted mode.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedModeDefaults {
    /// Whether each row belongs to the delivery target.
    accepted_delivery_target: bool,
    /// Default alias target.
    alias_of: Option<ModeId>,
    /// Detection-rule identifier template.
    detection_rule_id_template: String,
    /// Fixture identifier templates.
    fixture_id_templates: Vec<String>,
    /// Required capability tiers.
    required_claims: Vec<CapabilityTier>,
    /// Achieved capability tiers.
    achieved_claims: Vec<CapabilityTier>,
    /// Evidence lifecycle state.
    evidence_state: AcceptedEvidenceState,
    /// Advertisement lifecycle state.
    advertisement: AcceptedModeAdvertisement,
    /// Source used to derive the owner.
    owner_source: String,
    /// Source used to derive required platforms.
    required_platforms_source: String,
    /// Fields that a compact row may override.
    allowed_override_fields: Vec<String>,
}

/// Reviewed compact mode overrides declared by the accepted target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedModeOverrides {
    /// Delivery-target membership override.
    accepted_delivery_target: Option<bool>,
    /// Alias target override.
    alias_of: Option<ModeId>,
    /// Detection-rule identity override.
    detection_rule_id: Option<DetectionRuleId>,
    /// Fixture inventory override.
    fixture_ids: Option<Vec<String>>,
    /// Required capability claims override.
    required_claims: Option<Vec<CapabilityTier>>,
    /// Achieved capability claims override.
    achieved_claims: Option<Vec<CapabilityTier>>,
    /// Evidence lifecycle override.
    evidence_state: Option<AcceptedEvidenceState>,
    /// Advertisement lifecycle override.
    advertisement: Option<AcceptedModeAdvertisement>,
    /// Owner override.
    owner: Option<String>,
    /// Required platform override.
    required_platforms: Option<Vec<PlatformId>>,
}

impl AcceptedModeOverrides {
    /// Return whether the row carries no actual override.
    fn is_empty(&self) -> bool {
        self.accepted_delivery_target.is_none()
            && self.alias_of.is_none()
            && self.detection_rule_id.is_none()
            && self.fixture_ids.is_none()
            && self.required_claims.is_none()
            && self.achieved_claims.is_none()
            && self.evidence_state.is_none()
            && self.advertisement.is_none()
            && self.owner.is_none()
            && self.required_platforms.is_none()
    }
}

/// One compact mode row in the accepted target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCompactMode {
    /// Stable accepted mode identity.
    mode_id: ModeId,
    /// Public mode spelling.
    public_mode: PublicMode,
    /// Historical or target origin label.
    origin: String,
    /// Normalized parser-capability identity.
    parser_id: AcceptedParserId,
    /// Future delivery pack.
    pack_id: PackId,
    /// Optional deterministic transform that runs before the selected parser.
    pre_parse_transform: Option<AcceptedPreParseTransform>,
    /// Optional reviewed overrides.
    overrides: Option<AcceptedModeOverrides>,
}

/// Default fields materialized for every compact accepted parser.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedParserDefaults {
    /// Default parser implementation kind.
    kind: AcceptedParserKind,
    /// Optional grammar symbol.
    grammar_symbol: Option<String>,
    /// Optional tree-sitter ABI.
    tree_sitter_abi: Option<ParserAbiVersion>,
    /// Parser asset identifier template.
    asset_id_template: String,
    /// Query-pack identifier template.
    query_pack_id_template: String,
    /// Evidence lifecycle state.
    evidence_state: AcceptedParserEvidenceState,
    /// Whether the parser is currently advertised.
    advertised: bool,
    /// Source used to derive the owner.
    owner_source: String,
    /// Source used to derive required platforms.
    required_platforms_source: String,
    /// Fields that a compact row may override.
    allowed_override_fields: Vec<String>,
}

/// Reviewed compact parser overrides.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedParserOverrides {
    /// Parser implementation kind override.
    kind: Option<AcceptedParserKind>,
    /// Grammar symbol override.
    grammar_symbol: Option<String>,
    /// External parser ABI version override.
    tree_sitter_abi: Option<ParserAbiVersion>,
    /// Parser asset identity override.
    asset_id: Option<AssetId>,
    /// Extraction query-pack identity override.
    query_pack_id: Option<QueryPackId>,
    /// Evidence lifecycle override.
    evidence_state: Option<AcceptedParserEvidenceState>,
    /// Advertisement override.
    advertised: Option<bool>,
    /// Owner override.
    owner: Option<String>,
    /// Required platform override.
    required_platforms: Option<Vec<PlatformId>>,
}

impl AcceptedParserOverrides {
    /// Return whether the row carries no actual override.
    fn is_empty(&self) -> bool {
        self.kind.is_none()
            && self.grammar_symbol.is_none()
            && self.tree_sitter_abi.is_none()
            && self.asset_id.is_none()
            && self.query_pack_id.is_none()
            && self.evidence_state.is_none()
            && self.advertised.is_none()
            && self.owner.is_none()
            && self.required_platforms.is_none()
    }
}

/// One compact parser row in the accepted target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCompactParser {
    /// Stable normalized parser identity.
    parser_id: AcceptedParserId,
    /// Future delivery pack.
    pack_id: PackId,
    /// Public modes normalized through this parser.
    normalized_modes: Vec<PublicMode>,
    /// Optional reviewed overrides.
    overrides: Option<AcceptedParserOverrides>,
}

/// Closed accepted process boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum AcceptedPackProcess {
    /// The long-lived default `ProjectAtlas` process.
    #[serde(rename = "projectatlas")]
    ProjectAtlas,
    /// A separately supervised worker process.
    SupervisedWorker,
}

impl AcceptedPackProcess {
    /// Return the lock runtime that owns this accepted process boundary.
    const fn lock_runtime(self) -> PackRuntime {
        match self {
            Self::ProjectAtlas => PackRuntime::InProcess,
            Self::SupervisedWorker => PackRuntime::SupervisedWorker,
        }
    }
}

/// One pack row in the accepted target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedPack {
    /// Stable pack identity.
    pack_id: PackId,
    /// Whether the pack itself is mandatory.
    required: Option<bool>,
    /// Whether accepted breadth requires the pack.
    required_for_accepted_breadth: Option<bool>,
    /// Whether the pack is installed by default.
    installed_by_default: bool,
    /// Closed process boundary.
    process: AcceptedPackProcess,
    /// Whether scan/query may access the network.
    network_during_scan_or_query: bool,
    /// Optional language owner.
    language_owner: Option<String>,
}

/// Accepted-set count and advertisement policy.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedSetPolicy {
    /// Minimum runnable modes.
    minimum_runnable_modes: usize,
    /// Minimum normalized parser capabilities.
    minimum_normalized_parser_capabilities: usize,
    /// Exact target runnable modes.
    target_runnable_modes: usize,
    /// Exact target parser capabilities.
    target_normalized_parser_capabilities: usize,
    /// Whether aliases count as runnable modes.
    aliases_count_toward_modes: bool,
    /// Whether a shared fallback counts as a parser.
    shared_fallback_counts_as_parser: bool,
    /// Whether advertisement requires an achieved manifest.
    advertisement_requires_achieved_manifest: bool,
    /// Minimum existing public modes.
    minimum_current_public_modes: usize,
}

/// One standard-name row in the accepted language crosswalk.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCrosswalkEntry {
    /// Stable accepted-name identity.
    accepted_name_id: AcceptedNameId,
    /// Human standard-name spelling.
    standard_name: String,
    /// Optional explicit dialect.
    dialect: Option<String>,
    /// Selected accepted mode.
    mode_id: ModeId,
    /// Mapping class.
    mapping: CrosswalkMapping,
}

/// Complete accepted language-name crosswalk.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedLanguageCrosswalk {
    /// Crosswalk inventory identity.
    identity: String,
    /// Crosswalk rows.
    entries: Vec<AcceptedCrosswalkEntry>,
}

/// Closed accepted capability family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedCapabilityFamily {
    /// Normal atlas-first agent workflow behavior.
    AgentWorkflow,
    /// Optional architecture, impact, or trace analysis.
    Analysis,
    /// Typed derived enrichment.
    Enrichment,
    /// Graph entity family.
    Entity,
    /// Explicit-root call-only federation.
    Federation,
    /// Incremental indexing behavior.
    Incremental,
    /// Optional or default feature-pack lifecycle.
    Pack,
    /// Typed graph relation family.
    Relation,
    /// Lexical, semantic, or hybrid retrieval.
    Search,
    /// Graph snapshot behavior.
    Snapshot,
}

/// Closed accepted capability owner.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedCapabilityOwner {
    /// CLI, MCP, and runtime composition.
    #[serde(rename = "projectatlas-cli")]
    Cli,
    /// Shared domain contracts.
    #[serde(rename = "projectatlas-core")]
    Core,
    /// `SQLite` persistence.
    #[serde(rename = "projectatlas-db")]
    Db,
    /// Query and agent-workflow services.
    #[serde(rename = "projectatlas-service")]
    Service,
    /// Parser extraction and semantic resolution.
    #[serde(rename = "projectatlas-symbols")]
    Symbols,
}

/// Closed candidate evidence lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedEvidenceState {
    /// Required evidence has not yet passed.
    Pending,
}

impl AcceptedEvidenceState {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

/// Closed accepted-mode advertisement lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedModeAdvertisement {
    /// Public advertisement remains blocked until achieved evidence exists.
    BlockedUntilAchievedManifest,
}

impl AcceptedModeAdvertisement {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::BlockedUntilAchievedManifest => "blocked-until-achieved-manifest",
        }
    }

    /// Return whether the lifecycle permits a public support claim.
    const fn is_advertised(self) -> bool {
        match self {
            Self::BlockedUntilAchievedManifest => false,
        }
    }
}

/// Closed accepted-parser evidence lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedParserEvidenceState {
    /// Existing compiled behavior awaits later claim evidence.
    Pending,
    /// Future parser delivery awaits asset, fixture, and platform verification.
    PendingAssetFixtureAndPlatformVerification,
}

impl AcceptedParserEvidenceState {
    /// Return the stable accepted-target spelling.
    const fn contract_tag(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::PendingAssetFixtureAndPlatformVerification => {
                "pending-asset-fixture-and-platform-verification"
            }
        }
    }
}

/// Closed relation producer lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedProducerState {
    /// Existing relation behavior is projected through the legacy contract.
    ImplementedLegacyProjection,
    /// The accepted producer remains pending.
    Pending,
}

/// Closed direct or inferred evidence class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedEvidenceClass {
    /// Evidence comes directly from syntax or typed metadata.
    Direct,
    /// Evidence is a labeled deterministic inference.
    Inferred,
}

/// Closed relation persistence mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedPersistenceMode {
    /// Derived relation rows persist under a structural slot.
    PersistentDerivedSlot,
    /// Federation facts exist only inside one bounded call.
    BoundedCallMemoryOnly,
}

/// Closed accepted relation implementation state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedRelationImplementationState {
    /// The owning ledger migration remains pending.
    PendingLedgerMigration,
    /// The call-only service remains pending.
    PendingCallOnlyService,
}

/// Closed accepted accuracy-decision identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AcceptedAccuracyDecision {
    /// Decide each family through its corrected adverse confidence bound.
    CorrectedAdverseConfidenceBoundPerFamily,
}

/// Evidence requirement for one accepted support tier.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedClaimContract {
    /// Exact evidence rule for this tier.
    evidence: String,
}

/// Evidence fields required by one relation profile.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRelationEvidenceProfile {
    /// Direct or inferred evidence classes accepted by the profile.
    evidence_classes: Vec<AcceptedEvidenceClass>,
    /// Required typed evidence fields.
    required_fields: Vec<String>,
}

/// Persistence contract for one relation family profile.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRelationPersistenceProfile {
    /// Persistent slot or call-only ownership.
    mode: AcceptedPersistenceMode,
    /// Optional relation-owned payload schema.
    payload_schema: Option<String>,
    /// Ratified relation-owned stable identity fields.
    stable_identity_fields: Vec<String>,
    /// Required `SQLite` tables.
    tables: Vec<String>,
    /// Required `SQLite` indexes.
    indexes: Vec<String>,
    /// Current implementation lifecycle.
    implementation_state: AcceptedRelationImplementationState,
    /// Whether persistence is prohibited for this profile.
    persistence_prohibited: Option<bool>,
}

/// Per-family accepted accuracy gate.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRelationAccuracyGate {
    /// Minimum precision threshold.
    minimum_precision: f64,
    /// Minimum recall threshold.
    minimum_recall: f64,
    /// Metrics required for the decision.
    required_metrics: Vec<String>,
    /// Registered decision function.
    decision: AcceptedAccuracyDecision,
}

/// Traceability fields owned by one accepted relation capability.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRelationTraceability {
    /// Fully qualified typed enum variant.
    typed_enum: String,
    /// Stable serialized relation spelling.
    serialized_kind: String,
    /// Owning Rust module.
    owning_module: String,
    /// Producing module or future owner.
    producer: String,
    /// Producer implementation lifecycle.
    producer_state: AcceptedProducerState,
    /// Referenced evidence profile.
    evidence_profile: String,
    /// Referenced persistence profile.
    persistence_profile: String,
    /// Referenced invalidation profile.
    invalidation_profile: String,
    /// Public query consumers.
    query_surfaces: Vec<String>,
    /// Settings exposure path.
    settings_exposure: String,
    /// Fixture inventory source.
    fixture_inventory_source: String,
    /// Referenced accuracy gate.
    accuracy_gate: String,
    /// Current relation availability.
    availability: AcceptedEvidenceState,
}

/// One accepted non-language capability row.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCapability {
    /// Stable capability identity.
    capability_id: String,
    /// Closed capability family.
    family: AcceptedCapabilityFamily,
    /// Smallest owning crate.
    owner: AcceptedCapabilityOwner,
    /// Future delivery pack.
    pack_id: PackId,
    /// Existing or accepted public consumers.
    public_surfaces: Vec<String>,
    /// Required fixture identities.
    fixture_ids: Vec<String>,
    /// Measurable acceptance rule.
    acceptance_rule: String,
    /// Relation-only traceability contract.
    traceability: Option<AcceptedRelationTraceability>,
    /// Current evidence lifecycle.
    evidence_state: AcceptedEvidenceState,
    /// Whether this candidate is publicly advertised.
    advertised: bool,
}

/// Shared relation traceability profiles referenced by relation capabilities.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRelationTraceabilityContract {
    /// Traceability schema version.
    schema_version: u32,
    /// Capability-row source path.
    matrix_source: String,
    /// Fully qualified owning enum.
    typed_enum: String,
    /// Fixture identity template.
    fixture_id_template: String,
    /// Required fixture classes.
    fixture_classes: Vec<String>,
    /// Settings exposure path.
    settings_exposure: String,
    /// Named evidence profiles.
    evidence_profiles: BTreeMap<String, AcceptedRelationEvidenceProfile>,
    /// Named persistence profiles.
    persistence_profiles: BTreeMap<String, AcceptedRelationPersistenceProfile>,
    /// Named invalidation input sets.
    invalidation_profiles: BTreeMap<String, Vec<String>>,
    /// Named accuracy gates.
    accuracy_gates: BTreeMap<String, AcceptedRelationAccuracyGate>,
}

/// Typed envelope for the pinned accepted capability registry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedCapabilityRegistry {
    /// Accepted registry schema version.
    schema_version: u32,
    /// Accepted registry format.
    format: AcceptedRegistryFormat,
    /// Accepted registry identity.
    registry_id: AcceptedRegistryId,
    /// Binding role.
    binding_role: String,
    /// Candidate lifecycle status.
    status: String,
    /// Accepted-set digest algorithm.
    accepted_set_digest_algorithm: DigestAlgorithm,
    /// Accepted-set semantic digest.
    accepted_set_digest: Sha256Digest,
    /// Accepted-set policy.
    accepted_set_policy: AcceptedSetPolicy,
    /// Declared inventory counts.
    counts: AcceptedCounts,
    /// Required release platforms.
    required_platforms: Vec<PlatformId>,
    /// Detection policy.
    detection_policy: AcceptedDetectionPolicy,
    /// Compact mode defaults.
    mode_defaults: AcceptedModeDefaults,
    /// Compact parser defaults.
    parser_defaults: AcceptedParserDefaults,
    /// Accepted pack rows.
    packs: Vec<AcceptedPack>,
    /// Compact accepted modes.
    modes: Vec<AcceptedCompactMode>,
    /// Compact accepted parsers.
    parsers: Vec<AcceptedCompactParser>,
    /// Accepted standard-name crosswalk.
    accepted_language_crosswalk: AcceptedLanguageCrosswalk,
    /// General accepted capabilities.
    capabilities: Vec<AcceptedCapability>,
    /// Closed support-tier evidence contracts.
    claim_types: BTreeMap<CapabilityTier, AcceptedClaimContract>,
    /// Shared relation traceability contract.
    relation_traceability_contract: AcceptedRelationTraceabilityContract,
    /// Achieved manifest, which must remain absent for this candidate.
    achieved_manifest: Option<de::IgnoredAny>,
    /// Reason no achieved manifest is present.
    achieved_manifest_reason: String,
}

/// Materialized accepted mode used for semantic digest and cross-validation.
#[derive(Clone, Debug)]
struct AcceptedModeContract {
    /// Stable mode identity.
    mode_id: ModeId,
    /// Public mode spelling.
    public_mode: PublicMode,
    /// Normalized parser identity.
    parser_id: AcceptedParserId,
    /// Future delivery pack.
    pack_id: PackId,
    /// Optional deterministic transform that runs before the selected parser.
    pre_parse_transform: Option<AcceptedPreParseTransform>,
    /// Derived owner.
    owner: String,
    /// Whether the row belongs to the accepted delivery target.
    accepted_delivery_target: bool,
    /// Optional alias target.
    alias_of: Option<ModeId>,
    /// Materialized detection-rule identity.
    detection_rule_id: DetectionRuleId,
    /// Materialized fixture identities.
    fixture_ids: Vec<String>,
    /// Required release platforms.
    required_platforms: Vec<PlatformId>,
    /// Required capability claims.
    required_claims: Vec<CapabilityTier>,
    /// Achieved capability claims.
    achieved_claims: Vec<CapabilityTier>,
    /// Evidence lifecycle state.
    evidence_state: AcceptedEvidenceState,
    /// Advertisement lifecycle state.
    advertisement: AcceptedModeAdvertisement,
}

/// Materialized accepted parser used for semantic digest and cross-validation.
#[derive(Clone, Debug)]
struct AcceptedParserContract {
    /// Stable parser identity.
    parser_id: AcceptedParserId,
    /// Parser implementation kind.
    kind: AcceptedParserKind,
    /// Future delivery pack.
    pack_id: PackId,
    /// Derived owner.
    owner: String,
    /// Optional grammar symbol.
    grammar_symbol: Option<String>,
    /// Optional external parser ABI version.
    tree_sitter_abi: Option<ParserAbiVersion>,
    /// Stable future parser asset identity.
    asset_id: AssetId,
    /// Stable future extraction query-pack identity.
    query_pack_id: QueryPackId,
    /// Evidence lifecycle state.
    evidence_state: AcceptedParserEvidenceState,
    /// Whether this accepted parser is advertised.
    advertised: bool,
    /// Public modes normalized through this parser.
    normalized_modes: Vec<PublicMode>,
    /// Required release platforms.
    required_platforms: Vec<PlatformId>,
}

/// Materialized accepted target needed by the composite generator.
#[derive(Debug)]
struct AcceptedTargetContract {
    /// Typed source target.
    source: AcceptedCapabilityRegistry,
    /// Materialized modes.
    modes: Vec<AcceptedModeContract>,
    /// Materialized parsers.
    parsers: Vec<AcceptedParserContract>,
}

/// One broad or API-only historical extension row.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalDetection {
    /// Extension spelling.
    extension: String,
    /// Selected public language.
    language: String,
}

/// One historical exact-filename row.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalExactFilename {
    /// Exact filename.
    file_name: String,
    /// Deliberately conflicting supplied extension.
    conflicting_extension: String,
    /// Selected public language.
    language: String,
}

/// One historical negative detection row.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalNegativeDetection {
    /// Test path.
    path: String,
    /// Expected normalized extension.
    extension: String,
    /// Expected public language or empty string.
    language: String,
}

/// One historical extension-normalization row.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalExtensionNormalization {
    /// Test path.
    path: String,
    /// Expected normalized extension.
    extension: String,
}

/// Historical parser-kind spelling.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum HistoricalParserKind {
    /// Compiled tree-sitter parser.
    TreeSitter,
    /// Manifest adapter.
    Manifest,
    /// Structural adapter.
    Structural,
    /// Fallback parser.
    Fallback,
}

/// One intentional Cargo-routing correction.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalCargoRoutingCorrection {
    /// Stable correction case identity.
    case_id: String,
    /// Test path.
    path: String,
    /// Explicitly supplied language.
    supplied_language: String,
    /// Historical v0.3.26 candidate result.
    baseline_symbol_candidate: bool,
    /// Accepted exact-filename candidate result.
    accepted_symbol_candidate: bool,
    /// Historical parser kind.
    baseline_parser_kind: HistoricalParserKind,
    /// Accepted parser kind.
    accepted_parser_kind: HistoricalParserKind,
    /// Reviewed disposition.
    disposition: String,
    /// Durable rationale.
    rationale: String,
}

/// Historical symbol-adapter spelling.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum HistoricalSymbolAdapter {
    /// No symbol extraction.
    None,
    /// Compiled tree-sitter extraction.
    TreeSitter,
    /// Manifest extraction.
    Manifest,
    /// Vue structural extraction.
    VueStructural,
    /// `PowerShell` structural extraction.
    PowershellStructural,
    /// Fallback extraction.
    Fallback,
}

/// One ordered historical language pipeline.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalLanguagePipeline {
    /// Public language spelling.
    language: String,
    /// Historical support class.
    support: ParserSupport,
    /// Historical summary adapter.
    summary_adapter: SummaryAdapterId,
    /// Historical symbol adapter.
    symbol_adapter: HistoricalSymbolAdapter,
}

/// One ordered historical language augmenter route.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalAugmenterRoute {
    /// Public language spelling.
    language: String,
    /// Base symbol-adapter spelling.
    base_adapter: HistoricalSymbolAdapter,
    /// Ordered augmenter identity.
    augmenter: AugmenterId,
    /// Zero-based order within the route.
    ordinal: usize,
}

/// One specialized historical parser witness.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalSpecializedParser {
    /// Public language spelling.
    language: String,
    /// Exact compiled grammar component.
    parser_component: String,
    /// Minimal source witness.
    source: String,
    /// Expected symbol kind.
    symbol_kind: String,
    /// Expected symbol name.
    symbol_name: String,
}

/// Frozen path class used by the adapter-precedence cross-product.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum HistoricalAdapterPathClass {
    /// Exact Cargo manifest filename.
    CargoManifest,
    /// Exact Cargo lock filename.
    CargoLock,
    /// Vue component extension.
    Vue,
    /// `PowerShell` script extension.
    Powershell,
    /// Ordinary built-in-parser extension.
    Ordinary,
    /// Suffix-only Cargo manifest near miss.
    CargoManifestNearMiss,
    /// Suffix-only Cargo lock near miss.
    CargoLockNearMiss,
}

/// Effective adapter selected by one frozen precedence cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum HistoricalAdapterExpectation {
    /// Cargo manifest parser.
    CargoManifest,
    /// Cargo lock parser.
    CargoLock,
    /// Vue structural parser.
    Vue,
    /// `PowerShell` structural parser.
    Powershell,
    /// Built-in tree-sitter parser.
    BuiltIn,
    /// Conservative fallback parser.
    Fallback,
}

impl HistoricalAdapterExpectation {
    /// Return the registry mode that owns this effective adapter.
    const fn public_mode(self) -> &'static str {
        match self {
            Self::CargoManifest => "cargo-manifest",
            Self::CargoLock => "cargo-lock",
            Self::Vue => "vue",
            Self::Powershell => "powershell",
            Self::BuiltIn => "rust",
            Self::Fallback => "ruby",
        }
    }

    /// Return whether a current registry pipeline preserves this adapter identity.
    const fn matches_pipeline(self, pipeline: &SymbolPipeline) -> bool {
        match self {
            Self::CargoManifest => matches!(
                pipeline,
                SymbolPipeline::Manifest {
                    adapter: ManifestAdapterId::CargoManifest
                }
            ),
            Self::CargoLock => matches!(
                pipeline,
                SymbolPipeline::Manifest {
                    adapter: ManifestAdapterId::CargoLock
                }
            ),
            Self::Vue => matches!(
                pipeline,
                SymbolPipeline::Structural {
                    adapter: SymbolAdapterId::Vue
                }
            ),
            Self::Powershell => matches!(
                pipeline,
                SymbolPipeline::Structural {
                    adapter: SymbolAdapterId::Powershell
                }
            ),
            Self::BuiltIn => matches!(pipeline, SymbolPipeline::BuiltIn { .. }),
            Self::Fallback => matches!(pipeline, SymbolPipeline::Fallback { .. }),
        }
    }
}

/// One complete supplied-mode row in the adapter-precedence cross-product.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalAdapterPrecedence {
    /// Closed path class.
    path_class: HistoricalAdapterPathClass,
    /// Basename expanded across accepted repository path styles by runtime tests.
    path: String,
    /// Effective adapter without a supplied language.
    absent: HistoricalAdapterExpectation,
    /// Effective adapter for supplied cargo-manifest.
    cargo_manifest: HistoricalAdapterExpectation,
    /// Effective adapter for supplied cargo-lock.
    cargo_lock: HistoricalAdapterExpectation,
    /// Effective adapter for supplied vue.
    vue: HistoricalAdapterExpectation,
    /// Effective adapter for supplied powershell.
    powershell: HistoricalAdapterExpectation,
    /// Effective adapter for supplied rust.
    built_in: HistoricalAdapterExpectation,
    /// Effective adapter for supplied ruby.
    fallback: HistoricalAdapterExpectation,
    /// Effective adapter for an unrecognized supplied mode.
    unknown: HistoricalAdapterExpectation,
}

impl HistoricalAdapterPrecedence {
    /// Return the complete supplied-mode row in contract order.
    const fn expectations(&self) -> [HistoricalAdapterExpectation; 8] {
        [
            self.absent,
            self.cargo_manifest,
            self.cargo_lock,
            self.vue,
            self.powershell,
            self.built_in,
            self.fallback,
            self.unknown,
        ]
    }
}

/// Frozen non-specialized-path precedence shared by ordinary and Cargo near-miss paths.
const COMPATIBLE_GENERIC_PATH_PRECEDENCE: [HistoricalAdapterExpectation; 8] = [
    HistoricalAdapterExpectation::Fallback,
    HistoricalAdapterExpectation::CargoManifest,
    HistoricalAdapterExpectation::CargoLock,
    HistoricalAdapterExpectation::Vue,
    HistoricalAdapterExpectation::Powershell,
    HistoricalAdapterExpectation::BuiltIn,
    HistoricalAdapterExpectation::Fallback,
    HistoricalAdapterExpectation::Fallback,
];

/// Frozen v0.3.26 precedence with the two reviewed exact-filename corrections.
const FROZEN_COMPATIBLE_ADAPTER_PRECEDENCE: [[HistoricalAdapterExpectation; 8]; 7] = [
    [
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoLock,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoManifest,
    ],
    [HistoricalAdapterExpectation::CargoLock; 8],
    [
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoLock,
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::Vue,
    ],
    [
        HistoricalAdapterExpectation::Powershell,
        HistoricalAdapterExpectation::CargoManifest,
        HistoricalAdapterExpectation::CargoLock,
        HistoricalAdapterExpectation::Vue,
        HistoricalAdapterExpectation::Powershell,
        HistoricalAdapterExpectation::Powershell,
        HistoricalAdapterExpectation::Powershell,
        HistoricalAdapterExpectation::Powershell,
    ],
    COMPATIBLE_GENERIC_PATH_PRECEDENCE,
    COMPATIBLE_GENERIC_PATH_PRECEDENCE,
    COMPATIBLE_GENERIC_PATH_PRECEDENCE,
];

/// Complete typed historical v0.3.26 runtime contract.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalRuntimeContract {
    /// Historical fixture schema version.
    schema_version: u32,
    /// Frozen release identity.
    baseline_release: ReleaseId,
    /// Frozen commit identity.
    baseline_commit: RevisionId,
    /// Scanner-visible ordered extension rows.
    broad_detection: Vec<HistoricalDetection>,
    /// API-only ordered extension rows.
    api_only_detection: Vec<HistoricalDetection>,
    /// Exact-filename precedence rows.
    exact_filenames: Vec<HistoricalExactFilename>,
    /// Negative and case-sensitivity rows.
    negative_detection: Vec<HistoricalNegativeDetection>,
    /// Compound-extension normalization rows.
    extension_normalization: Vec<HistoricalExtensionNormalization>,
    /// Intentional exact Cargo-routing corrections.
    cargo_routing_corrections: Vec<HistoricalCargoRoutingCorrection>,
    /// Ordered current language pipelines.
    language_pipelines: Vec<HistoricalLanguagePipeline>,
    /// Ordered post-parser and fallback augmenter routes.
    augmenter_routes: Vec<HistoricalAugmenterRoute>,
    /// Compiled parser witnesses.
    specialized_parsers: Vec<HistoricalSpecializedParser>,
    /// Complete path-class by supplied-mode adapter-precedence cross-product.
    adapter_precedence: [HistoricalAdapterPrecedence; 7],
}

/// Decode, reconcile, and render the complete language-registry contract.
fn validate_and_generate(
    lock_bytes: &[u8],
    fixed_inputs: &FixedInputBytes<'_>,
) -> Result<GeneratedArtifacts, LanguageRegistryError> {
    let lock = decode_language_registry_lock(lock_bytes)?;

    verify_raw_digest(
        fixed_inputs.accepted_capability_registry,
        &lock.accepted_target.raw_sha256,
        "accepted capability registry",
    )?;
    verify_raw_digest(
        fixed_inputs.historical_runtime_contract,
        &lock.historical_contract.raw_sha256,
        "historical runtime contract",
    )?;
    verify_raw_digest(
        fixed_inputs.parser_pack_trust,
        &lock.parser_pack_trust.raw_sha256,
        "parser-pack trust manifest",
    )?;

    reject_duplicate_json_keys(
        fixed_inputs.accepted_capability_registry,
        "accepted capability registry",
    )?;
    let accepted_source = serde_json::from_slice::<AcceptedCapabilityRegistry>(
        fixed_inputs.accepted_capability_registry,
    )
    .map_err(|source| LanguageRegistryError::JsonDecode {
        label: "accepted capability registry",
        source,
    })?;
    let accepted = materialize_accepted_target(accepted_source)?;

    let historical_text = std::str::from_utf8(fixed_inputs.historical_runtime_contract)
        .map_err(|source| LanguageRegistryError::HistoricalDecode(source.to_string()))?;
    let historical_value = toon_format::decode_default::<serde_json::Value>(historical_text)
        .map_err(|source| LanguageRegistryError::HistoricalDecode(source.to_string()))?;
    let historical = serde_json::from_value::<HistoricalRuntimeContract>(historical_value)
        .map_err(|source| LanguageRegistryError::HistoricalDecode(source.to_string()))?;
    let parser_pack_trust = decode_parser_pack_trust(fixed_inputs.parser_pack_trust)?;
    let parser_pack_installed_byte_limit = validate_parser_pack_trust(
        &lock,
        &parser_pack_trust,
        fixed_inputs.parser_pack_payloads,
        fixed_inputs.repository_intelligence_contracts,
    )?;

    validate_accepted_target(&lock, &accepted)?;
    validate_registry_lock(&lock, &accepted)?;
    validate_historical_contract(&lock, &historical)?;

    let accepted_digest = accepted_set_digest(&accepted)?;
    require_equal(
        accepted_digest.as_str(),
        accepted.source.accepted_set_digest.as_str(),
        "accepted target declared semantic digest",
    )?;
    require_equal(
        accepted_digest.as_str(),
        lock.accepted_target.accepted_set_sha256.as_str(),
        "accepted target lock semantic digest",
    )?;

    let source_lock_sha256 = sha256_hex(lock_bytes);
    let registry_contract_sha256 = registry_contract_digest(
        &lock,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    render_generated_artifacts(
        &lock,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
        &source_lock_sha256,
        &registry_contract_sha256,
    )
}

/// Return the lowercase SHA-256 digest of exact bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Verify exact external bytes against their lock binding.
fn verify_raw_digest(
    bytes: &[u8],
    expected: &Sha256Digest,
    label: &str,
) -> Result<(), LanguageRegistryError> {
    let actual = sha256_hex(bytes);
    require_equal(&actual, expected.as_str(), &format!("{label} raw SHA-256"))
}

/// Require two strings to be equal while retaining their field identity.
fn require_equal(actual: &str, expected: &str, field: &str) -> Result<(), LanguageRegistryError> {
    if actual == expected {
        Ok(())
    } else {
        Err(LanguageRegistryError::Validation(format!(
            "{field} differs: expected {expected:?}, found {actual:?}"
        )))
    }
}

/// Materialize the compact accepted target exactly as its external contract specifies.
fn materialize_accepted_target(
    source: AcceptedCapabilityRegistry,
) -> Result<AcceptedTargetContract, LanguageRegistryError> {
    validate_accepted_defaults(&source)?;
    let owners = source
        .packs
        .iter()
        .filter_map(|pack| {
            pack.language_owner
                .as_ref()
                .map(|owner| (pack.pack_id.clone(), owner.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    let modes = source
        .modes
        .iter()
        .map(|mode| {
            let overrides = mode.overrides.as_ref();
            let default_owner = owners.get(&mode.pack_id).cloned().ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "accepted mode {} references pack {} without a language owner",
                    mode.mode_id.as_str(),
                    mode.pack_id.as_str()
                ))
            })?;
            let owner = overrides
                .and_then(|values| values.owner.clone())
                .unwrap_or(default_owner);
            let fixture_ids = overrides
                .and_then(|values| values.fixture_ids.clone())
                .unwrap_or_else(|| {
                    source
                        .mode_defaults
                        .fixture_id_templates
                        .iter()
                        .map(|template| {
                            template.replace("{public_mode}", mode.public_mode.as_str())
                        })
                        .collect()
                });
            let detection_rule_id = overrides
                .and_then(|values| values.detection_rule_id.clone())
                .map_or_else(
                    || {
                        DetectionRuleId::try_from(
                            source
                                .mode_defaults
                                .detection_rule_id_template
                                .replace("{public_mode}", mode.public_mode.as_str()),
                        )
                    },
                    Ok,
                )?;
            Ok(AcceptedModeContract {
                mode_id: mode.mode_id.clone(),
                public_mode: mode.public_mode.clone(),
                parser_id: mode.parser_id.clone(),
                pack_id: mode.pack_id.clone(),
                pre_parse_transform: mode.pre_parse_transform.clone(),
                owner,
                accepted_delivery_target: overrides
                    .and_then(|values| values.accepted_delivery_target)
                    .unwrap_or(source.mode_defaults.accepted_delivery_target),
                alias_of: overrides
                    .and_then(|values| values.alias_of.clone())
                    .or_else(|| source.mode_defaults.alias_of.clone()),
                detection_rule_id,
                fixture_ids,
                required_platforms: overrides
                    .and_then(|values| values.required_platforms.clone())
                    .unwrap_or_else(|| source.required_platforms.clone()),
                required_claims: overrides
                    .and_then(|values| values.required_claims.clone())
                    .unwrap_or_else(|| source.mode_defaults.required_claims.clone()),
                achieved_claims: overrides
                    .and_then(|values| values.achieved_claims.clone())
                    .unwrap_or_else(|| source.mode_defaults.achieved_claims.clone()),
                evidence_state: overrides
                    .and_then(|values| values.evidence_state)
                    .unwrap_or(source.mode_defaults.evidence_state),
                advertisement: overrides
                    .and_then(|values| values.advertisement)
                    .unwrap_or(source.mode_defaults.advertisement),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let parsers = source
        .parsers
        .iter()
        .map(|parser| {
            let overrides = parser.overrides.as_ref();
            let parser_suffix = parser
                .parser_id
                .as_str()
                .strip_prefix("parse.")
                .filter(|suffix| !suffix.is_empty())
                .ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "accepted parser {} has no materializable parser suffix",
                        parser.parser_id.as_str()
                    ))
                })?;
            let default_owner = owners.get(&parser.pack_id).cloned().ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "accepted parser {} references pack {} without a language owner",
                    parser.parser_id.as_str(),
                    parser.pack_id.as_str()
                ))
            })?;
            let owner = overrides
                .and_then(|values| values.owner.clone())
                .unwrap_or(default_owner);
            let kind = overrides
                .and_then(|values| values.kind)
                .unwrap_or(source.parser_defaults.kind);
            let asset_id = overrides
                .and_then(|values| values.asset_id.clone())
                .map_or_else(
                    || {
                        AssetId::try_from(
                            source
                                .parser_defaults
                                .asset_id_template
                                .replace("{parser_suffix}", parser_suffix),
                        )
                    },
                    Ok,
                )?;
            let query_pack_id = overrides
                .and_then(|values| values.query_pack_id.clone())
                .map_or_else(
                    || {
                        QueryPackId::try_from(
                            source
                                .parser_defaults
                                .query_pack_id_template
                                .replace("{parser_suffix}", parser_suffix),
                        )
                    },
                    Ok,
                )?;
            Ok(AcceptedParserContract {
                parser_id: parser.parser_id.clone(),
                kind,
                pack_id: parser.pack_id.clone(),
                owner,
                grammar_symbol: overrides
                    .and_then(|values| values.grammar_symbol.clone())
                    .or_else(|| source.parser_defaults.grammar_symbol.clone()),
                tree_sitter_abi: overrides
                    .and_then(|values| values.tree_sitter_abi.clone())
                    .or_else(|| source.parser_defaults.tree_sitter_abi.clone()),
                asset_id,
                query_pack_id,
                evidence_state: overrides
                    .and_then(|values| values.evidence_state)
                    .unwrap_or(source.parser_defaults.evidence_state),
                advertised: overrides
                    .and_then(|values| values.advertised)
                    .unwrap_or(source.parser_defaults.advertised),
                normalized_modes: parser.normalized_modes.clone(),
                required_platforms: overrides
                    .and_then(|values| values.required_platforms.clone())
                    .unwrap_or_else(|| source.required_platforms.clone()),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(AcceptedTargetContract {
        source,
        modes,
        parsers,
    })
}

/// Validate materialization templates and fail-closed accepted-target defaults.
fn validate_accepted_defaults(
    source: &AcceptedCapabilityRegistry,
) -> Result<(), LanguageRegistryError> {
    let mode = &source.mode_defaults;
    if !mode.accepted_delivery_target
        || mode.alias_of.is_some()
        || mode.detection_rule_id_template != "detect.{public_mode}"
        || mode.fixture_id_templates
            != [
                "lang.{public_mode}.valid".to_string(),
                "lang.{public_mode}.malformed".to_string(),
            ]
        || mode.required_claims != [CapabilityTier::Detected, CapabilityTier::Parsed]
        || !mode.achieved_claims.is_empty()
        || mode.evidence_state != AcceptedEvidenceState::Pending
        || mode.advertisement != AcceptedModeAdvertisement::BlockedUntilAchievedManifest
        || mode.owner_source != "pack.language_owner"
        || mode.required_platforms_source != "registry.required_platforms"
        || !mode
            .allowed_override_fields
            .iter()
            .map(String::as_str)
            .eq(ACCEPTED_MODE_OVERRIDE_FIELDS.iter().copied())
    {
        return Err(LanguageRegistryError::Validation(
            "accepted mode materialization defaults drifted".to_string(),
        ));
    }
    let parser = &source.parser_defaults;
    if parser.kind != AcceptedParserKind::TreeSitterOrVettedParser
        || parser.grammar_symbol.is_some()
        || parser.tree_sitter_abi.is_some()
        || parser.asset_id_template != "asset.{parser_suffix}"
        || parser.query_pack_id_template != "queries.{parser_suffix}"
        || parser.evidence_state
            != AcceptedParserEvidenceState::PendingAssetFixtureAndPlatformVerification
        || parser.advertised
        || parser.owner_source != "pack.language_owner"
        || parser.required_platforms_source != "registry.required_platforms"
        || !parser
            .allowed_override_fields
            .iter()
            .map(String::as_str)
            .eq(ACCEPTED_PARSER_OVERRIDE_FIELDS.iter().copied())
    {
        return Err(LanguageRegistryError::Validation(
            "accepted parser materialization defaults drifted".to_string(),
        ));
    }
    Ok(())
}

/// Validate the only accepted pre-parse transform and reject every other owner.
fn validate_accepted_pre_parse_transform(
    mode: &AcceptedModeContract,
) -> Result<(), LanguageRegistryError> {
    if mode.mode_id.as_str() != OBJECTSCRIPT_EXPORT_XML_MODE_ID {
        if mode.pre_parse_transform.is_some() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} cannot own a pre-parse transform",
                mode.mode_id.as_str()
            )));
        }
        return Ok(());
    }

    let transform = mode.pre_parse_transform.as_ref().ok_or_else(|| {
        LanguageRegistryError::Validation(format!(
            "accepted mode {} is missing its required pre-parse transform",
            mode.mode_id.as_str()
        ))
    })?;
    if transform.transform_id.as_str() != OBJECTSCRIPT_EXPORT_XML_TRANSFORM_ID
        || transform.version != OBJECTSCRIPT_EXPORT_XML_TRANSFORM_VERSION
        || !transform.deterministic
        || transform.target_mode_id != mode.mode_id
        || transform.target_parser_id != mode.parser_id
        || transform.target_parser_id.as_str() != OBJECTSCRIPT_UDL_PARSER_ID
        || transform.detection_rule_id != mode.detection_rule_id
    {
        return Err(LanguageRegistryError::Validation(format!(
            "accepted mode {} pre-parse transform identity, target, detection ownership, or deterministic behavior drifted",
            mode.mode_id.as_str()
        )));
    }

    let limits = &transform.limits;
    if limits.max_input_bytes != OBJECTSCRIPT_EXPORT_XML_MAX_INPUT_BYTES
        || limits.max_derived_output_bytes != OBJECTSCRIPT_EXPORT_XML_MAX_DERIVED_OUTPUT_BYTES
        || limits.max_records != OBJECTSCRIPT_EXPORT_XML_MAX_RECORDS
        || limits.max_nesting_depth != OBJECTSCRIPT_EXPORT_XML_MAX_NESTING_DEPTH
        || limits.max_diagnostics != OBJECTSCRIPT_EXPORT_XML_MAX_DIAGNOSTICS
        || limits.deadline_ms != OBJECTSCRIPT_EXPORT_XML_DEADLINE_MS
    {
        return Err(LanguageRegistryError::Validation(format!(
            "accepted mode {} pre-parse transform limits drifted",
            mode.mode_id.as_str()
        )));
    }

    let cancellation = &transform.cancellation;
    if !cancellation.enabled
        || cancellation.poll_interval_ms != OBJECTSCRIPT_EXPORT_XML_CANCELLATION_POLL_MS
        || cancellation.grace_period_ms != OBJECTSCRIPT_EXPORT_XML_CANCELLATION_GRACE_MS
    {
        return Err(LanguageRegistryError::Validation(format!(
            "accepted mode {} pre-parse transform cancellation contract drifted",
            mode.mode_id.as_str()
        )));
    }

    let mapping = &transform.source_mapping;
    if !mapping.original_file_identity
        || !mapping.per_record_provenance
        || !mapping.every_derived_fact
        || !mapping.every_diagnostic
    {
        return Err(LanguageRegistryError::Validation(format!(
            "accepted mode {} pre-parse transform source mapping is incomplete",
            mode.mode_id.as_str()
        )));
    }

    let failure = &transform.failure_policy;
    if failure.empty_input != AcceptedTransformFailureCoverage::Unavailable
        || failure.malformed_input != AcceptedTransformFailureCoverage::PartialOrUnavailable
        || failure.oversized_input != AcceptedTransformFailureCoverage::Unavailable
        || failure.deeply_nested_input != AcceptedTransformFailureCoverage::Unavailable
        || failure.unrelated_parser_fallback
        || failure.guessed_symbols_after_failure
        || failure.coverage_after_failure != AcceptedTransformFailureCoverage::PartialOrUnavailable
    {
        return Err(LanguageRegistryError::Validation(format!(
            "accepted mode {} pre-parse transform failure behavior drifted",
            mode.mode_id.as_str()
        )));
    }
    Ok(())
}

/// Recompute the accepted target's delimiter-based semantic digest.
fn accepted_set_digest(
    accepted: &AcceptedTargetContract,
) -> Result<Sha256Digest, LanguageRegistryError> {
    let policy = &accepted.source.accepted_set_policy;
    let platforms = accepted
        .source
        .required_platforms
        .iter()
        .map(PlatformId::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let mut parts = vec![format!(
        "policy|{}|{}|{platforms}",
        policy.target_runnable_modes, policy.target_normalized_parser_capabilities
    )];

    let mut modes = accepted.modes.iter().collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode.mode_id.as_str());
    for mode in modes {
        let pre_parse_transform =
            serde_json::to_string(&mode.pre_parse_transform).map_err(|source| {
                LanguageRegistryError::Validation(format!(
                    "accepted pre-parse transform serialization failed: {source}"
                ))
            })?;
        parts.push(format!(
            "mode|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            mode.mode_id.as_str(),
            mode.public_mode.as_str(),
            mode.parser_id.as_str(),
            mode.pack_id.as_str(),
            mode.owner,
            mode.alias_of.as_ref().map_or("", ModeId::as_str),
            mode.fixture_ids.join(","),
            mode.required_platforms
                .iter()
                .map(PlatformId::as_str)
                .collect::<Vec<_>>()
                .join(","),
            mode.required_claims
                .iter()
                .map(|claim| claim.contract_tag())
                .collect::<Vec<_>>()
                .join(","),
            pre_parse_transform
        ));
    }

    let mut parsers = accepted.parsers.iter().collect::<Vec<_>>();
    parsers.sort_by_key(|parser| parser.parser_id.as_str());
    for parser in parsers {
        parts.push(format!(
            "parser|{}|{}|{}|{}|{}|{}",
            parser.parser_id.as_str(),
            parser.kind.contract_tag(),
            parser.pack_id.as_str(),
            parser.owner,
            parser
                .normalized_modes
                .iter()
                .map(PublicMode::as_str)
                .collect::<Vec<_>>()
                .join(","),
            parser
                .required_platforms
                .iter()
                .map(PlatformId::as_str)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    let mut crosswalk = accepted
        .source
        .accepted_language_crosswalk
        .entries
        .iter()
        .collect::<Vec<_>>();
    crosswalk.sort_by_key(|row| row.accepted_name_id.as_str());
    for row in crosswalk {
        parts.push(format!(
            "crosswalk|{}|{}|{}|{}|{}",
            row.accepted_name_id.as_str(),
            row.standard_name,
            row.dialect.as_deref().unwrap_or_default(),
            row.mode_id.as_str(),
            row.mapping.contract_tag()
        ));
    }

    let digest = sha256_hex(parts.join("\n").as_bytes());
    Ok(Sha256Digest(digest))
}

/// Validate accepted capability rows and their closed relation traceability profiles.
fn validate_accepted_capabilities(
    source: &AcceptedCapabilityRegistry,
    pack_ids: &BTreeSet<PackId>,
) -> Result<(), LanguageRegistryError> {
    const RELATION_MATRIX_SOURCE: &str = "capabilities[family=relation].traceability";
    const RELATION_ENUM: &str = "projectatlas_core::graph::GraphRelationKind";
    const RELATION_OWNER: &str = "projectatlas_core::graph";
    const RELATION_SETTINGS: &str = "atlas_settings.graph.relation_families";
    const FIXTURE_SOURCE: &str = "capability.fixture_ids";

    let expected_claims = BTreeSet::from([
        CapabilityTier::Detected,
        CapabilityTier::Parsed,
        CapabilityTier::Symbols,
        CapabilityTier::Semantic,
        CapabilityTier::Benchmarked,
    ]);
    let actual_claims = source.claim_types.keys().copied().collect::<BTreeSet<_>>();
    if actual_claims != expected_claims
        || source
            .claim_types
            .values()
            .any(|claim| claim.evidence.trim().is_empty())
    {
        return Err(LanguageRegistryError::Validation(
            "accepted claim-type evidence contracts are incomplete".to_string(),
        ));
    }

    let contract = &source.relation_traceability_contract;
    let fixture_classes = contract
        .fixture_classes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if contract.schema_version == 0
        || contract.matrix_source != RELATION_MATRIX_SOURCE
        || contract.typed_enum != RELATION_ENUM
        || contract.fixture_id_template.trim().is_empty()
        || fixture_classes
            != BTreeSet::from([
                "positive",
                "ambiguous-or-unresolved",
                "adversarial-negative",
            ])
        || contract.settings_exposure != RELATION_SETTINGS
        || contract.evidence_profiles.is_empty()
        || contract.persistence_profiles.is_empty()
        || contract.invalidation_profiles.is_empty()
        || contract.accuracy_gates.is_empty()
    {
        return Err(LanguageRegistryError::Validation(
            "accepted relation traceability contract identity or inventory drifted".to_string(),
        ));
    }
    for (profile_id, profile) in &contract.evidence_profiles {
        let classes = profile
            .evidence_classes
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let fields = profile.required_fields.iter().collect::<BTreeSet<_>>();
        if profile_id.trim().is_empty()
            || classes.len() != profile.evidence_classes.len()
            || classes.is_empty()
            || fields.len() != profile.required_fields.len()
            || fields.iter().any(|field| field.trim().is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted relation evidence profile {profile_id:?} is invalid"
            )));
        }
    }
    for (profile_id, profile) in &contract.persistence_profiles {
        let valid = profile.payload_schema.is_none()
            && profile.stable_identity_fields.is_empty()
            && match profile.mode {
                AcceptedPersistenceMode::PersistentDerivedSlot => {
                    profile.implementation_state
                        == AcceptedRelationImplementationState::PendingLedgerMigration
                        && profile.persistence_prohibited.is_none()
                        && !profile.tables.is_empty()
                        && !profile.indexes.is_empty()
                }
                AcceptedPersistenceMode::BoundedCallMemoryOnly => {
                    profile.implementation_state
                        == AcceptedRelationImplementationState::PendingCallOnlyService
                        && profile.persistence_prohibited == Some(true)
                        && profile.tables.is_empty()
                        && profile.indexes.is_empty()
                }
            };
        if profile_id.trim().is_empty()
            || !valid
            || profile.tables.iter().any(|table| table.trim().is_empty())
            || profile.indexes.iter().any(|index| index.trim().is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted relation persistence profile {profile_id:?} is invalid"
            )));
        }
    }
    for (profile_id, inputs) in &contract.invalidation_profiles {
        let unique = inputs.iter().collect::<BTreeSet<_>>();
        if profile_id.trim().is_empty()
            || inputs.is_empty()
            || unique.len() != inputs.len()
            || inputs.iter().any(|input| input.trim().is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted relation invalidation profile {profile_id:?} is invalid"
            )));
        }
    }
    for (gate_id, gate) in &contract.accuracy_gates {
        if gate_id.trim().is_empty()
            || !(0.0..=1.0).contains(&gate.minimum_precision)
            || !(0.0..=1.0).contains(&gate.minimum_recall)
            || gate.minimum_precision == 0.0
            || gate.minimum_recall == 0.0
            || gate.required_metrics.is_empty()
            || gate
                .required_metrics
                .iter()
                .any(|metric| metric.trim().is_empty())
            || gate.decision != AcceptedAccuracyDecision::CorrectedAdverseConfidenceBoundPerFamily
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted relation accuracy gate {gate_id:?} is invalid"
            )));
        }
    }

    let mut capability_ids = BTreeSet::new();
    let mut relation_kinds = BTreeSet::new();
    for capability in &source.capabilities {
        let Some((prefix, _)) = capability.capability_id.split_once('.') else {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted capability {:?} has no responsibility prefix",
                capability.capability_id
            )));
        };
        validate_identifier(&capability.capability_id, &format!("{prefix}."))?;
        let public_surfaces = capability.public_surfaces.iter().collect::<BTreeSet<_>>();
        let fixture_ids = capability.fixture_ids.iter().collect::<BTreeSet<_>>();
        if !capability_ids.insert(capability.capability_id.as_str())
            || !pack_ids.contains(&capability.pack_id)
            || capability.public_surfaces.is_empty()
            || public_surfaces.len() != capability.public_surfaces.len()
            || capability
                .public_surfaces
                .iter()
                .any(|surface| surface.trim().is_empty())
            || capability.fixture_ids.is_empty()
            || fixture_ids.len() != capability.fixture_ids.len()
            || capability
                .fixture_ids
                .iter()
                .any(|fixture| fixture.trim().is_empty())
            || capability.acceptance_rule.trim().is_empty()
            || capability.evidence_state != AcceptedEvidenceState::Pending
            || capability.advertised
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted capability {:?} owned by {:?} is invalid",
                capability.capability_id, capability.owner
            )));
        }

        let is_relation = capability.family == AcceptedCapabilityFamily::Relation;
        if is_relation != capability.traceability.is_some() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted capability {:?} has inconsistent relation traceability",
                capability.capability_id
            )));
        }
        let Some(trace) = capability.traceability.as_ref() else {
            continue;
        };
        let variant = trace
            .serialized_kind
            .split('-')
            .map(|part| {
                let mut characters = part.chars();
                characters.next().map_or_else(String::new, |first| {
                    first.to_ascii_uppercase().to_string() + characters.as_str()
                })
            })
            .collect::<String>();
        let legacy_kind = matches!(
            trace.serialized_kind.as_str(),
            "calls" | "contains" | "depends-on" | "imports"
        );
        let legacy_projection =
            trace.producer_state == AcceptedProducerState::ImplementedLegacyProjection;
        if !relation_kinds.insert(trace.serialized_kind.as_str())
            || trace.serialized_kind.trim().is_empty()
            || trace.typed_enum != format!("{}::{variant}", contract.typed_enum)
            || trace.owning_module != RELATION_OWNER
            || trace.producer.trim().is_empty()
            || legacy_kind != legacy_projection
            || !contract
                .evidence_profiles
                .contains_key(&trace.evidence_profile)
            || !contract
                .persistence_profiles
                .contains_key(&trace.persistence_profile)
            || !contract
                .invalidation_profiles
                .contains_key(&trace.invalidation_profile)
            || trace.query_surfaces.is_empty()
            || capability
                .public_surfaces
                .iter()
                .any(|surface| !trace.query_surfaces.contains(surface))
            || trace.settings_exposure != contract.settings_exposure
            || trace.fixture_inventory_source != FIXTURE_SOURCE
            || !contract.accuracy_gates.contains_key(&trace.accuracy_gate)
            || trace.availability != AcceptedEvidenceState::Pending
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted relation capability {:?} is not traceable",
                capability.capability_id
            )));
        }
    }
    Ok(())
}

/// Validate the accepted future-delivery target without treating it as current behavior.
fn validate_accepted_target(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> Result<(), LanguageRegistryError> {
    let source = &accepted.source;
    if source.schema_version != ACCEPTED_CAPABILITY_SCHEMA_VERSION
        || source.format != AcceptedRegistryFormat::CapabilityRegistry
        || source.accepted_set_digest_algorithm != DigestAlgorithm::Sha256
        || source.binding_role != "delivery-inventory"
        || source.status != "candidate-pending-evidence"
        || source.achieved_manifest.is_some()
        || source.achieved_manifest_reason.trim().is_empty()
    {
        return Err(LanguageRegistryError::Validation(
            "accepted target envelope is not a pending delivery inventory".to_string(),
        ));
    }
    require_equal(
        lock.accepted_target.path.as_str(),
        ACCEPTED_TARGET_PATH,
        "accepted target path",
    )?;
    require_equal(
        source.registry_id.as_str(),
        lock.accepted_target.registry_id.as_str(),
        "accepted target registry identity",
    )?;

    let counts = &source.counts;
    let policy = &source.accepted_set_policy;
    if counts.modes != accepted.modes.len()
        || counts.normalized_parser_capabilities != accepted.parsers.len()
        || counts.capabilities != source.capabilities.len()
        || counts.accepted_language_crosswalk_entries
            != source.accepted_language_crosswalk.entries.len()
        || policy.minimum_runnable_modes > counts.modes
        || policy.target_runnable_modes != counts.modes
        || policy.minimum_normalized_parser_capabilities > counts.normalized_parser_capabilities
        || policy.target_normalized_parser_capabilities != counts.normalized_parser_capabilities
        || policy.minimum_current_public_modes != counts.current_public_modes
        || policy.aliases_count_toward_modes
        || policy.shared_fallback_counts_as_parser
        || !policy.advertisement_requires_achieved_manifest
    {
        return Err(LanguageRegistryError::Validation(
            "accepted target counts or fail-closed set policy drifted".to_string(),
        ));
    }
    if source.required_platforms.is_empty()
        || source.claim_types.is_empty()
        || source.mode_defaults.allowed_override_fields.is_empty()
        || source.parser_defaults.allowed_override_fields.is_empty()
    {
        return Err(LanguageRegistryError::Validation(
            "accepted target omits required platform, claim, or override policy".to_string(),
        ));
    }
    if source.detection_policy.precedence != ACCEPTED_DETECTION_PRECEDENCE
        || source.detection_policy.custom_language
        || !source.detection_policy.ambiguity_requires_named_resolver
        || !source.detection_policy.aliases_acyclic
    {
        return Err(LanguageRegistryError::Validation(
            "accepted target detection precedence or ambiguity policy drifted".to_string(),
        ));
    }

    let lock_pack_runtimes = lock
        .packs
        .iter()
        .map(|pack| (pack.pack_id.clone(), pack.runtime))
        .collect::<BTreeMap<_, _>>();
    let mut pack_ids = BTreeSet::new();
    let mut pack_language_owners = BTreeMap::new();
    for pack in &source.packs {
        if !pack_ids.insert(pack.pack_id.clone()) || pack.network_during_scan_or_query {
            return Err(LanguageRegistryError::Validation(format!(
                "invalid or duplicate accepted pack {}",
                pack.pack_id.as_str()
            )));
        }
        let expected_process = match pack.pack_id.as_str() {
            DEFAULT_CORE_PACK_ID
                if pack.required == Some(true)
                    && pack.required_for_accepted_breadth.is_none()
                    && pack.installed_by_default
                    && pack.language_owner.is_some() =>
            {
                AcceptedPackProcess::ProjectAtlas
            }
            BROAD_LANGUAGE_PACK_ID
                if pack.required.is_none()
                    && pack.required_for_accepted_breadth == Some(true)
                    && !pack.installed_by_default
                    && pack.language_owner.is_some() =>
            {
                AcceptedPackProcess::SupervisedWorker
            }
            SEMANTIC_PACK_ID
                if pack.required == Some(false)
                    && pack.required_for_accepted_breadth.is_none()
                    && !pack.installed_by_default
                    && pack.language_owner.is_none() =>
            {
                AcceptedPackProcess::SupervisedWorker
            }
            _ => {
                return Err(LanguageRegistryError::Validation(format!(
                    "accepted pack {} has inconsistent ownership fields",
                    pack.pack_id.as_str()
                )));
            }
        };
        if pack.process != expected_process {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted pack {} crosses its required process boundary: expected {:?}, found {:?}",
                pack.pack_id.as_str(),
                expected_process,
                pack.process
            )));
        }
        let lock_runtime = lock_pack_runtimes.get(&pack.pack_id).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "accepted pack {} has no matching lock runtime",
                pack.pack_id.as_str()
            ))
        })?;
        if pack.process.lock_runtime() != *lock_runtime {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted pack {} process {:?} disagrees with lock runtime {:?}",
                pack.pack_id.as_str(),
                pack.process,
                lock_runtime
            )));
        }
        if let Some(owner) = &pack.language_owner {
            pack_language_owners.insert(pack.pack_id.clone(), owner.clone());
        }
    }
    let lock_pack_ids = lock
        .packs
        .iter()
        .map(|pack| pack.pack_id.clone())
        .collect::<BTreeSet<_>>();
    if pack_ids != lock_pack_ids {
        let missing = pack_ids
            .difference(&lock_pack_ids)
            .map(PackId::as_str)
            .collect::<Vec<_>>();
        let unexpected = lock_pack_ids
            .difference(&pack_ids)
            .map(PackId::as_str)
            .collect::<Vec<_>>();
        return Err(LanguageRegistryError::Validation(format!(
            "language registry pack ownership differs from the accepted target: missing declarations {missing:?}, unexpected declarations {unexpected:?}"
        )));
    }
    validate_accepted_capabilities(source, &pack_ids)?;

    let mut mode_ids = BTreeSet::new();
    let mut public_modes = BTreeSet::new();
    let mut detection_rule_ids = BTreeSet::new();
    let mut modes_by_parser =
        BTreeMap::<AcceptedParserId, (PackId, String, BTreeSet<PublicMode>)>::new();
    for (compact, mode) in source.modes.iter().zip(&accepted.modes) {
        let fixture_ids = mode.fixture_ids.iter().collect::<BTreeSet<_>>();
        if compact.origin.trim().is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} has an empty origin",
                mode.mode_id.as_str()
            )));
        }
        if compact
            .overrides
            .as_ref()
            .is_some_and(AcceptedModeOverrides::is_empty)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} declares an empty override object",
                mode.mode_id.as_str()
            )));
        }
        validate_accepted_pre_parse_transform(mode)?;
        if !mode_ids.insert(mode.mode_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate accepted mode identifier {}",
                mode.mode_id.as_str()
            )));
        }
        if !public_modes.insert(mode.public_mode.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate accepted public mode {}",
                mode.public_mode.as_str()
            )));
        }
        if !detection_rule_ids.insert(mode.detection_rule_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate accepted detection-rule identifier {}",
                mode.detection_rule_id.as_str()
            )));
        }
        let expected_owner = pack_language_owners.get(&mode.pack_id).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "accepted mode {} references undeclared language pack {}",
                mode.mode_id.as_str(),
                mode.pack_id.as_str()
            ))
        })?;
        if &mode.owner != expected_owner {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} owner {:?} does not match pack {} language owner {:?}",
                mode.mode_id.as_str(),
                mode.owner,
                mode.pack_id.as_str(),
                expected_owner
            )));
        }
        if !mode.accepted_delivery_target {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} is a phantom outside the delivery target",
                mode.mode_id.as_str()
            )));
        }
        if mode.fixture_ids.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} has no required fixtures",
                mode.mode_id.as_str()
            )));
        }
        if fixture_ids.len() != mode.fixture_ids.len()
            || mode
                .fixture_ids
                .iter()
                .any(|fixture| fixture.trim().is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} has an empty or duplicate fixture identity",
                mode.mode_id.as_str()
            )));
        }
        if mode.required_platforms.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} has no required release platforms",
                mode.mode_id.as_str()
            )));
        }
        if mode.required_claims.is_empty()
            || !capability_claims_are_ordered_prefix(&mode.required_claims)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} required_claims {:?} are not a supported ordered prefix of {:?}",
                mode.mode_id.as_str(),
                mode.required_claims,
                CAPABILITY_TIER_ORDER
            )));
        }
        if !capability_claims_are_ordered_prefix(&mode.achieved_claims)
            || !mode.required_claims.starts_with(&mode.achieved_claims)
            || !mode.achieved_claims.is_empty()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted mode {} achieved_claims {:?} are unsupported for the pending accepted target",
                mode.mode_id.as_str(),
                mode.achieved_claims
            )));
        }
        match modes_by_parser.entry(mode.parser_id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((
                    mode.pack_id.clone(),
                    mode.owner.clone(),
                    BTreeSet::from([mode.public_mode.clone()]),
                ));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (pack_id, owner, modes) = entry.get_mut();
                if pack_id != &mode.pack_id || owner != &mode.owner {
                    return Err(LanguageRegistryError::Validation(format!(
                        "accepted modes sharing parser {} disagree on pack ownership",
                        mode.parser_id.as_str()
                    )));
                }
                modes.insert(mode.public_mode.clone());
            }
        }
    }
    for mode in &accepted.modes {
        if let Some(alias) = &mode.alias_of {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted canonical delivery mode {} cannot declare alias target {}",
                mode.mode_id.as_str(),
                alias.as_str()
            )));
        }
    }

    let mut parser_ids = BTreeSet::new();
    for (compact, parser) in source.parsers.iter().zip(&accepted.parsers) {
        if !parser_ids.insert(parser.parser_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate accepted parser identifier {}",
                parser.parser_id.as_str()
            )));
        }
        if compact
            .overrides
            .as_ref()
            .is_some_and(AcceptedParserOverrides::is_empty)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} declares an empty override object",
                parser.parser_id.as_str()
            )));
        }
        let expected_owner = pack_language_owners.get(&parser.pack_id).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "accepted parser {} references undeclared language pack {}",
                parser.parser_id.as_str(),
                parser.pack_id.as_str()
            ))
        })?;
        if &parser.owner != expected_owner {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} owner {:?} does not match pack {} language owner {:?}",
                parser.parser_id.as_str(),
                parser.owner,
                parser.pack_id.as_str(),
                expected_owner
            )));
        }
        if parser.required_platforms.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} has no required release platforms",
                parser.parser_id.as_str()
            )));
        }
        if parser.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID
            && (parser.evidence_state
                != AcceptedParserEvidenceState::PendingAssetFixtureAndPlatformVerification
                || parser.advertised)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted broad parser {} must remain pending asset, fixture, and platform verification and unadvertised",
                parser.parser_id.as_str()
            )));
        }
        if parser.advertised {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} is advertised before its evidence is complete",
                parser.parser_id.as_str()
            )));
        }
        if parser
            .grammar_symbol
            .as_ref()
            .is_some_and(|symbol| symbol.trim().is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} has an empty grammar symbol",
                parser.parser_id.as_str()
            )));
        }
        if parser.kind == AcceptedParserKind::BuiltinManifest
            && (parser.grammar_symbol.is_some() || parser.tree_sitter_abi.is_some())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted built-in manifest parser {} declares executable grammar fields",
                parser.parser_id.as_str()
            )));
        }
        let delivered_asset = lock
            .assets
            .iter()
            .find(|asset| asset.asset_id == parser.asset_id);
        match (delivered_asset, &parser.tree_sitter_abi) {
            (Some(asset), Some(abi_version)) => {
                if asset.pack_id != parser.pack_id {
                    return Err(LanguageRegistryError::Validation(format!(
                        "accepted parser {} asset {} is owned by pack {}, not declared pack {}",
                        parser.parser_id.as_str(),
                        parser.asset_id.as_str(),
                        asset.pack_id.as_str(),
                        parser.pack_id.as_str()
                    )));
                }
                let expected_version = abi_version.as_str().parse::<u32>().map_err(|source| {
                    LanguageRegistryError::Validation(format!(
                        "accepted parser {} has nonnumeric tree-sitter ABI {:?}: {source}",
                        parser.parser_id.as_str(),
                        abi_version.as_str()
                    ))
                })?;
                if asset.abi.version != expected_version {
                    return Err(LanguageRegistryError::Validation(format!(
                        "accepted parser {} expects tree-sitter ABI version {}, but parser asset {} declares {} version {}",
                        parser.parser_id.as_str(),
                        abi_version.as_str(),
                        parser.asset_id.as_str(),
                        asset.abi.abi_id.as_str(),
                        asset.abi.version
                    )));
                }
            }
            (Some(asset), None) => {
                return Err(LanguageRegistryError::Validation(format!(
                    "accepted parser {} has delivered parser asset {} with {} version {} but no tree-sitter ABI claim",
                    parser.parser_id.as_str(),
                    parser.asset_id.as_str(),
                    asset.abi.abi_id.as_str(),
                    asset.abi.version
                )));
            }
            (None, Some(abi_version)) => {
                return Err(LanguageRegistryError::Validation(format!(
                    "accepted parser {} declares tree-sitter ABI {} but parser asset {} is missing",
                    parser.parser_id.as_str(),
                    abi_version.as_str(),
                    parser.asset_id.as_str()
                )));
            }
            (None, None) => {}
        }
        if let Some(query_pack) = lock
            .query_packs
            .iter()
            .find(|query| query.id == parser.query_pack_id)
            && query_pack.pack_id != parser.pack_id
        {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} and query pack {} have incompatible pack ownership",
                parser.parser_id.as_str(),
                parser.query_pack_id.as_str()
            )));
        }
        let normalized = parser
            .normalized_modes
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let (mode_pack, mode_owner, owned_modes) =
            modes_by_parser.get(&parser.parser_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "accepted parser {} has no accepted delivery mode",
                    parser.parser_id.as_str()
                ))
            })?;
        if mode_pack != &parser.pack_id || mode_owner != &parser.owner {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} pack {} owner {:?} disagrees with its accepted modes",
                parser.parser_id.as_str(),
                parser.pack_id.as_str(),
                parser.owner
            )));
        }
        if normalized.len() != parser.normalized_modes.len() || owned_modes != &normalized {
            return Err(LanguageRegistryError::Validation(format!(
                "accepted parser {} does not exactly own its normalized modes",
                parser.parser_id.as_str()
            )));
        }
    }
    if modes_by_parser
        .keys()
        .any(|parser_id| !parser_ids.contains(parser_id))
    {
        return Err(LanguageRegistryError::Validation(
            "accepted mode references a missing parser capability".to_string(),
        ));
    }

    if source
        .accepted_language_crosswalk
        .identity
        .trim()
        .is_empty()
    {
        return Err(LanguageRegistryError::Validation(
            "accepted language crosswalk has no identity".to_string(),
        ));
    }
    let mut accepted_names = BTreeSet::new();
    for row in &source.accepted_language_crosswalk.entries {
        if row.standard_name.trim().is_empty()
            || !accepted_names.insert(row.accepted_name_id.clone())
            || !mode_ids.contains(&row.mode_id)
            || (row.mapping == CrosswalkMapping::DialectMode && row.dialect.is_none())
            || (row.mapping != CrosswalkMapping::DialectMode && row.dialect.is_some())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "invalid accepted crosswalk row {}",
                row.accepted_name_id.as_str()
            )));
        }
    }
    Ok(())
}

/// Validate current-runtime identities, references, paths, ownership, and collisions.
fn validate_registry_lock(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> Result<(), LanguageRegistryError> {
    if lock.schema_version != LANGUAGE_REGISTRY_SCHEMA_VERSION
        || lock.format != RegistryFormat::LanguageRegistryLock
        || lock.capability_tiers.as_slice() != CAPABILITY_TIER_ORDER
    {
        return Err(LanguageRegistryError::Validation(
            "unsupported language registry lock schema, format, or tier vocabulary".to_string(),
        ));
    }
    require_equal(
        lock.historical_contract.path.as_str(),
        HISTORICAL_CONTRACT_PATH,
        "historical contract path",
    )?;

    let mut pack_ids = BTreeSet::new();
    let mut pack_contracts = BTreeMap::new();
    for pack in &lock.packs {
        if !pack_ids.insert(pack.pack_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate registry pack {}",
                pack.pack_id.as_str()
            )));
        }
        match (pack.pack_id.as_str(), pack.ownership, pack.runtime) {
            (DEFAULT_CORE_PACK_ID, PackOwnership::DefaultCore, PackRuntime::InProcess)
            | (
                BROAD_LANGUAGE_PACK_ID | SEMANTIC_PACK_ID,
                PackOwnership::Optional,
                PackRuntime::SupervisedWorker,
            ) => {}
            _ => {
                return Err(LanguageRegistryError::Validation(format!(
                    "pack {} crosses its required runtime boundary",
                    pack.pack_id.as_str()
                )));
            }
        }
        pack_contracts.insert(pack.pack_id.clone(), (pack.ownership, pack.runtime));
    }

    let mut current_mode_ids = BTreeSet::new();
    let mut public_modes = BTreeSet::new();
    let accepted_modes = accepted
        .modes
        .iter()
        .map(|mode| (mode.mode_id.clone(), mode))
        .collect::<BTreeMap<_, _>>();
    let accepted_parsers = accepted
        .parsers
        .iter()
        .map(|parser| (parser.parser_id.clone(), parser))
        .collect::<BTreeMap<_, _>>();
    for mode in &lock.current_modes {
        if !current_mode_ids.insert(mode.mode_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate current mode identifier {}",
                mode.mode_id.as_str()
            )));
        }
        if !public_modes.insert(mode.public_mode.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate current public mode {}",
                mode.public_mode.as_str()
            )));
        }
        let (ownership, _) = pack_contracts.get(&mode.current_pack_id).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "current mode {} references undeclared pack {}",
                mode.mode_id.as_str(),
                mode.current_pack_id.as_str()
            ))
        })?;
        if *ownership != PackOwnership::DefaultCore {
            return Err(LanguageRegistryError::Validation(format!(
                "current mode {} is not owned by default core",
                mode.mode_id.as_str()
            )));
        }
        let future = accepted_modes.get(&mode.accepted_mode_id).ok_or_else(|| {
            LanguageRegistryError::Validation(format!(
                "current mode {} references missing accepted mode {}",
                mode.mode_id.as_str(),
                mode.accepted_mode_id.as_str()
            ))
        })?;
        if future.public_mode != mode.public_mode {
            return Err(LanguageRegistryError::Validation(format!(
                "current mode {} changes public spelling in the accepted target",
                mode.mode_id.as_str()
            )));
        }
        if mode.alias_of.is_some() {
            return Err(LanguageRegistryError::Validation(format!(
                "current public mode {} is hidden behind an alias",
                mode.mode_id.as_str()
            )));
        }
        let pipeline_matches = matches!(
            (mode.parser_support, &mode.symbols),
            (ParserSupport::Native, SymbolPipeline::BuiltIn { .. })
                | (ParserSupport::Manifest, SymbolPipeline::Manifest { .. })
                | (
                    ParserSupport::Structural,
                    SymbolPipeline::Skip | SymbolPipeline::Structural { .. }
                )
                | (
                    ParserSupport::Fallback,
                    SymbolPipeline::Fallback { .. } | SymbolPipeline::Structural { .. }
                )
        );
        if !pipeline_matches {
            return Err(LanguageRegistryError::Validation(format!(
                "current mode {} has inconsistent parser support and symbol routing",
                mode.mode_id.as_str()
            )));
        }
    }
    if lock.current_modes.len() != accepted.source.counts.current_public_modes {
        return Err(LanguageRegistryError::Validation(
            "current public mode count differs from the accepted target binding".to_string(),
        ));
    }

    validate_detection_rules(lock, &current_mode_ids)?;
    validate_semantic_modes(lock, &current_mode_ids)?;

    let fixture_ids = unique_ids(
        lock.fixtures.iter().map(|fixture| &fixture.fixture_id),
        "fixture",
        FixtureId::as_str,
    )?;
    let evidence_ids = unique_ids(
        lock.evidence.iter().map(|evidence| &evidence.evidence_id),
        "evidence",
        EvidenceId::as_str,
    )?;
    unique_ids(
        lock.assets.iter().map(|asset| &asset.asset_id),
        "parser asset",
        AssetId::as_str,
    )?;
    unique_ids(
        lock.query_packs.iter().map(|query| &query.id),
        "query pack",
        QueryPackId::as_str,
    )?;
    let assets_by_id = lock
        .assets
        .iter()
        .map(|asset| (asset.asset_id.clone(), asset))
        .collect::<BTreeMap<_, _>>();
    let query_packs_by_id = lock
        .query_packs
        .iter()
        .map(|query| (query.id.clone(), query))
        .collect::<BTreeMap<_, _>>();

    let mut parser_ids = BTreeSet::new();
    let mut built_in_parsers = BTreeSet::new();
    let mut parser_components_by_builtin = BTreeMap::new();
    for parser in &lock.parser_components {
        if !parser_ids.insert(parser.parser_id.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate parser component identifier {}",
                parser.parser_id.as_str()
            )));
        }
        if !built_in_parsers.insert(parser.built_in_parser) {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} repeats built-in parser {:?}",
                parser.parser_id.as_str(),
                parser.built_in_parser
            )));
        }
        let (ownership, runtime) =
            pack_contracts.get(&parser.current_pack_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "parser component {} references undeclared pack {}",
                    parser.parser_id.as_str(),
                    parser.current_pack_id.as_str()
                ))
            })?;
        if parser.current_pack_id.as_str() != DEFAULT_CORE_PACK_ID
            || *ownership != PackOwnership::DefaultCore
            || *runtime != PackRuntime::InProcess
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} is not a closed default-core in-process parser choice",
                parser.parser_id.as_str()
            )));
        }
        let expected_component_id =
            format!("parser.builtin.{}", parser.built_in_parser.contract_tag());
        if parser.parser_id.as_str() != expected_component_id.as_str() {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} does not match built-in parser identity {:?}; expected {}",
                parser.parser_id.as_str(),
                parser.built_in_parser,
                expected_component_id
            )));
        }
        if parser.implementation != ParserImplementation::CompiledTreeSitter {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} has unsupported implementation {:?}",
                parser.parser_id.as_str(),
                parser.implementation
            )));
        }
        if parser.abi.state != AbiState::CurrentCompiledContract
            || parser.abi.abi_id.as_str() != CURRENT_COMPILED_PARSER_ABI_ID
            || parser.abi.version != CURRENT_COMPILED_PARSER_ABI_VERSION
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} declares incompatible ABI {} version {} state {:?}; expected {} version {} state {:?}",
                parser.parser_id.as_str(),
                parser.abi.abi_id.as_str(),
                parser.abi.version,
                parser.abi.state,
                CURRENT_COMPILED_PARSER_ABI_ID,
                CURRENT_COMPILED_PARSER_ABI_VERSION,
                AbiState::CurrentCompiledContract
            )));
        }
        if let Some(asset_id) = &parser.asset_id {
            let asset = assets_by_id.get(asset_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "parser component {} references missing parser asset {}",
                    parser.parser_id.as_str(),
                    asset_id.as_str()
                ))
            })?;
            if asset.pack_id != parser.current_pack_id {
                return Err(LanguageRegistryError::Validation(format!(
                    "parser component {} asset {} is owned by pack {}, not component pack {}",
                    parser.parser_id.as_str(),
                    asset_id.as_str(),
                    asset.pack_id.as_str(),
                    parser.current_pack_id.as_str()
                )));
            }
            if asset.abi != parser.abi {
                return Err(LanguageRegistryError::Validation(format!(
                    "parser component {} ABI {} version {} is incompatible with asset {} ABI {} version {}",
                    parser.parser_id.as_str(),
                    parser.abi.abi_id.as_str(),
                    parser.abi.version,
                    asset_id.as_str(),
                    asset.abi.abi_id.as_str(),
                    asset.abi.version
                )));
            }
        }
        if let Some(query_pack_id) = &parser.query_pack_id {
            let query = query_packs_by_id.get(query_pack_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "parser component {} references missing query pack {}",
                    parser.parser_id.as_str(),
                    query_pack_id.as_str()
                ))
            })?;
            if query.pack_id != parser.current_pack_id {
                return Err(LanguageRegistryError::Validation(format!(
                    "parser component {} query pack {} is owned by pack {}, not component pack {}",
                    parser.parser_id.as_str(),
                    query_pack_id.as_str(),
                    query.pack_id.as_str(),
                    parser.current_pack_id.as_str()
                )));
            }
        }
        if parser.fixture_ids.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} has no fixtures",
                parser.parser_id.as_str()
            )));
        }
        let unique_fixture_ids = parser.fixture_ids.iter().collect::<BTreeSet<_>>();
        if unique_fixture_ids.len() != parser.fixture_ids.len() {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} repeats a fixture identity",
                parser.parser_id.as_str()
            )));
        }
        if let Some(missing_fixture) = parser
            .fixture_ids
            .iter()
            .find(|id| !fixture_ids.contains(*id))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} references missing fixture {}",
                parser.parser_id.as_str(),
                missing_fixture.as_str()
            )));
        }
        if let Some(missing_evidence) = parser
            .provenance_evidence_ids
            .iter()
            .find(|id| !evidence_ids.contains(*id))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "parser component {} references missing provenance evidence {}",
                parser.parser_id.as_str(),
                missing_evidence.as_str()
            )));
        }
        parser_components_by_builtin.insert(parser.built_in_parser, parser);
    }
    let mut routed_built_in_parsers = BTreeSet::new();
    for mode in &lock.current_modes {
        if let SymbolPipeline::BuiltIn { parser, augmenters } = &mode.symbols {
            if augmenters.iter().collect::<BTreeSet<_>>().len() != augmenters.len() {
                return Err(LanguageRegistryError::Validation(format!(
                    "current mode {} references an invalid built-in route",
                    mode.mode_id.as_str()
                )));
            }
            let component = parser_components_by_builtin.get(parser).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "current mode {} has no unique parser component for {:?}",
                    mode.mode_id.as_str(),
                    parser
                ))
            })?;
            routed_built_in_parsers.insert(*parser);
            let accepted_mode = accepted_modes.get(&mode.accepted_mode_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "current built-in mode {} references missing accepted mode {}",
                    mode.mode_id.as_str(),
                    mode.accepted_mode_id.as_str()
                ))
            })?;
            let expected_accepted_parser_id = AcceptedParserId::try_from(format!(
                "parse.{}",
                component.built_in_parser.contract_tag()
            ))?;
            if accepted_mode.pack_id.as_str() != DEFAULT_CORE_PACK_ID
                || accepted_mode.parser_id != expected_accepted_parser_id
            {
                return Err(LanguageRegistryError::Validation(format!(
                    "current built-in mode {} does not remain bound to default-core accepted parser {}",
                    mode.mode_id.as_str(),
                    expected_accepted_parser_id.as_str()
                )));
            }
            let accepted_parser =
                accepted_parsers
                    .get(&accepted_mode.parser_id)
                    .ok_or_else(|| {
                        LanguageRegistryError::Validation(format!(
                            "current built-in mode {} accepted parser {} is missing",
                            mode.mode_id.as_str(),
                            accepted_mode.parser_id.as_str()
                        ))
                    })?;
            if accepted_parser.pack_id.as_str() != DEFAULT_CORE_PACK_ID
                || accepted_parser.parser_id != expected_accepted_parser_id
            {
                return Err(LanguageRegistryError::Validation(format!(
                    "current built-in parser {:?} crosses the default-core accepted parser boundary through {}",
                    parser,
                    accepted_parser.parser_id.as_str()
                )));
            }
        }
        if let SymbolPipeline::Fallback { augmenters } = &mode.symbols
            && augmenters.iter().collect::<BTreeSet<_>>().len() != augmenters.len()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "current mode {} repeats a fallback augmenter",
                mode.mode_id.as_str()
            )));
        }
    }
    if routed_built_in_parsers != built_in_parsers {
        return Err(LanguageRegistryError::Validation(
            "current parser components and built-in routes do not describe the same closed inventory"
                .to_string(),
        ));
    }

    for asset in &lock.assets {
        if !pack_ids.contains(&asset.pack_id)
            || asset.abi.version == 0
            || asset.patches.iter().collect::<BTreeSet<_>>().len() != asset.patches.len()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "invalid parser asset {}",
                asset.asset_id.as_str()
            )));
        }
        let _digest = asset.digest_sha256.as_str();
    }
    for query in &lock.query_packs {
        if !pack_ids.contains(&query.pack_id) {
            return Err(LanguageRegistryError::Validation(format!(
                "query pack {} references an unknown pack",
                query.id.as_str()
            )));
        }
        let _digest = query.digest_sha256.as_str();
    }
    let embedded_adapter_ids = unique_ids(
        lock.embedded_adapters
            .iter()
            .map(|adapter| &adapter.adapter_id),
        "embedded adapter",
        EmbeddedAdapterId::as_str,
    )?;
    for adapter in &lock.embedded_adapters {
        if !embedded_adapter_ids.contains(&adapter.adapter_id) {
            return Err(LanguageRegistryError::Validation(format!(
                "embedded adapter {} is absent from its unique identity inventory",
                adapter.adapter_id.as_str()
            )));
        }
        for (field, mode_id) in [
            ("host_mode_id", &adapter.host_mode_id),
            ("embedded_mode_id", &adapter.embedded_mode_id),
        ] {
            if !accepted_modes.contains_key(mode_id) {
                return Err(LanguageRegistryError::Validation(format!(
                    "embedded adapter {} {field} references missing accepted mode {}",
                    adapter.adapter_id.as_str(),
                    mode_id.as_str()
                )));
            }
        }
        if !pack_ids.contains(&adapter.pack_id) {
            return Err(LanguageRegistryError::Validation(format!(
                "embedded adapter {} references undeclared pack {}",
                adapter.adapter_id.as_str(),
                adapter.pack_id.as_str()
            )));
        }
        if let Some(query_pack_id) = &adapter.query_pack_id {
            let query_pack = query_packs_by_id.get(query_pack_id).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "embedded adapter {} references missing query pack {}",
                    adapter.adapter_id.as_str(),
                    query_pack_id.as_str()
                ))
            })?;
            if query_pack.pack_id != adapter.pack_id {
                return Err(LanguageRegistryError::Validation(format!(
                    "embedded adapter {} query pack {} is owned by pack {}, not adapter pack {}",
                    adapter.adapter_id.as_str(),
                    query_pack_id.as_str(),
                    query_pack.pack_id.as_str(),
                    adapter.pack_id.as_str()
                )));
            }
        }
        if adapter.fixture_ids.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "embedded adapter {} has no fixtures",
                adapter.adapter_id.as_str()
            )));
        }
        if adapter.fixture_ids.iter().collect::<BTreeSet<_>>().len() != adapter.fixture_ids.len() {
            return Err(LanguageRegistryError::Validation(format!(
                "embedded adapter {} repeats a fixture identity",
                adapter.adapter_id.as_str()
            )));
        }
        if let Some(missing_fixture) = adapter
            .fixture_ids
            .iter()
            .find(|fixture_id| !fixture_ids.contains(*fixture_id))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "embedded adapter {} references missing fixture {}",
                adapter.adapter_id.as_str(),
                missing_fixture.as_str()
            )));
        }
    }
    let provider_ids = unique_ids(
        lock.semantic_providers
            .iter()
            .map(|provider| &provider.provider_id),
        "semantic provider",
        SemanticProviderId::as_str,
    )?;
    for provider in &lock.semantic_providers {
        if !provider_ids.contains(&provider.provider_id) {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} is absent from its unique identity inventory",
                provider.provider_id.as_str()
            )));
        }
        if !pack_ids.contains(&provider.pack_id) || provider.pack_id.as_str() != "semantic-pack" {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} references unsupported pack {}",
                provider.provider_id.as_str(),
                provider.pack_id.as_str()
            )));
        }
        if provider.mode_ids.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} has no accepted modes",
                provider.provider_id.as_str()
            )));
        }
        if provider.mode_ids.iter().collect::<BTreeSet<_>>().len() != provider.mode_ids.len() {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} repeats an accepted mode identity",
                provider.provider_id.as_str()
            )));
        }
        if let Some(missing_mode) = provider
            .mode_ids
            .iter()
            .find(|id| !accepted_modes.contains_key(*id))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} references missing accepted mode {}",
                provider.provider_id.as_str(),
                missing_mode.as_str()
            )));
        }
        if provider.fixture_ids.is_empty() {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} has no fixtures",
                provider.provider_id.as_str()
            )));
        }
        if provider.fixture_ids.iter().collect::<BTreeSet<_>>().len() != provider.fixture_ids.len()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} repeats a fixture identity",
                provider.provider_id.as_str()
            )));
        }
        if let Some(missing_fixture) = provider
            .fixture_ids
            .iter()
            .find(|id| !fixture_ids.contains(*id))
        {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic provider {} references missing fixture {}",
                provider.provider_id.as_str(),
                missing_fixture.as_str()
            )));
        }
    }
    for fixture in &lock.fixtures {
        if fixture
            .verification
            .evidence_ids
            .iter()
            .any(|id| !evidence_ids.contains(id))
            || (fixture.verification.state == VerificationState::Verified
                && fixture.verification.evidence_ids.is_empty())
        {
            return Err(LanguageRegistryError::Validation(format!(
                "fixture {} has invalid verification evidence",
                fixture.fixture_id.as_str()
            )));
        }
    }
    for evidence in &lock.evidence {
        if evidence.kind == EvidenceKind::FrozenRuntimeContract
            && (evidence.path != lock.historical_contract.path
                || evidence.digest_sha256 != lock.historical_contract.raw_sha256)
        {
            return Err(LanguageRegistryError::Validation(format!(
                "historical evidence {} is not bound to the frozen contract",
                evidence.evidence_id.as_str()
            )));
        }
    }
    validate_registry_path_inventory(lock)
}

/// Collect unique typed identifiers and report duplicates with their owning family.
fn unique_ids<'a, T>(
    values: impl Iterator<Item = &'a T>,
    family: &str,
    spelling: impl Fn(&T) -> &str,
) -> Result<BTreeSet<T>, LanguageRegistryError>
where
    T: Clone + Ord + 'a,
{
    let mut unique = BTreeSet::new();
    for value in values {
        if !unique.insert(value.clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate {family} identifier {}",
                spelling(value)
            )));
        }
    }
    Ok(unique)
}

/// Validate ordered detection precedence and collision semantics.
fn validate_detection_rules(
    lock: &LanguageRegistryLock,
    current_mode_ids: &BTreeSet<ModeId>,
) -> Result<(), LanguageRegistryError> {
    let mut ids = BTreeSet::new();
    let mut claimed_mode_ids = BTreeSet::new();
    let mut claims = BTreeMap::<(String, String), Vec<(&DetectionRule, String)>>::new();
    for rule in &lock.detection_rules {
        if !ids.insert(rule.id().clone()) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate detection-rule identifier {}",
                rule.id().as_str()
            )));
        }
        if !current_mode_ids.contains(rule.mode_id()) {
            return Err(LanguageRegistryError::Validation(format!(
                "detection rule {} references missing current mode {}",
                rule.id().as_str(),
                rule.mode_id().as_str()
            )));
        }
        claimed_mode_ids.insert(rule.mode_id().clone());
        if let DetectionRule::Content {
            detector_id,
            detector_kind,
            mode_id,
            ..
        } = rule
        {
            if detector_id.detection_kind() != *detector_kind {
                return Err(LanguageRegistryError::Validation(format!(
                    "content detector {} requires {} but was declared as {}",
                    detector_id.contract_id(),
                    detector_id.detection_kind().contract_tag(),
                    detector_kind.contract_tag()
                )));
            }
            if detector_id.mode_id() != mode_id.as_str() {
                return Err(LanguageRegistryError::Validation(format!(
                    "content detector {} requires mode {} but was declared for {}",
                    detector_id.contract_id(),
                    detector_id.mode_id(),
                    mode_id.as_str()
                )));
            }
        }
        let pattern = rule.pattern();
        let pattern_valid = match rule {
            DetectionRule::ExactFilename { .. } => {
                !pattern.is_empty()
                    && pattern.is_ascii()
                    && !pattern.contains(['/', '\\'])
                    && !matches!(pattern, "." | "..")
            }
            DetectionRule::CompoundExtension { .. } | DetectionRule::Extension { .. } => {
                pattern.starts_with('.')
                    && pattern.len() > 1
                    && pattern.is_ascii()
                    && !pattern.contains(['/', '\\'])
                    && !pattern.ends_with('.')
            }
            DetectionRule::Content { .. } => true,
        };
        if !pattern_valid {
            return Err(LanguageRegistryError::Validation(format!(
                "detection rule {} has invalid pattern {pattern:?}",
                rule.id().as_str()
            )));
        }
        let key = (rule.layer_tag().to_string(), pattern.to_ascii_lowercase());
        let overlapping = claims.entry(key).or_default();
        for (prior, prior_pattern) in overlapping.iter() {
            let lookup_patterns_overlap = prior_pattern == pattern
                || prior.case_policy() == CasePolicy::AsciiInsensitive
                || rule.case_policy() == CasePolicy::AsciiInsensitive;
            let path_patterns_overlap = prior_pattern == pattern
                || prior.path_case_policy() == CasePolicy::AsciiInsensitive
                || rule.path_case_policy() == CasePolicy::AsciiInsensitive;
            let patterns_overlap = lookup_patterns_overlap || path_patterns_overlap;
            let equivalent_scanner_alias = matches!(rule, DetectionRule::Extension { .. })
                && matches!(prior, DetectionRule::Extension { .. })
                && prior.mode_id() == rule.mode_id()
                && prior.scanner_visible()
                && rule.scanner_visible()
                && prior.case_policy() == CasePolicy::AsciiInsensitive
                && rule.case_policy() == CasePolicy::AsciiInsensitive
                && !prior_pattern.eq(pattern)
                && prior_pattern.eq_ignore_ascii_case(pattern);
            if patterns_overlap && !equivalent_scanner_alias {
                return Err(LanguageRegistryError::Validation(format!(
                    "detection rules {} (mode {}) and {} (mode {}) ambiguously claim {} field values {prior_pattern:?} and {pattern:?} at {} precedence across lookup/path matching",
                    prior.id().as_str(),
                    prior.mode_id().as_str(),
                    rule.id().as_str(),
                    rule.mode_id().as_str(),
                    rule.layer_tag(),
                    rule.layer_tag()
                )));
            }
        }
        overlapping.push((rule, pattern.to_string()));
    }
    if let Some(phantom_mode) = current_mode_ids.difference(&claimed_mode_ids).next() {
        return Err(LanguageRegistryError::Validation(format!(
            "current mode {} has no detection rule",
            phantom_mode.as_str()
        )));
    }
    let declared_content_detectors = lock
        .detection_rules
        .iter()
        .filter_map(|rule| match rule {
            DetectionRule::Content { detector_id, .. } => Some(*detector_id),
            DetectionRule::ExactFilename { .. }
            | DetectionRule::CompoundExtension { .. }
            | DetectionRule::Extension { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let required_content_detectors = BUILT_IN_CONTENT_DETECTORS
        .into_iter()
        .collect::<BTreeSet<_>>();
    if !declared_content_detectors.is_empty()
        && declared_content_detectors != required_content_detectors
    {
        return Err(LanguageRegistryError::Validation(format!(
            "built-in content-detector inventory mismatch: declared {declared_content_detectors:?}, required {required_content_detectors:?}"
        )));
    }
    Ok(())
}

/// Validate closed semantic modes and their current base-language ownership.
fn validate_semantic_modes(
    lock: &LanguageRegistryLock,
    current_mode_ids: &BTreeSet<ModeId>,
) -> Result<(), LanguageRegistryError> {
    let mut declared = BTreeSet::new();
    for rule in &lock.semantic_modes {
        if !declared.insert(rule.mode) {
            return Err(LanguageRegistryError::Validation(format!(
                "duplicate semantic mode {}",
                rule.mode.public_mode()
            )));
        }
        if !current_mode_ids.contains(&rule.base_mode_id) {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic mode {} references missing base mode {}",
                rule.mode.public_mode(),
                rule.base_mode_id.as_str()
            )));
        }
        if rule.mode.base_mode_id() != rule.base_mode_id.as_str() {
            return Err(LanguageRegistryError::Validation(format!(
                "semantic mode {} requires base mode {} but was declared for {}",
                rule.mode.public_mode(),
                rule.mode.base_mode_id(),
                rule.base_mode_id.as_str()
            )));
        }
    }
    let required = SEMANTIC_MODES.into_iter().collect::<BTreeSet<_>>();
    if !declared.is_empty() && declared != required {
        return Err(LanguageRegistryError::Validation(format!(
            "semantic-mode inventory mismatch: declared {declared:?}, required {required:?}"
        )));
    }
    Ok(())
}

/// Reject portable lock paths that collide under ASCII-insensitive filesystems.
fn validate_registry_path_inventory(
    lock: &LanguageRegistryLock,
) -> Result<(), LanguageRegistryError> {
    let mut paths = vec![&lock.accepted_target.path, &lock.historical_contract.path];
    paths.extend(lock.assets.iter().map(|asset| &asset.path));
    paths.extend(lock.assets.iter().flat_map(|asset| asset.patches.iter()));
    paths.extend(lock.query_packs.iter().map(|query| &query.path));
    paths.extend(lock.fixtures.iter().map(|fixture| &fixture.path));
    paths.extend(lock.evidence.iter().map(|evidence| &evidence.path));
    let mut folded = BTreeMap::<String, &str>::new();
    for path in paths {
        let key = path.as_str().to_ascii_lowercase();
        if let Some(prior) = folded.insert(key, path.as_str())
            && prior != path.as_str()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "registry paths {prior:?} and {:?} collide by ASCII case",
                path.as_str()
            )));
        }
    }
    Ok(())
}

/// Reconcile the composite lock with the independently frozen v0.3.26 behavior contract.
fn validate_historical_contract(
    lock: &LanguageRegistryLock,
    historical: &HistoricalRuntimeContract,
) -> Result<(), LanguageRegistryError> {
    if historical.schema_version != HISTORICAL_RUNTIME_CONTRACT_SCHEMA_VERSION {
        return Err(LanguageRegistryError::Validation(format!(
            "historical runtime contract schema version must be {HISTORICAL_RUNTIME_CONTRACT_SCHEMA_VERSION}, found {}",
            historical.schema_version
        )));
    }
    require_equal(
        historical.baseline_release.as_str(),
        lock.historical_contract.release.as_str(),
        "historical release identity",
    )?;
    require_equal(
        historical.baseline_commit.as_str(),
        lock.historical_contract.commit.as_str(),
        "historical commit identity",
    )?;

    let public_by_mode = lock
        .current_modes
        .iter()
        .map(|mode| (mode.mode_id.clone(), mode.public_mode.as_str()))
        .collect::<BTreeMap<_, _>>();
    let detection_projection = |scanner_visible: bool| {
        lock.detection_rules
            .iter()
            .filter(|rule| {
                rule.scanner_visible() == scanner_visible
                    && !matches!(
                        rule,
                        DetectionRule::ExactFilename { .. } | DetectionRule::Content { .. }
                    )
            })
            .map(|rule| {
                let language = public_by_mode.get(rule.mode_id()).copied().ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "historical detection rule {} references a missing mode",
                        rule.id().as_str()
                    ))
                })?;
                Ok((rule.pattern(), language))
            })
            .collect::<Result<Vec<_>, LanguageRegistryError>>()
    };
    let broad = detection_projection(true)?;
    let api_only = detection_projection(false)?;
    if !historical_detection_matches(&broad, &historical.broad_detection)
        || !historical_detection_matches(&api_only, &historical.api_only_detection)
    {
        return Err(LanguageRegistryError::Validation(
            "ordered historical scanner or API-only detection projection drifted".to_string(),
        ));
    }

    let exact = lock
        .detection_rules
        .iter()
        .filter_map(|rule| match rule {
            DetectionRule::ExactFilename {
                file_name, mode_id, ..
            } => Some((file_name.as_str(), mode_id)),
            DetectionRule::CompoundExtension { .. }
            | DetectionRule::Extension { .. }
            | DetectionRule::Content { .. } => None,
        })
        .collect::<Vec<_>>();
    if exact.len() != historical.exact_filenames.len() {
        return Err(LanguageRegistryError::Validation(
            "historical exact-filename row count drifted".to_string(),
        ));
    }
    for ((file_name, mode_id), witness) in exact.iter().zip(&historical.exact_filenames) {
        let language = public_by_mode.get(*mode_id).copied().unwrap_or_default();
        if *file_name != witness.file_name
            || language != witness.language
            || witness.conflicting_extension.is_empty()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "historical exact-filename witness for {:?} drifted",
                witness.file_name
            )));
        }
    }

    for witness in &historical.extension_normalization {
        let extension = normalized_extension(lock, &witness.path);
        if extension != witness.extension {
            return Err(LanguageRegistryError::Validation(format!(
                "extension normalization for {:?} differs: expected {:?}, found {:?}",
                witness.path, witness.extension, extension
            )));
        }
    }
    for witness in &historical.negative_detection {
        let language = detect_public_mode(lock, &public_by_mode, &witness.path, &witness.extension)
            .unwrap_or_default();
        if language != witness.language {
            return Err(LanguageRegistryError::Validation(format!(
                "negative or case-sensitive detection witness {:?} drifted",
                witness.path
            )));
        }
    }

    if historical.language_pipelines.len() != lock.current_modes.len() {
        return Err(LanguageRegistryError::Validation(
            "historical language-pipeline row count drifted".to_string(),
        ));
    }
    for (mode, witness) in lock
        .current_modes
        .iter()
        .zip(&historical.language_pipelines)
    {
        if mode.public_mode.as_str() != witness.language
            || mode.parser_support != witness.support
            || mode.summary_adapter != witness.summary_adapter
            || historical_symbol_adapter(&mode.symbols) != witness.symbol_adapter
        {
            return Err(LanguageRegistryError::Validation(format!(
                "historical language pipeline for {} drifted",
                mode.public_mode.as_str()
            )));
        }
    }

    let augmenter_routes = lock
        .current_modes
        .iter()
        .flat_map(|mode| {
            let (base, augmenters) = match &mode.symbols {
                SymbolPipeline::BuiltIn { augmenters, .. } => {
                    (HistoricalSymbolAdapter::TreeSitter, augmenters.as_slice())
                }
                SymbolPipeline::Fallback { augmenters } => {
                    (HistoricalSymbolAdapter::Fallback, augmenters.as_slice())
                }
                SymbolPipeline::Skip
                | SymbolPipeline::Manifest { .. }
                | SymbolPipeline::Structural { .. } => {
                    (HistoricalSymbolAdapter::None, &[] as &[AugmenterId])
                }
            };
            augmenters
                .iter()
                .enumerate()
                .map(move |(ordinal, augmenter)| {
                    (mode.public_mode.as_str(), base, *augmenter, ordinal)
                })
        })
        .collect::<Vec<_>>();
    if augmenter_routes.len() != historical.augmenter_routes.len() {
        return Err(LanguageRegistryError::Validation(
            "historical augmenter-route row count drifted".to_string(),
        ));
    }
    for ((language, base, augmenter, ordinal), witness) in
        augmenter_routes.iter().zip(&historical.augmenter_routes)
    {
        if *language != witness.language
            || *base != witness.base_adapter
            || *augmenter != witness.augmenter
            || *ordinal != witness.ordinal
        {
            return Err(LanguageRegistryError::Validation(format!(
                "historical augmenter route for {:?} drifted",
                witness.language
            )));
        }
    }

    let modes_by_public = lock
        .current_modes
        .iter()
        .map(|mode| (mode.public_mode.as_str(), mode))
        .collect::<BTreeMap<_, _>>();
    for witness in &historical.specialized_parsers {
        let mode = modes_by_public
            .get(witness.language.as_str())
            .ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "specialized parser witness references missing language {:?}",
                    witness.language
                ))
            })?;
        let SymbolPipeline::BuiltIn { parser, .. } = &mode.symbols else {
            return Err(LanguageRegistryError::Validation(format!(
                "specialized parser witness {:?} does not select a built-in parser",
                witness.language
            )));
        };
        if built_in_parser_component(*parser) != witness.parser_component
            || witness.source.is_empty()
            || witness.symbol_kind.is_empty()
            || witness.symbol_name.is_empty()
        {
            return Err(LanguageRegistryError::Validation(format!(
                "specialized parser witness for {:?} is incomplete or misrouted",
                witness.language
            )));
        }
    }
    let path_classes = [
        HistoricalAdapterPathClass::CargoManifest,
        HistoricalAdapterPathClass::CargoLock,
        HistoricalAdapterPathClass::Vue,
        HistoricalAdapterPathClass::Powershell,
        HistoricalAdapterPathClass::Ordinary,
        HistoricalAdapterPathClass::CargoManifestNearMiss,
        HistoricalAdapterPathClass::CargoLockNearMiss,
    ];
    for ((witness, expected_class), expected_row) in historical
        .adapter_precedence
        .iter()
        .zip(path_classes)
        .zip(FROZEN_COMPATIBLE_ADAPTER_PRECEDENCE)
    {
        if witness.path_class != expected_class || witness.path.is_empty() {
            return Err(LanguageRegistryError::Validation(
                "historical adapter-precedence path inventory drifted".to_string(),
            ));
        }
        if witness.expectations() != expected_row {
            return Err(LanguageRegistryError::Validation(
                "historical adapter-precedence expectation matrix drifted".to_string(),
            ));
        }
        for expectation in witness.expectations() {
            let public_mode = expectation.public_mode();
            let mode = modes_by_public.get(public_mode).ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "historical adapter-precedence mode {public_mode:?} is absent"
                ))
            })?;
            if !expectation.matches_pipeline(&mode.symbols) {
                return Err(LanguageRegistryError::Validation(format!(
                    "historical adapter-precedence mode {public_mode:?} drifted"
                )));
            }
        }
    }
    validate_cargo_routing_corrections(lock, &historical.cargo_routing_corrections)
}

/// Return the exact historical Cargo grammar component for one built-in parser.
const fn built_in_parser_component(parser: BuiltInParserId) -> &'static str {
    match parser {
        BuiltInParserId::Rust => "tree-sitter-rust",
        BuiltInParserId::Python => "tree-sitter-python",
        BuiltInParserId::Javascript => "tree-sitter-javascript",
        BuiltInParserId::Typescript => "tree-sitter-typescript-language-typescript",
        BuiltInParserId::Tsx => "tree-sitter-typescript-language-tsx",
        BuiltInParserId::Java => "tree-sitter-java",
        BuiltInParserId::Kotlin => "tree-sitter-kotlin-ng",
        BuiltInParserId::Csharp => "tree-sitter-c-sharp",
        BuiltInParserId::Go => "tree-sitter-go",
        BuiltInParserId::ObjectiveC => "tree-sitter-objc",
        BuiltInParserId::Zig => "tree-sitter-zig",
        BuiltInParserId::C => "tree-sitter-c",
        BuiltInParserId::Cpp => "tree-sitter-cpp",
    }
}

/// Compare an ordered detection projection without erasing raw scanner spellings.
fn historical_detection_matches(actual: &[(&str, &str)], expected: &[HistoricalDetection]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((extension, language), row)| {
                *extension == row.extension && *language == row.language
            })
}

/// Return the final path component while accepting both historical separator forms.
fn portable_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Reproduce current compound-extension then ordinary-extension normalization.
fn normalized_extension(lock: &LanguageRegistryLock, path: &str) -> String {
    let basename = portable_basename(path);
    for rule in &lock.detection_rules {
        let DetectionRule::CompoundExtension {
            extension,
            path_suffix_case,
            ..
        } = rule
        else {
            continue;
        };
        let matches = match path_suffix_case {
            CasePolicy::Sensitive => basename.ends_with(extension),
            CasePolicy::AsciiInsensitive => basename
                .get(basename.len().saturating_sub(extension.len())..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(extension)),
        };
        if matches {
            return extension.clone();
        }
    }
    basename
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or_else(String::new, |index| basename[index..].to_ascii_lowercase())
}

/// Apply the exact-filename and normalized-extension rules to one historical witness.
fn detect_public_mode<'a>(
    lock: &'a LanguageRegistryLock,
    public_by_mode: &BTreeMap<ModeId, &'a str>,
    path: &str,
    normalized_extension: &str,
) -> Option<&'a str> {
    let basename = portable_basename(path);
    for rule in &lock.detection_rules {
        if let DetectionRule::ExactFilename {
            file_name,
            case,
            mode_id,
            ..
        } = rule
        {
            let matches = match case {
                CasePolicy::Sensitive => basename == file_name,
                CasePolicy::AsciiInsensitive => basename.eq_ignore_ascii_case(file_name),
            };
            if matches {
                return public_by_mode.get(mode_id).copied();
            }
        }
    }
    for rule in &lock.detection_rules {
        let (extension, case, mode_id) = match rule {
            DetectionRule::CompoundExtension {
                extension,
                case,
                mode_id,
                ..
            }
            | DetectionRule::Extension {
                extension,
                case,
                mode_id,
                ..
            } => (extension, case, mode_id),
            DetectionRule::ExactFilename { .. } | DetectionRule::Content { .. } => continue,
        };
        let matches = match case {
            CasePolicy::Sensitive => normalized_extension == extension,
            CasePolicy::AsciiInsensitive => normalized_extension.eq_ignore_ascii_case(extension),
        };
        if matches {
            return public_by_mode.get(mode_id).copied();
        }
    }
    None
}

/// Project one current symbol route onto its frozen historical adapter class.
const fn historical_symbol_adapter(pipeline: &SymbolPipeline) -> HistoricalSymbolAdapter {
    match pipeline {
        SymbolPipeline::Skip => HistoricalSymbolAdapter::None,
        SymbolPipeline::BuiltIn { .. } => HistoricalSymbolAdapter::TreeSitter,
        SymbolPipeline::Manifest { .. } => HistoricalSymbolAdapter::Manifest,
        SymbolPipeline::Structural {
            adapter: SymbolAdapterId::Vue,
        } => HistoricalSymbolAdapter::VueStructural,
        SymbolPipeline::Structural {
            adapter: SymbolAdapterId::Powershell,
        } => HistoricalSymbolAdapter::PowershellStructural,
        SymbolPipeline::Fallback { .. } => HistoricalSymbolAdapter::Fallback,
    }
}

/// Validate the two reviewed near-miss Cargo routing corrections without suffix matching.
fn validate_cargo_routing_corrections(
    lock: &LanguageRegistryLock,
    corrections: &[HistoricalCargoRoutingCorrection],
) -> Result<(), LanguageRegistryError> {
    for correction in corrections {
        let basename = portable_basename(&correction.path);
        let exact_match = lock.detection_rules.iter().any(|rule| {
            matches!(
                rule,
                DetectionRule::ExactFilename {
                    file_name,
                    case: CasePolicy::Sensitive,
                    ..
                } if file_name == basename
            )
        });
        if correction.case_id.is_empty()
            || correction.supplied_language.is_empty()
                && Path::new(&correction.path)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
            || !correction.baseline_symbol_candidate
            || correction.accepted_symbol_candidate
            || correction.baseline_parser_kind != HistoricalParserKind::Manifest
            || correction.accepted_parser_kind != HistoricalParserKind::Fallback
            || correction.disposition != "intentional-correction"
            || correction.rationale.trim().is_empty()
            || exact_match
        {
            return Err(LanguageRegistryError::Validation(format!(
                "Cargo routing correction {:?} is incomplete or no longer exact-filename-only",
                correction.case_id
            )));
        }
    }
    Ok(())
}

/// Explicit length-prefixed encoder for the semantic composite-registry digest.
struct ContractDigest {
    /// Incremental SHA-256 state.
    hasher: Sha256,
}

impl ContractDigest {
    /// Start the versioned digest domain.
    fn new() -> Self {
        let mut digest = Self {
            hasher: Sha256::new(),
        };
        digest.record("digest-envelope");
        digest.field("domain", REGISTRY_DIGEST_DOMAIN);
        digest.number("encoding-version", REGISTRY_DIGEST_VERSION);
        digest
    }

    /// Encode one explicitly tagged record boundary.
    fn record(&mut self, tag: &str) {
        self.bytes(b"record");
        self.bytes(tag.as_bytes());
    }

    /// Encode one explicitly tagged UTF-8 field.
    fn field(&mut self, tag: &str, value: &str) {
        self.bytes(b"field");
        self.bytes(tag.as_bytes());
        self.bytes(value.as_bytes());
    }

    /// Encode one explicitly tagged boolean field.
    fn boolean(&mut self, tag: &str, value: bool) {
        self.bytes(b"boolean");
        self.bytes(tag.as_bytes());
        self.bytes(&[u8::from(value)]);
    }

    /// Encode one explicitly tagged unsigned integer without host-sized bytes.
    fn number(&mut self, tag: &str, value: u64) {
        self.bytes(b"number");
        self.bytes(tag.as_bytes());
        self.bytes(&value.to_le_bytes());
    }

    /// Encode one explicitly tagged optional UTF-8 field.
    fn optional(&mut self, tag: &str, value: Option<&str>) {
        self.bytes(b"optional");
        self.bytes(tag.as_bytes());
        match value {
            Some(value) => {
                self.bytes(b"some");
                self.bytes(value.as_bytes());
            }
            None => self.bytes(b"none"),
        }
    }

    /// Encode a sequence boundary and its platform-independent length.
    fn sequence(&mut self, tag: &str, length: usize) {
        self.bytes(b"sequence");
        self.bytes(tag.as_bytes());
        self.bytes(&(length as u64).to_le_bytes());
    }

    /// Feed one u64-little-endian-length-prefixed byte string.
    fn bytes(&mut self, value: &[u8]) {
        self.hasher.update((value.len() as u64).to_le_bytes());
        self.hasher.update(value);
    }

    /// Finish the digest as lowercase hexadecimal.
    fn finish(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }
}

/// Compute the semantic composite digest from typed, reconciled records only.
fn registry_contract_digest(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
) -> String {
    let mut digest = ContractDigest::new();
    digest.record("registry-lock");
    digest.number("schema-version", u64::from(lock.schema_version));
    digest.field("format", "projectatlas.language-registry-lock");
    digest.field("registry-id", lock.registry_id.as_str());
    digest.record("accepted-binding");
    digest.field("path", lock.accepted_target.path.as_str());
    digest.field("registry-id", lock.accepted_target.registry_id.as_str());
    digest.field(
        "accepted-set-sha256",
        lock.accepted_target.accepted_set_sha256.as_str(),
    );
    digest.field("raw-sha256", lock.accepted_target.raw_sha256.as_str());
    digest.record("historical-binding");
    digest.field("path", lock.historical_contract.path.as_str());
    digest.field("release", lock.historical_contract.release.as_str());
    digest.field("commit", lock.historical_contract.commit.as_str());
    digest.field("raw-sha256", lock.historical_contract.raw_sha256.as_str());
    digest.record("parser-pack-trust-binding");
    digest.field("path", lock.parser_pack_trust.path.as_str());
    digest.field("raw-sha256", lock.parser_pack_trust.raw_sha256.as_str());
    digest.number(
        "broad-language-pack-installed-byte-limit",
        parser_pack_installed_byte_limit,
    );
    let mut candidates = parser_pack_trust.candidates.iter().collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.candidate_id.as_str());
    digest.sequence("parser-pack-candidates", candidates.len());
    for candidate in candidates {
        digest.record("parser-pack-candidate");
        digest.field("candidate-id", candidate.candidate_id.as_str());
        digest.field("eligibility", candidate.eligibility.contract_tag());
        digest.boolean("advertised", candidate.advertised);
        digest.field("pack-id", candidate.pack_id.as_str());
        digest.field("release-version", &candidate.release_version);
        digest.field("pack-abi-id", candidate.pack_abi.abi_id.as_str());
        digest.number("pack-abi-version", u64::from(candidate.pack_abi.version));
        digest.field("grammar-abi-id", candidate.grammar_abi.abi_id.as_str());
        digest.number(
            "grammar-abi-version",
            u64::from(candidate.grammar_abi.version),
        );
        digest.field(
            "grammar-abi-state",
            candidate.grammar_abi.state.contract_tag(),
        );
        digest.field("packaged-platform", candidate.packaged_platform.as_str());
        digest.field("payload-root", candidate.payload_root.as_str());
        digest.field("manifest-path", candidate.manifest_path.as_str());
        digest.number("installed-bytes", candidate.installed_bytes);
        let mut inventory = candidate.inventory.iter().collect::<Vec<_>>();
        inventory.sort_by_key(|file| file.path.as_str());
        digest.sequence("candidate-files", inventory.len());
        for file in inventory {
            digest.record("candidate-file");
            digest.field("path", file.path.as_str());
            digest.field("role", file.role.contract_tag());
            digest.number("bytes", file.bytes);
            digest.field("sha256", file.sha256.as_str());
        }
        digest.record("candidate-provenance");
        digest.field("source", &candidate.provenance.source);
        digest.field("revision", candidate.provenance.revision.as_str());
        digest.field(
            "source-package-sha256",
            candidate.provenance.source_package_sha256.as_str(),
        );
        digest.field("toolchain", &candidate.provenance.toolchain);
        digest.field(
            "local-output-identity",
            candidate
                .provenance
                .evidence_state
                .local_output_identity
                .contract_tag(),
        );
        digest.field(
            "hosted-reproduction",
            candidate
                .provenance
                .evidence_state
                .hosted_reproduction
                .contract_tag(),
        );
        digest.field(
            "raw-build-logs",
            candidate
                .provenance
                .evidence_state
                .raw_build_logs
                .contract_tag(),
        );
        digest.field(
            "packaged-windows-identity",
            candidate
                .provenance
                .evidence_state
                .packaged_windows_identity
                .contract_tag(),
        );
        digest.field("record-path", candidate.provenance.record_path.as_str());
        digest.sequence("patch-paths", candidate.provenance.patch_paths.len());
        for path in &candidate.provenance.patch_paths {
            digest.field("patch-path", path.as_str());
        }
        digest.sequence("candidate-licenses", candidate.licenses.len());
        for license in &candidate.licenses {
            digest.record("candidate-license");
            digest.field("component", &license.component);
            digest.field("expression", &license.expression);
            digest.field("record-path", license.record_path.as_str());
        }
        digest.field(
            "advisory-record-path",
            candidate.advisory_record_path.as_str(),
        );
        digest.field(
            "wasm-validation-record-path",
            candidate.wasm_validation_record_path.as_str(),
        );
        digest.field("sbom-record-path", candidate.sbom_record_path.as_str());
    }

    let mut packs = lock.packs.iter().collect::<Vec<_>>();
    packs.sort_by_key(|pack| pack.pack_id.as_str());
    digest.sequence("packs", packs.len());
    for pack in packs {
        digest.record("pack");
        digest.field("pack-id", pack.pack_id.as_str());
        digest.field("ownership", pack.ownership.contract_tag());
        digest.field("runtime", pack.runtime.contract_tag());
    }

    digest.sequence("capability-tiers", lock.capability_tiers.len());
    for tier in &lock.capability_tiers {
        digest.field("tier", tier.contract_tag());
    }

    digest.sequence("ordered-detection-rules", lock.detection_rules.len());
    for rule in &lock.detection_rules {
        digest.record("detection-rule");
        digest.field("id", rule.id().as_str());
        digest.field("layer", rule.layer_tag());
        digest.field("pattern", rule.pattern());
        digest.field("lookup-case", rule.case_policy().contract_tag());
        digest.field("path-case", rule.path_case_policy().contract_tag());
        digest.boolean("scanner-visible", rule.scanner_visible());
        digest.field("mode-id", rule.mode_id().as_str());
        if let DetectionRule::Content {
            detector_id,
            detector_kind,
            ..
        } = rule
        {
            digest.field("content-detector", detector_id.contract_id());
            digest.field("content-kind", detector_kind.contract_tag());
        }
    }

    digest.sequence("semantic-modes", lock.semantic_modes.len());
    for semantic_mode in &lock.semantic_modes {
        digest.record("semantic-mode");
        digest.field("mode", semantic_mode.mode.public_mode());
        digest.field("base-mode-id", semantic_mode.base_mode_id.as_str());
    }

    digest.sequence("ordered-current-modes", lock.current_modes.len());
    for mode in &lock.current_modes {
        digest.record("current-mode");
        digest.field("mode-id", mode.mode_id.as_str());
        digest.field("public-mode", mode.public_mode.as_str());
        digest.field("accepted-mode-id", mode.accepted_mode_id.as_str());
        digest.optional("alias-of", mode.alias_of.as_ref().map(ModeId::as_str));
        digest.field("parser-support", mode.parser_support.contract_tag());
        digest.field("current-pack-id", mode.current_pack_id.as_str());
        digest.field("summary-adapter", mode.summary_adapter.contract_tag());
        encode_symbol_pipeline(&mut digest, &mode.symbols);
    }

    let mut components = lock.parser_components.iter().collect::<Vec<_>>();
    components.sort_by_key(|component| component.parser_id.as_str());
    digest.sequence("parser-components", components.len());
    for component in components {
        digest.record("parser-component");
        digest.field("parser-id", component.parser_id.as_str());
        digest.field("built-in-parser", component.built_in_parser.contract_tag());
        digest.field("implementation", component.implementation.contract_tag());
        digest.field("current-pack-id", component.current_pack_id.as_str());
        digest.field("abi-id", component.abi.abi_id.as_str());
        digest.number("abi-version", u64::from(component.abi.version));
        digest.field("abi-state", component.abi.state.contract_tag());
        digest.optional("asset-id", component.asset_id.as_ref().map(AssetId::as_str));
        digest.optional(
            "query-pack-id",
            component.query_pack_id.as_ref().map(QueryPackId::as_str),
        );
        encode_sorted_ids(
            &mut digest,
            "fixture-ids",
            component.fixture_ids.iter().map(FixtureId::as_str),
        );
        encode_sorted_ids(
            &mut digest,
            "provenance-evidence-ids",
            component
                .provenance_evidence_ids
                .iter()
                .map(EvidenceId::as_str),
        );
    }
    encode_registry_inventories(&mut digest, lock);
    encode_accepted_target(&mut digest, accepted);
    encode_historical_contract(&mut digest, historical);
    digest.finish()
}

/// Encode one closed current symbol route.
fn encode_symbol_pipeline(digest: &mut ContractDigest, pipeline: &SymbolPipeline) {
    digest.record("symbol-pipeline");
    digest.field("kind", pipeline.contract_tag());
    match pipeline {
        SymbolPipeline::Skip => {}
        SymbolPipeline::BuiltIn { parser, augmenters } => {
            digest.field("parser", parser.contract_tag());
            digest.sequence("augmenters", augmenters.len());
            for augmenter in augmenters {
                digest.field("augmenter", augmenter.contract_tag());
            }
        }
        SymbolPipeline::Manifest { adapter } => {
            digest.field("adapter", adapter.contract_tag());
        }
        SymbolPipeline::Structural { adapter } => {
            digest.field("adapter", adapter.contract_tag());
        }
        SymbolPipeline::Fallback { augmenters } => {
            digest.sequence("augmenters", augmenters.len());
            for augmenter in augmenters {
                digest.field("augmenter", augmenter.contract_tag());
            }
        }
    }
}

/// Encode an identity collection whose order is set-like.
fn encode_sorted_ids<'a>(
    digest: &mut ContractDigest,
    tag: &str,
    values: impl Iterator<Item = &'a str>,
) {
    let mut values = values.collect::<Vec<_>>();
    values.sort_unstable();
    digest.sequence(tag, values.len());
    for value in values {
        digest.field("id", value);
    }
}

/// Encode sorted lock inventories that are not runtime-order-sensitive.
fn encode_registry_inventories(digest: &mut ContractDigest, lock: &LanguageRegistryLock) {
    let mut assets = lock.assets.iter().collect::<Vec<_>>();
    assets.sort_by_key(|asset| asset.asset_id.as_str());
    digest.sequence("parser-assets", assets.len());
    for asset in assets {
        digest.record("parser-asset");
        digest.field("asset-id", asset.asset_id.as_str());
        digest.field("path", asset.path.as_str());
        digest.field("pack-id", asset.pack_id.as_str());
        digest.field("source", asset.source.as_str());
        digest.field("version", asset.version.as_str());
        digest.field("abi-id", asset.abi.abi_id.as_str());
        digest.number("abi-version", u64::from(asset.abi.version));
        digest.field("abi-state", asset.abi.state.contract_tag());
        digest.field("digest-sha256", asset.digest_sha256.as_str());
        digest.field("license", asset.license.as_str());
        digest.sequence("patches", asset.patches.len());
        for patch in &asset.patches {
            digest.field("patch", patch.as_str());
        }
    }
    let mut embedded_adapters = lock.embedded_adapters.iter().collect::<Vec<_>>();
    embedded_adapters.sort_by_key(|adapter| adapter.adapter_id.as_str());
    digest.sequence("embedded-adapters", embedded_adapters.len());
    for adapter in embedded_adapters {
        digest.record("embedded-adapter");
        digest.field("adapter-id", adapter.adapter_id.as_str());
        digest.field("host-mode-id", adapter.host_mode_id.as_str());
        digest.field("embedded-mode-id", adapter.embedded_mode_id.as_str());
        digest.field("pack-id", adapter.pack_id.as_str());
        digest.optional(
            "query-pack-id",
            adapter.query_pack_id.as_ref().map(QueryPackId::as_str),
        );
        encode_sorted_ids(
            digest,
            "fixture-ids",
            adapter.fixture_ids.iter().map(FixtureId::as_str),
        );
    }
    let mut queries = lock.query_packs.iter().collect::<Vec<_>>();
    queries.sort_by_key(|query| query.id.as_str());
    digest.sequence("query-packs", queries.len());
    for query in queries {
        digest.record("query-pack");
        digest.field("query-pack-id", query.id.as_str());
        digest.field("path", query.path.as_str());
        digest.field("pack-id", query.pack_id.as_str());
        digest.field("digest-sha256", query.digest_sha256.as_str());
    }
    let mut providers = lock.semantic_providers.iter().collect::<Vec<_>>();
    providers.sort_by_key(|provider| provider.provider_id.as_str());
    digest.sequence("semantic-providers", providers.len());
    for provider in providers {
        digest.record("semantic-provider");
        digest.field("provider-id", provider.provider_id.as_str());
        digest.field("pack-id", provider.pack_id.as_str());
        encode_sorted_ids(
            digest,
            "mode-ids",
            provider.mode_ids.iter().map(ModeId::as_str),
        );
        encode_sorted_ids(
            digest,
            "fixture-ids",
            provider.fixture_ids.iter().map(FixtureId::as_str),
        );
    }
    let mut fixtures = lock.fixtures.iter().collect::<Vec<_>>();
    fixtures.sort_by_key(|fixture| fixture.fixture_id.as_str());
    digest.sequence("fixtures", fixtures.len());
    for fixture in fixtures {
        digest.record("fixture");
        digest.field("fixture-id", fixture.fixture_id.as_str());
        digest.field("path", fixture.path.as_str());
        digest.field(
            "verification-state",
            fixture.verification.state.contract_tag(),
        );
        encode_sorted_ids(
            digest,
            "evidence-ids",
            fixture
                .verification
                .evidence_ids
                .iter()
                .map(EvidenceId::as_str),
        );
    }
    let mut evidence = lock.evidence.iter().collect::<Vec<_>>();
    evidence.sort_by_key(|entry| entry.evidence_id.as_str());
    digest.sequence("evidence", evidence.len());
    for entry in evidence {
        digest.record("evidence");
        digest.field("evidence-id", entry.evidence_id.as_str());
        digest.field("kind", entry.kind.contract_tag());
        digest.field("path", entry.path.as_str());
        digest.field("digest-sha256", entry.digest_sha256.as_str());
    }
}

/// Encode one optional accepted pre-parse transform into the composite contract.
fn encode_accepted_pre_parse_transform(
    digest: &mut ContractDigest,
    transform: Option<&AcceptedPreParseTransform>,
) {
    digest.boolean("pre-parse-transform-present", transform.is_some());
    let Some(transform) = transform else {
        return;
    };
    digest.record("accepted-pre-parse-transform");
    digest.field("transform-id", transform.transform_id.as_str());
    digest.number("version", u64::from(transform.version));
    digest.field("behavior", transform.behavior.contract_tag());
    digest.boolean("deterministic", transform.deterministic);
    digest.field("target-mode-id", transform.target_mode_id.as_str());
    digest.field("target-parser-id", transform.target_parser_id.as_str());
    digest.field(
        "detection-ownership",
        transform.detection_ownership.contract_tag(),
    );
    digest.field("detection-rule-id", transform.detection_rule_id.as_str());
    digest.number("max-input-bytes", transform.limits.max_input_bytes);
    digest.number(
        "max-derived-output-bytes",
        transform.limits.max_derived_output_bytes,
    );
    digest.number("max-records", u64::from(transform.limits.max_records));
    digest.number(
        "max-nesting-depth",
        u64::from(transform.limits.max_nesting_depth),
    );
    digest.number(
        "max-diagnostics",
        u64::from(transform.limits.max_diagnostics),
    );
    digest.number("deadline-ms", transform.limits.deadline_ms);
    digest.boolean("cancellation-enabled", transform.cancellation.enabled);
    digest.number(
        "cancellation-poll-interval-ms",
        transform.cancellation.poll_interval_ms,
    );
    digest.number(
        "cancellation-grace-period-ms",
        transform.cancellation.grace_period_ms,
    );
    digest.boolean(
        "source-map-original-file-identity",
        transform.source_mapping.original_file_identity,
    );
    digest.boolean(
        "source-map-per-record-provenance",
        transform.source_mapping.per_record_provenance,
    );
    digest.boolean(
        "source-map-every-derived-fact",
        transform.source_mapping.every_derived_fact,
    );
    digest.boolean(
        "source-map-every-diagnostic",
        transform.source_mapping.every_diagnostic,
    );
    digest.field("security-dtd", transform.security.dtd.contract_tag());
    digest.field(
        "security-entity-expansion",
        transform.security.entity_expansion.contract_tag(),
    );
    digest.field(
        "security-external-resources",
        transform.security.external_resources.contract_tag(),
    );
    digest.field(
        "security-schema-fetch",
        transform.security.schema_fetch.contract_tag(),
    );
    digest.field(
        "security-execution",
        transform.security.execution.contract_tag(),
    );
    digest.field(
        "empty-input",
        transform.failure_policy.empty_input.contract_tag(),
    );
    digest.field(
        "malformed-input",
        transform.failure_policy.malformed_input.contract_tag(),
    );
    digest.field(
        "oversized-input",
        transform.failure_policy.oversized_input.contract_tag(),
    );
    digest.field(
        "deeply-nested-input",
        transform.failure_policy.deeply_nested_input.contract_tag(),
    );
    digest.field(
        "multi-record-input",
        transform.failure_policy.multi_record_input.contract_tag(),
    );
    digest.boolean(
        "unrelated-parser-fallback",
        transform.failure_policy.unrelated_parser_fallback,
    );
    digest.boolean(
        "guessed-symbols-after-failure",
        transform.failure_policy.guessed_symbols_after_failure,
    );
    digest.field(
        "coverage-after-failure",
        transform
            .failure_policy
            .coverage_after_failure
            .contract_tag(),
    );
}

/// Encode the materialized accepted delivery axis independently of current routing.
fn encode_accepted_target(digest: &mut ContractDigest, accepted: &AcceptedTargetContract) {
    let source = &accepted.source;
    digest.record("accepted-target");
    digest.number("schema-version", u64::from(source.schema_version));
    digest.field("registry-id", source.registry_id.as_str());
    digest.field("binding-role", &source.binding_role);
    digest.field("status", &source.status);
    digest.field("accepted-set-digest", source.accepted_set_digest.as_str());
    digest.number(
        "target-runnable-modes",
        source.accepted_set_policy.target_runnable_modes as u64,
    );
    digest.number(
        "target-parser-capabilities",
        source
            .accepted_set_policy
            .target_normalized_parser_capabilities as u64,
    );
    digest.boolean(
        "aliases-count-toward-modes",
        source.accepted_set_policy.aliases_count_toward_modes,
    );
    digest.boolean(
        "shared-fallback-counts-as-parser",
        source.accepted_set_policy.shared_fallback_counts_as_parser,
    );
    encode_sorted_ids(
        digest,
        "required-platforms",
        source.required_platforms.iter().map(PlatformId::as_str),
    );

    let mut modes = accepted.modes.iter().collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode.mode_id.as_str());
    digest.sequence("accepted-modes", modes.len());
    for mode in modes {
        digest.record("accepted-mode");
        digest.field("mode-id", mode.mode_id.as_str());
        digest.field("public-mode", mode.public_mode.as_str());
        digest.field("parser-id", mode.parser_id.as_str());
        digest.field("future-pack-id", mode.pack_id.as_str());
        digest.field("owner", &mode.owner);
        digest.boolean("accepted-delivery-target", mode.accepted_delivery_target);
        digest.optional("alias-of", mode.alias_of.as_ref().map(ModeId::as_str));
        digest.field("detection-rule-id", mode.detection_rule_id.as_str());
        encode_accepted_pre_parse_transform(digest, mode.pre_parse_transform.as_ref());
        encode_sorted_ids(
            digest,
            "fixture-ids",
            mode.fixture_ids.iter().map(String::as_str),
        );
        encode_sorted_ids(
            digest,
            "required-platforms",
            mode.required_platforms.iter().map(PlatformId::as_str),
        );
        let claims = mode
            .required_claims
            .iter()
            .map(|claim| claim.contract_tag());
        encode_sorted_ids(digest, "required-claims", claims);
        let achieved_claims = mode
            .achieved_claims
            .iter()
            .map(|claim| claim.contract_tag());
        encode_sorted_ids(digest, "achieved-claims", achieved_claims);
        digest.field("evidence-state", mode.evidence_state.contract_tag());
        digest.field("advertisement", mode.advertisement.contract_tag());
    }

    let mut parsers = accepted.parsers.iter().collect::<Vec<_>>();
    parsers.sort_by_key(|parser| parser.parser_id.as_str());
    digest.sequence("accepted-parsers", parsers.len());
    for parser in parsers {
        digest.record("accepted-parser");
        digest.field("parser-id", parser.parser_id.as_str());
        digest.field("kind", parser.kind.contract_tag());
        digest.field("future-pack-id", parser.pack_id.as_str());
        digest.field("owner", &parser.owner);
        digest.optional("grammar-symbol", parser.grammar_symbol.as_deref());
        digest.optional(
            "tree-sitter-abi",
            parser
                .tree_sitter_abi
                .as_ref()
                .map(ParserAbiVersion::as_str),
        );
        digest.field("asset-id", parser.asset_id.as_str());
        digest.field("query-pack-id", parser.query_pack_id.as_str());
        digest.field("evidence-state", parser.evidence_state.contract_tag());
        digest.boolean("advertised", parser.advertised);
        encode_sorted_ids(
            digest,
            "normalized-modes",
            parser.normalized_modes.iter().map(PublicMode::as_str),
        );
        encode_sorted_ids(
            digest,
            "required-platforms",
            parser.required_platforms.iter().map(PlatformId::as_str),
        );
    }

    let mut crosswalk = source
        .accepted_language_crosswalk
        .entries
        .iter()
        .collect::<Vec<_>>();
    crosswalk.sort_by_key(|row| row.accepted_name_id.as_str());
    digest.sequence("accepted-language-crosswalk", crosswalk.len());
    for row in crosswalk {
        digest.record("accepted-language-name");
        digest.field("accepted-name-id", row.accepted_name_id.as_str());
        digest.field("standard-name", &row.standard_name);
        digest.optional("dialect", row.dialect.as_deref());
        digest.field("mode-id", row.mode_id.as_str());
        digest.field("mapping", row.mapping.contract_tag());
    }
}

/// Encode every frozen behavioral witness in its authoritative order.
fn encode_historical_contract(digest: &mut ContractDigest, historical: &HistoricalRuntimeContract) {
    digest.record("historical-runtime-contract");
    digest.number("schema-version", u64::from(historical.schema_version));
    digest.field("release", historical.baseline_release.as_str());
    digest.field("commit", historical.baseline_commit.as_str());
    encode_historical_detection(digest, "broad-detection", &historical.broad_detection);
    encode_historical_detection(digest, "api-only-detection", &historical.api_only_detection);
    digest.sequence("exact-filenames", historical.exact_filenames.len());
    for row in &historical.exact_filenames {
        digest.record("exact-filename");
        digest.field("file-name", &row.file_name);
        digest.field("conflicting-extension", &row.conflicting_extension);
        digest.field("language", &row.language);
    }
    digest.sequence("negative-detection", historical.negative_detection.len());
    for row in &historical.negative_detection {
        digest.record("negative-detection");
        digest.field("path", &row.path);
        digest.field("extension", &row.extension);
        digest.field("language", &row.language);
    }
    digest.sequence(
        "extension-normalization",
        historical.extension_normalization.len(),
    );
    for row in &historical.extension_normalization {
        digest.record("extension-normalization");
        digest.field("path", &row.path);
        digest.field("extension", &row.extension);
    }
    digest.sequence(
        "cargo-routing-corrections",
        historical.cargo_routing_corrections.len(),
    );
    for row in &historical.cargo_routing_corrections {
        digest.record("cargo-routing-correction");
        digest.field("case-id", &row.case_id);
        digest.field("path", &row.path);
        digest.field("supplied-language", &row.supplied_language);
        digest.boolean("baseline-symbol-candidate", row.baseline_symbol_candidate);
        digest.boolean("accepted-symbol-candidate", row.accepted_symbol_candidate);
        digest.field(
            "baseline-parser-kind",
            historical_parser_kind_tag(row.baseline_parser_kind),
        );
        digest.field(
            "accepted-parser-kind",
            historical_parser_kind_tag(row.accepted_parser_kind),
        );
        digest.field("disposition", &row.disposition);
        digest.field("rationale", &row.rationale);
    }
    digest.sequence("language-pipelines", historical.language_pipelines.len());
    for row in &historical.language_pipelines {
        digest.record("language-pipeline");
        digest.field("language", &row.language);
        digest.field("support", row.support.contract_tag());
        digest.field("summary-adapter", row.summary_adapter.contract_tag());
        digest.field(
            "symbol-adapter",
            historical_symbol_adapter_tag(row.symbol_adapter),
        );
    }
    digest.sequence("augmenter-routes", historical.augmenter_routes.len());
    for row in &historical.augmenter_routes {
        digest.record("augmenter-route");
        digest.field("language", &row.language);
        digest.field(
            "base-adapter",
            historical_symbol_adapter_tag(row.base_adapter),
        );
        digest.field("augmenter", row.augmenter.contract_tag());
        digest.number("ordinal", row.ordinal as u64);
    }
    digest.sequence("specialized-parsers", historical.specialized_parsers.len());
    for row in &historical.specialized_parsers {
        digest.record("specialized-parser");
        digest.field("language", &row.language);
        digest.field("parser-component", &row.parser_component);
        digest.field("source", &row.source);
        digest.field("symbol-kind", &row.symbol_kind);
        digest.field("symbol-name", &row.symbol_name);
    }
    digest.sequence("adapter-precedence", historical.adapter_precedence.len());
    for row in &historical.adapter_precedence {
        digest.record("adapter-precedence-row");
        digest.field(
            "path-class",
            historical_adapter_path_class_tag(row.path_class),
        );
        digest.field("path", &row.path);
        digest.field("absent", historical_adapter_expectation_tag(row.absent));
        digest.field(
            "cargo-manifest",
            historical_adapter_expectation_tag(row.cargo_manifest),
        );
        digest.field(
            "cargo-lock",
            historical_adapter_expectation_tag(row.cargo_lock),
        );
        digest.field("vue", historical_adapter_expectation_tag(row.vue));
        digest.field(
            "powershell",
            historical_adapter_expectation_tag(row.powershell),
        );
        digest.field("built-in", historical_adapter_expectation_tag(row.built_in));
        digest.field("fallback", historical_adapter_expectation_tag(row.fallback));
        digest.field("unknown", historical_adapter_expectation_tag(row.unknown));
    }
}

/// Encode an ordered historical detection table.
fn encode_historical_detection(
    digest: &mut ContractDigest,
    tag: &str,
    rows: &[HistoricalDetection],
) {
    digest.sequence(tag, rows.len());
    for row in rows {
        digest.record("historical-detection");
        digest.field("extension", &row.extension);
        digest.field("language", &row.language);
    }
}

/// Return the stable historical parser-kind tag.
const fn historical_parser_kind_tag(kind: HistoricalParserKind) -> &'static str {
    match kind {
        HistoricalParserKind::TreeSitter => "tree-sitter",
        HistoricalParserKind::Manifest => "manifest",
        HistoricalParserKind::Structural => "structural",
        HistoricalParserKind::Fallback => "fallback",
    }
}

/// Return the stable frozen adapter path-class tag.
const fn historical_adapter_path_class_tag(kind: HistoricalAdapterPathClass) -> &'static str {
    match kind {
        HistoricalAdapterPathClass::CargoManifest => "cargo-manifest",
        HistoricalAdapterPathClass::CargoLock => "cargo-lock",
        HistoricalAdapterPathClass::Vue => "vue",
        HistoricalAdapterPathClass::Powershell => "powershell",
        HistoricalAdapterPathClass::Ordinary => "ordinary",
        HistoricalAdapterPathClass::CargoManifestNearMiss => "cargo-manifest-near-miss",
        HistoricalAdapterPathClass::CargoLockNearMiss => "cargo-lock-near-miss",
    }
}

/// Return the stable effective adapter tag.
const fn historical_adapter_expectation_tag(
    expectation: HistoricalAdapterExpectation,
) -> &'static str {
    match expectation {
        HistoricalAdapterExpectation::CargoManifest => "cargo-manifest",
        HistoricalAdapterExpectation::CargoLock => "cargo-lock",
        HistoricalAdapterExpectation::Vue => "vue",
        HistoricalAdapterExpectation::Powershell => "powershell",
        HistoricalAdapterExpectation::BuiltIn => "built-in",
        HistoricalAdapterExpectation::Fallback => "fallback",
    }
}

/// Return the stable historical adapter tag.
const fn historical_symbol_adapter_tag(adapter: HistoricalSymbolAdapter) -> &'static str {
    match adapter {
        HistoricalSymbolAdapter::None => "none",
        HistoricalSymbolAdapter::TreeSitter => "tree-sitter",
        HistoricalSymbolAdapter::Manifest => "manifest",
        HistoricalSymbolAdapter::VueStructural => "vue-structural",
        HistoricalSymbolAdapter::PowershellStructural => "powershell-structural",
        HistoricalSymbolAdapter::Fallback => "fallback",
    }
}

/// Quality-policy projection that owns the reference Rust toolchain.
#[derive(Deserialize)]
struct TestQualityPolicy {
    /// Repository-pinned reference toolchain.
    reference_toolchain: ReferenceToolchainPolicy,
}

/// Rust portion of the repository-pinned reference toolchain.
#[derive(Deserialize)]
struct ReferenceToolchainPolicy {
    /// Exact Rust toolchain required for generated projections.
    rust: String,
}

/// Concrete formatter owner for deterministic generated Rust projections.
struct GeneratedRustFormatter {
    /// Exact formatter executable proven to belong to the pinned toolchain sysroot.
    program: PathBuf,
    /// Local async runtime required only by bounded process supervision.
    runtime: tokio::runtime::Runtime,
}

impl GeneratedRustFormatter {
    /// Load the canonical formatter toolchain and create its local process runtime.
    fn new() -> Result<Self, LanguageRegistryError> {
        let policy =
            toml::from_str::<TestQualityPolicy>(TEST_QUALITY_POLICY).map_err(|source| {
                LanguageRegistryError::FormatRust {
                    owner: "generated Rust formatter policy",
                    detail: format!("failed to decode test-quality.toml: {source}"),
                }
            })?;
        let toolchain = policy.reference_toolchain.rust;
        if toolchain.is_empty() || toolchain.trim() != toolchain {
            return Err(LanguageRegistryError::FormatRust {
                owner: "generated Rust formatter policy",
                detail: "test-quality.toml reference_toolchain.rust must be nonempty without surrounding whitespace"
                    .to_string(),
            });
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| LanguageRegistryError::FormatRust {
                owner: "generated Rust formatter process",
                detail: format!("failed to create the formatter process runtime: {source}"),
            })?;
        let program = pinned_rustfmt_program(&runtime, &toolchain)?;
        Ok(Self { program, runtime })
    }

    /// Build the shell-free, repository-pinned formatter command for one file.
    fn command(&self, path: &Path) -> ProcessCommand {
        bounded_generated_rust_command(&self.program)
            .args(["--edition", "2024", "--style-edition", "2024"])
            .arg(path)
    }

    /// Run one formatter command to a reaped terminal outcome and reject partial evidence.
    fn run(
        &self,
        owner: &'static str,
        command: &ProcessCommand,
    ) -> Result<ProcessResult<String>, LanguageRegistryError> {
        run_bounded_generated_rust_process(&self.runtime, owner, command)
    }

    /// Format one generated Rust projection through the pinned process boundary.
    fn format(&self, owner: &'static str, source: &str) -> Result<String, LanguageRegistryError> {
        let mut input =
            NamedTempFile::new().map_err(|source| LanguageRegistryError::FormatRust {
                owner,
                detail: source.to_string(),
            })?;
        input
            .write_all(source.as_bytes())
            .and_then(|()| input.flush())
            .map_err(|source| LanguageRegistryError::FormatRust {
                owner,
                detail: source.to_string(),
            })?;
        self.run(owner, &self.command(input.path())).map(|_| ())?;
        fs::read_to_string(input.path()).map_err(|source| LanguageRegistryError::FormatRust {
            owner,
            detail: source.to_string(),
        })
    }
}

/// Apply the shared timeout, lifetime, window, and capture contract to one generated Rust process.
fn bounded_generated_rust_command(program: impl AsRef<std::ffi::OsStr>) -> ProcessCommand {
    ProcessCommand::new(program)
        .timeout(GENERATED_RUST_PROCESS_TIMEOUT)
        .kill_on_parent_death()
        .create_no_window()
        .output_buffer(
            OutputBufferPolicy::unbounded()
                .with_max_bytes(GENERATED_RUST_PROCESS_STREAM_LIMIT_BYTES),
        )
}

/// Run one bounded generated Rust process to a reaped terminal outcome.
fn run_bounded_generated_rust_process(
    runtime: &tokio::runtime::Runtime,
    owner: &'static str,
    command: &ProcessCommand,
) -> Result<ProcessResult<String>, LanguageRegistryError> {
    let output = runtime
        .block_on(command.output_string())
        .map_err(|source| LanguageRegistryError::GeneratedRustProcess {
            owner,
            detail: format!("process failed to launch or complete: {source}"),
        })?;
    if output.timed_out() {
        return Err(LanguageRegistryError::GeneratedRustProcess {
            owner,
            detail: format!(
                "process exceeded its configured timeout; stdout: {:?}; stderr: {:?}",
                output.stdout(),
                output.stderr()
            ),
        });
    }
    if output.truncated() {
        return Err(LanguageRegistryError::GeneratedRustProcess {
            owner,
            detail: format!(
                "process output exceeded {GENERATED_RUST_PROCESS_STREAM_LIMIT_BYTES} retained bytes per stream"
            ),
        });
    }
    match output.code() {
        Some(0) => Ok(output),
        Some(code) => Err(LanguageRegistryError::GeneratedRustProcess {
            owner,
            detail: format!(
                "process exited with code {code}; stdout: {:?}; stderr: {:?}",
                output.stdout(),
                output.stderr()
            ),
        }),
        None => Err(LanguageRegistryError::GeneratedRustProcess {
            owner,
            detail: format!(
                "process terminated without an exit code; stdout: {:?}; stderr: {:?}",
                output.stdout(),
                output.stderr()
            ),
        }),
    }
}

/// Resolve and verify the exact rustfmt executable in the active pinned Rust sysroot.
fn pinned_rustfmt_program(
    runtime: &tokio::runtime::Runtime,
    toolchain: &str,
) -> Result<PathBuf, LanguageRegistryError> {
    let sysroot_command = bounded_generated_rust_command("rustc").args(["--print", "sysroot"]);
    let sysroot_output = run_bounded_generated_rust_process(
        runtime,
        "generated Rust formatter toolchain",
        &sysroot_command,
    )?;
    let sysroot_text = sysroot_output.stdout().trim();
    if sysroot_text.is_empty() || sysroot_text.lines().count() != 1 {
        return Err(LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: "rustc --print sysroot did not return exactly one nonempty path".to_string(),
        });
    }
    let sysroot =
        fs::canonicalize(sysroot_text).map_err(|source| LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!("failed to resolve rustc sysroot {sysroot_text:?}: {source}"),
        })?;
    let rustc = sysroot
        .join("bin")
        .join(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let rustfmt = sysroot
        .join("bin")
        .join(format!("rustfmt{}", std::env::consts::EXE_SUFFIX));
    for (name, program) in [("rustc", &rustc), ("rustfmt", &rustfmt)] {
        if !fs::metadata(program).is_ok_and(|metadata| metadata.is_file()) {
            return Err(LanguageRegistryError::FormatRust {
                owner: "generated Rust formatter toolchain",
                detail: format!(
                    "pinned sysroot {} has no regular {name} executable at {}",
                    sysroot.display(),
                    program.display()
                ),
            });
        }
    }

    let rustc_command = bounded_generated_rust_command(&rustc).args(["--version", "--verbose"]);
    let rustc_output = run_bounded_generated_rust_process(
        runtime,
        "generated Rust formatter toolchain",
        &rustc_command,
    )?;
    let release = toolchain_version_field(rustc_output.stdout(), "release")?;
    if release != toolchain {
        return Err(LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!(
                "active Rust sysroot release {release:?} does not match test-quality.toml reference_toolchain.rust {toolchain:?}"
            ),
        });
    }
    let rustc_commit = toolchain_version_field(rustc_output.stdout(), "commit-hash")?;
    let rustc_date = toolchain_version_field(rustc_output.stdout(), "commit-date")?;

    let rustfmt_command = bounded_generated_rust_command(&rustfmt).arg("--version");
    let rustfmt_output = run_bounded_generated_rust_process(
        runtime,
        "generated Rust formatter toolchain",
        &rustfmt_command,
    )?;
    let (rustfmt_commit, rustfmt_date) = rustfmt_compiler_identity(rustfmt_output.stdout())?;
    if rustfmt_commit.len() < 7
        || !rustfmt_commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !rustc_commit.starts_with(rustfmt_commit)
        || rustfmt_date != rustc_date
    {
        return Err(LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!(
                "rustfmt compiler identity {rustfmt_commit:?} {rustfmt_date:?} does not match pinned rustc {rustc_commit:?} {rustc_date:?}"
            ),
        });
    }
    Ok(rustfmt)
}

/// Read one required `rustc --version --verbose` field.
fn toolchain_version_field<'a>(
    output: &'a str,
    field: &str,
) -> Result<&'a str, LanguageRegistryError> {
    let prefix = format!("{field}:");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!("rustc --version --verbose omitted {field}"),
        })
}

/// Read the compiler commit prefix and date embedded in `rustfmt --version`.
fn rustfmt_compiler_identity(output: &str) -> Result<(&str, &str), LanguageRegistryError> {
    let identity = output
        .trim()
        .rsplit_once('(')
        .and_then(|(_, suffix)| suffix.strip_suffix(')'))
        .ok_or_else(|| LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!("rustfmt --version has no compiler identity: {output:?}"),
        })?;
    let mut fields = identity.split_whitespace();
    let commit = fields.next().unwrap_or_default();
    let date = fields.next().unwrap_or_default();
    if commit.is_empty() || date.is_empty() || fields.next().is_some() {
        return Err(LanguageRegistryError::FormatRust {
            owner: "generated Rust formatter toolchain",
            detail: format!("rustfmt --version has an invalid compiler identity: {output:?}"),
        });
    }
    Ok((commit, date))
}

/// Render every fixed output from one validated in-memory contract.
fn render_generated_artifacts(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<GeneratedArtifacts, LanguageRegistryError> {
    let formatter = GeneratedRustFormatter::new()?;
    Ok(GeneratedArtifacts {
        core: formatter
            .format(
                "core detection registry",
                &render_core_registry(lock, source_lock_sha256, registry_contract_sha256)?,
            )?
            .into_bytes(),
        symbols: formatter
            .format(
                "symbol routing registry",
                &render_symbols_registry(lock, source_lock_sha256, registry_contract_sha256)?,
            )?
            .into_bytes(),
        cli: formatter
            .format(
                "CLI language policy registry",
                &render_cli_registry(
                    lock,
                    accepted,
                    parser_pack_trust,
                    parser_pack_installed_byte_limit,
                    source_lock_sha256,
                    registry_contract_sha256,
                )?,
            )?
            .into_bytes(),
        evidence: render_capability_state(
            lock,
            accepted,
            historical,
            parser_pack_trust,
            parser_pack_installed_byte_limit,
            source_lock_sha256,
            registry_contract_sha256,
        )?,
        documentation: render_documentation_support_matrix(
            lock,
            accepted,
            parser_pack_trust,
            parser_pack_installed_byte_limit,
            source_lock_sha256,
            registry_contract_sha256,
        )?,
    })
}

/// Format one generated Rust projection with the workspace's pinned formatter.
#[cfg(test)]
fn format_generated_rust(
    owner: &'static str,
    source: &str,
) -> Result<String, LanguageRegistryError> {
    GeneratedRustFormatter::new()?.format(owner, source)
}

/// Append deterministic formatted text and normalize the impossible string-format error.
fn push_format(
    output: &mut String,
    arguments: fmt::Arguments<'_>,
) -> Result<(), LanguageRegistryError> {
    output.write_fmt(arguments).map_err(|source| {
        LanguageRegistryError::Validation(format!("generated text formatting failed: {source}"))
    })
}

/// Quote one ASCII registry value as a Rust string literal.
fn rust_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(char::escape_default)
        .collect::<String>();
    format!("\"{escaped}\"")
}

/// Render one unsigned integer as a readable Rust literal.
fn rust_u64_literal(value: u64) -> String {
    let digits = value.to_string();
    let mut literal = String::new();
    for (index, digit) in digits.bytes().enumerate() {
        if index != 0 && (digits.len() - index).is_multiple_of(3) {
            literal.push('_');
        }
        literal.push(char::from(digit));
    }
    literal
}

/// Return the generated Rust expression for one closed built-in parser.
const fn rust_built_in_parser(parser: BuiltInParserId) -> &'static str {
    match parser {
        BuiltInParserId::Rust => "BuiltInParser::Rust",
        BuiltInParserId::Python => "BuiltInParser::Python",
        BuiltInParserId::Javascript => "BuiltInParser::JavaScript",
        BuiltInParserId::Typescript => "BuiltInParser::TypeScript",
        BuiltInParserId::Tsx => "BuiltInParser::Tsx",
        BuiltInParserId::Java => "BuiltInParser::Java",
        BuiltInParserId::Kotlin => "BuiltInParser::Kotlin",
        BuiltInParserId::Csharp => "BuiltInParser::CSharp",
        BuiltInParserId::Go => "BuiltInParser::Go",
        BuiltInParserId::ObjectiveC => "BuiltInParser::ObjectiveC",
        BuiltInParserId::Zig => "BuiltInParser::Zig",
        BuiltInParserId::C => "BuiltInParser::C",
        BuiltInParserId::Cpp => "BuiltInParser::Cpp",
    }
}

/// Return the generated Rust expression for one current parser-support class.
const fn rust_parser_support(support: ParserSupport) -> &'static str {
    match support {
        ParserSupport::Native => "LanguageParserSupport::Native",
        ParserSupport::Manifest => "LanguageParserSupport::Manifest",
        ParserSupport::Structural => "LanguageParserSupport::Structural",
        ParserSupport::Fallback => "LanguageParserSupport::Fallback",
    }
}

/// Return the generated Rust expression for one closed symbol augmenter.
const fn rust_symbol_augmenter(augmenter: AugmenterId) -> &'static str {
    match augmenter {
        AugmenterId::Kotlin => "SymbolAugmenter::Kotlin",
        AugmenterId::GradleKotlin => "SymbolAugmenter::GradleKotlin",
        AugmenterId::ObjectiveC => "SymbolAugmenter::ObjectiveC",
        AugmenterId::Zig => "SymbolAugmenter::Zig",
        AugmenterId::GradleGroovy => "SymbolAugmenter::GradleGroovy",
    }
}

/// Shared deterministic generated-file preamble.
fn render_rust_header(
    output: &mut String,
    owner: &str,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<(), LanguageRegistryError> {
    output.push_str(
        "// @generated by `cargo projectatlas-lints language-registry write`; do not edit.\n",
    );
    push_format(
        output,
        format_args!("//! Generated {owner} language-registry projection.\n\n"),
    )?;
    push_format(
        output,
        format_args!(
            "pub(crate) const LANGUAGE_REGISTRY_SOURCE_LOCK_SHA256: &str = {};\n",
            rust_string(source_lock_sha256)
        ),
    )?;
    push_format(
        output,
        format_args!(
            "pub(crate) const LANGUAGE_REGISTRY_CONTRACT_SHA256: &str = {};\n\n",
            rust_string(registry_contract_sha256)
        ),
    )
}

/// Render the core detection and public-mode projection.
fn render_core_registry(
    lock: &LanguageRegistryLock,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<String, LanguageRegistryError> {
    let mut output = String::new();
    render_rust_header(
        &mut output,
        "core detection",
        source_lock_sha256,
        registry_contract_sha256,
    )?;
    output.push_str(
        "/// Parser coverage level available for a detected language family.\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub enum LanguageParserSupport {\n\
         \x20   /// A native tree-sitter adapter backs symbol extraction.\n\
         \x20   Native,\n\
         \x20   /// A manifest-specific parser backs package/dependency extraction.\n\
         \x20   Manifest,\n\
         \x20   /// A deterministic structural summarizer backs agent-facing summaries.\n\
         \x20   Structural,\n\
         \x20   /// A conservative fallback parser is the current coverage boundary.\n\
         \x20   Fallback,\n\
         }\n\n\
         /// Static parser coverage metadata for one detected language family.\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub struct LanguageSpec {\n\
         \x20   /// Detected language or file-family identifier.\n\
         \x20   pub language: &'static str,\n\
         \x20   /// Parser coverage level.\n\
         \x20   pub parser_support: LanguageParserSupport,\n\
         }\n\n\
         #[allow(dead_code, reason = \"closed generated content matcher consumed only by the rich detector\")]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum LanguageContentDetector {\n\
         \x20   ShebangPython,\n\
         \x20   ShebangShell,\n\
         \x20   ShebangJavascript,\n\
         \x20   ShebangRuby,\n\
         \x20   ShebangPerl,\n\
         \x20   SignaturePhp,\n\
         \x20   SignatureXml,\n\
         \x20   ContextDockerBuild,\n\
         }\n\n\
         /// Optional semantic interpretation layered over a compatible base language.\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub enum LanguageSemanticMode {\n\
         \x20   /// Kubernetes resource semantics over YAML.\n\
         \x20   Kubernetes,\n\
         \x20   /// Kustomize configuration semantics over YAML.\n\
         \x20   Kustomize,\n\
         }\n\n\
         #[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum DetectionStage {\n\
         \x20   ExactFilename,\n\
         \x20   CompoundExtension,\n\
         \x20   Extension,\n\
         \x20   Shebang,\n\
         \x20   ContentSignature,\n\
         \x20   ProjectContext,\n\
         }\n\n\
         #[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum DetectionCase {\n\
         \x20   Sensitive,\n\
         \x20   AsciiInsensitive,\n\
         }\n\n\
         #[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguageDetectionRule {\n\
         \x20   pub(crate) id: &'static str,\n\
         \x20   pub(crate) stage: DetectionStage,\n\
         \x20   pub(crate) pattern: &'static str,\n\
         \x20   pub(crate) lookup_case: DetectionCase,\n\
         \x20   pub(crate) path_case: DetectionCase,\n\
         \x20   pub(crate) content_detector: Option<LanguageContentDetector>,\n\
         \x20   pub(crate) scanner_visible: bool,\n\
         \x20   pub(crate) language: &'static str,\n\
         }\n\n\
         #[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         pub(crate) static LANGUAGE_DETECTION_RULES: &[LanguageDetectionRule] = &[\n",
    );
    for rule in &lock.detection_rules {
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == rule.mode_id())
            .ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "detection rule {} references missing current mode {}",
                    rule.id().as_str(),
                    rule.mode_id().as_str()
                ))
            })?;
        let stage = match rule {
            DetectionRule::ExactFilename { .. } => "DetectionStage::ExactFilename",
            DetectionRule::CompoundExtension { .. } => "DetectionStage::CompoundExtension",
            DetectionRule::Extension { .. } => "DetectionStage::Extension",
            DetectionRule::Content {
                detector_kind: ContentDetectionKind::Shebang,
                ..
            } => "DetectionStage::Shebang",
            DetectionRule::Content {
                detector_kind: ContentDetectionKind::ContentSignature,
                ..
            } => "DetectionStage::ContentSignature",
            DetectionRule::Content {
                detector_kind: ContentDetectionKind::ProjectContext,
                ..
            } => "DetectionStage::ProjectContext",
        };
        let lookup_case = match rule.case_policy() {
            CasePolicy::Sensitive => "DetectionCase::Sensitive",
            CasePolicy::AsciiInsensitive => "DetectionCase::AsciiInsensitive",
        };
        let path_case = match rule.path_case_policy() {
            CasePolicy::Sensitive => "DetectionCase::Sensitive",
            CasePolicy::AsciiInsensitive => "DetectionCase::AsciiInsensitive",
        };
        let content_detector = match rule {
            DetectionRule::Content { detector_id, .. } => format!(
                "Some(LanguageContentDetector::{})",
                detector_id.rust_variant()
            ),
            DetectionRule::ExactFilename { .. }
            | DetectionRule::CompoundExtension { .. }
            | DetectionRule::Extension { .. } => "None".to_string(),
        };
        push_format(
            &mut output,
            format_args!(
                "    LanguageDetectionRule {{ id: {}, stage: {stage}, pattern: {}, lookup_case: {lookup_case}, path_case: {path_case}, content_detector: {content_detector}, scanner_visible: {}, language: {} }},\n",
                rust_string(rule.id().as_str()),
                rust_string(rule.pattern()),
                rule.scanner_visible(),
                rust_string(public_mode.public_mode.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");
    if !lock.semantic_modes.is_empty() {
        output.push_str(
            "impl LanguageSemanticMode {\n\
         \x20   /// Return whether this semantic mode can refine the detected base language.\n\
         \x20   #[must_use]\n\
         \x20   pub fn is_compatible_with(self, base_language: &str) -> bool {\n\
         \x20       match self {\n",
        );
        let mut semantic_variants_by_base = BTreeMap::<&str, Vec<&str>>::new();
        for semantic_mode in &lock.semantic_modes {
            let base_mode = lock
                .current_modes
                .iter()
                .find(|mode| mode.mode_id == semantic_mode.base_mode_id)
                .ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "semantic mode {} references missing base mode {}",
                        semantic_mode.mode.public_mode(),
                        semantic_mode.base_mode_id.as_str()
                    ))
                })?;
            semantic_variants_by_base
                .entry(base_mode.public_mode.as_str())
                .or_default()
                .push(semantic_mode.mode.rust_variant());
        }
        for (base_language, semantic_variants) in semantic_variants_by_base {
            output.push_str("            ");
            for (index, semantic_variant) in semantic_variants.iter().enumerate() {
                if index > 0 {
                    output.push_str(" | ");
                }
                push_format(&mut output, format_args!("Self::{semantic_variant}"))?;
            }
            push_format(
                &mut output,
                format_args!(" => base_language == {},\n", rust_string(base_language)),
            )?;
        }
        output.push_str("        }\n    }\n}\n\n");
    }

    if lock
        .detection_rules
        .iter()
        .any(|rule| matches!(rule, DetectionRule::Content { .. }))
    {
        output.push_str(
            "pub(crate) const fn detect_content_language(detector: LanguageContentDetector) -> &'static str {\n\
         \x20   match detector {\n",
        );
        for rule in &lock.detection_rules {
            let DetectionRule::Content {
                detector_id,
                mode_id,
                ..
            } = rule
            else {
                continue;
            };
            let public_mode = lock
                .current_modes
                .iter()
                .find(|mode| &mode.mode_id == mode_id)
                .ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "content detector {} references missing current mode {}",
                        detector_id.contract_id(),
                        mode_id.as_str()
                    ))
                })?;
            push_format(
                &mut output,
                format_args!(
                    "        LanguageContentDetector::{} => {},\n",
                    detector_id.rust_variant(),
                    rust_string(public_mode.public_mode.as_str())
                ),
            )?;
        }
        output.push_str("    }\n}\n\n");
    }
    output.push_str(
        "#[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguageMode {\n\
         \x20   pub(crate) mode_id: &'static str,\n\
         \x20   pub(crate) public_mode: &'static str,\n\
         \x20   pub(crate) accepted_mode_id: &'static str,\n\
         \x20   pub(crate) alias_of: Option<&'static str>,\n\
         \x20   pub(crate) current_pack_id: &'static str,\n\
         }\n\n\
         #[allow(dead_code, reason = \"complete generated registry projection retains validated metadata outside the runtime routing facade\")]\n\
         pub(crate) static CURRENT_LANGUAGE_MODES: &[LanguageMode] = &[\n",
    );
    for mode in &lock.current_modes {
        let alias_of = mode.alias_of.as_ref().map_or_else(
            || "None".to_string(),
            |alias| format!("Some({})", rust_string(alias.as_str())),
        );
        push_format(
            &mut output,
            format_args!(
                "    LanguageMode {{ mode_id: {}, public_mode: {}, accepted_mode_id: {}, alias_of: {alias_of}, current_pack_id: {} }},\n",
                rust_string(mode.mode_id.as_str()),
                rust_string(mode.public_mode.as_str()),
                rust_string(mode.accepted_mode_id.as_str()),
                rust_string(mode.current_pack_id.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");

    output.push_str("pub(crate) static SCANNER_SOURCE_EXTENSIONS: &[&str] = &[\n");
    for rule in &lock.detection_rules {
        if rule.scanner_visible()
            && matches!(
                rule,
                DetectionRule::CompoundExtension { .. } | DetectionRule::Extension { .. }
            )
        {
            push_format(
                &mut output,
                format_args!("    {},\n", rust_string(rule.pattern())),
            )?;
        }
    }
    output.push_str("];\n\n");

    output.push_str("pub(crate) static CURRENT_LANGUAGE_SPECS: &[LanguageSpec] = &[\n");
    for mode in &lock.current_modes {
        push_format(
            &mut output,
            format_args!(
                "    LanguageSpec {{ language: {}, parser_support: {} }},\n",
                rust_string(mode.public_mode.as_str()),
                rust_parser_support(mode.parser_support)
            ),
        )?;
    }
    output.push_str("];\n\n");

    let mut compound_rules = lock
        .detection_rules
        .iter()
        .filter(|rule| matches!(rule, DetectionRule::CompoundExtension { .. }))
        .collect::<Vec<_>>();
    compound_rules.sort_by(|left, right| {
        right
            .pattern()
            .len()
            .cmp(&left.pattern().len())
            .then_with(|| left.id().as_str().cmp(right.id().as_str()))
    });
    output.push_str(
        "pub(crate) fn detect_compound_extension(path: &str, extension: Option<&str>) -> Option<&'static str> {\n\
         \x20   let basename = path.rsplit(['/', '\\\\']).next().unwrap_or(path);\n",
    );
    for rule in &compound_rules {
        let DetectionRule::CompoundExtension {
            extension,
            case,
            path_suffix_case,
            mode_id,
            ..
        } = rule
        else {
            continue;
        };
        let path_condition = match path_suffix_case {
            CasePolicy::Sensitive => {
                format!("basename.ends_with({})", rust_string(extension))
            }
            CasePolicy::AsciiInsensitive => format!(
                "basename.get(basename.len().saturating_sub({}.len())..).is_some_and(|suffix| suffix.eq_ignore_ascii_case({}))",
                rust_string(extension),
                rust_string(extension)
            ),
        };
        let extension_condition = match case {
            CasePolicy::Sensitive => {
                format!("extension == Some({})", rust_string(extension))
            }
            CasePolicy::AsciiInsensitive => format!(
                "extension.is_some_and(|candidate| candidate.eq_ignore_ascii_case({}))",
                rust_string(extension)
            ),
        };
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == mode_id)
            .ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "compound detection rule {} references missing current mode {}",
                    rule.id().as_str(),
                    mode_id.as_str()
                ))
            })?;
        push_format(
            &mut output,
            format_args!(
                "    if {path_condition} || {extension_condition} {{\n        return Some({});\n    }}\n",
                rust_string(public_mode.public_mode.as_str())
            ),
        )?;
    }
    output.push_str("    None\n}\n\n");

    output.push_str(
        "pub(crate) fn normalized_compound_extension(path: &str) -> Option<&'static str> {\n\
         \x20   let basename = path.rsplit(['/', '\\\\']).next().unwrap_or(path);\n",
    );
    for rule in &compound_rules {
        let DetectionRule::CompoundExtension {
            extension,
            path_suffix_case,
            ..
        } = rule
        else {
            continue;
        };
        let path_condition = match path_suffix_case {
            CasePolicy::Sensitive => {
                format!("basename.ends_with({})", rust_string(extension))
            }
            CasePolicy::AsciiInsensitive => format!(
                "basename.get(basename.len().saturating_sub({}.len())..).is_some_and(|suffix| suffix.eq_ignore_ascii_case({}))",
                rust_string(extension),
                rust_string(extension)
            ),
        };
        push_format(
            &mut output,
            format_args!(
                "    if {path_condition} {{\n        return Some({});\n    }}\n",
                rust_string(extension)
            ),
        )?;
    }
    output.push_str("    None\n}\n\n");

    let mut sensitive_extensions = BTreeSet::new();
    let mut insensitive_extensions = BTreeSet::new();
    let mut sensitive_extension_groups = Vec::<(String, Vec<String>)>::new();
    let mut insensitive_extension_groups = Vec::<(String, Vec<String>)>::new();
    for rule in &lock.detection_rules {
        if !matches!(
            rule,
            DetectionRule::CompoundExtension { .. } | DetectionRule::Extension { .. }
        ) {
            continue;
        }
        let (seen, groups, pattern) = match rule.case_policy() {
            CasePolicy::Sensitive => (
                &mut sensitive_extensions,
                &mut sensitive_extension_groups,
                rule.pattern().to_string(),
            ),
            CasePolicy::AsciiInsensitive => (
                &mut insensitive_extensions,
                &mut insensitive_extension_groups,
                rule.pattern().to_ascii_lowercase(),
            ),
        };
        if !seen.insert(pattern.clone()) {
            continue;
        }
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == rule.mode_id())
            .ok_or_else(|| {
                LanguageRegistryError::Validation(format!(
                    "detection rule {} references missing current mode {}",
                    rule.id().as_str(),
                    rule.mode_id().as_str()
                ))
            })?;
        let public_mode = public_mode.public_mode.as_str();
        if let Some((_, patterns)) = groups.iter_mut().find(|(mode, _)| mode == public_mode) {
            patterns.push(pattern);
        } else {
            groups.push((public_mode.to_string(), vec![pattern]));
        }
    }

    output.push_str("pub(crate) fn detect_extension(extension: &str) -> Option<&'static str> {\n");
    if insensitive_extension_groups.is_empty() {
        output.push_str("    match extension {\n");
        for (public_mode, patterns) in sensitive_extension_groups {
            let patterns = patterns
                .iter()
                .map(|pattern| rust_string(pattern))
                .collect::<Vec<_>>()
                .join(" | ");
            push_format(
                &mut output,
                format_args!(
                    "        {} => Some({}),\n",
                    patterns,
                    rust_string(&public_mode)
                ),
            )?;
        }
        output.push_str("        _ => None,\n    }\n}\n\n");
    } else {
        if !sensitive_extension_groups.is_empty() {
            output.push_str("    match extension {\n");
            for (public_mode, patterns) in sensitive_extension_groups {
                let patterns = patterns
                    .iter()
                    .map(|pattern| rust_string(pattern))
                    .collect::<Vec<_>>()
                    .join(" | ");
                push_format(
                    &mut output,
                    format_args!(
                        "        {} => return Some({}),\n",
                        patterns,
                        rust_string(&public_mode)
                    ),
                )?;
            }
            output.push_str("        _ => {}\n    }\n");
        }
        output.push_str(
            "    let extension = extension.to_ascii_lowercase();\n\
             \x20   match extension.as_str() {\n",
        );
        for (public_mode, patterns) in insensitive_extension_groups {
            let patterns = patterns
                .iter()
                .map(|pattern| rust_string(pattern))
                .collect::<Vec<_>>()
                .join(" | ");
            push_format(
                &mut output,
                format_args!(
                    "        {} => Some({}),\n",
                    patterns,
                    rust_string(&public_mode)
                ),
            )?;
        }
        output.push_str("        _ => None,\n    }\n}\n\n");
    }

    let has_insensitive_exact_filename = lock.detection_rules.iter().any(|rule| {
        matches!(rule, DetectionRule::ExactFilename { .. })
            && rule.case_policy() == CasePolicy::AsciiInsensitive
    });
    output.push_str(
        "pub(crate) fn detect_exact_filename(file_name: &str) -> Option<&'static str> {\n",
    );
    let sensitive_exact_filenames = lock
        .detection_rules
        .iter()
        .filter(|rule| {
            matches!(rule, DetectionRule::ExactFilename { .. })
                && rule.case_policy() == CasePolicy::Sensitive
        })
        .collect::<Vec<_>>();
    if !sensitive_exact_filenames.is_empty() {
        output.push_str("    match file_name {\n");
        for rule in sensitive_exact_filenames {
            let public_mode = lock
                .current_modes
                .iter()
                .find(|mode| &mode.mode_id == rule.mode_id())
                .ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "detection rule {} references missing current mode {}",
                        rule.id().as_str(),
                        rule.mode_id().as_str()
                    ))
                })?;
            let result = if has_insensitive_exact_filename {
                format!(
                    "return Some({})",
                    rust_string(public_mode.public_mode.as_str())
                )
            } else {
                format!("Some({})", rust_string(public_mode.public_mode.as_str()))
            };
            push_format(
                &mut output,
                format_args!("        {} => {result},\n", rust_string(rule.pattern())),
            )?;
        }
        if has_insensitive_exact_filename {
            output.push_str("        _ => {}\n    }\n");
        } else {
            output.push_str("        _ => None,\n    }\n}\n");
        }
    }
    if has_insensitive_exact_filename {
        for rule in &lock.detection_rules {
            if !matches!(rule, DetectionRule::ExactFilename { .. })
                || rule.case_policy() != CasePolicy::AsciiInsensitive
            {
                continue;
            }
            let public_mode = lock
                .current_modes
                .iter()
                .find(|mode| &mode.mode_id == rule.mode_id())
                .ok_or_else(|| {
                    LanguageRegistryError::Validation(format!(
                        "detection rule {} references missing current mode {}",
                        rule.id().as_str(),
                        rule.mode_id().as_str()
                    ))
                })?;
            push_format(
                &mut output,
                format_args!(
                    "    if file_name.eq_ignore_ascii_case({}) {{\n        return Some({});\n    }}\n",
                    rust_string(rule.pattern()),
                    rust_string(public_mode.public_mode.as_str())
                ),
            )?;
        }
        output.push_str("    None\n}\n");
    }
    Ok(output)
}

/// Render closed current parser, adapter, and augmenter routes.
fn render_symbols_registry(
    lock: &LanguageRegistryLock,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<String, LanguageRegistryError> {
    let mut output = String::new();
    render_rust_header(
        &mut output,
        "symbols routing",
        source_lock_sha256,
        registry_contract_sha256,
    )?;
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum BuiltInParser {\n\
         \x20   Rust, Python, JavaScript, TypeScript, Tsx, Java, Kotlin, CSharp, Go, ObjectiveC, Zig, C, Cpp,\n\
         }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum ManifestAdapter { CargoManifest, CargoLock }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum StructuralAdapter { Vue, PowerShell }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum SymbolAugmenter { Kotlin, GradleKotlin, ObjectiveC, Zig, GradleGroovy }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum SymbolRoute {\n\
         \x20   Skip,\n\
         \x20   BuiltIn { parser: BuiltInParser, augmenters: &'static [SymbolAugmenter] },\n\
         \x20   Manifest(ManifestAdapter),\n\
         \x20   Structural(StructuralAdapter),\n\
         \x20   Fallback { augmenters: &'static [SymbolAugmenter] },\n\
         }\n\n\
         impl SymbolRoute {\n\
         \x20   pub(crate) const fn augmenters(&self) -> &'static [SymbolAugmenter] {\n\
         \x20       match self {\n\
         \x20           Self::BuiltIn { augmenters, .. } | Self::Fallback { augmenters, .. } => augmenters,\n\
         \x20           Self::Skip | Self::Manifest(_) | Self::Structural(_) => &[],\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguageSymbolRoute {\n\
         \x20   pub(crate) public_mode: &'static str,\n\
         \x20   pub(crate) route: SymbolRoute,\n\
         }\n\n\
         pub(crate) static CURRENT_SYMBOL_ROUTES: &[LanguageSymbolRoute] = &[\n",
    );
    for mode in &lock.current_modes {
        let public_mode = rust_string(mode.public_mode.as_str());
        let row = match &mode.symbols {
            SymbolPipeline::Skip => format!(
                "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Skip }},\n"
            ),
            SymbolPipeline::BuiltIn { parser, augmenters } => {
                let augmenter_literals = augmenters
                    .iter()
                    .map(|augmenter| rust_symbol_augmenter(*augmenter))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::BuiltIn {{ parser: {}, augmenters: &[{augmenter_literals}] }} }},\n",
                    rust_built_in_parser(*parser)
                )
            }
            SymbolPipeline::Manifest { adapter } => {
                let adapter = match adapter {
                    ManifestAdapterId::CargoManifest => "ManifestAdapter::CargoManifest",
                    ManifestAdapterId::CargoLock => "ManifestAdapter::CargoLock",
                };
                format!(
                    "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Manifest({adapter}) }},\n"
                )
            }
            SymbolPipeline::Structural { adapter } => {
                let adapter = match adapter {
                    SymbolAdapterId::Vue => "StructuralAdapter::Vue",
                    SymbolAdapterId::Powershell => "StructuralAdapter::PowerShell",
                };
                format!(
                    "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Structural({adapter}) }},\n"
                )
            }
            SymbolPipeline::Fallback { augmenters } => {
                let augmenter_literals = augmenters
                    .iter()
                    .map(|augmenter| rust_symbol_augmenter(*augmenter))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Fallback {{ augmenters: &[{augmenter_literals}] }} }},\n"
                )
            }
        };
        output.push_str(&row);
    }
    output.push_str("];\n\n");
    output.push_str(
        "pub(crate) fn symbol_route_for_public_mode(\n\
         \x20   public_mode: &str,\n\
         ) -> Option<&'static SymbolRoute> {\n\
         \x20   match public_mode {\n",
    );
    for (index, mode) in lock.current_modes.iter().enumerate() {
        push_format(
            &mut output,
            format_args!(
                "        {} => Some(&CURRENT_SYMBOL_ROUTES[{index}].route),\n",
                rust_string(mode.public_mode.as_str())
            ),
        )?;
    }
    output.push_str("        _ => None,\n    }\n}\n\n");
    output.push_str("pub(crate) static SPECIALIZED_LANGUAGES: &[&str] = &[\n");
    for mode in &lock.current_modes {
        if matches!(mode.symbols, SymbolPipeline::BuiltIn { .. }) {
            push_format(
                &mut output,
                format_args!("    {},\n", rust_string(mode.public_mode.as_str())),
            )?;
        }
    }
    output.push_str("];\n\n");
    output.push_str(
        "pub(crate) fn built_in_parser_for_public_mode(\n\
         \x20   public_mode: &str,\n\
         ) -> Option<BuiltInParser> {\n\
         \x20   match symbol_route_for_public_mode(public_mode)? {\n\
         \x20       SymbolRoute::BuiltIn { parser, .. } => Some(*parser),\n\
         \x20       SymbolRoute::Skip\n\
         \x20       | SymbolRoute::Manifest(_)\n\
         \x20       | SymbolRoute::Structural(_)\n\
         \x20       | SymbolRoute::Fallback { .. } => None,\n\
         \x20   }\n\
         }\n\n",
    );
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum ParserImplementation { CompiledTreeSitter }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum ParserAbiState { CurrentCompiledContract, PendingPackVerification }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct ParserComponentContract {\n\
         \x20   pub(crate) parser_id: &'static str,\n\
         \x20   pub(crate) built_in_parser: BuiltInParser,\n\
         \x20   pub(crate) implementation: ParserImplementation,\n\
         \x20   pub(crate) current_pack_id: &'static str,\n\
         \x20   pub(crate) abi_id: &'static str,\n\
         \x20   pub(crate) abi_version: u32,\n\
         \x20   pub(crate) abi_state: ParserAbiState,\n\
         \x20   pub(crate) asset_id: Option<&'static str>,\n\
         \x20   pub(crate) query_pack_id: Option<&'static str>,\n\
         \x20   pub(crate) fixture_ids: &'static [&'static str],\n\
         \x20   pub(crate) provenance_evidence_ids: &'static [&'static str],\n\
         }\n\n\
         pub(crate) static CURRENT_PARSER_COMPONENTS: &[ParserComponentContract] = &[\n",
    );
    for component in &lock.parser_components {
        let asset_id = component.asset_id.as_ref().map_or_else(
            || "None".to_string(),
            |asset| format!("Some({})", rust_string(asset.as_str())),
        );
        let query_pack_id = component.query_pack_id.as_ref().map_or_else(
            || "None".to_string(),
            |query_pack| format!("Some({})", rust_string(query_pack.as_str())),
        );
        let fixture_ids = component
            .fixture_ids
            .iter()
            .map(|fixture| rust_string(fixture.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let provenance_evidence_ids = component
            .provenance_evidence_ids
            .iter()
            .map(|evidence| rust_string(evidence.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let implementation = match component.implementation {
            ParserImplementation::CompiledTreeSitter => "ParserImplementation::CompiledTreeSitter",
        };
        let abi_state = match component.abi.state {
            AbiState::CurrentCompiledContract => "ParserAbiState::CurrentCompiledContract",
            AbiState::PendingPackVerification => "ParserAbiState::PendingPackVerification",
        };
        push_format(
            &mut output,
            format_args!(
                "    ParserComponentContract {{ parser_id: {}, built_in_parser: {}, implementation: {implementation}, current_pack_id: {}, abi_id: {}, abi_version: {}, abi_state: {abi_state}, asset_id: {asset_id}, query_pack_id: {query_pack_id}, fixture_ids: &[{fixture_ids}], provenance_evidence_ids: &[{provenance_evidence_ids}] }},\n",
                rust_string(component.parser_id.as_str()),
                rust_built_in_parser(component.built_in_parser),
                rust_string(component.current_pack_id.as_str()),
                rust_string(component.abi.abi_id.as_str()),
                component.abi.version
            ),
        )?;
    }
    output.push_str("];\n\n");
    output.push_str(
        "pub(crate) const fn parser_component_id(parser: BuiltInParser) -> &'static str {\n\
         \x20   match parser {\n",
    );
    for component in &lock.parser_components {
        push_format(
            &mut output,
            format_args!(
                "        {} => {},\n",
                rust_built_in_parser(component.built_in_parser),
                rust_string(component.parser_id.as_str())
            ),
        )?;
    }
    output.push_str("    }\n}\n\n");
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct EmbeddedLanguageAdapter {\n\
         \x20   pub(crate) adapter_id: &'static str,\n\
         \x20   pub(crate) host_mode_id: &'static str,\n\
         \x20   pub(crate) embedded_mode_id: &'static str,\n\
         \x20   pub(crate) pack_id: &'static str,\n\
         \x20   pub(crate) query_pack_id: Option<&'static str>,\n\
         \x20   pub(crate) fixture_ids: &'static [&'static str],\n\
         }\n\n\
         pub(crate) static EMBEDDED_LANGUAGE_ADAPTERS: &[EmbeddedLanguageAdapter] = &[\n",
    );
    for adapter in &lock.embedded_adapters {
        let query_pack_id = adapter.query_pack_id.as_ref().map_or_else(
            || "None".to_string(),
            |query_pack| format!("Some({})", rust_string(query_pack.as_str())),
        );
        let fixture_ids = adapter
            .fixture_ids
            .iter()
            .map(|fixture| rust_string(fixture.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        push_format(
            &mut output,
            format_args!(
                "    EmbeddedLanguageAdapter {{ adapter_id: {}, host_mode_id: {}, embedded_mode_id: {}, pack_id: {}, query_pack_id: {query_pack_id}, fixture_ids: &[{fixture_ids}] }},\n",
                rust_string(adapter.adapter_id.as_str()),
                rust_string(adapter.host_mode_id.as_str()),
                rust_string(adapter.embedded_mode_id.as_str()),
                rust_string(adapter.pack_id.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct ExtractionQueryPack {\n\
         \x20   pub(crate) query_pack_id: &'static str,\n\
         \x20   pub(crate) path: &'static str,\n\
         \x20   pub(crate) pack_id: &'static str,\n\
         \x20   pub(crate) digest_sha256: &'static str,\n\
         }\n\n\
         pub(crate) static EXTRACTION_QUERY_PACKS: &[ExtractionQueryPack] = &[\n",
    );
    for query_pack in &lock.query_packs {
        push_format(
            &mut output,
            format_args!(
                "    ExtractionQueryPack {{ query_pack_id: {}, path: {}, pack_id: {}, digest_sha256: {} }},\n",
                rust_string(query_pack.id.as_str()),
                rust_string(query_pack.path.as_str()),
                rust_string(query_pack.pack_id.as_str()),
                rust_string(query_pack.digest_sha256.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct SemanticProviderContract {\n\
         \x20   pub(crate) provider_id: &'static str,\n\
         \x20   pub(crate) pack_id: &'static str,\n\
         \x20   pub(crate) mode_ids: &'static [&'static str],\n\
         \x20   pub(crate) fixture_ids: &'static [&'static str],\n\
         }\n\n\
         pub(crate) static SEMANTIC_PROVIDERS: &[SemanticProviderContract] = &[\n",
    );
    for provider in &lock.semantic_providers {
        let mode_ids = provider
            .mode_ids
            .iter()
            .map(|mode| rust_string(mode.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let fixture_ids = provider
            .fixture_ids
            .iter()
            .map(|fixture| rust_string(fixture.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        push_format(
            &mut output,
            format_args!(
                "    SemanticProviderContract {{ provider_id: {}, pack_id: {}, mode_ids: &[{mode_ids}], fixture_ids: &[{fixture_ids}] }},\n",
                rust_string(provider.provider_id.as_str()),
                rust_string(provider.pack_id.as_str())
            ),
        )?;
    }
    output.push_str("];\n");
    Ok(output)
}

/// Render CLI summary and support policy from current behavior only.
fn render_cli_registry(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<String, LanguageRegistryError> {
    let mut output = String::new();
    render_rust_header(
        &mut output,
        "CLI language policy",
        source_lock_sha256,
        registry_contract_sha256,
    )?;
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum ParserSupport { Native, Manifest, Structural, Fallback }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum SummaryAdapter { None, Markdown, Json, Yaml, Css, Html, Toon, ConfigText, Toml, Xml, PowerShell }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum SymbolRouteKind { Skip, BuiltIn, Manifest, Structural, Fallback }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguagePolicy {\n\
         \x20   pub(crate) public_mode: &'static str,\n\
         \x20   pub(crate) parser_support: ParserSupport,\n\
         \x20   pub(crate) summary_adapter: SummaryAdapter,\n\
         \x20   pub(crate) symbol_route: SymbolRouteKind,\n\
         }\n\n\
         pub(crate) static CURRENT_LANGUAGE_POLICY: &[LanguagePolicy] = &[\n",
    );
    for mode in &lock.current_modes {
        let parser_support = match mode.parser_support {
            ParserSupport::Native => "ParserSupport::Native",
            ParserSupport::Manifest => "ParserSupport::Manifest",
            ParserSupport::Structural => "ParserSupport::Structural",
            ParserSupport::Fallback => "ParserSupport::Fallback",
        };
        let summary_adapter = match mode.summary_adapter {
            SummaryAdapterId::None => "SummaryAdapter::None",
            SummaryAdapterId::Markdown => "SummaryAdapter::Markdown",
            SummaryAdapterId::Json => "SummaryAdapter::Json",
            SummaryAdapterId::Yaml => "SummaryAdapter::Yaml",
            SummaryAdapterId::Css => "SummaryAdapter::Css",
            SummaryAdapterId::Html => "SummaryAdapter::Html",
            SummaryAdapterId::Toon => "SummaryAdapter::Toon",
            SummaryAdapterId::ConfigText => "SummaryAdapter::ConfigText",
            SummaryAdapterId::Toml => "SummaryAdapter::Toml",
            SummaryAdapterId::Xml => "SummaryAdapter::Xml",
            SummaryAdapterId::Powershell => "SummaryAdapter::PowerShell",
        };
        let symbol_route = match &mode.symbols {
            SymbolPipeline::Skip => "SymbolRouteKind::Skip",
            SymbolPipeline::BuiltIn { .. } => "SymbolRouteKind::BuiltIn",
            SymbolPipeline::Manifest { .. } => "SymbolRouteKind::Manifest",
            SymbolPipeline::Structural { .. } => "SymbolRouteKind::Structural",
            SymbolPipeline::Fallback { .. } => "SymbolRouteKind::Fallback",
        };
        push_format(
            &mut output,
            format_args!(
                "    LanguagePolicy {{ public_mode: {}, parser_support: {parser_support}, summary_adapter: {summary_adapter}, symbol_route: {symbol_route} }},\n",
                rust_string(mode.public_mode.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");
    output.push_str(
        "pub(crate) fn language_policy_for_public_mode(\n\
         \x20   public_mode: &str,\n\
         ) -> Option<&'static LanguagePolicy> {\n\
         \x20   match public_mode {\n",
    );
    for (index, mode) in lock.current_modes.iter().enumerate() {
        push_format(
            &mut output,
            format_args!(
                "        {} => Some(&CURRENT_LANGUAGE_POLICY[{index}]),\n",
                rust_string(mode.public_mode.as_str())
            ),
        )?;
    }
    output.push_str("        _ => None,\n    }\n}\n\n");
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum AcceptedAdvertisement { BlockedUntilAchievedManifest }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum CapabilityTier { Detected, Parsed, Symbols, Semantic, Benchmarked }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum PackOwnership { DefaultCore, Optional }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) enum PackRuntime { InProcess, SupervisedWorker }\n\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguageRegistrySettings {\n\
         \x20   pub(crate) registry_id: &'static str,\n\
         \x20   pub(crate) accepted_registry_id: &'static str,\n\
         \x20   pub(crate) accepted_set_sha256: &'static str,\n\
         \x20   pub(crate) accepted_advertisement: AcceptedAdvertisement,\n\
         \x20   pub(crate) current_mode_count: usize,\n\
         \x20   pub(crate) accepted_mode_count: usize,\n\
         \x20   pub(crate) accepted_pre_parse_transform_count: usize,\n\
         \x20   pub(crate) normalized_parser_capability_count: usize,\n\
         \x20   pub(crate) parser_component_count: usize,\n\
         \x20   pub(crate) parser_asset_count: usize,\n\
         \x20   pub(crate) embedded_adapter_count: usize,\n\
         \x20   pub(crate) query_pack_count: usize,\n\
         \x20   pub(crate) semantic_provider_count: usize,\n\
         }\n\n",
    );
    let accepted_advertisement = match accepted.source.mode_defaults.advertisement {
        AcceptedModeAdvertisement::BlockedUntilAchievedManifest => {
            "AcceptedAdvertisement::BlockedUntilAchievedManifest"
        }
    };
    push_format(
        &mut output,
        format_args!(
            "pub(crate) static LANGUAGE_REGISTRY_SETTINGS: LanguageRegistrySettings = LanguageRegistrySettings {{ registry_id: {}, accepted_registry_id: {}, accepted_set_sha256: {}, accepted_advertisement: {accepted_advertisement}, current_mode_count: {}, accepted_mode_count: {}, accepted_pre_parse_transform_count: {}, normalized_parser_capability_count: {}, parser_component_count: {}, parser_asset_count: {}, embedded_adapter_count: {}, query_pack_count: {}, semantic_provider_count: {} }};\n\n",
            rust_string(lock.registry_id.as_str()),
            rust_string(accepted.source.registry_id.as_str()),
            rust_string(accepted.source.accepted_set_digest.as_str()),
            lock.current_modes.len(),
            accepted.modes.len(),
            accepted
                .modes
                .iter()
                .filter(|mode| mode.pre_parse_transform.is_some())
                .count(),
            accepted.parsers.len(),
            lock.parser_components.len(),
            lock.assets.len(),
            lock.embedded_adapters.len(),
            lock.query_packs.len(),
            lock.semantic_providers.len()
        ),
    )?;
    output.push_str("pub(crate) static LANGUAGE_CAPABILITY_TIERS: &[CapabilityTier] = &[");
    for (index, tier) in lock.capability_tiers.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(match tier {
            CapabilityTier::Detected => "CapabilityTier::Detected",
            CapabilityTier::Parsed => "CapabilityTier::Parsed",
            CapabilityTier::Symbols => "CapabilityTier::Symbols",
            CapabilityTier::Semantic => "CapabilityTier::Semantic",
            CapabilityTier::Benchmarked => "CapabilityTier::Benchmarked",
        });
    }
    output.push_str("];\n\n");
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct LanguagePackSettings {\n\
         \x20   pub(crate) pack_id: &'static str,\n\
         \x20   pub(crate) ownership: PackOwnership,\n\
         \x20   pub(crate) runtime: PackRuntime,\n\
         }\n\n\
         pub(crate) static LANGUAGE_PACK_SETTINGS: &[LanguagePackSettings] = &[\n",
    );
    for pack in &lock.packs {
        let ownership = match pack.ownership {
            PackOwnership::DefaultCore => "PackOwnership::DefaultCore",
            PackOwnership::Optional => "PackOwnership::Optional",
        };
        let runtime = match pack.runtime {
            PackRuntime::InProcess => "PackRuntime::InProcess",
            PackRuntime::SupervisedWorker => "PackRuntime::SupervisedWorker",
        };
        push_format(
            &mut output,
            format_args!(
                "    LanguagePackSettings {{ pack_id: {}, ownership: {ownership}, runtime: {runtime} }},\n",
                rust_string(pack.pack_id.as_str())
            ),
        )?;
    }
    output.push_str("];\n\n");
    render_evaluation_parser_pack_trust(
        &mut output,
        parser_pack_trust,
        parser_pack_installed_byte_limit,
    )?;
    Ok(output)
}

/// Render test-only parser-pack trust records without adding production reachability.
fn render_evaluation_parser_pack_trust(
    output: &mut String,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
) -> Result<(), LanguageRegistryError> {
    output.push_str(
        "#[cfg(test)]\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         pub(crate) struct EvaluationParserPackTrust {\n\
         \x20   pub(crate) candidate_id: &'static str,\n\
         \x20   pub(crate) eligibility: &'static str,\n\
         \x20   pub(crate) pack_id: &'static str,\n\
         \x20   pub(crate) release_version: &'static str,\n\
         \x20   pub(crate) advertised: bool,\n\
         \x20   pub(crate) pack_abi_id: &'static str,\n\
         \x20   pub(crate) pack_abi_version: u32,\n\
         \x20   pub(crate) grammar_abi_id: &'static str,\n\
         \x20   pub(crate) grammar_abi_version: u32,\n\
         \x20   pub(crate) grammar_abi_state: &'static str,\n\
         \x20   pub(crate) packaged_platform: &'static str,\n\
         \x20   pub(crate) payload_root: &'static str,\n\
         \x20   pub(crate) manifest_path: &'static str,\n\
         \x20   pub(crate) installed_bytes: u64,\n\
         \x20   pub(crate) installed_byte_limit: u64,\n\
         }\n\n\
         #[cfg(test)]\n\
         pub(crate) static EVALUATION_PARSER_PACK_TRUST: &[EvaluationParserPackTrust] = &[\n",
    );
    for candidate in &parser_pack_trust.candidates {
        let installed_bytes = rust_u64_literal(candidate.installed_bytes);
        let installed_byte_limit = rust_u64_literal(parser_pack_installed_byte_limit);
        push_format(
            output,
            format_args!(
                "    EvaluationParserPackTrust {{ candidate_id: {}, eligibility: {}, pack_id: {}, release_version: {}, advertised: false, pack_abi_id: {}, pack_abi_version: {}, grammar_abi_id: {}, grammar_abi_version: {}, grammar_abi_state: {}, packaged_platform: {}, payload_root: {}, manifest_path: {}, installed_bytes: {installed_bytes}, installed_byte_limit: {installed_byte_limit} }},\n",
                rust_string(candidate.candidate_id.as_str()),
                rust_string(candidate.eligibility.contract_tag()),
                rust_string(candidate.pack_id.as_str()),
                rust_string(&candidate.release_version),
                rust_string(candidate.pack_abi.abi_id.as_str()),
                candidate.pack_abi.version,
                rust_string(candidate.grammar_abi.abi_id.as_str()),
                candidate.grammar_abi.version,
                rust_string(candidate.grammar_abi.state.contract_tag()),
                rust_string(candidate.packaged_platform.as_str()),
                rust_string(candidate.payload_root.as_str()),
                rust_string(candidate.manifest_path.as_str()),
            ),
        )?;
    }
    output.push_str("];\n");
    Ok(())
}

/// Complete structured evidence document generated from separate current and accepted axes.
#[derive(Serialize)]
struct CapabilityStateEvidence<'a> {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable evidence format.
    format: &'static str,
    /// Composite registry identity.
    registry_id: &'a str,
    /// Exact source-lock digest.
    source_lock_sha256: &'a str,
    /// Semantic composite-registry digest.
    registry_contract_sha256: &'a str,
    /// Current runtime state.
    current: CurrentCapabilityState<'a>,
    /// Accepted future-delivery target.
    accepted_target: AcceptedCapabilityState<'a>,
    /// Frozen prior-runtime binding.
    historical_contract: HistoricalCapabilityState<'a>,
    /// Release-owned, exact-digest-bound parser-pack evaluation trust.
    parser_pack_trust: ParserPackTrustState<'a>,
    /// Accepted capability-set parity projection.
    accepted_capability_parity: AcceptedCapabilityParity<'a>,
    /// Additive language settings projection.
    settings: LanguageSettingsState<'a>,
    /// Parser and fixture conformance inventory without execution claims.
    conformance_inventory: ConformanceInventory<'a>,
    /// Per-component inputs for later exact SBOM and provenance generation.
    sbom_inputs: SbomInputInventory<'a>,
}

/// Generated evaluation-only parser-pack trust state with no runtime selection claim.
#[derive(Serialize)]
struct ParserPackTrustState<'a> {
    /// Stable trust projection format.
    format: ParserPackTrustFormat,
    /// Exact release-owned trust-manifest path.
    path: &'a str,
    /// Exact release-owned trust-manifest digest.
    raw_sha256: &'a str,
    /// Authoritative broad-language-pack installed-byte ceiling.
    installed_byte_limit: u64,
    /// Explicit selection lifecycle; this projection cannot select a runtime candidate.
    selection_state: ParserPackSelectionState,
    /// Candidate selected for runtime use; always absent while selection is blocked.
    selected_candidate: Option<&'a str>,
    /// Achieved conformance manifest; always absent before the final evidence gates pass.
    achieved_manifest: Option<&'a str>,
    /// Complete validated evaluation candidate inventory.
    candidates: &'a [ParserPackCandidateTrust],
}

/// Closed runtime-selection state for the generated parser-pack trust projection.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ParserPackSelectionState {
    /// No evaluated candidate is reachable by the runtime.
    Blocked,
}

/// Accepted-set rows and their current evidence-complete state.
#[derive(Serialize)]
struct AcceptedCapabilityParity<'a> {
    /// Whether every accepted mode and parser has achieved its declared contract.
    complete: bool,
    /// Accepted mode rows joined to current runtime behavior when present.
    modes: Vec<CapabilityModeState<'a>>,
    /// Accepted normalized parser-capability rows.
    parsers: Vec<AcceptedParserState<'a>>,
    /// Accepted standard-name and dialect crosswalk.
    language_crosswalk: Vec<AcceptedLanguageState<'a>>,
    /// Accepted non-language capability inventory.
    capabilities: &'a [AcceptedCapability],
    /// Evidence requirements for every ordered language-support tier.
    claim_types: &'a BTreeMap<CapabilityTier, AcceptedClaimContract>,
    /// Shared relation traceability contract referenced by relation rows.
    relation_traceability_contract: &'a AcceptedRelationTraceabilityContract,
}

/// Registry-derived fields ready for additive CLI and MCP settings wiring.
#[derive(Serialize)]
struct LanguageSettingsState<'a> {
    /// Composite registry identity.
    registry_id: &'a str,
    /// Semantic composite-registry digest.
    registry_contract_sha256: &'a str,
    /// Accepted capability-set identity.
    accepted_registry_id: &'a str,
    /// Accepted capability-set semantic digest.
    accepted_set_sha256: &'a str,
    /// Honest accepted-set advertisement lifecycle.
    accepted_advertisement: AcceptedModeAdvertisement,
    /// Ordered support-tier vocabulary.
    capability_tiers: &'a [CapabilityTier],
    /// Current and optional feature-pack boundaries.
    packs: &'a [RegistryPack],
    /// Current public-mode count.
    current_modes: usize,
    /// Accepted delivery-mode count.
    accepted_modes: usize,
    /// Accepted pre-parse-transform count.
    accepted_pre_parse_transforms: usize,
    /// Accepted normalized parser-capability count.
    accepted_parser_capabilities: usize,
    /// Current compiled parser-component count.
    parser_components: usize,
    /// Declared external parser-asset count.
    parser_assets: usize,
    /// Declared embedded-adapter count.
    embedded_adapters: usize,
    /// Declared extraction-query-pack count.
    query_packs: usize,
    /// Declared semantic-provider count.
    semantic_providers: usize,
}

/// Expected conformance inputs for one accepted mode.
#[derive(Serialize)]
struct ModeConformanceState<'a> {
    /// Accepted stable mode identity.
    mode_id: &'a str,
    /// Public mode spelling.
    public_mode: &'a str,
    /// Accepted normalized parser identity.
    parser_id: &'a str,
    /// Optional accepted transform that runs before the selected parser.
    pre_parse_transform: Option<&'a AcceptedPreParseTransform>,
    /// Required evidence tiers.
    required_claims: Vec<&'a str>,
    /// Currently achieved evidence tiers.
    achieved_claims: Vec<&'a str>,
    /// Expected fixture identities from the accepted contract.
    fixture_ids: Vec<&'a str>,
    /// Expected fixtures already declared in the live registry.
    registered_fixture_ids: Vec<&'a str>,
    /// Expected fixtures not yet declared in the live registry.
    missing_fixture_ids: Vec<&'a str>,
    /// Required packaged release platforms.
    required_platforms: Vec<&'a str>,
    /// Current evidence lifecycle.
    evidence_state: AcceptedEvidenceState,
    /// Whether the row is currently advertised.
    advertised: bool,
}

/// Parser, adapter, fixture, and provider rows available to conformance tooling.
#[derive(Serialize)]
struct ConformanceInventory<'a> {
    /// Accepted mode expectations.
    modes: Vec<ModeConformanceState<'a>>,
    /// Live registry fixtures and their verification state.
    fixtures: &'a [RegistryFixture],
    /// Live extraction-query packs.
    query_packs: &'a [QueryPack],
    /// Live embedded-language adapters.
    embedded_adapters: &'a [EmbeddedAdapter],
    /// Live semantic providers.
    semantic_providers: &'a [SemanticProvider],
    /// Live evidence artifacts.
    evidence: &'a [RegistryEvidence],
}

/// One honest current-versus-accepted documentation support row.
#[derive(Serialize)]
struct DocumentationSupportRow<'a> {
    /// Accepted stable mode identity.
    mode_id: &'a str,
    /// Public mode spelling.
    public_mode: &'a str,
    /// Accepted normalized parser identity.
    parser_id: &'a str,
    /// Optional accepted transform that runs before the selected parser.
    pre_parse_transform: Option<&'a AcceptedPreParseTransform>,
    /// Accepted future feature-pack owner.
    future_pack_id: &'a str,
    /// Required evidence tiers.
    required_claims: Vec<&'a str>,
    /// Currently achieved evidence tiers.
    achieved_claims: Vec<&'a str>,
    /// Current evidence lifecycle.
    evidence_state: AcceptedEvidenceState,
    /// Accepted advertisement lifecycle.
    advertisement: AcceptedModeAdvertisement,
    /// Whether this row is currently advertised.
    advertised: bool,
    /// Current runtime behavior when already public.
    current: Option<CurrentModeState<'a>>,
}

/// Registry-derived support rows consumed by documentation and release tooling.
#[derive(Serialize)]
struct DocumentationSupportMatrix<'a> {
    /// Honest current-versus-accepted support rows.
    modes: Vec<DocumentationSupportRow<'a>>,
    /// Accepted standard names and dialect mappings.
    language_crosswalk: Vec<AcceptedLanguageState<'a>>,
}

/// Standalone generated documentation and release support artifact.
#[derive(Serialize)]
struct DocumentationSupportDocument<'a> {
    /// Documentation-support schema version.
    schema_version: u32,
    /// Stable documentation-support format.
    format: &'static str,
    /// Composite registry identity.
    registry_id: &'a str,
    /// Exact source-lock digest.
    source_lock_sha256: &'a str,
    /// Semantic composite-registry digest.
    registry_contract_sha256: &'a str,
    /// Accepted capability-set identity.
    accepted_registry_id: &'a str,
    /// Accepted capability-set semantic digest.
    accepted_set_sha256: &'a str,
    /// Whether the complete accepted set is currently supportable as a public claim.
    parity_complete: bool,
    /// Explicitly evaluation-only, unselected parser-pack trust projection.
    parser_pack_trust: ParserPackTrustState<'a>,
    /// Honest current-versus-accepted support rows and standard-name crosswalk.
    support: DocumentationSupportMatrix<'a>,
}

/// Resolved parser-component input for later exact SBOM generation.
#[derive(Serialize)]
struct ParserComponentSbomInput<'a> {
    /// Stable parser-component identity.
    parser_id: &'a str,
    /// Closed compiled parser selection.
    built_in_parser: BuiltInParserId,
    /// Current implementation form.
    implementation: ParserImplementation,
    /// Current feature-pack owner.
    pack_id: &'a str,
    /// Current ABI identity.
    abi_id: &'a str,
    /// Current ABI version.
    abi_version: u32,
    /// Current ABI evidence state.
    abi_state: AbiState,
    /// Optional resolved parser-asset declaration.
    asset: Option<&'a ParserAsset>,
    /// Optional resolved extraction-query declaration.
    query_pack: Option<&'a QueryPack>,
    /// Resolved component fixture declarations.
    fixtures: Vec<&'a RegistryFixture>,
    /// Resolved component provenance evidence.
    provenance_evidence: Vec<&'a RegistryEvidence>,
}

/// Complete registry inputs for later exact component SBOM/provenance records.
#[derive(Serialize)]
struct SbomInputInventory<'a> {
    /// Resolved current parser components.
    parser_components: Vec<ParserComponentSbomInput<'a>>,
    /// Declared external parser assets, including source/version/digest/license/patch data.
    parser_assets: &'a [ParserAsset],
    /// Declared extraction-query packs.
    query_packs: &'a [QueryPack],
}

/// Current runtime inventory retained independently of future packaging.
#[derive(Serialize)]
struct CurrentCapabilityState<'a> {
    /// Ordered detection rules.
    detection_rules: &'a [DetectionRule],
    /// Closed semantic refinements and compatible base modes.
    semantic_modes: &'a [SemanticModeRule],
    /// Ordered current modes.
    modes: &'a [CurrentLanguageMode],
    /// Current compiled parser components.
    parser_components: &'a [ParserComponent],
    /// Current feature-pack boundaries.
    packs: &'a [RegistryPack],
}

/// Accepted target binding and derived inventory counts.
#[derive(Serialize)]
struct AcceptedCapabilityState<'a> {
    /// Accepted target identity.
    registry_id: &'a str,
    /// Existing accepted-set semantic digest.
    accepted_set_sha256: &'a str,
    /// Exact accepted target raw digest.
    raw_sha256: &'a str,
    /// Accepted mode count derived from the target.
    modes: usize,
    /// Accepted normalized parser-capability count.
    normalized_parser_capabilities: usize,
    /// Accepted pre-parse-transform count.
    pre_parse_transforms: usize,
    /// Accepted crosswalk count.
    crosswalk_entries: usize,
    /// All accepted rows remain unadvertised pending evidence.
    advertisement: AcceptedModeAdvertisement,
}

/// Frozen historical contract identity and row counts.
#[derive(Serialize)]
struct HistoricalCapabilityState<'a> {
    /// Historical release identity.
    release: &'a str,
    /// Historical commit identity.
    commit: &'a str,
    /// Exact historical fixture digest.
    raw_sha256: &'a str,
    /// Ordered public pipeline rows.
    language_pipelines: usize,
    /// Ordered post-parser/fallback augmenter rows.
    augmenter_routes: usize,
}

/// Current projection nested under one accepted mode when it already exists.
#[derive(Serialize)]
struct CurrentModeState<'a> {
    /// Current stable mode identity.
    mode_id: &'a str,
    /// Current feature-pack owner.
    pack_id: &'a str,
    /// Current parser support class.
    parser_support: &'a str,
    /// Current summary adapter.
    summary_adapter: &'a str,
    /// Current symbol route.
    symbol_route: &'a str,
}

/// One accepted delivery mode with an optional separate current projection.
#[derive(Serialize)]
struct CapabilityModeState<'a> {
    /// Accepted stable mode identity.
    accepted_mode_id: &'a str,
    /// Public mode spelling.
    public_mode: &'a str,
    /// Accepted normalized parser identity.
    parser_id: &'a str,
    /// Optional accepted transform that runs before the selected parser.
    pre_parse_transform: Option<&'a AcceptedPreParseTransform>,
    /// Future delivery pack; never used as current routing.
    future_pack_id: &'a str,
    /// Accepted owner.
    owner: &'a str,
    /// Whether this row belongs to the accepted delivery target.
    accepted_delivery_target: bool,
    /// Optional accepted alias target.
    alias_of: Option<&'a str>,
    /// Materialized accepted detection-rule identity.
    detection_rule_id: &'a str,
    /// Accepted fixture inventory.
    fixture_ids: Vec<&'a str>,
    /// Required packaged release platforms.
    required_platforms: Vec<&'a str>,
    /// Required tier claims.
    required_claims: Vec<&'a str>,
    /// Achieved tier claims, empty for the pending candidate.
    achieved_claims: Vec<&'a str>,
    /// Whether this target row is advertised.
    advertised: bool,
    /// Current evidence lifecycle.
    evidence_state: AcceptedEvidenceState,
    /// Accepted advertisement lifecycle.
    advertisement: AcceptedModeAdvertisement,
    /// Current runtime projection when the mode already exists.
    current: Option<CurrentModeState<'a>>,
}

/// One accepted normalized parser capability.
#[derive(Serialize)]
struct AcceptedParserState<'a> {
    /// Stable accepted parser identity.
    parser_id: &'a str,
    /// Pending parser implementation class.
    kind: &'a str,
    /// Future delivery pack.
    future_pack_id: &'a str,
    /// Accepted owner.
    owner: &'a str,
    /// Optional external grammar ABI version.
    tree_sitter_abi: Option<&'a str>,
    /// Stable future parser asset identity.
    asset_id: &'a str,
    /// Stable future extraction-query-pack identity.
    query_pack_id: &'a str,
    /// Current parser evidence lifecycle.
    evidence_state: AcceptedParserEvidenceState,
    /// Whether this accepted parser is advertised.
    advertised: bool,
    /// Required packaged release platforms.
    required_platforms: Vec<&'a str>,
    /// Public modes normalized by this parser.
    normalized_modes: Vec<&'a str>,
}

/// One accepted standard-name or dialect crosswalk row.
#[derive(Serialize)]
struct AcceptedLanguageState<'a> {
    /// Stable accepted-name identity.
    accepted_name_id: &'a str,
    /// Standard-name spelling.
    standard_name: &'a str,
    /// Optional explicit dialect.
    dialect: Option<&'a str>,
    /// Selected accepted mode.
    mode_id: &'a str,
    /// Mapping class.
    mapping: &'a str,
}

/// Project one current mode into the shared generated support shape.
fn current_mode_state(current: &CurrentLanguageMode) -> CurrentModeState<'_> {
    CurrentModeState {
        mode_id: current.mode_id.as_str(),
        pack_id: current.current_pack_id.as_str(),
        parser_support: current.parser_support.contract_tag(),
        summary_adapter: current.summary_adapter.contract_tag(),
        symbol_route: current.symbols.contract_tag(),
    }
}

/// Build the deterministic accepted-mode parity rows.
fn accepted_mode_states<'a>(
    lock: &'a LanguageRegistryLock,
    accepted: &'a AcceptedTargetContract,
) -> Vec<CapabilityModeState<'a>> {
    let current_by_accepted = lock
        .current_modes
        .iter()
        .map(|mode| (mode.accepted_mode_id.as_str(), mode))
        .collect::<BTreeMap<_, _>>();
    let mut accepted_modes = accepted.modes.iter().collect::<Vec<_>>();
    accepted_modes.sort_by_key(|mode| mode.mode_id.as_str());
    accepted_modes
        .into_iter()
        .map(|mode| CapabilityModeState {
            accepted_mode_id: mode.mode_id.as_str(),
            public_mode: mode.public_mode.as_str(),
            parser_id: mode.parser_id.as_str(),
            pre_parse_transform: mode.pre_parse_transform.as_ref(),
            future_pack_id: mode.pack_id.as_str(),
            owner: &mode.owner,
            accepted_delivery_target: mode.accepted_delivery_target,
            alias_of: mode.alias_of.as_ref().map(ModeId::as_str),
            detection_rule_id: mode.detection_rule_id.as_str(),
            fixture_ids: mode.fixture_ids.iter().map(String::as_str).collect(),
            required_platforms: mode
                .required_platforms
                .iter()
                .map(PlatformId::as_str)
                .collect(),
            required_claims: mode
                .required_claims
                .iter()
                .map(|claim| claim.contract_tag())
                .collect(),
            achieved_claims: mode
                .achieved_claims
                .iter()
                .map(|claim| claim.contract_tag())
                .collect(),
            advertised: mode.advertisement.is_advertised(),
            evidence_state: mode.evidence_state,
            advertisement: mode.advertisement,
            current: current_by_accepted
                .get(mode.mode_id.as_str())
                .map(|current| current_mode_state(current)),
        })
        .collect()
}

/// Build the deterministic accepted parser-capability rows.
fn accepted_parser_states(accepted: &AcceptedTargetContract) -> Vec<AcceptedParserState<'_>> {
    let mut accepted_parsers = accepted.parsers.iter().collect::<Vec<_>>();
    accepted_parsers.sort_by_key(|parser| parser.parser_id.as_str());
    accepted_parsers
        .into_iter()
        .map(|parser| AcceptedParserState {
            parser_id: parser.parser_id.as_str(),
            kind: parser.kind.contract_tag(),
            future_pack_id: parser.pack_id.as_str(),
            owner: &parser.owner,
            tree_sitter_abi: parser
                .tree_sitter_abi
                .as_ref()
                .map(ParserAbiVersion::as_str),
            asset_id: parser.asset_id.as_str(),
            query_pack_id: parser.query_pack_id.as_str(),
            evidence_state: parser.evidence_state,
            advertised: parser.advertised,
            required_platforms: parser
                .required_platforms
                .iter()
                .map(PlatformId::as_str)
                .collect(),
            normalized_modes: parser
                .normalized_modes
                .iter()
                .map(PublicMode::as_str)
                .collect(),
        })
        .collect()
}

/// Build the deterministic accepted standard-name crosswalk rows.
fn accepted_language_states(accepted: &AcceptedTargetContract) -> Vec<AcceptedLanguageState<'_>> {
    let mut crosswalk = accepted
        .source
        .accepted_language_crosswalk
        .entries
        .iter()
        .collect::<Vec<_>>();
    crosswalk.sort_by_key(|row| row.accepted_name_id.as_str());
    crosswalk
        .into_iter()
        .map(|row| AcceptedLanguageState {
            accepted_name_id: row.accepted_name_id.as_str(),
            standard_name: &row.standard_name,
            dialect: row.dialect.as_deref(),
            mode_id: row.mode_id.as_str(),
            mapping: row.mapping.contract_tag(),
        })
        .collect()
}

/// Return whether the accepted candidate has a complete achieved-evidence contract.
fn accepted_parity_complete(_accepted: &AcceptedTargetContract) -> bool {
    // The accepted-target validator currently admits only the pending-candidate envelope and
    // requires its achieved manifest to be absent. A future schema must add and validate the
    // complete typed achieved-evidence contract before this projection can become true.
    false
}

/// Build fixture expectations without claiming that pending fixtures have run.
fn conformance_inventory<'a>(
    lock: &'a LanguageRegistryLock,
    accepted: &'a AcceptedTargetContract,
) -> ConformanceInventory<'a> {
    let registered = lock
        .fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut accepted_modes = accepted.modes.iter().collect::<Vec<_>>();
    accepted_modes.sort_by_key(|mode| mode.mode_id.as_str());
    let modes = accepted_modes
        .into_iter()
        .map(|mode| {
            let registered_fixture_ids = mode
                .fixture_ids
                .iter()
                .map(String::as_str)
                .filter(|fixture| registered.contains(fixture))
                .collect();
            let missing_fixture_ids = mode
                .fixture_ids
                .iter()
                .map(String::as_str)
                .filter(|fixture| !registered.contains(fixture))
                .collect();
            ModeConformanceState {
                mode_id: mode.mode_id.as_str(),
                public_mode: mode.public_mode.as_str(),
                parser_id: mode.parser_id.as_str(),
                pre_parse_transform: mode.pre_parse_transform.as_ref(),
                required_claims: mode
                    .required_claims
                    .iter()
                    .map(|claim| claim.contract_tag())
                    .collect(),
                achieved_claims: mode
                    .achieved_claims
                    .iter()
                    .map(|claim| claim.contract_tag())
                    .collect(),
                fixture_ids: mode.fixture_ids.iter().map(String::as_str).collect(),
                registered_fixture_ids,
                missing_fixture_ids,
                required_platforms: mode
                    .required_platforms
                    .iter()
                    .map(PlatformId::as_str)
                    .collect(),
                evidence_state: mode.evidence_state,
                advertised: mode.advertisement.is_advertised(),
            }
        })
        .collect();
    ConformanceInventory {
        modes,
        fixtures: &lock.fixtures,
        query_packs: &lock.query_packs,
        embedded_adapters: &lock.embedded_adapters,
        semantic_providers: &lock.semantic_providers,
        evidence: &lock.evidence,
    }
}

/// Build documentation rows from the same accepted/current state as parity output.
fn documentation_support_matrix<'a>(
    lock: &'a LanguageRegistryLock,
    accepted: &'a AcceptedTargetContract,
) -> DocumentationSupportMatrix<'a> {
    let current_by_accepted = lock
        .current_modes
        .iter()
        .map(|mode| (mode.accepted_mode_id.as_str(), mode))
        .collect::<BTreeMap<_, _>>();
    let mut accepted_modes = accepted.modes.iter().collect::<Vec<_>>();
    accepted_modes.sort_by_key(|mode| mode.mode_id.as_str());
    let modes = accepted_modes
        .into_iter()
        .map(|mode| DocumentationSupportRow {
            mode_id: mode.mode_id.as_str(),
            public_mode: mode.public_mode.as_str(),
            parser_id: mode.parser_id.as_str(),
            pre_parse_transform: mode.pre_parse_transform.as_ref(),
            future_pack_id: mode.pack_id.as_str(),
            required_claims: mode
                .required_claims
                .iter()
                .map(|claim| claim.contract_tag())
                .collect(),
            achieved_claims: mode
                .achieved_claims
                .iter()
                .map(|claim| claim.contract_tag())
                .collect(),
            evidence_state: mode.evidence_state,
            advertisement: mode.advertisement,
            advertised: mode.advertisement.is_advertised(),
            current: current_by_accepted
                .get(mode.mode_id.as_str())
                .map(|current| current_mode_state(current)),
        })
        .collect();
    DocumentationSupportMatrix {
        modes,
        language_crosswalk: accepted_language_states(accepted),
    }
}

/// Resolve one current parser component into complete registry-owned SBOM inputs.
fn sbom_input_inventory(
    lock: &LanguageRegistryLock,
) -> Result<SbomInputInventory<'_>, LanguageRegistryError> {
    let parser_components = lock
        .parser_components
        .iter()
        .map(|component| {
            let asset = component
                .asset_id
                .as_ref()
                .map(|asset_id| {
                    lock.assets
                        .iter()
                        .find(|asset| asset.asset_id == *asset_id)
                        .ok_or_else(|| {
                            LanguageRegistryError::Validation(format!(
                                "parser component {} references missing SBOM asset {}",
                                component.parser_id.as_str(),
                                asset_id.as_str()
                            ))
                        })
                })
                .transpose()?;
            let query_pack = component
                .query_pack_id
                .as_ref()
                .map(|query_pack_id| {
                    lock.query_packs
                        .iter()
                        .find(|query_pack| query_pack.id == *query_pack_id)
                        .ok_or_else(|| {
                            LanguageRegistryError::Validation(format!(
                                "parser component {} references missing SBOM query pack {}",
                                component.parser_id.as_str(),
                                query_pack_id.as_str()
                            ))
                        })
                })
                .transpose()?;
            let fixtures = component
                .fixture_ids
                .iter()
                .map(|fixture_id| {
                    lock.fixtures
                        .iter()
                        .find(|fixture| fixture.fixture_id == *fixture_id)
                        .ok_or_else(|| {
                            LanguageRegistryError::Validation(format!(
                                "parser component {} references missing SBOM fixture {}",
                                component.parser_id.as_str(),
                                fixture_id.as_str()
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let provenance_evidence = component
                .provenance_evidence_ids
                .iter()
                .map(|evidence_id| {
                    lock.evidence
                        .iter()
                        .find(|evidence| evidence.evidence_id == *evidence_id)
                        .ok_or_else(|| {
                            LanguageRegistryError::Validation(format!(
                                "parser component {} references missing SBOM evidence {}",
                                component.parser_id.as_str(),
                                evidence_id.as_str()
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ParserComponentSbomInput {
                parser_id: component.parser_id.as_str(),
                built_in_parser: component.built_in_parser,
                implementation: component.implementation,
                pack_id: component.current_pack_id.as_str(),
                abi_id: component.abi.abi_id.as_str(),
                abi_version: component.abi.version,
                abi_state: component.abi.state,
                asset,
                query_pack,
                fixtures,
                provenance_evidence,
            })
        })
        .collect::<Result<Vec<_>, LanguageRegistryError>>()?;
    Ok(SbomInputInventory {
        parser_components,
        parser_assets: &lock.assets,
        query_packs: &lock.query_packs,
    })
}

/// Render the validated capability state as deterministic pretty JSON plus newline.
fn render_capability_state(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<Vec<u8>, LanguageRegistryError> {
    let parity_complete = accepted_parity_complete(accepted);
    let accepted_capability_parity = AcceptedCapabilityParity {
        complete: parity_complete,
        modes: accepted_mode_states(lock, accepted),
        parsers: accepted_parser_states(accepted),
        language_crosswalk: accepted_language_states(accepted),
        capabilities: &accepted.source.capabilities,
        claim_types: &accepted.source.claim_types,
        relation_traceability_contract: &accepted.source.relation_traceability_contract,
    };

    let evidence = CapabilityStateEvidence {
        schema_version: 1,
        format: "projectatlas.language-capability-state",
        registry_id: lock.registry_id.as_str(),
        source_lock_sha256,
        registry_contract_sha256,
        current: CurrentCapabilityState {
            detection_rules: &lock.detection_rules,
            semantic_modes: &lock.semantic_modes,
            modes: &lock.current_modes,
            parser_components: &lock.parser_components,
            packs: &lock.packs,
        },
        accepted_target: AcceptedCapabilityState {
            registry_id: accepted.source.registry_id.as_str(),
            accepted_set_sha256: accepted.source.accepted_set_digest.as_str(),
            raw_sha256: lock.accepted_target.raw_sha256.as_str(),
            modes: accepted.modes.len(),
            normalized_parser_capabilities: accepted.parsers.len(),
            pre_parse_transforms: accepted
                .modes
                .iter()
                .filter(|mode| mode.pre_parse_transform.is_some())
                .count(),
            crosswalk_entries: accepted.source.accepted_language_crosswalk.entries.len(),
            advertisement: accepted.source.mode_defaults.advertisement,
        },
        historical_contract: HistoricalCapabilityState {
            release: historical.baseline_release.as_str(),
            commit: historical.baseline_commit.as_str(),
            raw_sha256: lock.historical_contract.raw_sha256.as_str(),
            language_pipelines: historical.language_pipelines.len(),
            augmenter_routes: historical.augmenter_routes.len(),
        },
        parser_pack_trust: ParserPackTrustState {
            format: ParserPackTrustFormat::ParserPackTrust,
            path: lock.parser_pack_trust.path.as_str(),
            raw_sha256: lock.parser_pack_trust.raw_sha256.as_str(),
            installed_byte_limit: parser_pack_installed_byte_limit,
            selection_state: ParserPackSelectionState::Blocked,
            selected_candidate: None,
            achieved_manifest: None,
            candidates: &parser_pack_trust.candidates,
        },
        accepted_capability_parity,
        settings: LanguageSettingsState {
            registry_id: lock.registry_id.as_str(),
            registry_contract_sha256,
            accepted_registry_id: accepted.source.registry_id.as_str(),
            accepted_set_sha256: accepted.source.accepted_set_digest.as_str(),
            accepted_advertisement: accepted.source.mode_defaults.advertisement,
            capability_tiers: &lock.capability_tiers,
            packs: &lock.packs,
            current_modes: lock.current_modes.len(),
            accepted_modes: accepted.modes.len(),
            accepted_pre_parse_transforms: accepted
                .modes
                .iter()
                .filter(|mode| mode.pre_parse_transform.is_some())
                .count(),
            accepted_parser_capabilities: accepted.parsers.len(),
            parser_components: lock.parser_components.len(),
            parser_assets: lock.assets.len(),
            embedded_adapters: lock.embedded_adapters.len(),
            query_packs: lock.query_packs.len(),
            semantic_providers: lock.semantic_providers.len(),
        },
        conformance_inventory: conformance_inventory(lock, accepted),
        sbom_inputs: sbom_input_inventory(lock)?,
    };
    let mut bytes = serde_json::to_vec_pretty(&evidence).map_err(|source| {
        LanguageRegistryError::Validation(format!(
            "capability-state serialization failed: {source}"
        ))
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Render the standalone documentation and release support matrix.
fn render_documentation_support_matrix(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
) -> Result<Vec<u8>, LanguageRegistryError> {
    let document = DocumentationSupportDocument {
        schema_version: 1,
        format: "projectatlas.language-capabilities",
        registry_id: lock.registry_id.as_str(),
        source_lock_sha256,
        registry_contract_sha256,
        accepted_registry_id: accepted.source.registry_id.as_str(),
        accepted_set_sha256: accepted.source.accepted_set_digest.as_str(),
        parity_complete: accepted_parity_complete(accepted),
        parser_pack_trust: ParserPackTrustState {
            format: ParserPackTrustFormat::ParserPackTrust,
            path: lock.parser_pack_trust.path.as_str(),
            raw_sha256: lock.parser_pack_trust.raw_sha256.as_str(),
            installed_byte_limit: parser_pack_installed_byte_limit,
            selection_state: ParserPackSelectionState::Blocked,
            selected_candidate: None,
            achieved_manifest: None,
            candidates: &parser_pack_trust.candidates,
        },
        support: documentation_support_matrix(lock, accepted),
    };
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(|source| {
        LanguageRegistryError::Validation(format!(
            "documentation support serialization failed: {source}"
        ))
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
#[path = "language_registry_tests.rs"]
mod tests;
