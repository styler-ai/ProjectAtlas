//! Assemble one immutable optional-parser platform artifact from pinned native libraries.

use object::endian::LittleEndian as LE;
use object::read::ReadRef as _;
use object::read::elf::{ElfFile, FileHeader as ElfFileHeader, ProgramHeader as _};
use object::read::macho::{MachHeader, MachOFile};
use object::read::pe::{ImageNtHeaders, Import as PeImport, PeFile};
use object::{Architecture, BinaryFormat, File as NativeObject, Object, ObjectKind, ObjectSymbol};
use projectatlas_core::optional_parser_pack::{
    AcceptedGrammar, GrammarFixture, GrammarFixtureOrigin,
    OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_SCHEMA_VERSION,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES,
    OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY, OptionalParserPackArtifactManifest,
    OptionalParserPackManifest, PackPlatform, PackRelativePath, ParserPackCandidateIdentity,
    ParserPackNativeAudit, ParserPackNetworkDenial, ParserPackNetworkIsolation,
    ParserPackOfflineConstruction, ParserPackPayloadFile, ParserPackPayloadMeasurements,
    ParserPackPayloadRole, ParserPackSourceAsset, ParserPackVerifiedControl, Sha256Digest,
    SourceRevision,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tar::EntryType;

/// Pinned platform-input schema accepted by this release tool.
const SOURCE_INTAKE_SCHEMA_VERSION: u32 = 2;
/// Pinned grammar-source evidence schema accepted by this release tool.
const SOURCE_EVIDENCE_SCHEMA_VERSION: u32 = 2;
/// Retained fixture-corpus schema accepted by this release tool.
const FIXTURE_CORPUS_SCHEMA_VERSION: u32 = 1;
/// Maximum pinned platform-input bytes.
const MAX_SOURCE_INTAKE_BYTES: usize = 1024 * 1024;
/// Maximum pinned grammar source-evidence bytes.
const MAX_SOURCE_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum native-import policy bytes.
const MAX_IMPORT_POLICY_BYTES: usize = 1024 * 1024;
/// Maximum hosted construction-context bytes.
const MAX_ASSEMBLY_CONTEXT_BYTES: usize = 16 * 1024;
/// Maximum acquired upstream parser-inventory bytes.
const MAX_UPSTREAM_PARSER_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Maximum `ProjectAtlas` license bytes.
const MAX_LICENSE_BYTES: usize = 1024 * 1024;
/// Maximum compressed source bundle bytes.
const MAX_SOURCE_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum entries admitted from the pinned source bundle.
const MAX_ARCHIVE_ENTRIES: usize = 512;
/// Maximum expanded bytes for one native library.
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum sum of source-bundle entry payload bytes.
const MAX_ARCHIVE_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum selected grammar-library bytes.
const MAX_SELECTED_LIBRARY_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum parser-worker bytes.
const MAX_WORKER_BYTES: usize = 256 * 1024 * 1024;
/// Maximum runtime-containment broker bytes.
const MAX_CONTAINMENT_BROKER_BYTES: usize = 16 * 1024 * 1024;
/// Maximum bytes accepted from either containment-broker contract stream.
const MAX_CONTAINMENT_BROKER_CONTRACT_OUTPUT_BYTES: usize = 4 * 1024;
/// Maximum managed native-entry imports accepted from the containment broker.
const MAX_CONTAINMENT_BROKER_MANAGED_IMPORTS: usize = 1_024;
/// Maximum imported symbols audited for one library.
const MAX_IMPORTS_PER_LIBRARY: usize = 65_536;
/// Maximum exports audited for one library.
const MAX_EXPORTS_PER_LIBRARY: usize = 256;
/// Maximum native dependencies audited for one library.
const MAX_NATIVE_LIBRARIES_PER_LIBRARY: usize = 64;
/// Maximum exported symbols retained for the parser-worker audit.
const MAX_EXPORTS_PER_WORKER: usize = 1_024;
/// Maximum named definitions retained for the parser-worker audit.
const MAX_DEFINED_SYMBOLS_PER_WORKER: usize = 262_144;
/// Maximum UTF-8 bytes retained for one audited native name.
const MAX_NATIVE_AUDIT_NAME_BYTES: usize = 4 * 1024;
/// Hard wall-clock limit for the containment broker's build-contract probe.
const CONTAINMENT_BROKER_BUILD_CONTRACT_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll interval while waiting for the bounded containment-broker probe.
const CONTAINMENT_BROKER_BUILD_CONTRACT_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Canonical accepted logical-manifest filename.
const ACCEPTED_MANIFEST_FILE_NAME: &str = "accepted-capabilities.json";
/// Canonical retained fixture-corpus filename.
const FIXTURE_CORPUS_FILE_NAME: &str = "optional-parser-pack-corpus.json";
/// Canonical `ProjectAtlas` license filename.
const PROJECT_LICENSE_FILE_NAME: &str = "LICENSE";
/// Canonical native-import policy filename.
const IMPORT_POLICY_FILE_NAME: &str = "native-import-policy.json";
/// Canonical normalized native-audit report filename.
const NATIVE_AUDIT_REPORT_FILE_NAME: &str = "native-audit-report.json";
/// Canonical immutable platform-manifest filename.
const ARTIFACT_MANIFEST_FILE_NAME: &str = "artifact-manifest.json";
/// Canonical native-library directory.
const LIB_DIRECTORY_NAME: &str = "lib";
/// Broker mode that reports its compiled runtime and managed native-entry contract.
const CONTAINMENT_BROKER_BUILD_CONTRACT_ARGUMENT: &str = "--build-contract";
/// Fixed prefix for the containment broker's compiled build contract.
const CONTAINMENT_BROKER_BUILD_CONTRACT_PREFIX: &str =
    "projectatlas-parser-containment-build-contract-v1";
/// Required architecture vocabulary in the containment broker's build contract.
const CONTAINMENT_BROKER_BUILD_CONTRACT_ARCHITECTURE: &str = "x86_64";
/// Positional sentinel for a platform with no runtime-containment broker.
const NO_CONTAINMENT_BROKER_ARGUMENT: &str = "-";
/// Prefix reserved for Tree-sitter grammar constructor symbols.
const TREE_SITTER_SYMBOL_PREFIX: &str = "tree_sitter_";
/// Complete allowed generated external-scanner export suffix set.
const EXTERNAL_SCANNER_EXPORT_SUFFIXES: &[&str] = &[
    "create",
    "destroy",
    "scan",
    "serialize",
    "deserialize",
    "reset",
];

/// Outer release-tool result boundary.
type ToolResult<T> = Result<T, Box<dyn Error>>;

/// Pinned upstream release and per-platform native-asset authority.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformBundleIntake {
    /// Platform-input schema version.
    schema_version: u32,
    /// Cargo package name.
    source_package: String,
    /// Cargo package version.
    source_version: String,
    /// Published Cargo archive identity.
    cargo_archive: CargoArchivePin,
    /// Native release identity.
    native_release: NativeReleasePin,
    /// Upstream parser inventory asset.
    upstream_release_manifest: SourceAssetPin,
    /// Complete required native bundle set.
    platforms: Vec<PlatformBundlePin>,
}

/// Published Cargo archive identity from the pinned intake.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoArchivePin {
    /// Embedded VCS revision.
    vcs_revision: SourceRevision,
    /// Embedded monorepo crate path.
    path_in_vcs: String,
    /// Exact Cargo archive digest.
    sha256: Sha256Digest,
}

/// Native release identity from the pinned intake.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeReleasePin {
    /// Release tag.
    tag: String,
    /// Release commit.
    revision: SourceRevision,
    /// Parser-source bundle digest.
    source_bundle_sha256: Sha256Digest,
}

/// Strict pinned grammar-source evidence retained beside the release inputs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEvidenceWire {
    /// Source-evidence schema version.
    schema_version: u32,
    /// Cargo package name.
    source_package: String,
    /// Cargo package version.
    source_version: String,
    /// Published Cargo archive identity.
    cargo_archive: CargoArchivePin,
    /// Native release identity.
    native_release: NativeReleasePin,
    /// Strictly language-sorted grammar evidence rows.
    rows: Vec<SourceEvidenceRowWire>,
}

/// One pinned grammar source, license, ABI, and fixture evidence row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEvidenceRowWire {
    /// Canonical accepted language identity.
    language_id: String,
    /// Exact grammar repository and compile-input evidence.
    source: GrammarSourceEvidenceWire,
    /// Upstream package's declared license label.
    license_label: String,
    /// Exact applicable license files.
    licenses: Vec<GrammarLicenseEvidenceWire>,
    /// Grammar ABI reported by the pinned source package.
    abi: u32,
    /// Exact exported grammar constructor symbol.
    export_symbol: String,
    /// Platform-neutral dynamic-library stem.
    library_stem: String,
    /// Positive and negative fixture evidence.
    fixtures: FixtureCorpusPairWire,
}

/// Exact repository and deterministic compile-input evidence for one grammar.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrammarSourceEvidenceWire {
    /// HTTPS source repository.
    repository: String,
    /// Full pinned repository revision.
    revision: SourceRevision,
    /// Optional repository-relative grammar subtree.
    subdirectory: Option<String>,
    /// Closed deterministic compile-input digest algorithm.
    compile_input_digest_algorithm: String,
    /// Exact deterministic compile-input digest.
    compile_input_digest: Sha256Digest,
    /// Non-zero number of admitted compile inputs.
    compile_files: u64,
}

/// Exact upstream license-file evidence for one grammar.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrammarLicenseEvidenceWire {
    /// Repository-relative license path.
    source_path: String,
    /// Exact upstream Git blob identity.
    source_blob: SourceRevision,
    /// Exact UTF-8 byte length.
    byte_length: u64,
    /// SHA-256 of the exact UTF-8 license text.
    sha256: Sha256Digest,
    /// Exact applicable license text.
    text: String,
}

/// Exact upstream auxiliary asset pin.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAssetPin {
    /// HTTPS release URL.
    url: String,
    /// Exact asset digest.
    sha256: Sha256Digest,
    /// Exact asset bytes.
    byte_length: u64,
}

/// Exact upstream native bundle pin for one target.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformBundlePin {
    /// `ProjectAtlas` target triple.
    platform: PackPlatform,
    /// Upstream platform identity.
    upstream_platform: String,
    /// HTTPS release URL.
    url: String,
    /// Exact bundle digest.
    sha256: Sha256Digest,
    /// Exact compressed bundle bytes.
    byte_length: u64,
}

/// Strict retained fixture corpus packaged beside the accepted manifest.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCorpusWire {
    /// Fixture-corpus schema version.
    schema_version: u32,
    /// Exact pinned source-evidence document digest.
    source_manifest_sha256: Sha256Digest,
    /// Strictly language-sorted accepted fixture rows.
    rows: Vec<FixtureCorpusRowWire>,
}

/// One accepted language's retained fixture evidence.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCorpusRowWire {
    /// Canonical accepted language identity.
    language_id: String,
    /// Positive and negative fixture evidence.
    fixtures: FixtureCorpusPairWire,
}

/// Positive and negative corpus fixtures for one accepted grammar.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCorpusPairWire {
    /// Natural positive fixture expected to parse without an error.
    positive: PositiveCorpusFixtureWire,
    /// Non-vacuous negative fixture expected to contain a parser error.
    negative: NegativeCorpusFixtureWire,
}

/// Natural positive fixture and optional exact upstream tree evidence.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PositiveCorpusFixtureWire {
    /// Evidence origin.
    origin: GrammarFixtureOrigin,
    /// Exact upstream repository-relative source path.
    source_path: String,
    /// Exact upstream corpus case name.
    case_name: String,
    /// Exact source text.
    source: String,
    /// SHA-256 of the exact source text.
    source_sha256: Sha256Digest,
    /// Optional exact expected Tree-sitter S-expression.
    expected_tree: Option<String>,
    /// SHA-256 of `expected_tree` when it is present.
    expected_tree_sha256: Option<Sha256Digest>,
}

/// Non-vacuous negative fixture with its required error outcome.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NegativeCorpusFixtureWire {
    /// Evidence origin.
    origin: GrammarFixtureOrigin,
    /// Exact upstream repository-relative source path.
    source_path: String,
    /// Exact upstream corpus case name.
    case_name: String,
    /// Exact source text.
    source: String,
    /// SHA-256 of the exact source text.
    source_sha256: Sha256Digest,
    /// Required parser-error outcome.
    expected_error: bool,
}

/// Closed import/dependency policy parsed at the release boundary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeImportPolicy {
    /// Policy schema version.
    schema_version: u32,
    /// Exact forbidden normalized symbols.
    forbidden_import_symbols: Vec<String>,
    /// Forbidden normalized symbol prefixes.
    forbidden_import_symbol_prefixes: Vec<String>,
    /// Exact forbidden normalized symbols for the shipped worker.
    worker_forbidden_import_symbols: Vec<String>,
    /// Forbidden normalized worker symbol prefixes.
    worker_forbidden_import_symbol_prefixes: Vec<String>,
    /// Complete required platform allowlists.
    platforms: Vec<PlatformImportPolicy>,
}

/// Closed native dependency allowlist for one target.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformImportPolicy {
    /// `ProjectAtlas` target triple.
    platform: PackPlatform,
    /// Complete permitted direct native dependencies.
    allowed_libraries: Vec<String>,
    /// System runtime libraries that the trusted worker must map before containment.
    worker_preloaded_libraries: Vec<String>,
    /// Complete permitted PE-loader imports of the platform containment broker.
    containment_broker_pe_loader_libraries: Vec<String>,
    /// Whether the platform broker must carry a valid CLR runtime header.
    containment_broker_clr_runtime_header_required: bool,
    /// Complete permitted managed P/Invoke modules of the platform containment broker.
    containment_broker_managed_modules: Vec<String>,
}

/// Exact file facts retained in the normalized native audit report.
#[derive(Debug, Serialize)]
struct AuditedFile {
    /// Artifact-relative path.
    path: String,
    /// Exact file digest.
    sha256: String,
    /// Exact file bytes.
    byte_length: u64,
}

/// Strict versioned native-audit report emitted with one platform artifact.
#[derive(Debug, Serialize)]
struct NativeAuditReport<'a> {
    /// Audit schema understood by the release verifier.
    schema_version: u32,
    /// Exact parser-worker file identity and native facts.
    worker: WorkerArtifact,
    /// Platform admission broker facts, absent only when the target needs no broker.
    containment_broker: Option<&'a ContainmentBrokerArtifact>,
    /// Accepted grammar-library audit rows in logical manifest order.
    grammars: &'a [GrammarArtifact],
}

/// Normalized native audit row for the artifact-bound containment broker.
#[derive(Debug, Serialize)]
struct ContainmentBrokerArtifact {
    /// Exact packaged broker file facts.
    file: AuditedFile,
    /// External runtime family required to start the broker.
    runtime_family: String,
    /// Observed native binary format.
    binary_format: String,
    /// Observed native architecture.
    architecture: String,
    /// Observed native object kind.
    object_kind: String,
    /// Exact native PE entry point, which is zero for the managed broker.
    entry_point: String,
    /// RVA of the validated CLR runtime header.
    clr_runtime_header_rva: u32,
    /// Exact byte length of the validated CLR runtime header.
    clr_runtime_header_size: u32,
    /// Complete sorted PE-loader dependencies.
    pe_loader_libraries: Vec<String>,
    /// Distinct normalized PE imported-symbol count.
    pe_imported_symbol_count: usize,
    /// Digest of sorted normalized PE imported symbols.
    pe_imported_symbols_sha256: String,
    /// Complete sorted managed P/Invoke module set.
    managed_modules: Vec<String>,
    /// Number of compiled managed P/Invoke method imports.
    managed_import_count: usize,
    /// Digest of sorted normalized managed P/Invoke method imports.
    managed_imports_sha256: String,
    /// Sorted normalized native-export count.
    export_count: usize,
    /// Digest of sorted normalized native exports.
    exports_sha256: String,
}

/// Normalized native audit row for the parser worker.
#[derive(Debug, Serialize)]
struct WorkerArtifact {
    /// Exact packaged worker file facts.
    file: AuditedFile,
    /// Observed native binary format.
    binary_format: String,
    /// Observed native architecture.
    architecture: String,
    /// Observed native object kind.
    object_kind: String,
    /// Exact native entry point rendered without JSON integer precision loss.
    entry_point: String,
    /// Observed direct native dependencies.
    native_libraries: Vec<String>,
    /// Number of distinct normalized imported symbols.
    imported_symbol_count: usize,
    /// Digest of the normalized imported-symbol set.
    imported_symbols_sha256: String,
    /// Number of native exports, including duplicate names if present.
    export_count: usize,
    /// Digest of the sorted normalized native-export sequence.
    exports_sha256: String,
    /// Whether a named definition table was available to the object reader.
    defined_symbol_evidence_available: bool,
    /// Exact named-definition count when evidence was available.
    defined_symbol_count: Option<usize>,
    /// Digest of the sorted normalized definition sequence when available.
    defined_symbols_sha256: Option<String>,
}

/// Normalized native audit row for one accepted grammar.
#[derive(Debug, Serialize)]
struct GrammarArtifact {
    /// Accepted logical language identity.
    language_id: String,
    /// Exact packaged library facts.
    file: AuditedFile,
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

/// Hosted construction context supplied by the isolated release job.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssemblyContextWire {
    /// Exact source/toolchain candidate identity.
    candidate: ParserPackCandidateIdentity,
    /// Workflow-observed offline and physical network-denial state.
    construction: OfflineConstructionWire,
}

/// Primitive workflow observations converted into typed verified controls.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfflineConstructionWire {
    /// Cargo ran in frozen mode against the exact lockfile.
    cargo_frozen: ObservedControl,
    /// Cargo ran in offline mode after the bounded acquisition stage.
    cargo_offline: ObservedControl,
    /// The grammar dependency's own offline mode was forced.
    dependency_offline: ObservedControl,
    /// The dependency's broad language-selection variable was absent.
    language_selector_absent: ObservedControl,
    /// The dependency's failed-grammar override was absent.
    failed_grammar_override_absent: ObservedControl,
    /// Physical egress denial and canary observations.
    network_denial: NetworkDenialWire,
}

/// Primitive three-path egress-canary observations from the release job.
#[derive(Debug, Deserialize)]
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

/// One primitive workflow observation that must be true before artifact emission.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct ObservedControl(bool);

impl ObservedControl {
    /// Convert a successful observation into the closed manifest success state.
    fn into_verified(self, field: &str) -> ToolResult<ParserPackVerifiedControl> {
        if !self.0 {
            return Err(invalid(format!(
                "assembly context field `{field}` was not verified"
            )));
        }
        Ok(ParserPackVerifiedControl::Verified)
    }
}

impl AssemblyContextWire {
    /// Convert workflow observations into the artifact's typed verified controls.
    fn into_artifact_context(
        self,
    ) -> ToolResult<(ParserPackCandidateIdentity, ParserPackOfflineConstruction)> {
        let construction = self.construction;
        Ok((
            self.candidate,
            ParserPackOfflineConstruction {
                cargo_frozen: construction
                    .cargo_frozen
                    .into_verified("construction.cargo_frozen")?,
                cargo_offline: construction
                    .cargo_offline
                    .into_verified("construction.cargo_offline")?,
                dependency_offline: construction
                    .dependency_offline
                    .into_verified("construction.dependency_offline")?,
                zero_embedded_grammars: ParserPackVerifiedControl::Verified,
                language_selector_absent: construction
                    .language_selector_absent
                    .into_verified("construction.language_selector_absent")?,
                failed_grammar_override_absent: construction
                    .failed_grammar_override_absent
                    .into_verified("construction.failed_grammar_override_absent")?,
                network_denial: ParserPackNetworkDenial {
                    mechanism: construction.network_denial.mechanism,
                    dns_denied: construction.network_denial.dns_denied,
                    direct_tcp_denied: construction.network_denial.direct_tcp_denied,
                    https_denied: construction.network_denial.https_denied,
                },
            },
        ))
    }
}

/// Closed command inputs for one platform artifact.
struct Inputs {
    /// Accepted logical manifest path.
    accepted_manifest: PathBuf,
    /// Retained fixture corpus path.
    fixture_corpus: PathBuf,
    /// Pinned source evidence that owns the corpus provenance digest.
    source_evidence: PathBuf,
    /// `ProjectAtlas` license path.
    project_license: PathBuf,
    /// Pinned platform-bundle intake path.
    bundle_intake: PathBuf,
    /// Closed native-import policy path.
    import_policy: PathBuf,
    /// Hosted assembly-context path.
    assembly_context: PathBuf,
    /// Verified upstream native bundle path.
    source_archive: PathBuf,
    /// Verified acquired upstream parser-inventory path.
    upstream_parser_manifest: PathBuf,
    /// Target-native parser worker path.
    worker: PathBuf,
    /// Target-native runtime-containment broker, present only on Windows.
    containment_broker: Option<PathBuf>,
    /// Target platform.
    platform: PackPlatform,
    /// New artifact output directory.
    output: PathBuf,
}

/// Validated value retained together with its exact bytes and digest.
struct VerifiedInput<T> {
    /// Validated typed value.
    value: T,
    /// Exact source bytes.
    bytes: Vec<u8>,
    /// SHA-256 of the exact source bytes.
    sha256: String,
}

/// Normalized audit result for one native library.
struct NativeInspection {
    /// Native binary format.
    binary_format: String,
    /// Native architecture.
    architecture: String,
    /// Direct native dependencies.
    native_libraries: Vec<String>,
    /// Distinct normalized imported-symbol count.
    imported_symbol_count: usize,
    /// Digest of normalized imported symbols.
    imported_symbols_sha256: String,
}

/// Bounded native facts retained for the exact parser worker.
struct WorkerInspection {
    /// Native binary format.
    binary_format: String,
    /// Native architecture.
    architecture: String,
    /// Native object kind.
    object_kind: String,
    /// Exact native entry point.
    entry_point: String,
    /// Direct native dependencies.
    native_libraries: Vec<String>,
    /// Distinct normalized imported-symbol count.
    imported_symbol_count: usize,
    /// Digest of normalized imported symbols.
    imported_symbols_sha256: String,
    /// Sorted normalized native-export count.
    export_count: usize,
    /// Digest of sorted normalized native exports.
    exports_sha256: String,
    /// Whether named definition evidence was present.
    defined_symbol_evidence_available: bool,
    /// Named definition count when evidence was present.
    defined_symbol_count: Option<usize>,
    /// Digest of sorted normalized definitions when evidence was present.
    defined_symbols_sha256: Option<String>,
}

/// Bounded native facts retained for the exact runtime-containment broker.
struct ContainmentBrokerInspection {
    /// Native binary format.
    binary_format: String,
    /// Native architecture.
    architecture: String,
    /// Native object kind.
    object_kind: String,
    /// Exact native PE entry point, which is zero for the managed broker.
    entry_point: String,
    /// RVA of the validated CLR runtime header.
    clr_runtime_header_rva: u32,
    /// Exact byte length of the validated CLR runtime header.
    clr_runtime_header_size: u32,
    /// Direct PE-loader dependencies.
    pe_loader_libraries: Vec<String>,
    /// Distinct normalized PE imported-symbol count.
    pe_imported_symbol_count: usize,
    /// Digest of normalized PE imported symbols.
    pe_imported_symbols_sha256: String,
    /// Sorted normalized native-export count.
    export_count: usize,
    /// Digest of sorted normalized native exports.
    exports_sha256: String,
}

/// Exact bytes and normalized audit row for one platform containment broker.
struct AssembledContainmentBroker {
    /// Exact broker bytes written into the immutable artifact.
    bytes: Vec<u8>,
    /// Broker audit row bound to those bytes.
    audit: ContainmentBrokerArtifact,
}

/// Compiled managed-runtime facts reported by the exact containment broker.
struct VerifiedContainmentBrokerBuildContract {
    /// External runtime family observed through the broker contract.
    runtime_family: String,
    /// Complete sorted managed P/Invoke module set.
    managed_modules: Vec<String>,
    /// Number of compiled managed P/Invoke method imports.
    managed_import_count: usize,
    /// Digest of sorted normalized managed P/Invoke method imports.
    managed_imports_sha256: String,
}

/// Bounded bytes drained from one child-process stream.
struct BoundedProcessOutput {
    /// Prefix retained up to the configured ceiling.
    bytes: Vec<u8>,
    /// Whether the child emitted more than the accepted ceiling.
    exceeded: bool,
}

/// Accepted grammar audit rows produced while assembling.
struct AssembledLibraries {
    /// Strictly language-sorted audit rows.
    grammars: Vec<GrammarArtifact>,
}

/// Safely classified pinned source-bundle member.
enum BundleMember {
    /// The one exact `./` bundle root directory.
    RootDirectory,
    /// One flat platform-native grammar library.
    NativeLibrary(String),
}

fn main() -> ToolResult<()> {
    let inputs = parse_inputs()?;
    assemble(&inputs)
}

/// Parse the exact positional release-tool input contract.
fn parse_inputs() -> ToolResult<Inputs> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 13 {
        return Err(invalid(
            "usage: assemble_optional_parser_artifact <accepted-manifest> <fixture-corpus> \
             <source-evidence> <project-license> <bundle-intake> <import-policy> <assembly-context> \
             <source-bundle.tar.zst> <upstream-parsers.json> <worker> <containment-broker-or-dash> \
             <target-triple> \
             <output-directory>",
        ));
    }
    let platform = parse_platform(&arguments[11])?;
    Ok(Inputs {
        accepted_manifest: PathBuf::from(&arguments[0]),
        fixture_corpus: PathBuf::from(&arguments[1]),
        source_evidence: PathBuf::from(&arguments[2]),
        project_license: PathBuf::from(&arguments[3]),
        bundle_intake: PathBuf::from(&arguments[4]),
        import_policy: PathBuf::from(&arguments[5]),
        assembly_context: PathBuf::from(&arguments[6]),
        source_archive: PathBuf::from(&arguments[7]),
        upstream_parser_manifest: PathBuf::from(&arguments[8]),
        worker: PathBuf::from(&arguments[9]),
        containment_broker: parse_containment_broker_input(&arguments[10], platform)?,
        platform,
        output: PathBuf::from(&arguments[12]),
    })
}

/// Resolve one supported target triple.
fn parse_platform(value: &OsString) -> ToolResult<PackPlatform> {
    let value = value
        .to_str()
        .ok_or_else(|| invalid("target triple must be valid UTF-8"))?;
    PackPlatform::ALL
        .iter()
        .copied()
        .find(|platform| platform.as_str() == value)
        .ok_or_else(|| invalid(format!("unsupported optional-parser target {value:?}")))
}

/// Resolve the platform-specific broker argument without accepting an ambiguous omission.
fn parse_containment_broker_input(
    value: &OsString,
    platform: PackPlatform,
) -> ToolResult<Option<PathBuf>> {
    let absent = value == OsStr::new(NO_CONTAINMENT_BROKER_ARGUMENT);
    match (platform.containment_broker_file_name(), absent) {
        (None, true) => Ok(None),
        (None, false) => Err(invalid(
            "Linux optional-parser artifacts must use '-' for the absent containment broker",
        )),
        (Some(_), true) => Err(invalid(
            "Windows optional-parser artifacts require an explicit containment broker",
        )),
        (Some(_), false) => Ok(Some(PathBuf::from(value))),
    }
}

/// Resolve the exact worker through an explicitly absolute regular-file path.
fn canonical_worker_path(path: &Path) -> ToolResult<PathBuf> {
    canonical_artifact_executable_path(path, "parser worker")
}

/// Resolve one artifact executable through an explicitly absolute regular-file path.
fn canonical_artifact_executable_path(path: &Path, role: &str) -> ToolResult<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid(format!(
            "{role} path must be absolute: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|source| {
        invalid(format!(
            "cannot canonicalize {role} {}: {source}",
            path.display()
        ))
    })?;
    if !canonical.is_absolute() || !fs::metadata(&canonical)?.is_file() {
        return Err(invalid(format!(
            "{role} must resolve to an absolute regular file: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

/// Read and audit the exact platform broker before any artifact bytes are staged.
fn assemble_containment_broker(
    path: Option<&Path>,
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
) -> ToolResult<Option<AssembledContainmentBroker>> {
    let Some(path) = path else {
        if platform.containment_broker_file_name().is_some() {
            return Err(invalid(
                "platform requires a runtime-containment broker but none was supplied",
            ));
        }
        return Ok(None);
    };
    if platform.containment_broker_file_name().is_none() {
        return Err(invalid(
            "platform does not admit a runtime-containment broker",
        ));
    }
    let canonical = canonical_artifact_executable_path(path, "runtime-containment broker")?;
    let bytes = read_bounded(&canonical, MAX_CONTAINMENT_BROKER_BYTES)?;
    let sha256 = sha256_bytes(&bytes);
    let inspection = inspect_containment_broker(&bytes, platform, platform_policy)?;
    let build_contract =
        verify_containment_broker_build_contract(&canonical, &bytes, platform_policy)?;
    let file_name = platform
        .containment_broker_file_name()
        .ok_or_else(|| invalid("platform containment-broker filename is absent"))?;
    Ok(Some(AssembledContainmentBroker {
        audit: ContainmentBrokerArtifact {
            file: AuditedFile {
                path: file_name.to_owned(),
                sha256,
                byte_length: u64::try_from(bytes.len())?,
            },
            runtime_family: build_contract.runtime_family,
            binary_format: inspection.binary_format,
            architecture: inspection.architecture,
            object_kind: inspection.object_kind,
            entry_point: inspection.entry_point,
            clr_runtime_header_rva: inspection.clr_runtime_header_rva,
            clr_runtime_header_size: inspection.clr_runtime_header_size,
            pe_loader_libraries: inspection.pe_loader_libraries,
            pe_imported_symbol_count: inspection.pe_imported_symbol_count,
            pe_imported_symbols_sha256: inspection.pe_imported_symbols_sha256,
            managed_modules: build_contract.managed_modules,
            managed_import_count: build_contract.managed_import_count,
            managed_imports_sha256: build_contract.managed_imports_sha256,
            export_count: inspection.export_count,
            exports_sha256: inspection.exports_sha256,
        },
        bytes,
    }))
}

/// Execute and validate the exact broker's compiled managed-runtime contract.
fn verify_containment_broker_build_contract(
    broker: &Path,
    expected_bytes: &[u8],
    platform_policy: &PlatformImportPolicy,
) -> ToolResult<VerifiedContainmentBrokerBuildContract> {
    if !broker.is_absolute() {
        return Err(invalid(
            "containment-broker build-contract path is not absolute",
        ));
    }
    let working_directory = tempfile::Builder::new()
        .prefix("projectatlas-containment-contract-")
        .tempdir()?;
    if broker
        .parent()
        .is_some_and(|parent| working_directory.path().starts_with(parent))
    {
        return Err(invalid(
            "containment-broker build-contract working directory is not external to the broker",
        ));
    }
    let mut child = Command::new(broker)
        .arg(CONTAINMENT_BROKER_BUILD_CONTRACT_ARGUMENT)
        .env_clear()
        .current_dir(working_directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            invalid(format!(
                "cannot start containment-broker build-contract probe {}: {source}",
                broker.display()
            ))
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("containment-broker contract stdout pipe is absent"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| invalid("containment-broker contract stderr pipe is absent"))?;
    let stdout_reader = thread::spawn(move || {
        drain_bounded_process_output(stdout, MAX_CONTAINMENT_BROKER_CONTRACT_OUTPUT_BYTES)
    });
    let stderr_reader = thread::spawn(move || {
        drain_bounded_process_output(stderr, MAX_CONTAINMENT_BROKER_CONTRACT_OUTPUT_BYTES)
    });
    let wait_result = wait_for_containment_broker_build_contract(&mut child);
    let stdout = join_bounded_process_output(stdout_reader, "stdout")?;
    let stderr = join_bounded_process_output(stderr_reader, "stderr")?;
    wait_result?;
    if stdout.exceeded || stderr.exceeded {
        return Err(invalid(
            "containment-broker build-contract output exceeded its bound",
        ));
    }
    if !stderr.bytes.is_empty() {
        return Err(invalid(
            "containment-broker build-contract wrote unexpected stderr",
        ));
    }
    let contract = parse_containment_broker_build_contract(&stdout.bytes, platform_policy)?;
    let observed_bytes = read_bounded(broker, MAX_CONTAINMENT_BROKER_BYTES)?;
    if observed_bytes != expected_bytes {
        return Err(invalid(
            "containment broker changed while its build contract was being verified",
        ));
    }
    Ok(contract)
}

/// Drain a process stream without letting excessive output block child termination.
fn drain_bounded_process_output(
    mut reader: impl io::Read,
    maximum: usize,
) -> io::Result<BoundedProcessOutput> {
    let mut retained = Vec::with_capacity(maximum.min(4 * 1024));
    let mut exceeded = false;
    let mut buffer = [0_u8; 4 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = maximum.saturating_sub(retained.len());
        let accepted = remaining.min(read);
        retained.extend_from_slice(&buffer[..accepted]);
        exceeded |= accepted != read;
    }
    Ok(BoundedProcessOutput {
        bytes: retained,
        exceeded,
    })
}

/// Join one bounded process-stream reader without accepting a panic as evidence.
fn join_bounded_process_output(
    reader: thread::JoinHandle<io::Result<BoundedProcessOutput>>,
    stream: &str,
) -> ToolResult<BoundedProcessOutput> {
    reader
        .join()
        .map_err(|_panic| invalid(format!("containment-broker {stream} reader panicked")))?
        .map_err(|source| invalid(format!("cannot read containment-broker {stream}: {source}")))
}

/// Wait for the broker contract probe, terminating and reaping it at the hard deadline.
fn wait_for_containment_broker_build_contract(child: &mut Child) -> ToolResult<()> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(invalid(format!(
                    "containment-broker build-contract probe failed with {status}"
                )));
            }
            Ok(None) if started.elapsed() >= CONTAINMENT_BROKER_BUILD_CONTRACT_TIMEOUT => {
                return terminate_and_reap_containment_broker(
                    child,
                    "containment-broker build-contract probe exceeded its hard timeout",
                );
            }
            Ok(None) => {
                let remaining =
                    CONTAINMENT_BROKER_BUILD_CONTRACT_TIMEOUT.saturating_sub(started.elapsed());
                thread::sleep(remaining.min(CONTAINMENT_BROKER_BUILD_CONTRACT_POLL_INTERVAL));
            }
            Err(source) => {
                return terminate_and_reap_containment_broker(
                    child,
                    &format!("cannot poll containment-broker build-contract probe: {source}"),
                );
            }
        }
    }
}

/// Parse the one fixed ASCII broker contract and bind it to the closed policy.
fn parse_containment_broker_build_contract(
    bytes: &[u8],
    platform_policy: &PlatformImportPolicy,
) -> ToolResult<VerifiedContainmentBrokerBuildContract> {
    if bytes.is_empty()
        || bytes.len() > MAX_CONTAINMENT_BROKER_CONTRACT_OUTPUT_BYTES
        || !bytes.is_ascii()
    {
        return Err(invalid(
            "containment-broker build contract is not bounded non-empty ASCII",
        ));
    }
    let text = std::str::from_utf8(bytes)?;
    let line = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .ok_or_else(|| invalid("containment-broker build contract is not one complete line"))?;
    if line.contains(['\r', '\n']) {
        return Err(invalid(
            "containment-broker build contract contains extra lines",
        ));
    }
    let fields = line.split('|').collect::<Vec<_>>();
    if fields.len() != 6 || fields[0] != CONTAINMENT_BROKER_BUILD_CONTRACT_PREFIX {
        return Err(invalid(
            "containment-broker build contract has an unsupported shape or version",
        ));
    }
    let runtime_family = fields[1]
        .strip_prefix("runtime=")
        .ok_or_else(|| invalid("containment-broker build contract runtime field is absent"))?;
    let architecture = fields[2]
        .strip_prefix("architecture=")
        .ok_or_else(|| invalid("containment-broker build contract architecture field is absent"))?;
    let managed_modules = fields[3]
        .strip_prefix("modules=")
        .ok_or_else(|| invalid("containment-broker build contract modules field is absent"))?
        .split(',')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let managed_import_count = fields[4]
        .strip_prefix("methods=")
        .ok_or_else(|| invalid("containment-broker build contract method count is absent"))?
        .parse::<usize>()?;
    let managed_imports_sha256 = fields[5]
        .strip_prefix("imports_sha256=")
        .ok_or_else(|| invalid("containment-broker build contract digest is absent"))?;
    validate_sorted_unique_normalized(&managed_modules, "containment-broker managed modules")?;
    let expected_modules = platform_policy
        .containment_broker_managed_modules
        .iter()
        .map(|module| normalize_library(module, PackPlatform::WindowsX86_64))
        .collect::<Vec<_>>();
    if runtime_family != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY
        || architecture != CONTAINMENT_BROKER_BUILD_CONTRACT_ARCHITECTURE
        || managed_modules != expected_modules
        || managed_import_count == 0
        || managed_import_count > MAX_CONTAINMENT_BROKER_MANAGED_IMPORTS
    {
        return Err(invalid(
            "containment-broker build contract does not match the closed runtime policy",
        ));
    }
    Sha256Digest::new(managed_imports_sha256)?;
    Ok(VerifiedContainmentBrokerBuildContract {
        runtime_family: runtime_family.to_owned(),
        managed_modules,
        managed_import_count,
        managed_imports_sha256: managed_imports_sha256.to_owned(),
    })
}

/// Terminate and synchronously reap a containment broker whose probe cannot continue.
fn terminate_and_reap_containment_broker(child: &mut Child, reason: &str) -> ToolResult<()> {
    let termination = match child.kill() {
        Ok(()) => "terminated".to_owned(),
        Err(source) => format!("termination failed: {source}"),
    };
    let reap = match child.wait() {
        Ok(status) => format!("reaped with {status}"),
        Err(source) => format!("reap failed: {source}"),
    };
    Err(invalid(format!("{reason}; {termination}; {reap}")))
}

/// Validate all inputs and assemble one immutable staged platform payload.
fn assemble(inputs: &Inputs) -> ToolResult<()> {
    if inputs.output.exists() {
        return Err(invalid(format!(
            "output directory already exists: {}",
            inputs.output.display()
        )));
    }

    let accepted = read_validated_manifest(&inputs.accepted_manifest)?;
    let source_evidence = read_json_with_sidecar::<SourceEvidenceWire>(
        &inputs.source_evidence,
        MAX_SOURCE_EVIDENCE_BYTES,
    )?;
    let fixture_corpus = read_json_payload::<FixtureCorpusWire>(
        &inputs.fixture_corpus,
        OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
    )?;
    validate_fixture_corpus(
        &fixture_corpus.value,
        &source_evidence.value,
        &source_evidence.sha256,
        &accepted.value,
    )?;
    let project_license = read_bounded(&inputs.project_license, MAX_LICENSE_BYTES)?;
    let intake = read_json_with_sidecar::<PlatformBundleIntake>(
        &inputs.bundle_intake,
        MAX_SOURCE_INTAKE_BYTES,
    )?;
    validate_intake(&intake.value, &accepted.value)?;
    let bundle_pin = intake
        .value
        .platforms
        .iter()
        .find(|pin| pin.platform == inputs.platform)
        .ok_or_else(|| invalid("selected platform is absent from source intake"))?;
    verify_source_archive(&inputs.source_archive, bundle_pin)?;
    let parsers_manifest_sha256 = verify_upstream_parser_manifest(
        &inputs.upstream_parser_manifest,
        &intake.value.upstream_release_manifest,
    )?;

    let policy = read_json_with_sidecar::<NativeImportPolicy>(
        &inputs.import_policy,
        MAX_IMPORT_POLICY_BYTES,
    )?;
    let platform_policy = validate_policy(&policy.value, inputs.platform)?;
    let context =
        read_json::<AssemblyContextWire>(&inputs.assembly_context, MAX_ASSEMBLY_CONTEXT_BYTES)?;

    let worker_path = canonical_worker_path(&inputs.worker)?;
    let worker_bytes = read_bounded(&worker_path, MAX_WORKER_BYTES)?;
    let worker_sha256 = sha256_bytes(&worker_bytes);
    let worker_inspection = inspect_worker(
        &worker_bytes,
        inputs.platform,
        platform_policy,
        &policy.value,
    )?;
    let containment_broker = assemble_containment_broker(
        inputs.containment_broker.as_deref(),
        inputs.platform,
        platform_policy,
    )?;
    let (candidate, construction) = context.into_artifact_context()?;

    let output_parent = inputs
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".projectatlas-parser-pack-")
        .tempdir_in(output_parent)?;
    let lib_directory = staging.path().join(LIB_DIRECTORY_NAME);
    fs::create_dir(&lib_directory)?;

    let assembled = assemble_libraries(
        &inputs.source_archive,
        &accepted.value,
        inputs.platform,
        platform_policy,
        &policy.value,
        &lib_directory,
    )?;
    let worker_name = inputs.platform.worker_file_name();
    let worker_byte_length = u64::try_from(worker_bytes.len())?;
    let mut native_audit_bytes = serde_json::to_vec_pretty(&NativeAuditReport {
        schema_version: OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION,
        worker: WorkerArtifact {
            file: AuditedFile {
                path: worker_name.to_owned(),
                sha256: worker_sha256.clone(),
                byte_length: worker_byte_length,
            },
            binary_format: worker_inspection.binary_format,
            architecture: worker_inspection.architecture,
            object_kind: worker_inspection.object_kind,
            entry_point: worker_inspection.entry_point,
            native_libraries: worker_inspection.native_libraries,
            imported_symbol_count: worker_inspection.imported_symbol_count,
            imported_symbols_sha256: worker_inspection.imported_symbols_sha256,
            export_count: worker_inspection.export_count,
            exports_sha256: worker_inspection.exports_sha256,
            defined_symbol_evidence_available: worker_inspection.defined_symbol_evidence_available,
            defined_symbol_count: worker_inspection.defined_symbol_count,
            defined_symbols_sha256: worker_inspection.defined_symbols_sha256,
        },
        containment_broker: containment_broker.as_ref().map(|broker| &broker.audit),
        grammars: &assembled.grammars,
    })?;
    native_audit_bytes.push(b'\n');

    fs::write(
        staging.path().join(ACCEPTED_MANIFEST_FILE_NAME),
        &accepted.bytes,
    )?;
    fs::write(
        staging.path().join(FIXTURE_CORPUS_FILE_NAME),
        &fixture_corpus.bytes,
    )?;
    fs::write(
        staging.path().join(PROJECT_LICENSE_FILE_NAME),
        &project_license,
    )?;
    fs::write(staging.path().join(IMPORT_POLICY_FILE_NAME), &policy.bytes)?;
    fs::write(
        staging.path().join(NATIVE_AUDIT_REPORT_FILE_NAME),
        &native_audit_bytes,
    )?;
    fs::write(staging.path().join(worker_name), &worker_bytes)?;
    if let Some(broker) = &containment_broker {
        let broker_name = inputs
            .platform
            .containment_broker_file_name()
            .ok_or_else(|| invalid("assembled containment broker has no platform filename"))?;
        fs::write(staging.path().join(broker_name), &broker.bytes)?;
    }

    let mut files = assembled
        .grammars
        .iter()
        .map(|grammar| {
            payload_file(
                &grammar.file.path,
                ParserPackPayloadRole::GrammarLibrary {
                    language_id: grammar.language_id.clone(),
                },
                grammar.file.byte_length,
                &grammar.file.sha256,
            )
        })
        .collect::<ToolResult<Vec<_>>>()?;
    files.extend([
        payload_file_for_bytes(
            ACCEPTED_MANIFEST_FILE_NAME,
            ParserPackPayloadRole::AcceptedManifest,
            &accepted.bytes,
        )?,
        payload_file_for_bytes(
            FIXTURE_CORPUS_FILE_NAME,
            ParserPackPayloadRole::FixtureCorpus,
            &fixture_corpus.bytes,
        )?,
        payload_file_for_bytes(
            PROJECT_LICENSE_FILE_NAME,
            ParserPackPayloadRole::ProjectLicense,
            &project_license,
        )?,
        payload_file_for_bytes(
            IMPORT_POLICY_FILE_NAME,
            ParserPackPayloadRole::NativeImportPolicy,
            &policy.bytes,
        )?,
        payload_file_for_bytes(
            NATIVE_AUDIT_REPORT_FILE_NAME,
            ParserPackPayloadRole::NativeAuditReport,
            &native_audit_bytes,
        )?,
        payload_file(
            worker_name,
            ParserPackPayloadRole::Worker,
            worker_byte_length,
            &worker_sha256,
        )?,
    ]);
    if let Some(broker) = &containment_broker {
        let broker_name = inputs
            .platform
            .containment_broker_file_name()
            .ok_or_else(|| invalid("assembled containment broker has no platform filename"))?;
        files.push(payload_file_for_bytes(
            broker_name,
            ParserPackPayloadRole::ContainmentBroker,
            &broker.bytes,
        )?);
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let measurements = ParserPackPayloadMeasurements::from_files(&files)?;
    let source_asset_name = bundle_pin
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid("source asset URL has no basename"))?;
    let manifest = OptionalParserPackArtifactManifest {
        schema_version: OPTIONAL_PARSER_PACK_ARTIFACT_SCHEMA_VERSION,
        pack_id: accepted.value.pack_id().to_owned(),
        projectatlas_version: accepted.value.runtime().projectatlas_version.clone(),
        platform: inputs.platform,
        candidate,
        accepted_manifest_sha256: Sha256Digest::new(accepted.sha256)?,
        capability_set_digest: accepted.value.capability_set_digest().clone(),
        fixture_corpus_sha256: Sha256Digest::new(fixture_corpus.sha256)?,
        source_asset: ParserPackSourceAsset {
            release_tag: intake.value.native_release.tag.clone(),
            release_revision: intake.value.native_release.revision.clone(),
            name: source_asset_name.to_owned(),
            sha256: bundle_pin.sha256.clone(),
            bytes: bundle_pin.byte_length,
            parsers_manifest_sha256: Sha256Digest::new(parsers_manifest_sha256)?,
        },
        construction,
        native_audit: ParserPackNativeAudit {
            policy_sha256: Sha256Digest::new(policy.sha256)?,
            report_sha256: Sha256Digest::new(sha256_bytes(&native_audit_bytes))?,
            audited_libraries: u32::try_from(assembled.grammars.len())?,
            forbidden_imports: 0,
            unexpected_dependencies: 0,
            missing_exports: 0,
            unexpected_exports: 0,
        },
        measurements,
        files,
    };
    manifest.validate(&accepted.value)?;
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(
        staging.path().join(ARTIFACT_MANIFEST_FILE_NAME),
        &manifest_bytes,
    )?;

    verify_staged_artifact(staging.path(), &manifest)?;
    fs::rename(staging.path(), &inputs.output)?;
    Ok(())
}

/// Construct one payload record from exact in-memory bytes.
fn payload_file_for_bytes(
    path: &str,
    role: ParserPackPayloadRole,
    bytes: &[u8],
) -> ToolResult<ParserPackPayloadFile> {
    payload_file(
        path,
        role,
        u64::try_from(bytes.len())?,
        &sha256_bytes(bytes),
    )
}

/// Construct one validated payload record from measured file facts.
fn payload_file(
    path: &str,
    role: ParserPackPayloadRole,
    bytes: u64,
    sha256: &str,
) -> ToolResult<ParserPackPayloadFile> {
    Ok(ParserPackPayloadFile {
        path: PackRelativePath::new(path)?,
        role,
        bytes,
        sha256: Sha256Digest::new(sha256)?,
    })
}

/// Read and domain-validate the accepted logical manifest and sidecar.
fn read_validated_manifest(path: &Path) -> ToolResult<VerifiedInput<OptionalParserPackManifest>> {
    let bytes = read_bounded(path, OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES)?;
    verify_sidecar(path, &bytes)?;
    let value = OptionalParserPackManifest::from_json(&bytes).map_err(|source| {
        invalid(format!(
            "invalid accepted manifest {}: {source}",
            path.display()
        ))
    })?;
    Ok(VerifiedInput {
        value,
        sha256: sha256_bytes(&bytes),
        bytes,
    })
}

/// Read typed bounded JSON bound by its adjacent SHA-256 sidecar.
fn read_json_with_sidecar<T>(path: &Path, maximum: usize) -> ToolResult<VerifiedInput<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = read_bounded(path, maximum)?;
    verify_sidecar(path, &bytes)?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|source| invalid(format!("invalid JSON {}: {source}", path.display())))?;
    Ok(VerifiedInput {
        value,
        sha256: sha256_bytes(&bytes),
        bytes,
    })
}

/// Read one bounded ephemeral typed JSON input.
fn read_json<T>(path: &Path, maximum: usize) -> ToolResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(&read_bounded(path, maximum)?)
        .map_err(|source| invalid(format!("invalid JSON {}: {source}", path.display())))
}

/// Validate a shipped JSON payload and retain its exact bytes.
fn read_json_payload<T>(path: &Path, maximum: usize) -> ToolResult<VerifiedInput<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = read_bounded(path, maximum)?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|source| invalid(format!("invalid JSON {}: {source}", path.display())))?;
    Ok(VerifiedInput {
        value,
        sha256: sha256_bytes(&bytes),
        bytes,
    })
}

/// Read a regular file without exceeding its owning byte ceiling.
fn read_bounded(path: &Path, maximum: usize) -> ToolResult<Vec<u8>> {
    let maximum_u64 = u64::try_from(maximum)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid(format!(
            "expected a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > maximum_u64 {
        return Err(invalid(format!(
            "{} is {} bytes; maximum is {maximum}",
            path.display(),
            metadata.len()
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum_u64.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid(format!(
            "{} exceeded the {maximum}-byte read bound",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Verify one exact-byte file against its adjacent digest sidecar.
fn verify_sidecar(path: &Path, bytes: &[u8]) -> ToolResult<()> {
    if bytes.contains(&b'\r') {
        return Err(invalid(format!(
            "sidecar-bound JSON must use canonical LF line endings: {}",
            path.display()
        )));
    }
    let sidecar = sidecar_path(path);
    let sidecar_bytes = read_bounded(&sidecar, 256)?;
    let sidecar_text = std::str::from_utf8(&sidecar_bytes)?;
    let mut fields = sidecar_text.split_whitespace();
    let expected_sha256 = fields
        .next()
        .ok_or_else(|| invalid(format!("empty SHA-256 sidecar: {}", sidecar.display())))?;
    let expected_name = fields
        .next()
        .ok_or_else(|| invalid(format!("missing sidecar file name: {}", sidecar.display())))?;
    if fields.next().is_some() {
        return Err(invalid(format!(
            "unexpected fields in SHA-256 sidecar: {}",
            sidecar.display()
        )));
    }
    let actual_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("sidecar-bound file name must be valid UTF-8"))?;
    let actual_sha256 = sha256_bytes(bytes);
    if expected_name != actual_name || expected_sha256 != actual_sha256 {
        return Err(invalid(format!(
            "SHA-256 sidecar mismatch for {}",
            path.display()
        )));
    }
    Ok(())
}

/// Derive the adjacent `.sha256` path without interpreting its contents.
fn sidecar_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(".sha256");
    PathBuf::from(value)
}

/// Bind the retained fixture corpus exactly to the accepted logical rows.
fn validate_fixture_corpus(
    corpus: &FixtureCorpusWire,
    source_evidence: &SourceEvidenceWire,
    source_evidence_sha256: &str,
    accepted: &OptionalParserPackManifest,
) -> ToolResult<()> {
    if corpus.schema_version != FIXTURE_CORPUS_SCHEMA_VERSION {
        return Err(invalid(format!(
            "fixture corpus schema {} does not match {FIXTURE_CORPUS_SCHEMA_VERSION}",
            corpus.schema_version
        )));
    }
    if corpus.source_manifest_sha256.as_str() != source_evidence_sha256 {
        return Err(invalid(
            "fixture corpus does not bind the pinned source-evidence document",
        ));
    }
    validate_source_evidence(source_evidence, accepted)?;
    if corpus.rows.len() != accepted.grammars().len() {
        return Err(invalid(format!(
            "fixture corpus has {} rows; accepted manifest requires {}",
            corpus.rows.len(),
            accepted.grammars().len()
        )));
    }
    for ((row, evidence), grammar) in corpus
        .rows
        .iter()
        .zip(&source_evidence.rows)
        .zip(accepted.grammars())
    {
        if row.language_id != grammar.language_id || evidence.language_id != grammar.language_id {
            return Err(invalid(format!(
                "fixture evidence row {:?} does not match accepted grammar {:?}",
                row.language_id, grammar.language_id
            )));
        }
        validate_corpus_fixture(
            &row.language_id,
            "positive",
            row.fixtures.positive.origin,
            &row.fixtures.positive.source_path,
            &row.fixtures.positive.case_name,
            &row.fixtures.positive.source,
            &row.fixtures.positive.source_sha256,
            &grammar.fixtures.positive,
        )?;
        match (
            &row.fixtures.positive.expected_tree,
            &row.fixtures.positive.expected_tree_sha256,
        ) {
            (None, None) => {}
            (Some(tree), Some(digest))
                if !tree.is_empty() && sha256_bytes(tree.as_bytes()) == digest.as_str() => {}
            _ => {
                return Err(invalid(format!(
                    "fixture corpus {:?} positive expected-tree evidence is inconsistent",
                    row.language_id
                )));
            }
        }
        if row.fixtures.positive.expected_tree != evidence.fixtures.positive.expected_tree
            || row.fixtures.positive.expected_tree_sha256
                != evidence.fixtures.positive.expected_tree_sha256
        {
            return Err(invalid(format!(
                "fixture corpus {:?} positive expected-tree evidence differs from the pinned source evidence",
                row.language_id
            )));
        }
        validate_corpus_fixture(
            &row.language_id,
            "negative",
            row.fixtures.negative.origin,
            &row.fixtures.negative.source_path,
            &row.fixtures.negative.case_name,
            &row.fixtures.negative.source,
            &row.fixtures.negative.source_sha256,
            &grammar.fixtures.negative,
        )?;
        if !row.fixtures.negative.expected_error {
            return Err(invalid(format!(
                "fixture corpus {:?} negative row does not require a parser error",
                row.language_id
            )));
        }
    }
    Ok(())
}

/// Bind the complete pinned source-evidence document to the accepted manifest.
fn validate_source_evidence(
    evidence: &SourceEvidenceWire,
    accepted: &OptionalParserPackManifest,
) -> ToolResult<()> {
    let accepted_source = accepted.source();
    if evidence.schema_version != SOURCE_EVIDENCE_SCHEMA_VERSION
        || evidence.source_package != accepted_source.package
        || evidence.source_version != accepted_source.version
        || evidence.cargo_archive.vcs_revision != accepted_source.cargo_archive.vcs_revision
        || evidence.cargo_archive.path_in_vcs != accepted_source.cargo_archive.path_in_vcs
        || evidence.cargo_archive.sha256 != accepted_source.cargo_archive.sha256
        || evidence.native_release.tag != accepted_source.native_release.tag
        || evidence.native_release.revision != accepted_source.native_release.revision
        || evidence.native_release.source_bundle_sha256
            != accepted_source.native_release.source_bundle_sha256
    {
        return Err(invalid(
            "pinned source evidence does not match the accepted source identity",
        ));
    }
    if evidence.rows.len() != accepted.grammars().len() {
        return Err(invalid(format!(
            "source evidence has {} rows; accepted manifest requires {}",
            evidence.rows.len(),
            accepted.grammars().len()
        )));
    }
    for (row, grammar) in evidence.rows.iter().zip(accepted.grammars()) {
        validate_source_evidence_row(row, grammar, accepted)?;
    }
    Ok(())
}

/// Validate one source-evidence row against its accepted grammar capability.
fn validate_source_evidence_row(
    row: &SourceEvidenceRowWire,
    grammar: &AcceptedGrammar,
    accepted: &OptionalParserPackManifest,
) -> ToolResult<()> {
    let subdirectory = row.source.subdirectory.as_deref().unwrap_or(".");
    if row.language_id != grammar.language_id
        || row.source.repository != grammar.source.repository_url
        || row.source.revision != grammar.source.revision
        || subdirectory != grammar.source.subdirectory
        || row.source.compile_input_digest_algorithm != "sha256-file-tree-v1"
        || row.source.compile_input_digest != grammar.source.compile_input_sha256
        || row.source.compile_files == 0
        || row.abi != grammar.abi_export.expected_abi
        || row.export_symbol != grammar.abi_export.export_symbol.as_str()
        || row.library_stem != grammar.abi_export.library_stem.as_str()
        || row.license_label.trim().is_empty()
    {
        return Err(invalid(format!(
            "source evidence {:?} differs from its accepted grammar capability",
            row.language_id
        )));
    }
    validate_corpus_fixture(
        &row.language_id,
        "source-evidence positive",
        row.fixtures.positive.origin,
        &row.fixtures.positive.source_path,
        &row.fixtures.positive.case_name,
        &row.fixtures.positive.source,
        &row.fixtures.positive.source_sha256,
        &grammar.fixtures.positive,
    )?;
    validate_expected_tree(&row.language_id, &row.fixtures.positive)?;
    validate_corpus_fixture(
        &row.language_id,
        "source-evidence negative",
        row.fixtures.negative.origin,
        &row.fixtures.negative.source_path,
        &row.fixtures.negative.case_name,
        &row.fixtures.negative.source,
        &row.fixtures.negative.source_sha256,
        &grammar.fixtures.negative,
    )?;
    if !row.fixtures.negative.expected_error {
        return Err(invalid(format!(
            "source evidence {:?} negative row does not require a parser error",
            row.language_id
        )));
    }
    if row.licenses.len() != grammar.license_record_ids.len() {
        return Err(invalid(format!(
            "source evidence {:?} license count differs from its accepted capability",
            row.language_id
        )));
    }
    let mut matched_license_ids = BTreeSet::new();
    for license in &row.licenses {
        let byte_length = u64::try_from(license.text.len())?;
        if license.byte_length != byte_length
            || sha256_bytes(license.text.as_bytes()) != license.sha256.as_str()
        {
            return Err(invalid(format!(
                "source evidence {:?} license digest is inconsistent",
                row.language_id
            )));
        }
        let matched_id = grammar.license_record_ids.iter().find(|license_id| {
            accepted.licenses().iter().any(|candidate| {
                candidate.id.as_str() == license_id.as_str()
                    && license.source_path == candidate.source_path
                    && row.source.repository == candidate.repository_url
                    && row.source.revision == candidate.revision
                    && license.text == candidate.text
            })
        });
        let Some(matched_id) = matched_id else {
            return Err(invalid(format!(
                "source evidence {:?} license differs from its accepted record",
                row.language_id
            )));
        };
        if license.source_blob.as_str().len() != 40
            || !matched_license_ids.insert(matched_id.as_str())
        {
            return Err(invalid(format!(
                "source evidence {:?} contains duplicate or invalid license evidence",
                row.language_id
            )));
        }
    }
    if matched_license_ids.len() != grammar.license_record_ids.len() {
        return Err(invalid(format!(
            "source evidence {:?} does not cover every accepted license",
            row.language_id
        )));
    }
    Ok(())
}

/// Require internally consistent optional expected-tree evidence.
fn validate_expected_tree(
    language_id: &str,
    fixture: &PositiveCorpusFixtureWire,
) -> ToolResult<()> {
    match (&fixture.expected_tree, &fixture.expected_tree_sha256) {
        (None, None) => Ok(()),
        (Some(tree), Some(digest))
            if !tree.is_empty() && sha256_bytes(tree.as_bytes()) == digest.as_str() =>
        {
            Ok(())
        }
        _ => Err(invalid(format!(
            "source evidence {language_id:?} positive expected-tree evidence is inconsistent"
        ))),
    }
}

/// Match one corpus fixture to its accepted manifest row and exact source digest.
fn validate_corpus_fixture(
    language_id: &str,
    role: &str,
    origin: GrammarFixtureOrigin,
    source_path: &str,
    case_name: &str,
    source: &str,
    source_sha256: &Sha256Digest,
    accepted: &GrammarFixture,
) -> ToolResult<()> {
    if origin != accepted.origin
        || source_path != accepted.path
        || case_name != accepted.case_name
        || source != accepted.source
    {
        return Err(invalid(format!(
            "fixture corpus {language_id:?} {role} evidence differs from the accepted manifest"
        )));
    }
    if sha256_bytes(source.as_bytes()) != source_sha256.as_str() {
        return Err(invalid(format!(
            "fixture corpus {language_id:?} {role} source digest is invalid"
        )));
    }
    Ok(())
}

/// Bind the platform intake to the accepted logical source identity.
fn validate_intake(
    intake: &PlatformBundleIntake,
    manifest: &OptionalParserPackManifest,
) -> ToolResult<()> {
    if intake.schema_version != SOURCE_INTAKE_SCHEMA_VERSION
        || intake.source_package != manifest.source().package
        || intake.source_version != manifest.source().version
        || intake.cargo_archive.vcs_revision != manifest.source().cargo_archive.vcs_revision
        || intake.cargo_archive.path_in_vcs != manifest.source().cargo_archive.path_in_vcs
        || intake.cargo_archive.sha256 != manifest.source().cargo_archive.sha256
        || intake.native_release.tag != manifest.source().native_release.tag
        || intake.native_release.revision != manifest.source().native_release.revision
        || intake.native_release.source_bundle_sha256
            != manifest.source().native_release.source_bundle_sha256
    {
        return Err(invalid(
            "platform-bundle intake does not match the accepted source identity",
        ));
    }
    validate_source_asset_pin(&intake.upstream_release_manifest)?;
    let mut seen = BTreeSet::new();
    for pin in &intake.platforms {
        validate_source_asset(pin.url.as_str(), &pin.sha256, pin.byte_length)?;
        if pin.upstream_platform.is_empty() || !seen.insert(pin.platform) {
            return Err(invalid("invalid or duplicate platform-bundle intake row"));
        }
    }
    if seen != PackPlatform::ALL.iter().copied().collect() {
        return Err(invalid(
            "platform-bundle intake must contain the complete required target set",
        ));
    }
    Ok(())
}

/// Validate an auxiliary upstream source-asset pin.
fn validate_source_asset_pin(pin: &SourceAssetPin) -> ToolResult<()> {
    validate_source_asset(&pin.url, &pin.sha256, pin.byte_length)
}

/// Validate one bounded HTTPS source-asset identity.
fn validate_source_asset(url: &str, _sha256: &Sha256Digest, byte_length: u64) -> ToolResult<()> {
    if !url.starts_with("https://") || byte_length == 0 || byte_length > MAX_SOURCE_ARCHIVE_BYTES {
        return Err(invalid("invalid bounded HTTPS source-asset pin"));
    }
    Ok(())
}

/// Verify the complete compressed source archive before decompression.
fn verify_source_archive(path: &Path, pin: &PlatformBundlePin) -> ToolResult<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() != pin.byte_length {
        return Err(invalid(format!(
            "source archive byte length does not match its pin: {}",
            path.display()
        )));
    }
    let actual_sha256 = sha256_file(path, MAX_SOURCE_ARCHIVE_BYTES)?;
    if actual_sha256 != pin.sha256.as_str() {
        return Err(invalid(format!(
            "source archive SHA-256 does not match its pin: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Rehash the acquired upstream parser inventory and bind its exact pin.
fn verify_upstream_parser_manifest(path: &Path, pin: &SourceAssetPin) -> ToolResult<String> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file()
        || metadata.len() != pin.byte_length
        || metadata.len() > MAX_UPSTREAM_PARSER_MANIFEST_BYTES
    {
        return Err(invalid(format!(
            "upstream parser manifest byte length does not match its bounded pin: {}",
            path.display()
        )));
    }
    let actual_sha256 = sha256_file(path, MAX_UPSTREAM_PARSER_MANIFEST_BYTES)?;
    if actual_sha256 != pin.sha256.as_str() {
        return Err(invalid(format!(
            "upstream parser manifest SHA-256 does not match its pin: {}",
            path.display()
        )));
    }
    Ok(actual_sha256)
}

/// Validate the closed import policy and select one platform row.
fn validate_policy(
    policy: &NativeImportPolicy,
    selected_platform: PackPlatform,
) -> ToolResult<&PlatformImportPolicy> {
    if policy.schema_version != OPTIONAL_PARSER_PACK_NATIVE_IMPORT_POLICY_SCHEMA_VERSION {
        return Err(invalid("unsupported native-import policy schema"));
    }
    validate_sorted_unique_normalized(&policy.forbidden_import_symbols, "forbidden symbols")?;
    validate_sorted_unique_normalized(
        &policy.forbidden_import_symbol_prefixes,
        "forbidden symbol prefixes",
    )?;
    validate_sorted_unique_normalized(
        &policy.worker_forbidden_import_symbols,
        "worker forbidden symbols",
    )?;
    validate_sorted_unique_normalized(
        &policy.worker_forbidden_import_symbol_prefixes,
        "worker forbidden symbol prefixes",
    )?;
    let mut seen = BTreeSet::new();
    let mut selected = None;
    for row in &policy.platforms {
        if !seen.insert(row.platform) {
            return Err(invalid("duplicate native-import platform policy"));
        }
        let normalized = row
            .allowed_libraries
            .iter()
            .map(|library| normalize_library(library, row.platform))
            .collect::<Vec<_>>();
        if normalized.is_empty()
            || normalized.windows(2).any(|pair| pair[0] >= pair[1])
            || normalized.iter().any(String::is_empty)
        {
            return Err(invalid(
                "native library allowlist must be sorted and unique",
            ));
        }
        let preloaded = row
            .worker_preloaded_libraries
            .iter()
            .map(|library| normalize_library(library, row.platform))
            .collect::<Vec<_>>();
        if preloaded.windows(2).any(|pair| pair[0] >= pair[1])
            || preloaded.iter().any(String::is_empty)
            || preloaded
                .iter()
                .any(|library| normalized.binary_search(library).is_err())
        {
            return Err(invalid(
                "worker preloaded libraries must be a sorted unique subset of the platform allowlist",
            ));
        }
        let broker_pe_loader_libraries = row
            .containment_broker_pe_loader_libraries
            .iter()
            .map(|library| normalize_library(library, row.platform))
            .collect::<Vec<_>>();
        if broker_pe_loader_libraries
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || broker_pe_loader_libraries.iter().any(String::is_empty)
            || broker_pe_loader_libraries
                .iter()
                .any(|library| is_grammar_library_dependency(library))
        {
            return Err(invalid(
                "containment-broker PE-loader allowlist must be sorted, unique, and grammar-free",
            ));
        }
        let broker_managed_modules = row
            .containment_broker_managed_modules
            .iter()
            .map(|library| normalize_library(library, row.platform))
            .collect::<Vec<_>>();
        if broker_managed_modules
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || broker_managed_modules.iter().any(String::is_empty)
            || broker_managed_modules
                .iter()
                .any(|library| is_grammar_library_dependency(library))
        {
            return Err(invalid(
                "containment-broker managed-module allowlist must be sorted, unique, and grammar-free",
            ));
        }
        match row.platform {
            PackPlatform::LinuxX86_64
                if !broker_pe_loader_libraries.is_empty()
                    || row.containment_broker_clr_runtime_header_required
                    || !broker_managed_modules.is_empty() =>
            {
                return Err(invalid(
                    "Linux native-import policy must not admit a containment broker",
                ));
            }
            PackPlatform::WindowsX86_64
                if !row.containment_broker_clr_runtime_header_required
                    || broker_managed_modules.is_empty() =>
            {
                return Err(invalid(
                    "Windows native-import policy must bind the CLR header and managed dependencies",
                ));
            }
            PackPlatform::LinuxX86_64 | PackPlatform::WindowsX86_64 => {}
        }
        if row.platform == PackPlatform::WindowsX86_64
            && (broker_pe_loader_libraries
                != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_PE_LOADER_LIBRARIES
                || broker_managed_modules != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES)
        {
            return Err(invalid(
                "Windows containment-broker dependencies differ from the shipped closed contract",
            ));
        }
        if row.platform == selected_platform {
            selected = Some(row);
        }
    }
    if seen != PackPlatform::ALL.iter().copied().collect() {
        return Err(invalid(
            "native-import policy must contain the complete required target set",
        ));
    }
    selected.ok_or_else(|| invalid("selected target is absent from native-import policy"))
}

/// Require sorted unique lowercase normalized policy strings.
fn validate_sorted_unique_normalized(values: &[String], owner: &str) -> ToolResult<()> {
    if values.is_empty()
        || values.iter().any(|value| {
            value.is_empty()
                || !value.is_ascii()
                || value != &value.to_ascii_lowercase()
                || value.bytes().any(|byte| byte.is_ascii_whitespace())
        })
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid(format!("{owner} must be sorted normalized ASCII")));
    }
    Ok(())
}

/// Stream the pinned archive, audit, and copy exactly the accepted libraries.
fn assemble_libraries(
    source_archive: &Path,
    manifest: &OptionalParserPackManifest,
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
    policy: &NativeImportPolicy,
    output: &Path,
) -> ToolResult<AssembledLibraries> {
    let expected = manifest
        .grammars()
        .iter()
        .map(|grammar| (library_file_name(grammar, platform), grammar))
        .collect::<BTreeMap<_, _>>();
    if expected.len() != manifest.grammars().len() {
        return Err(invalid(
            "accepted grammars collide on a platform library name",
        ));
    }

    let file = BufReader::new(File::open(source_archive)?);
    let decoder = zstd::stream::read::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);
    let mut selected = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut saw_root = false;
    let mut entry_count = 0usize;
    let mut archive_payload_bytes = 0u64;
    let mut selected_bytes = 0u64;

    for entry in archive.entries()? {
        let mut entry = entry?;
        entry_count = entry_count
            .checked_add(1)
            .ok_or_else(|| invalid("archive entry count overflow"))?;
        if entry_count > MAX_ARCHIVE_ENTRIES {
            return Err(invalid("source archive contains too many entries"));
        }
        let entry_size = entry.size();
        if entry_size > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(invalid("source archive member exceeds the per-file bound"));
        }
        archive_payload_bytes = archive_payload_bytes
            .checked_add(entry_size)
            .ok_or_else(|| invalid("source archive byte count overflow"))?;
        if archive_payload_bytes > MAX_ARCHIVE_PAYLOAD_BYTES {
            return Err(invalid("source archive exceeds the expanded byte bound"));
        }

        match classify_bundle_member(
            entry.path_bytes().as_ref(),
            entry.header().entry_type(),
            platform,
        )? {
            BundleMember::RootDirectory => {
                if saw_root || entry_size != 0 {
                    return Err(invalid("duplicate or non-empty pinned bundle root"));
                }
                saw_root = true;
            }
            BundleMember::NativeLibrary(file_name) => {
                if !seen.insert(file_name.clone()) {
                    return Err(invalid(format!(
                        "duplicate source archive member {file_name:?}"
                    )));
                }
                let Some(grammar) = expected.get(&file_name).copied() else {
                    io::copy(&mut entry, &mut io::sink())?;
                    continue;
                };
                let entry_capacity = usize::try_from(entry_size)?;
                let mut bytes = Vec::with_capacity(entry_capacity);
                (&mut entry)
                    .take(MAX_ARCHIVE_ENTRY_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)?;
                if u64::try_from(bytes.len())? != entry_size {
                    return Err(invalid(format!(
                        "source archive member length mismatch for {file_name:?}"
                    )));
                }
                let inspection =
                    inspect_native_library(&bytes, grammar, platform, platform_policy, policy)?;
                let byte_length = u64::try_from(bytes.len())?;
                selected_bytes = selected_bytes
                    .checked_add(byte_length)
                    .ok_or_else(|| invalid("selected native library byte count overflow"))?;
                if selected_bytes > MAX_SELECTED_LIBRARY_BYTES {
                    return Err(invalid(
                        "selected native libraries exceed the pack byte bound",
                    ));
                }
                fs::write(output.join(&file_name), &bytes)?;
                selected.insert(
                    file_name.clone(),
                    GrammarArtifact {
                        language_id: grammar.language_id.clone(),
                        file: AuditedFile {
                            path: format!("{LIB_DIRECTORY_NAME}/{file_name}"),
                            sha256: sha256_bytes(&bytes),
                            byte_length,
                        },
                        export_symbol: grammar.abi_export.export_symbol.as_str().to_owned(),
                        expected_abi: grammar.abi_export.expected_abi,
                        binary_format: inspection.binary_format,
                        architecture: inspection.architecture,
                        native_libraries: inspection.native_libraries,
                        imported_symbol_count: inspection.imported_symbol_count,
                        imported_symbols_sha256: inspection.imported_symbols_sha256,
                    },
                );
            }
        }
    }

    if !saw_root {
        return Err(invalid(
            "pinned source bundle is missing its exact ./ root entry",
        ));
    }
    if selected.len() != expected.len() {
        let missing = expected
            .keys()
            .find(|name| !selected.contains_key(*name))
            .cloned()
            .unwrap_or_else(|| "<unknown>".to_owned());
        return Err(invalid(format!(
            "source archive is missing accepted native library {missing:?}"
        )));
    }
    let grammars = manifest
        .grammars()
        .iter()
        .map(|grammar| {
            selected
                .remove(&library_file_name(grammar, platform))
                .ok_or_else(|| invalid("accepted native library disappeared during assembly"))
        })
        .collect::<ToolResult<Vec<_>>>()?;
    Ok(AssembledLibraries { grammars })
}

/// Classify one archive entry under the exact pinned flat-bundle shape.
fn classify_bundle_member(
    raw_path: &[u8],
    entry_type: EntryType,
    platform: PackPlatform,
) -> ToolResult<BundleMember> {
    if entry_type.is_dir() && raw_path == b"./" {
        return Ok(BundleMember::RootDirectory);
    }
    if !entry_type.is_file() {
        return Err(invalid("source archive contains a non-file member"));
    }
    let path = std::str::from_utf8(raw_path)?;
    let name = path
        .strip_prefix("./")
        .ok_or_else(|| invalid("source archive file must use the pinned ./basename shape"))?;
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || !is_native_library_name(name, platform)
    {
        return Err(invalid(format!(
            "unsafe or unexpected source archive member {path:?}"
        )));
    }
    Ok(BundleMember::NativeLibrary(name.to_owned()))
}

/// Return whether a basename is a canonical native grammar library for a target.
fn is_native_library_name(name: &str, platform: PackPlatform) -> bool {
    let (prefix, suffix) = match platform {
        PackPlatform::LinuxX86_64 => ("libtree_sitter_", ".so"),
        PackPlatform::WindowsX86_64 => ("tree_sitter_", ".dll"),
    };
    name.strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .is_some_and(|stem| {
            !stem.is_empty()
                && stem
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

/// Derive one accepted grammar's target-native library basename.
fn library_file_name(grammar: &AcceptedGrammar, platform: PackPlatform) -> String {
    let stem = grammar.abi_export.library_stem.as_str();
    match platform {
        PackPlatform::LinuxX86_64 => format!("lib{stem}.so"),
        PackPlatform::WindowsX86_64 => format!("{stem}.dll"),
    }
}

/// Audit one native grammar library before admitting its bytes.
fn inspect_native_library(
    bytes: &[u8],
    grammar: &AcceptedGrammar,
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
    policy: &NativeImportPolicy,
) -> ToolResult<NativeInspection> {
    let object = NativeObject::parse(bytes)?;
    validate_object_identity(&object, platform, ObjectKind::Dynamic)?;
    let expected_export = grammar.abi_export.export_symbol.as_str();
    validate_exports(&object, expected_export, platform)?;
    let (native_libraries, imported_symbols) = inspect_imports(&object, platform)?;
    enforce_import_policy(
        &native_libraries,
        &imported_symbols,
        platform,
        platform_policy,
        policy,
    )?;
    Ok(NativeInspection {
        binary_format: binary_format_name(object.format()).to_owned(),
        architecture: architecture_name(object.architecture()).to_owned(),
        native_libraries: native_libraries.into_iter().collect(),
        imported_symbol_count: imported_symbols.len(),
        imported_symbols_sha256: sha256_strings(&imported_symbols),
    })
}

/// Require the supplied parser worker to match its declared target.
fn inspect_worker(
    bytes: &[u8],
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
    policy: &NativeImportPolicy,
) -> ToolResult<WorkerInspection> {
    let object = NativeObject::parse(bytes)?;
    validate_executable_identity(&object, platform, "parser worker")?;
    validate_worker_program_interpreter(&object, platform)?;
    let (native_libraries, imported_symbols) = inspect_imports(&object, platform)?;
    if let Some(library) = native_libraries
        .iter()
        .find(|library| is_grammar_library_dependency(library))
    {
        return Err(invalid(format!(
            "parser worker directly depends on grammar-shaped library {library:?}"
        )));
    }
    let allowed_libraries = platform_policy
        .allowed_libraries
        .iter()
        .map(|library| normalize_library(library, platform))
        .collect::<BTreeSet<_>>();
    if let Some(library) = native_libraries
        .iter()
        .find(|library| !allowed_libraries.contains(*library))
    {
        return Err(invalid(format!(
            "parser worker native dependency {library:?} is not allowed for {}",
            platform.as_str()
        )));
    }
    let required_preloads = platform_policy
        .worker_preloaded_libraries
        .iter()
        .map(|library| normalize_library(library, platform))
        .collect::<BTreeSet<_>>();
    if platform == PackPlatform::LinuxX86_64 && required_preloads != native_libraries {
        return Err(invalid(format!(
            "parser worker direct dependencies differ from the exact eager runtime policy: observed={native_libraries:?}; expected={required_preloads:?}"
        )));
    }
    enforce_symbol_denylist(
        &imported_symbols,
        &policy.worker_forbidden_import_symbols,
        &policy.worker_forbidden_import_symbol_prefixes,
        "parser worker",
    )?;
    let exports = inspect_executable_exports(&object, platform, "parser worker")?;
    let defined_symbols = inspect_worker_defined_symbols(&object, platform)?;
    Ok(WorkerInspection {
        binary_format: binary_format_name(object.format()).to_owned(),
        architecture: architecture_name(object.architecture()).to_owned(),
        object_kind: object_kind_name(object.kind()).to_owned(),
        entry_point: format!("0x{:016x}", object.entry()),
        native_libraries: native_libraries.into_iter().collect(),
        imported_symbol_count: imported_symbols.len(),
        imported_symbols_sha256: sha256_strings(&imported_symbols),
        export_count: exports.len(),
        exports_sha256: sha256_ordered_strings(&exports),
        defined_symbol_evidence_available: defined_symbols.is_some(),
        defined_symbol_count: defined_symbols.as_ref().map(Vec::len),
        defined_symbols_sha256: defined_symbols
            .as_ref()
            .map(|symbols| sha256_ordered_strings(symbols)),
    })
}

/// Require the target-specific program interpreter before the worker can become an artifact.
fn validate_worker_program_interpreter(
    object: &NativeObject<'_>,
    platform: PackPlatform,
) -> ToolResult<()> {
    let observed = match object {
        NativeObject::Elf32(file) => elf_program_interpreter(file)?,
        NativeObject::Elf64(file) => elf_program_interpreter(file)?,
        _ => None,
    };
    match (platform, observed.as_deref()) {
        (PackPlatform::LinuxX86_64, Some(OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME))
        | (PackPlatform::WindowsX86_64, None) => Ok(()),
        (PackPlatform::LinuxX86_64, _) => Err(invalid(format!(
            "parser worker ELF interpreter differs from the accepted Linux loader: observed={observed:?}; expected={OPTIONAL_PARSER_PACK_LINUX_RUNTIME_LOADER_BASENAME:?}"
        ))),
        (PackPlatform::WindowsX86_64, Some(interpreter)) => Err(invalid(format!(
            "Windows parser worker unexpectedly declares program interpreter {interpreter:?}"
        ))),
    }
}

/// Read one normalized absolute `PT_INTERP` basename from an ELF executable.
fn elf_program_interpreter<Elf>(file: &ElfFile<'_, Elf>) -> ToolResult<Option<String>>
where
    Elf: ElfFileHeader,
{
    let mut observed = None;
    for header in file.elf_program_headers() {
        let Some(bytes) = header.interpreter(file.endian(), file.data())? else {
            continue;
        };
        if observed.is_some() {
            return Err(invalid("parser worker declares multiple ELF interpreters"));
        }
        let path = std::str::from_utf8(bytes)?;
        if !path.starts_with('/') || path.contains('\\') {
            return Err(invalid(
                "parser worker ELF interpreter is not an absolute Unix path",
            ));
        }
        let basename = path
            .rsplit('/')
            .next()
            .filter(|basename| !basename.is_empty())
            .ok_or_else(|| invalid("parser worker ELF interpreter has no basename"))?;
        validate_native_audit_name(basename, "parser worker ELF interpreter")?;
        observed = Some(basename.to_owned());
    }
    Ok(observed)
}

/// Require the artifact-bound containment broker to match the closed Windows policy.
fn inspect_containment_broker(
    bytes: &[u8],
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
) -> ToolResult<ContainmentBrokerInspection> {
    if platform != PackPlatform::WindowsX86_64 {
        return Err(invalid(
            "runtime-containment broker is supported only for the Windows artifact",
        ));
    }
    let object = NativeObject::parse(bytes)?;
    validate_managed_windows_broker_identity(&object)?;
    let (clr_runtime_header_rva, clr_runtime_header_size) = inspect_clr_runtime_header(&object)?;
    let (native_libraries, imported_symbols) = inspect_imports(&object, platform)?;
    if let Some(library) = native_libraries
        .iter()
        .find(|library| is_grammar_library_dependency(library))
    {
        return Err(invalid(format!(
            "runtime-containment broker directly depends on grammar-shaped library {library:?}"
        )));
    }
    let allowed_libraries = platform_policy
        .containment_broker_pe_loader_libraries
        .iter()
        .map(|library| normalize_library(library, platform))
        .collect::<BTreeSet<_>>();
    if native_libraries != allowed_libraries {
        return Err(invalid(format!(
            "runtime-containment broker dependencies do not exactly match the {} policy",
            platform.as_str()
        )));
    }
    let exports = inspect_executable_exports(&object, platform, "runtime-containment broker")?;
    Ok(ContainmentBrokerInspection {
        binary_format: binary_format_name(object.format()).to_owned(),
        architecture: architecture_name(object.architecture()).to_owned(),
        object_kind: object_kind_name(object.kind()).to_owned(),
        entry_point: format!("0x{:016x}", object.entry()),
        clr_runtime_header_rva,
        clr_runtime_header_size,
        pe_loader_libraries: native_libraries.into_iter().collect(),
        pe_imported_symbol_count: imported_symbols.len(),
        pe_imported_symbols_sha256: sha256_strings(&imported_symbols),
        export_count: exports.len(),
        exports_sha256: sha256_ordered_strings(&exports),
    })
}

/// Retain every normalized executable export and reject grammar constructors.
fn inspect_executable_exports(
    object: &NativeObject<'_>,
    platform: PackPlatform,
    role: &str,
) -> ToolResult<Vec<String>> {
    let exports = object.exports()?;
    if exports.len() > MAX_EXPORTS_PER_WORKER {
        return Err(invalid(format!("{role} exports exceed the audit bound")));
    }
    let mut normalized = Vec::with_capacity(exports.len());
    for export in exports {
        let symbol = normalize_export(std::str::from_utf8(export.name())?, platform).to_owned();
        validate_native_audit_name(&symbol, "executable export")?;
        if symbol.starts_with(TREE_SITTER_SYMBOL_PREFIX) {
            return Err(invalid(format!(
                "{role} exports grammar constructor {symbol:?}"
            )));
        }
        normalized.push(symbol);
    }
    normalized.sort_unstable();
    Ok(normalized)
}

/// Retain bounded named-definition evidence when the native image provides it.
fn inspect_worker_defined_symbols(
    object: &NativeObject<'_>,
    platform: PackPlatform,
) -> ToolResult<Option<Vec<String>>> {
    let mut definitions = Vec::new();
    for symbol in object.symbols() {
        if !symbol.is_definition() {
            continue;
        }
        let name = symbol.name()?;
        if name.is_empty() {
            continue;
        }
        let normalized = normalize_export(name, platform).to_owned();
        validate_native_audit_name(&normalized, "parser worker defined symbol")?;
        if normalized.starts_with(TREE_SITTER_SYMBOL_PREFIX) {
            return Err(invalid(format!(
                "parser worker defines grammar constructor {normalized:?}"
            )));
        }
        definitions.push(normalized);
        if definitions.len() > MAX_DEFINED_SYMBOLS_PER_WORKER {
            return Err(invalid(
                "parser worker defined symbols exceed the audit bound",
            ));
        }
    }
    if definitions.is_empty() {
        Ok(None)
    } else {
        definitions.sort_unstable();
        Ok(Some(definitions))
    }
}

/// Recognize a direct dependency whose basename is reserved for grammar libraries.
fn is_grammar_library_dependency(library: &str) -> bool {
    let basename = library
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(library)
        .to_ascii_lowercase();
    basename
        .strip_prefix("lib")
        .unwrap_or(&basename)
        .starts_with(TREE_SITTER_SYMBOL_PREFIX)
}

/// Require an executable target identity while permitting Linux PIE encoding.
fn validate_executable_identity(
    object: &NativeObject<'_>,
    platform: PackPlatform,
    role: &str,
) -> ToolResult<()> {
    let expected_kind = if platform == PackPlatform::LinuxX86_64 {
        object.kind()
    } else {
        ObjectKind::Executable
    };
    validate_object_identity(object, platform, expected_kind)?;
    if object.entry() == 0
        || (platform == PackPlatform::LinuxX86_64
            && !matches!(object.kind(), ObjectKind::Executable | ObjectKind::Dynamic))
    {
        return Err(invalid(format!("{role} is not an executable native image")));
    }
    Ok(())
}

/// Require the managed Windows broker's PE32+ identity without inventing a native entry point.
fn validate_managed_windows_broker_identity(object: &NativeObject<'_>) -> ToolResult<()> {
    validate_object_identity(object, PackPlatform::WindowsX86_64, ObjectKind::Executable)?;
    let entry_point = format!("0x{:016x}", object.entry());
    if entry_point != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_NATIVE_ENTRY_POINT {
        return Err(invalid(
            "runtime-containment broker must use its managed CLR entry point",
        ));
    }
    Ok(())
}

/// Validate and retain the managed broker's bounded CLR runtime header evidence.
fn inspect_clr_runtime_header(object: &NativeObject<'_>) -> ToolResult<(u32, u32)> {
    let NativeObject::Pe64(file) = object else {
        return Err(invalid(
            "runtime-containment broker must be an x86-64 PE32+ image",
        ));
    };
    let directory = file
        .data_directories()
        .get(object::pe::IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR)
        .ok_or_else(|| invalid("runtime-containment broker is missing its CLR runtime header"))?;
    let (rva, size) = directory.address_range();
    let expected_size = u32::try_from(std::mem::size_of::<object::pe::ImageCor20Header>())?;
    if expected_size != OPTIONAL_PARSER_PACK_WINDOWS_BROKER_CLR_RUNTIME_HEADER_SIZE
        || size != expected_size
    {
        return Err(invalid(
            "runtime-containment broker has an invalid CLR runtime-header size",
        ));
    }
    let data = directory.data(file.data(), &file.section_table())?;
    let header = data
        .read_at::<object::pe::ImageCor20Header>(0)
        .map_err(|()| invalid("runtime-containment broker CLR header is truncated"))?;
    let (metadata_rva, metadata_size) = header.meta_data.address_range();
    if header.cb.get(LE) != expected_size
        || header.flags.get(LE) & object::pe::COMIMAGE_FLAGS_NATIVE_ENTRYPOINT != 0
        || header.entry_point_token_or_rva.get(LE) == 0
        || metadata_rva == 0
        || metadata_size == 0
    {
        return Err(invalid(
            "runtime-containment broker has invalid managed CLR entry metadata",
        ));
    }
    Ok((rva, size))
}

/// Require exact native format, architecture, kind, endianness, and width.
fn validate_object_identity(
    object: &NativeObject<'_>,
    platform: PackPlatform,
    expected_kind: ObjectKind,
) -> ToolResult<()> {
    let (expected_format, expected_architecture) = match platform {
        PackPlatform::LinuxX86_64 => (BinaryFormat::Elf, Architecture::X86_64),
        PackPlatform::WindowsX86_64 => (BinaryFormat::Pe, Architecture::X86_64),
    };
    if object.format() != expected_format
        || object.architecture() != expected_architecture
        || object.kind() != expected_kind
        || !object.is_little_endian()
        || !object.is_64()
    {
        return Err(invalid(format!(
            "native object identity mismatch: expected {expected_format:?}/{expected_architecture:?}/{expected_kind:?}"
        )));
    }
    Ok(())
}

/// Require the constructor and reject every other Tree-sitter grammar export.
fn validate_exports(
    object: &NativeObject<'_>,
    expected: &str,
    platform: PackPlatform,
) -> ToolResult<()> {
    let exports = object.exports()?;
    if exports.len() > MAX_EXPORTS_PER_LIBRARY {
        return Err(invalid("native library exports exceed the audit bound"));
    }
    let allowed = std::iter::once(expected.to_owned())
        .chain(
            EXTERNAL_SCANNER_EXPORT_SUFFIXES
                .iter()
                .map(|suffix| format!("{expected}_external_scanner_{suffix}")),
        )
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for export in exports {
        let raw = std::str::from_utf8(export.name())?;
        let normalized = normalize_export(raw, platform);
        validate_native_audit_name(normalized, "native export")?;
        if !normalized.starts_with("tree_sitter_") {
            continue;
        }
        if !allowed.contains(normalized) || !seen.insert(normalized.to_owned()) {
            return Err(invalid(format!(
                "unexpected or duplicate native export {raw:?} for {expected:?}"
            )));
        }
    }
    if !seen.contains(expected) {
        return Err(invalid(format!(
            "native library is missing expected export {expected:?}"
        )));
    }
    Ok(())
}

/// Return one export identity in the artifact platform vocabulary.
fn normalize_export(symbol: &str, _platform: PackPlatform) -> &str {
    symbol
}

/// Collect complete format-specific dependencies and normalized imports.
fn inspect_imports(
    object: &NativeObject<'_>,
    platform: PackPlatform,
) -> ToolResult<(BTreeSet<String>, BTreeSet<String>)> {
    let imports = object.imports()?;
    if imports.len() > MAX_IMPORTS_PER_LIBRARY {
        return Err(invalid("native imports exceed the audit bound"));
    }
    let mut symbols = BTreeSet::new();
    for import in &imports {
        let symbol = std::str::from_utf8(import.name())?;
        let normalized = normalize_import_symbol(symbol);
        validate_native_audit_name(&normalized, "native import")?;
        symbols.insert(normalized);
    }
    let mut libraries = match object {
        NativeObject::Elf32(file) => elf_needed_libraries(file)?,
        NativeObject::Elf64(file) => elf_needed_libraries(file)?,
        NativeObject::MachO32(file) => macho_load_libraries(file)?,
        NativeObject::MachO64(file) => macho_load_libraries(file)?,
        NativeObject::Pe32(file) => pe_import_libraries(file, &imports, &mut symbols)?,
        NativeObject::Pe64(file) => pe_import_libraries(file, &imports, &mut symbols)?,
        _ => return Err(invalid("unsupported native object format")),
    };
    if symbols.len() > MAX_IMPORTS_PER_LIBRARY {
        return Err(invalid("native imports exceed the audit bound"));
    }
    libraries = libraries
        .into_iter()
        .map(|library| normalize_library(&library, platform))
        .collect();
    if libraries.len() > MAX_NATIVE_LIBRARIES_PER_LIBRARY {
        return Err(invalid("native dependency count exceeds the audit bound"));
    }
    for library in &libraries {
        validate_native_audit_name(library, "native dependency")?;
    }
    Ok((libraries, symbols))
}

/// Read complete ELF direct dependencies from `DT_NEEDED` entries.
fn elf_needed_libraries<Elf>(file: &ElfFile<'_, Elf>) -> ToolResult<BTreeSet<String>>
where
    Elf: ElfFileHeader,
{
    let table = file.elf_dynamic_table()?;
    let mut libraries = BTreeSet::new();
    for dynamic in &table {
        if dynamic.tag == object::elf::DT_NEEDED {
            libraries.insert(std::str::from_utf8(table.string(dynamic)?)?.to_owned());
        }
    }
    Ok(libraries)
}

/// Read Mach-O load dependencies while excluding the dylib's own identity.
fn macho_load_libraries<Mach>(file: &MachOFile<'_, Mach>) -> ToolResult<BTreeSet<String>>
where
    Mach: MachHeader,
{
    let endian = file.endian();
    let mut commands = file.macho_load_commands()?;
    let mut libraries = BTreeSet::new();
    while let Some(command) = commands.next()? {
        if let Some(dylib) = command.dylib()? {
            let name = command.string(endian, dylib.dylib.name)?;
            libraries.insert(std::str::from_utf8(name)?.to_owned());
        }
    }
    Ok(libraries)
}

/// Read PE normal dependencies plus every delay-import descriptor and symbol.
fn pe_import_libraries<Pe>(
    file: &PeFile<'_, Pe>,
    normal_imports: &[object::read::Import<'_>],
    symbols: &mut BTreeSet<String>,
) -> ToolResult<BTreeSet<String>>
where
    Pe: ImageNtHeaders,
{
    let mut libraries = normal_imports
        .iter()
        .map(|import| std::str::from_utf8(import.library()).map(str::to_owned))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if let Some(table) = file
        .data_directories()
        .delay_load_import_table(file.data(), &file.section_table())?
    {
        let mut descriptors = table.descriptors()?;
        while let Some(descriptor) = descriptors.next()? {
            if descriptor.attributes.get(LE) != object::pe::IMAGE_DELAYLOAD_RVA_BASED {
                return Err(invalid("unsupported non-RVA PE delay-import descriptor"));
            }
            let library = std::str::from_utf8(table.name(descriptor.dll_name_rva.get(LE))?)?;
            libraries.insert(library.to_owned());
            let mut thunks = table.thunks(descriptor.import_name_table_rva.get(LE))?;
            while let Some(thunk) = thunks.next::<Pe>()? {
                match table.import::<Pe>(thunk)? {
                    PeImport::Name(_, name) => {
                        let normalized = normalize_import_symbol(std::str::from_utf8(name)?);
                        validate_native_audit_name(&normalized, "native delay import")?;
                        symbols.insert(normalized);
                    }
                    PeImport::Ordinal(ordinal) => {
                        return Err(invalid(format!(
                            "PE delay import by ordinal {ordinal} cannot be policy-audited"
                        )));
                    }
                }
            }
        }
    }
    Ok(libraries)
}

/// Enforce the platform dependency allowlist and forbidden symbol policy.
fn enforce_import_policy(
    libraries: &BTreeSet<String>,
    symbols: &BTreeSet<String>,
    platform: PackPlatform,
    platform_policy: &PlatformImportPolicy,
    policy: &NativeImportPolicy,
) -> ToolResult<()> {
    let allowed_libraries = platform_policy
        .allowed_libraries
        .iter()
        .map(|library| normalize_library(library, platform))
        .collect::<BTreeSet<_>>();
    if let Some(library) = libraries
        .iter()
        .find(|library| !allowed_libraries.contains(*library))
    {
        return Err(invalid(format!(
            "native library dependency {library:?} is not allowed for {}",
            platform.as_str()
        )));
    }
    enforce_symbol_denylist(
        symbols,
        &policy.forbidden_import_symbols,
        &policy.forbidden_import_symbol_prefixes,
        "grammar library",
    )
}

/// Reject exact or prefix-matched normalized imports for one binary role.
fn enforce_symbol_denylist(
    symbols: &BTreeSet<String>,
    exact: &[String],
    prefixes: &[String],
    owner: &str,
) -> ToolResult<()> {
    for symbol in symbols {
        if exact.binary_search(symbol).is_ok()
            || prefixes.iter().any(|prefix| symbol.starts_with(prefix))
        {
            return Err(invalid(format!(
                "{owner} import {symbol:?} violates the parser-pack policy"
            )));
        }
    }
    Ok(())
}

/// Bound one normalized native name before retaining or hashing it.
fn validate_native_audit_name(value: &str, owner: &str) -> ToolResult<()> {
    if value.is_empty() || value.len() > MAX_NATIVE_AUDIT_NAME_BYTES {
        return Err(invalid(format!(
            "{owner} is empty or exceeds the {MAX_NATIVE_AUDIT_NAME_BYTES}-byte audit bound"
        )));
    }
    Ok(())
}

/// Normalize platform decoration for policy comparison.
fn normalize_import_symbol(symbol: &str) -> String {
    symbol
        .strip_prefix('_')
        .unwrap_or(symbol)
        .split('@')
        .next()
        .unwrap_or(symbol)
        .to_ascii_lowercase()
}

/// Normalize native library identity only where the platform is case-insensitive.
fn normalize_library(library: &str, platform: PackPlatform) -> String {
    if platform == PackPlatform::WindowsX86_64 {
        library.to_ascii_lowercase()
    } else {
        library.to_owned()
    }
}

/// Render one supported object format into the audit vocabulary.
fn binary_format_name(format: BinaryFormat) -> &'static str {
    match format {
        BinaryFormat::Elf => "elf",
        BinaryFormat::MachO => "mach-o",
        BinaryFormat::Pe => "pe",
        _ => "unsupported",
    }
}

/// Render one supported architecture into the audit vocabulary.
fn architecture_name(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X86_64 => "x86_64",
        Architecture::Aarch64 => "aarch64",
        _ => "unsupported",
    }
}

/// Render one native object kind into the audit vocabulary.
fn object_kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Unknown => "unknown",
        ObjectKind::Relocatable => "relocatable",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic",
        ObjectKind::Core => "core",
        _ => "unsupported",
    }
}

/// Re-enumerate the staged tree and reject every unmanifested payload file.
fn verify_staged_artifact(
    staging: &Path,
    manifest: &OptionalParserPackArtifactManifest,
) -> ToolResult<()> {
    let actual_root_files = fs::read_dir(staging)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name())
        .collect::<BTreeSet<_>>();
    let mut expected_root_files = [
        OsString::from(ACCEPTED_MANIFEST_FILE_NAME),
        OsString::from(ARTIFACT_MANIFEST_FILE_NAME),
        OsString::from(FIXTURE_CORPUS_FILE_NAME),
        OsString::from(PROJECT_LICENSE_FILE_NAME),
        OsString::from(IMPORT_POLICY_FILE_NAME),
        OsString::from(NATIVE_AUDIT_REPORT_FILE_NAME),
        OsString::from(LIB_DIRECTORY_NAME),
        OsString::from(manifest.platform.worker_file_name()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if let Some(broker_name) = manifest.platform.containment_broker_file_name() {
        expected_root_files.insert(OsString::from(broker_name));
    }
    if actual_root_files != expected_root_files {
        return Err(invalid("staged artifact contains unexpected root files"));
    }
    for file in &manifest.files {
        let path = staging.join(file.path.as_str());
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            return Err(invalid(format!(
                "staged payload is not a regular file: {}",
                file.path.as_str()
            )));
        }
        if metadata.len() != file.bytes {
            return Err(invalid(format!(
                "staged payload byte length drifted: {}",
                file.path.as_str()
            )));
        }
        let observed_sha256 = sha256_file(&path, file.bytes)?;
        if observed_sha256 != file.sha256.as_str() {
            return Err(invalid(format!(
                "staged payload SHA-256 drifted: {}",
                file.path.as_str()
            )));
        }
    }
    let native_audit_report = manifest
        .files
        .iter()
        .find(|file| matches!(&file.role, ParserPackPayloadRole::NativeAuditReport))
        .ok_or_else(|| invalid("staged artifact has no native audit report"))?;
    if native_audit_report.sha256 != manifest.native_audit.report_sha256 {
        return Err(invalid(
            "staged native audit report digest differs from the native-audit summary",
        ));
    }
    let actual_libraries = fs::read_dir(staging.join(LIB_DIRECTORY_NAME))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name())
        .collect::<BTreeSet<_>>();
    let expected_libraries = manifest
        .files
        .iter()
        .filter(|file| matches!(file.role, ParserPackPayloadRole::GrammarLibrary { .. }))
        .map(|file| {
            Path::new(file.path.as_str())
                .file_name()
                .map(ToOwned::to_owned)
                .ok_or_else(|| invalid("artifact grammar path has no file name"))
        })
        .collect::<ToolResult<BTreeSet<_>>>()?;
    if actual_libraries != expected_libraries {
        return Err(invalid("staged artifact library inventory drifted"));
    }
    Ok(())
}

/// Stream SHA-256 over a file within an explicit byte limit.
fn sha256_file(path: &Path, maximum: u64) -> ToolResult<String> {
    let mut file = File::open(path)?.take(maximum.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read)?)
            .ok_or_else(|| invalid("SHA-256 input byte count overflow"))?;
        if total > maximum {
            return Err(invalid("SHA-256 input exceeds its byte bound"));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(lower_hex(&hasher.finalize()))
}

/// Compute lowercase SHA-256 for exact bytes.
fn sha256_bytes(bytes: &[u8]) -> String {
    lower_hex(&Sha256::digest(bytes))
}

/// Hash a sorted string set through a length-delimited projection.
fn sha256_strings(values: &BTreeSet<String>) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    lower_hex(&hasher.finalize())
}

/// Hash one already-sorted string sequence while retaining duplicate entries.
fn sha256_ordered_strings(values: &[String]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    lower_hex(&hasher.finalize())
}

/// Render bytes as canonical lowercase hexadecimal.
fn lower_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}

/// Construct one invalid-data error at the outer release-tool boundary.
fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

/// Focused archive-shape and normalization tests.
#[cfg(test)]
mod tests {
    use super::*;

    /// Reject a self-consistent digest pair whose JSON bytes are not canonical.
    #[test]
    fn sidecar_bound_json_rejects_carriage_returns() -> ToolResult<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("authority.json");
        let bytes = b"{\r\n  \"schema_version\": 1\r\n}\r\n";
        fs::write(&path, bytes)?;
        fs::write(
            sidecar_path(&path),
            format!("{}  authority.json\n", sha256_bytes(bytes)),
        )?;

        assert!(verify_sidecar(&path, bytes).is_err());
        Ok(())
    }

    /// Accept only the exact pinned root exception and flat native files.
    #[test]
    fn accepts_only_the_pinned_root_and_flat_native_files() -> ToolResult<()> {
        assert!(matches!(
            classify_bundle_member(b"./", EntryType::Directory, PackPlatform::LinuxX86_64)?,
            BundleMember::RootDirectory
        ));
        assert!(matches!(
            classify_bundle_member(
                b"./libtree_sitter_ada.so",
                EntryType::Regular,
                PackPlatform::LinuxX86_64
            )?,
            BundleMember::NativeLibrary(name) if name == "libtree_sitter_ada.so"
        ));
        Ok(())
    }

    /// Reject every non-file and unsafe or unexpected archive path shape.
    #[test]
    fn rejects_non_files_and_unsafe_or_extra_paths() {
        let cases = [
            (b"./link".as_slice(), EntryType::Symlink),
            (b"./directory/".as_slice(), EntryType::Directory),
            (b"../libtree_sitter_ada.so".as_slice(), EntryType::Regular),
            (b"/libtree_sitter_ada.so".as_slice(), EntryType::Regular),
            (
                b"./nested/libtree_sitter_ada.so".as_slice(),
                EntryType::Regular,
            ),
            (b".\\tree_sitter_ada.dll".as_slice(), EntryType::Regular),
            (b"./README".as_slice(), EntryType::Regular),
        ];
        for (path, entry_type) in cases {
            assert!(
                classify_bundle_member(path, entry_type, PackPlatform::LinuxX86_64).is_err(),
                "unexpectedly accepted {}",
                String::from_utf8_lossy(path)
            );
        }
    }

    /// Normalize symbols while preserving platform-sensitive dependency paths.
    #[test]
    fn normalizes_import_names_without_weakening_library_identity() {
        assert_eq!(normalize_import_symbol("_open@GLIBC_2.2.5"), "open");
        assert_eq!(
            normalize_library("KERNEL32.dll", PackPlatform::WindowsX86_64),
            "kernel32.dll"
        );
    }

    /// Recognize direct grammar dependencies across supported native path shapes.
    #[test]
    fn recognizes_grammar_shaped_worker_dependencies() {
        for library in [
            "tree_sitter_ada.dll",
            "libtree_sitter_ada.so",
            "/usr/local/lib/libtree_sitter_ada.dylib",
            r"C:\parser-pack\tree_sitter_ada.dll",
        ] {
            assert!(is_grammar_library_dependency(library), "{library}");
        }
        for library in ["kernel32.dll", "libc.so.6", "/usr/lib/libSystem.B.dylib"] {
            assert!(!is_grammar_library_dependency(library), "{library}");
        }
    }

    /// Preserve duplicate names in ordered evidence digests.
    #[test]
    fn ordered_symbol_digest_retains_duplicates() {
        let one = vec!["symbol".to_owned()];
        let two = vec!["symbol".to_owned(), "symbol".to_owned()];
        assert_ne!(sha256_ordered_strings(&one), sha256_ordered_strings(&two));
    }

    /// Bound every native name before it can affect retained audit state.
    #[test]
    fn native_audit_names_are_bounded() -> ToolResult<()> {
        validate_native_audit_name(&"a".repeat(MAX_NATIVE_AUDIT_NAME_BYTES), "test name")?;
        assert!(validate_native_audit_name("", "test name").is_err());
        assert!(
            validate_native_audit_name(&"a".repeat(MAX_NATIVE_AUDIT_NAME_BYTES + 1), "test name")
                .is_err()
        );
        Ok(())
    }

    /// Bind construction to the exact acquired upstream parser-inventory bytes.
    #[test]
    fn acquired_parser_manifest_must_match_its_pin() -> ToolResult<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("parsers.json");
        let bytes = br#"{"parsers":[]}"#;
        fs::write(&path, bytes)?;
        let pin = SourceAssetPin {
            url: "https://example.invalid/parsers.json".to_owned(),
            sha256: Sha256Digest::new(sha256_bytes(bytes))?,
            byte_length: u64::try_from(bytes.len())?,
        };
        assert_eq!(
            verify_upstream_parser_manifest(&path, &pin)?,
            pin.sha256.as_str()
        );

        fs::write(&path, br#"{"parsers":{}}"#)?;
        assert!(verify_upstream_parser_manifest(&path, &pin).is_err());
        Ok(())
    }

    /// Bind the retained corpus to its exact source evidence and accepted fixtures.
    #[test]
    fn retained_fixture_corpus_is_strict_and_manifest_bound() -> ToolResult<()> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let accepted = read_validated_manifest(
            &workspace.join("packaging/parser-pack/accepted-capabilities.json"),
        )?;
        let source_evidence = read_json_with_sidecar::<SourceEvidenceWire>(
            &workspace.join("packaging/parser-pack/sources/tree-sitter-language-pack-1.13.2.json"),
            MAX_SOURCE_EVIDENCE_BYTES,
        )?;
        let corpus_path = workspace.join("fixtures/languages/optional-parser-pack-corpus.json");
        let corpus = read_json_payload::<FixtureCorpusWire>(
            &corpus_path,
            OPTIONAL_PARSER_PACK_MANIFEST_MAX_BYTES,
        )?;
        validate_fixture_corpus(
            &corpus.value,
            &source_evidence.value,
            &source_evidence.sha256,
            &accepted.value,
        )?;

        let mut unknown_source_field: serde_json::Value =
            serde_json::from_slice(&source_evidence.bytes)?;
        unknown_source_field["rows"][0]
            .as_object_mut()
            .ok_or_else(|| invalid("source-evidence row is not an object"))?
            .insert("unmodeled_claim".to_owned(), serde_json::Value::Bool(true));
        if serde_json::from_value::<SourceEvidenceWire>(unknown_source_field).is_ok() {
            return Err(invalid("source evidence accepted an unknown row field"));
        }

        let mut unknown_field: serde_json::Value = serde_json::from_slice(&corpus.bytes)?;
        unknown_field["rows"][0]
            .as_object_mut()
            .ok_or_else(|| invalid("fixture corpus row is not an object"))?
            .insert("unmodeled_claim".to_owned(), serde_json::Value::Bool(true));
        if serde_json::from_value::<FixtureCorpusWire>(unknown_field).is_ok() {
            return Err(invalid("fixture corpus accepted an unknown row field"));
        }

        let mut reordered = corpus.value.clone();
        reordered.rows.swap(0, 1);
        if validate_fixture_corpus(
            &reordered,
            &source_evidence.value,
            &source_evidence.sha256,
            &accepted.value,
        )
        .is_ok()
        {
            return Err(invalid("fixture corpus accepted reordered language rows"));
        }

        let mut tampered_source = corpus.value.clone();
        tampered_source.rows[0]
            .fixtures
            .positive
            .source
            .push_str("tampered");
        tampered_source.rows[0].fixtures.positive.source_sha256 = Sha256Digest::new(sha256_bytes(
            tampered_source.rows[0].fixtures.positive.source.as_bytes(),
        ))?;
        if validate_fixture_corpus(
            &tampered_source,
            &source_evidence.value,
            &source_evidence.sha256,
            &accepted.value,
        )
        .is_ok()
        {
            return Err(invalid(
                "fixture corpus accepted source that differed from its manifest row",
            ));
        }

        let mut tampered_tree = corpus.value.clone();
        let tree = tampered_tree.rows[0]
            .fixtures
            .positive
            .expected_tree
            .as_mut()
            .ok_or_else(|| invalid("first fixture has no expected-tree evidence"))?;
        tree.push_str("tampered");
        let tree_sha256 = sha256_bytes(tree.as_bytes());
        tampered_tree.rows[0].fixtures.positive.expected_tree_sha256 =
            Some(Sha256Digest::new(tree_sha256)?);
        if validate_fixture_corpus(
            &tampered_tree,
            &source_evidence.value,
            &source_evidence.sha256,
            &accepted.value,
        )
        .is_ok()
        {
            return Err(invalid(
                "fixture corpus accepted changed expected-tree evidence with a recomputed digest",
            ));
        }

        let wrong_source_evidence = corpus.value.clone();
        let mut wrong_outcome = corpus.value;
        wrong_outcome.rows[0].fixtures.negative.expected_error = false;
        if validate_fixture_corpus(
            &wrong_outcome,
            &source_evidence.value,
            &source_evidence.sha256,
            &accepted.value,
        )
        .is_ok()
        {
            return Err(invalid(
                "fixture corpus accepted a negative without a required error",
            ));
        }
        if validate_fixture_corpus(
            &wrong_source_evidence,
            &source_evidence.value,
            &"0".repeat(64),
            &accepted.value,
        )
        .is_ok()
        {
            return Err(invalid(
                "fixture corpus accepted the wrong source-evidence digest",
            ));
        }
        Ok(())
    }

    /// Keep the native audit report versioned and worker-bound at its top level.
    #[test]
    fn native_audit_report_has_strict_versioned_shape() -> ToolResult<()> {
        let grammars = Vec::new();
        let report = serde_json::to_value(NativeAuditReport {
            schema_version: OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION,
            worker: WorkerArtifact {
                file: AuditedFile {
                    path: "projectatlas-parser-worker".to_owned(),
                    sha256: "0".repeat(64),
                    byte_length: 1,
                },
                binary_format: "elf".to_owned(),
                architecture: "x86_64".to_owned(),
                object_kind: "executable".to_owned(),
                entry_point: "0x0000000000000001".to_owned(),
                native_libraries: vec!["libc.so.6".to_owned()],
                imported_symbol_count: 1,
                imported_symbols_sha256: "1".repeat(64),
                export_count: 0,
                exports_sha256: "2".repeat(64),
                defined_symbol_evidence_available: false,
                defined_symbol_count: None,
                defined_symbols_sha256: None,
            },
            containment_broker: None,
            grammars: &grammars,
        })?;
        let object = report
            .as_object()
            .ok_or_else(|| invalid("native audit report did not serialize as an object"))?;
        assert_eq!(
            object.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(["containment_broker", "grammars", "schema_version", "worker",])
        );
        assert_eq!(
            object.get("containment_broker"),
            Some(&serde_json::Value::Null)
        );
        assert_eq!(
            object.get("schema_version"),
            Some(&serde_json::Value::from(
                OPTIONAL_PARSER_PACK_NATIVE_AUDIT_SCHEMA_VERSION
            ))
        );
        let worker = object
            .get("worker")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| invalid("native audit report worker was not an object"))?;
        assert!(worker.contains_key("file"));
        assert_eq!(
            worker.get("defined_symbol_evidence_available"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            worker.get("defined_symbol_count"),
            Some(&serde_json::Value::Null)
        );
        Ok(())
    }

    /// Keep platform broker presence explicit in the positional release-tool contract.
    #[test]
    fn containment_broker_argument_matches_the_selected_platform() -> ToolResult<()> {
        let absent = OsString::from(NO_CONTAINMENT_BROKER_ARGUMENT);
        assert!(parse_containment_broker_input(&absent, PackPlatform::LinuxX86_64)?.is_none());
        assert!(parse_containment_broker_input(&absent, PackPlatform::WindowsX86_64).is_err());
        let broker = OsString::from(r"C:\pack\projectatlas-parser-containment.exe");
        assert!(parse_containment_broker_input(&broker, PackPlatform::LinuxX86_64).is_err());
        assert_eq!(
            parse_containment_broker_input(&broker, PackPlatform::WindowsX86_64)?,
            Some(PathBuf::from(&broker))
        );
        Ok(())
    }

    /// Bind the broker-reported managed import surface to the exact closed policy.
    #[test]
    fn containment_broker_build_contract_is_strict_and_policy_bound() -> ToolResult<()> {
        let policy = serde_json::from_slice::<NativeImportPolicy>(include_bytes!(
            "../../../packaging/parser-pack/native-import-policy.json"
        ))?;
        let windows = validate_policy(&policy, PackPlatform::WindowsX86_64)?;
        let valid = b"projectatlas-parser-containment-build-contract-v1|runtime=windows-net-framework-clr-v4|architecture=x86_64|modules=advapi32.dll,kernel32.dll,userenv.dll|methods=30|imports_sha256=7fcfa105667226c185a368b879d31a5ccc8f99ea44480f4dbd181747f11fdcaa\r\n";
        let parsed = parse_containment_broker_build_contract(valid, windows)?;
        assert_eq!(
            parsed.runtime_family,
            OPTIONAL_PARSER_PACK_WINDOWS_BROKER_RUNTIME_FAMILY
        );
        assert_eq!(parsed.managed_import_count, 30);
        assert_eq!(
            parsed.managed_modules,
            OPTIONAL_PARSER_PACK_WINDOWS_BROKER_MANAGED_MODULES
        );

        for rejected in [
            valid.strip_suffix(b"\r\n").expect("known suffix"),
            b"projectatlas-parser-containment-build-contract-v2|runtime=windows-net-framework-clr-v4|architecture=x86_64|modules=advapi32.dll,kernel32.dll,userenv.dll|methods=30|imports_sha256=7fcfa105667226c185a368b879d31a5ccc8f99ea44480f4dbd181747f11fdcaa\n",
            b"projectatlas-parser-containment-build-contract-v1|runtime=windows-net-framework-clr-v4|architecture=anycpu|modules=advapi32.dll,kernel32.dll,userenv.dll|methods=30|imports_sha256=7fcfa105667226c185a368b879d31a5ccc8f99ea44480f4dbd181747f11fdcaa\n",
            b"projectatlas-parser-containment-build-contract-v1|runtime=windows-net-framework-clr-v4|architecture=x86_64|modules=advapi32.dll,kernel32.dll|methods=30|imports_sha256=7fcfa105667226c185a368b879d31a5ccc8f99ea44480f4dbd181747f11fdcaa\n",
            b"projectatlas-parser-containment-build-contract-v1|runtime=windows-net-framework-clr-v4|architecture=x86_64|modules=advapi32.dll,kernel32.dll,userenv.dll|methods=0|imports_sha256=7fcfa105667226c185a368b879d31a5ccc8f99ea44480f4dbd181747f11fdcaa\n",
        ] {
            assert!(
                parse_containment_broker_build_contract(rejected, windows).is_err(),
                "malformed broker contract was accepted"
            );
        }
        let oversized = vec![b'a'; MAX_CONTAINMENT_BROKER_CONTRACT_OUTPUT_BYTES + 1];
        assert!(parse_containment_broker_build_contract(&oversized, windows).is_err());
        Ok(())
    }

    /// Keep parser-worker validation static at the core artifact boundary.
    #[test]
    fn parser_worker_audit_has_no_process_launch_contract() {
        let source = include_str!("assemble_optional_parser_artifact.rs");
        for forbidden in [
            ["verify_", "worker_build_contract"].concat(),
            ["Command::new(", "worker", ")"].concat(),
            ["--verify-", "build-contract"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "core artifact assembly regained worker process ownership: {forbidden}"
            );
        }
        assert!(source.contains("inspect_worker("));
    }

    /// Reject malformed worker bytes through the static native audit on every pack target.
    #[test]
    fn static_worker_audit_rejects_malformed_native_artifacts() -> ToolResult<()> {
        let bytes = b"not-a-native-executable";
        let policy = serde_json::from_slice::<NativeImportPolicy>(include_bytes!(
            "../../../packaging/parser-pack/native-import-policy.json"
        ))?;
        for platform in PackPlatform::ALL.iter().copied() {
            let platform_policy = validate_policy(&policy, platform)?;
            assert!(
                inspect_worker(bytes, platform, platform_policy, &policy).is_err(),
                "malformed worker bytes passed the {} static audit",
                platform.as_str()
            );
        }
        Ok(())
    }

    /// Keep exact execution primitives denied without matching unrelated CRT setup helpers.
    #[test]
    fn checked_in_execution_policy_does_not_use_an_ambiguous_prefix() -> ToolResult<()> {
        let bytes = include_bytes!("../../../packaging/parser-pack/native-import-policy.json");
        let sidecar =
            include_str!("../../../packaging/parser-pack/native-import-policy.json.sha256");
        let declared_sha256 = sidecar
            .split_ascii_whitespace()
            .next()
            .ok_or_else(|| invalid("native-import policy sidecar is empty"))?;
        assert_eq!(sha256_bytes(bytes), declared_sha256);
        let policy = serde_json::from_slice::<NativeImportPolicy>(bytes)?;
        for platform in PackPlatform::ALL.iter().copied() {
            let _selected = validate_policy(&policy, platform)?;
        }
        let linux = validate_policy(&policy, PackPlatform::LinuxX86_64)?;
        assert_eq!(
            linux.worker_preloaded_libraries,
            ["libc.so.6", "libgcc_s.so.1", "libm.so.6", "libstdc++.so.6"]
        );
        assert!(linux.containment_broker_pe_loader_libraries.is_empty());
        assert!(!linux.containment_broker_clr_runtime_header_required);
        assert!(linux.containment_broker_managed_modules.is_empty());
        let windows = validate_policy(&policy, PackPlatform::WindowsX86_64)?;
        assert!(windows.worker_preloaded_libraries.is_empty());
        assert!(windows.containment_broker_pe_loader_libraries.is_empty());
        assert!(windows.containment_broker_clr_runtime_header_required);
        assert_eq!(
            windows.containment_broker_managed_modules,
            ["advapi32.dll", "kernel32.dll", "userenv.dll"]
        );
        assert!(
            !policy
                .forbidden_import_symbol_prefixes
                .iter()
                .any(|prefix| prefix == "exec")
        );
        assert!(
            !policy
                .worker_forbidden_import_symbol_prefixes
                .iter()
                .any(|prefix| prefix == "exec")
        );
        enforce_symbol_denylist(
            &BTreeSet::from(["execute_onexit_table".to_owned()]),
            &policy.forbidden_import_symbols,
            &policy.forbidden_import_symbol_prefixes,
            "test binary",
        )?;
        assert!(
            enforce_symbol_denylist(
                &BTreeSet::from(["execve".to_owned()]),
                &policy.forbidden_import_symbols,
                &policy.forbidden_import_symbol_prefixes,
                "test binary",
            )
            .is_err()
        );
        Ok(())
    }
}
