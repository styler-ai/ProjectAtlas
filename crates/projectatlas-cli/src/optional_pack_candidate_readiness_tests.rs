//! Validate fail-closed parser-host and semantic-pack candidate readiness.

use projectatlas_core::symbols::ParserKind;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

/// Candidate-readiness evidence compiled into the test binary.
const READINESS_BYTES: &[u8] = include_bytes!(
    "../../../docs/benchmarks/projectatlas-v0.4-optional-pack-candidate-readiness.json"
);
/// Registered evaluation operations, profiles, and result schemas.
const EVALUATION_MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-evaluation-manifest.json");
/// Workspace lock used to bind native grammar inputs.
const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");
/// Compiled grammar dependencies owned by the symbols crate.
const SYMBOLS_CARGO_MANIFEST: &str = include_str!("../../projectatlas-symbols/Cargo.toml");
/// Existing language-fixture baseline registry.
const FIXTURE_BASELINES: &str = include_str!("../../../fixtures/languages/baselines.toon");
/// Authoritative task list used to reject orphan blocker ownership.
const INTELLIGENCE_TASKS: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/tasks.md");

/// Resolve one repository-relative evidence path from the crate directory.
fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Complete typed view of candidate readiness.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateReadinessReport {
    /// Evidence schema version.
    schema_version: u32,
    /// Stable evidence kind.
    artifact_kind: String,
    /// Claim eligibility state.
    claim_status: ClaimStatus,
    /// Fail-closed selection rule.
    selection_rule: String,
    /// Parser-host readiness.
    parser_host: ParserHostReadiness,
    /// Optional semantic-pack readiness.
    semantic_pack: SemanticPackReadiness,
}

/// Whether the readiness report may support a selection claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum ClaimStatus {
    /// Inputs are registered but no selection is eligible.
    PreregisteredNotReady,
}

/// Parser-host candidate inventory and blockers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserHostReadiness {
    /// `OpenSpec` task that owns the readiness gate.
    owner_task: String,
    /// Current selection state.
    selection_state: SelectionState,
    /// Selected parser host, absent until all gates pass.
    selected_candidate: Option<String>,
    /// Independent measurement dimensions required before selection.
    required_dimensions: Vec<ParserDimension>,
    /// Exact result metrics owned by each required dimension.
    dimension_metrics: BTreeMap<ParserDimension, Vec<String>>,
    /// Closed parser-host candidate set.
    candidates: Vec<ParserCandidate>,
    /// Mechanical blockers with later owners.
    blockers: Vec<ReadinessBlocker>,
}

/// Optional semantic-pack candidate inventory and blockers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticPackReadiness {
    /// `OpenSpec` task that owns the readiness gate.
    owner_task: String,
    /// Current selection state.
    selection_state: SelectionState,
    /// Selected ANN backend, absent until all gates pass.
    selected_ann_candidate: Option<String>,
    /// Selected local model runtime, absent until all gates pass.
    selected_model_candidate: Option<String>,
    /// Independent measurement dimensions required before selection.
    required_dimensions: Vec<SemanticDimension>,
    /// Exact result metrics owned by each required dimension.
    dimension_metrics: BTreeMap<SemanticDimension, Vec<String>>,
    /// Pinned candidate sources to materialize in optional-pack work.
    candidate_shortlist: Vec<SemanticCandidate>,
    /// Pinned labeled corpus identity, absent until implemented.
    labeled_retrieval_corpus: Option<Value>,
    /// Deterministic model-input contract, absent until implemented.
    model_input_contract: Option<Value>,
    /// Backend-specific vector tolerance contract, absent until implemented.
    vector_tolerance_contract: Option<Value>,
    /// Registered semantic evaluation operation.
    runner_operation_id: String,
    /// Registered semantic result schema.
    result_schema: CandidateResultSchema,
    /// Registered semantic evaluation profile.
    profile: String,
    /// Default-core behavior retained while the pack is unavailable.
    default_core_fallback: String,
    /// Mechanical blockers with later owners.
    blockers: Vec<ReadinessBlocker>,
}

/// Closed readiness state used by both candidate families.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SelectionState {
    /// Parser selection is blocked.
    Blocked,
    /// Semantic selection is blocked until candidate evaluation passes.
    BlockedPendingEvaluation,
}

/// Closed parser-host candidate variants.
#[derive(Debug, Deserialize)]
#[serde(tag = "parser_mode", rename_all = "kebab-case", deny_unknown_fields)]
enum ParserCandidate {
    /// Existing trusted, compiled grammar crates.
    TrustedNativeGrammarCrates {
        /// Stable candidate identity.
        candidate_id: String,
        /// Candidate ownership family.
        candidate_kind: CandidateKind,
        /// Current readiness state.
        readiness: CandidateState,
        /// Registered runner operation.
        runner_operation_id: String,
        /// Registered result schema.
        result_schema: CandidateResultSchema,
        /// Registered evaluation profile.
        profile: String,
        /// Root of the language fixtures.
        fixture_root: String,
        /// Exact native grammar package inputs.
        packages: Vec<NativeGrammarPackage>,
        /// Evidence still required before selection.
        missing_evidence: Vec<String>,
    },
    /// Tree-sitter WebAssembly host plus a versioned grammar pack.
    VersionedWasmGrammarPack {
        /// Stable candidate identity.
        candidate_id: String,
        /// Candidate ownership family.
        candidate_kind: CandidateKind,
        /// Current readiness state.
        readiness: CandidateState,
        /// Registered runner operation.
        runner_operation_id: String,
        /// Registered result schema.
        result_schema: CandidateResultSchema,
        /// Registered evaluation profile.
        profile: String,
        /// Exact canonical runtime candidate.
        runtime_package: WasmRuntimePackage,
        /// Versioned pack manifest, absent while input materialization is blocked.
        grammar_pack_manifest: Option<Value>,
        /// Evidence still required before selection.
        missing_evidence: Vec<String>,
    },
}

/// Closed candidate ownership families.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum CandidateKind {
    /// Parser-host candidate.
    ParserHost,
    /// Approximate nearest-neighbor index candidate.
    AnnIndex,
    /// Local model runtime candidate.
    LocalModelRuntime,
}

/// Closed readiness values for candidate inputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum CandidateState {
    /// Inputs exist, but measurements are pending.
    InputReadyMeasurementPending,
    /// The versioned pack artifact does not yet exist.
    BlockedMissingPackArtifact,
    /// Candidate source is pinned but not integrated or measured.
    NotMaterialized,
}

/// Closed candidate result-schema identities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum CandidateResultSchema {
    /// Parser-host measurement result.
    ParserHostResult,
    /// Optional semantic measurement result.
    SemanticResult,
}

impl CandidateResultSchema {
    /// Return the serialized schema identity.
    const fn as_str(self) -> &'static str {
        match self {
            Self::ParserHostResult => "parser-host-result",
            Self::SemanticResult => "semantic-result",
        }
    }
}

/// Parser-host decision dimensions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum ParserDimension {
    /// Parse correctness and non-vacuous mode coverage.
    Correctness,
    /// Sustained parser throughput.
    Throughput,
    /// Complete supervised process-tree memory.
    ProcessTreeRss,
    /// Cold parser-host startup.
    Startup,
    /// Packaged parser-host bytes.
    PackageSize,
    /// Parser ABI compatibility.
    AbiCompatibility,
    /// Process and capability containment.
    Containment,
    /// Packaged release-platform portability.
    ReleasePortability,
}

/// Optional-semantic decision dimensions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum SemanticDimension {
    /// Mean reciprocal rank.
    Mrr,
    /// Normalized discounted cumulative gain at ten.
    #[serde(rename = "ndcg-at-10")]
    NdcgAt10,
    /// Recall at ten.
    #[serde(rename = "recall-at-10")]
    RecallAt10,
    /// Retrieval precision.
    Precision,
    /// Deterministic model-input identity.
    DeterministicModelInputs,
    /// Declared vector equality tolerance.
    VectorTolerance,
    /// License compatibility.
    Licensing,
    /// Packaged semantic bytes.
    PackageSize,
    /// Packaged release-platform support.
    PlatformSupport,
    /// Changed-row index update cost.
    UpdateCost,
    /// Complete supervised process-tree memory.
    ProcessTreeRss,
    /// Median query latency.
    #[serde(rename = "p50-latency")]
    P50Latency,
    /// Ninety-fifth percentile query latency.
    #[serde(rename = "p95-latency")]
    P95Latency,
}

/// Exact native grammar package and fixture binding.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeGrammarPackage {
    /// Cargo package name.
    name: String,
    /// Cargo package version.
    version: String,
    /// Registry archive checksum.
    checksum_sha256: String,
    /// Specialized `ProjectAtlas` language identifier.
    language: String,
    /// Nonempty repository fixture exercised by the candidate.
    fixture: String,
}

/// Canonical WebAssembly runtime package candidate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WasmRuntimePackage {
    /// Cargo package name.
    name: String,
    /// Cargo package version.
    version: String,
    /// Required Cargo feature.
    feature: String,
    /// Registry archive checksum.
    checksum_sha256: String,
}

/// Pinned semantic candidate source.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticCandidate {
    /// Stable candidate identity.
    candidate_id: String,
    /// Candidate ownership family.
    candidate_kind: CandidateKind,
    /// Current readiness state.
    readiness: CandidateState,
    /// Cargo package name.
    #[serde(rename = "crate")]
    crate_name: String,
    /// Cargo package version.
    version: String,
    /// Declared source license expression.
    license: String,
    /// Registry archive checksum.
    crate_sha256: String,
    /// Registry archive bytes.
    crate_bytes: u64,
}

/// Closed candidate-readiness blocker identities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum ReadinessBlockerCode {
    /// Versioned WebAssembly grammar manifest is absent.
    WasmPackManifestMissing,
    /// Parser candidate runner is not executable yet.
    ParserCandidateRunnerNotExecutable,
    /// Parser portability evidence is absent.
    HostedPortabilityEvidenceMissing,
    /// Labeled semantic retrieval corpus is absent.
    LabeledRetrievalCorpusMissing,
    /// Model-input and vector-tolerance contracts are absent.
    ModelInputAndVectorContractsMissing,
    /// Semantic candidate runner is not executable yet.
    SemanticCandidateRunnerNotExecutable,
    /// Hosted semantic resource evidence is absent.
    HostedResourceEvidenceMissing,
}

/// One fail-closed blocker and its later `OpenSpec` owners.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadinessBlocker {
    /// Stable blocker code.
    code: ReadinessBlockerCode,
    /// Tasks that must remove the blocker.
    owner_tasks: Vec<String>,
}

/// Minimal typed evaluation manifest view.
#[derive(Debug, Deserialize)]
struct EvaluationManifest {
    /// Registered profiles.
    profiles: Vec<EvaluationProfile>,
    /// Registered operations.
    operations: Vec<EvaluationOperation>,
    /// Exact candidate result schemas.
    result_schema: EvaluationResultSchema,
    /// Retained measurements, empty before campaigns run.
    measurements: Vec<Value>,
}

/// Candidate result metrics registered by the evaluation manifest.
#[derive(Debug, Deserialize)]
struct EvaluationResultSchema {
    /// Parser-host result metric fields.
    parser_host_result_metrics: Vec<String>,
    /// Semantic result metric fields.
    semantic_result_metrics: Vec<String>,
}

/// One evaluation profile identity.
#[derive(Debug, Deserialize)]
struct EvaluationProfile {
    /// Stable profile identifier.
    id: String,
}

/// One registered candidate operation.
#[derive(Debug, Deserialize)]
struct EvaluationOperation {
    /// Stable operation identifier.
    id: String,
    /// Required profile identifier.
    profile: String,
    /// Result schema identifier.
    result_schema: String,
}

/// Minimal typed Cargo lock view.
#[derive(Debug, Deserialize)]
struct CargoLock {
    /// Locked package rows.
    package: Vec<LockedPackage>,
}

/// One locked Cargo package.
#[derive(Debug, Deserialize)]
struct LockedPackage {
    /// Package name.
    name: String,
    /// Package version.
    version: String,
    /// Registry checksum when the package came from a registry.
    checksum: Option<String>,
}

/// Minimal symbols-crate manifest view.
#[derive(Debug, Deserialize)]
struct SymbolsCargoManifest {
    /// Direct dependencies whose names define the compiled grammar set.
    dependencies: BTreeMap<String, TomlValue>,
}

/// Load the typed candidate-readiness report.
fn readiness_report() -> Result<CandidateReadinessReport, Box<dyn Error>> {
    Ok(serde_json::from_slice(READINESS_BYTES)?)
}

/// Load the typed evaluation manifest.
fn evaluation_manifest() -> Result<EvaluationManifest, Box<dyn Error>> {
    Ok(serde_json::from_slice(EVALUATION_MANIFEST_BYTES)?)
}

/// Load the typed Cargo lockfile.
fn cargo_lock() -> Result<CargoLock, Box<dyn Error>> {
    Ok(toml::from_str(CARGO_LOCK)?)
}

/// Load the symbols-crate dependency manifest.
fn symbols_manifest() -> Result<SymbolsCargoManifest, Box<dyn Error>> {
    Ok(toml::from_str(SYMBOLS_CARGO_MANIFEST)?)
}

/// Load the language fixture baseline registry.
fn fixture_baselines() -> Result<Value, Box<dyn Error>> {
    let normalized = FIXTURE_BASELINES.replace("\r\n", "\n").replace('\r', "\n");
    toon_format::decode_default(&normalized).map_err(|error| {
        io::Error::other(format!("fixture baseline decode failed: {error}")).into()
    })
}

/// Convert a string sequence into a uniqueness-enforcing set.
fn exact_set(values: &[String]) -> Result<BTreeSet<&str>, Box<dyn Error>> {
    let set = values.iter().map(String::as_str).collect::<BTreeSet<_>>();
    require(set.len() == values.len(), "duplicate value in closed set")?;
    Ok(set)
}

/// Return every authoritative ARRI task identity without duplicating task totals.
fn authoritative_task_ids() -> BTreeSet<String> {
    INTELLIGENCE_TASKS
        .lines()
        .filter_map(|line| {
            line.strip_prefix("- [ ] ")
                .or_else(|| line.strip_prefix("- [x] "))
                .or_else(|| line.strip_prefix("- [X] "))
        })
        .filter_map(|line| line.split_once(' ').map(|(task_id, _description)| task_id))
        .map(|task_id| format!("ARRI-{task_id}"))
        .collect()
}

/// Validate one candidate's operation, profile, and result-schema binding.
fn require_operation_binding(
    manifest: &EvaluationManifest,
    operation_id: &str,
    profile: &str,
    result_schema: CandidateResultSchema,
) -> Result<(), Box<dyn Error>> {
    require(
        manifest.profiles.iter().any(|row| row.id == profile),
        format!("candidate profile {profile} is not registered"),
    )?;
    require(
        manifest.operations.iter().any(|row| {
            row.id == operation_id
                && row.profile == profile
                && row.result_schema == result_schema.as_str()
        }),
        format!("candidate operation {operation_id} is not bound to its profile and schema"),
    )
}

/// Validate exact, unique dimension-to-metric ownership and schema coverage.
fn validate_dimension_metrics<D>(
    required_dimensions: &[D],
    dimension_metrics: &BTreeMap<D, Vec<String>>,
    expected: &BTreeMap<D, BTreeSet<&str>>,
    schema_metrics: &[String],
    label: &str,
) -> Result<(), Box<dyn Error>>
where
    D: Copy + Ord,
{
    let required = required_dimensions.iter().copied().collect::<BTreeSet<_>>();
    require(
        required.len() == required_dimensions.len()
            && required == expected.keys().copied().collect()
            && required == dimension_metrics.keys().copied().collect(),
        format!("{label} required dimensions drifted"),
    )?;
    let mut mapped_metrics = BTreeSet::new();
    for (dimension, expected_metrics) in expected {
        let metrics = dimension_metrics
            .get(dimension)
            .ok_or_else(|| io::Error::other(format!("{label} dimension has no metric mapping")))?;
        let observed = exact_set(metrics)?;
        require(
            observed == *expected_metrics && !observed.is_empty(),
            format!("{label} dimension metric mapping drifted"),
        )?;
        for metric in observed {
            require(
                mapped_metrics.insert(metric),
                format!("{label} metric is owned by multiple dimensions"),
            )?;
        }
    }
    require(
        exact_set(schema_metrics)? == mapped_metrics,
        format!("{label} result schema is incomplete or contains an unmapped metric"),
    )
}

/// Validate exact blocker identities, owners, and authoritative task references.
fn validate_blockers(
    blockers: &[ReadinessBlocker],
    expected: &BTreeMap<ReadinessBlockerCode, BTreeSet<&str>>,
) -> Result<(), Box<dyn Error>> {
    let task_ids = authoritative_task_ids();
    let mut observed = BTreeMap::new();
    for blocker in blockers {
        let owners = exact_set(&blocker.owner_tasks)?;
        require(
            !owners.is_empty() && owners.iter().all(|owner| task_ids.contains(*owner)),
            "candidate blocker has an empty or unknown task owner",
        )?;
        require(
            observed.insert(blocker.code, owners).is_none(),
            "candidate blocker code is duplicated",
        )?;
    }
    require(observed == *expected, "candidate blocker ownership drifted")
}

/// Return whether a string is a lowercase SHA-256 identifier.
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate that a fixture is registered as non-vacuous tree-sitter evidence.
fn validate_fixture_registration(
    package: &NativeGrammarPackage,
    baselines: &Value,
) -> Result<(), Box<dyn Error>> {
    let relative = package
        .fixture
        .strip_prefix("fixtures/languages/")
        .ok_or_else(|| io::Error::other("grammar fixture is outside fixtures/languages"))?;
    let rows = baselines
        .get("summaries")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("fixture baseline has no summaries"))?;
    require(
        rows.iter().any(|row| {
            row.get("path").and_then(Value::as_str) == Some(relative)
                && row.get("language").and_then(Value::as_str) == Some(package.language.as_str())
                && row.get("parser_kind").and_then(Value::as_str)
                    == Some("tree-sitter-symbol-graph")
                && row.get("status").and_then(Value::as_str) == Some("ok")
                && row
                    .get("min_symbols")
                    .and_then(Value::as_u64)
                    .is_some_and(|value| value > 0)
        }),
        format!(
            "grammar fixture {} lacks a non-vacuous baseline",
            package.fixture
        ),
    )
}

/// Validate that native grammar rows exactly match compiled dependencies and real fixtures.
fn validate_native_grammar_packages(
    packages: &[NativeGrammarPackage],
    cargo_lock: &CargoLock,
    symbols_manifest: &SymbolsCargoManifest,
    baselines: &Value,
) -> Result<(), Box<dyn Error>> {
    let compiled = symbols_manifest
        .dependencies
        .keys()
        .filter(|name| name.starts_with("tree-sitter-"))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let package_names = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    require(
        package_names.len() == packages.len() && package_names == compiled,
        "native grammar candidate does not exactly match compiled grammar dependencies",
    )?;
    let mut languages = BTreeSet::new();
    let mut fixtures = BTreeSet::new();
    for package in packages {
        require(
            languages.insert(package.language.as_str())
                && fixtures.insert(package.fixture.as_str())
                && is_sha256(&package.checksum_sha256),
            format!(
                "native grammar package {} has duplicate or invalid inputs",
                package.name
            ),
        )?;
        require(
            has_exact_locked_package(
                cargo_lock,
                &package.name,
                &package.version,
                &package.checksum_sha256,
            ),
            format!(
                "native grammar package {} is not exactly lock-bound",
                package.name
            ),
        )?;
        let fixture_path = repository_path(&package.fixture);
        let source = fs::read_to_string(&fixture_path)?;
        let graph = projectatlas_symbols::extract_symbol_graph(
            &package.fixture,
            Some(&package.language),
            &source,
        );
        require(
            !source.trim().is_empty()
                && projectatlas_symbols::has_specialized_parser(&package.language)
                && graph.parser == ParserKind::TreeSitter
                && !graph.symbols.is_empty(),
            format!(
                "native grammar fixture {} is not non-vacuous tree-sitter evidence",
                package.fixture
            ),
        )?;
        validate_fixture_registration(package, baselines)?;
    }
    Ok(())
}

/// Return whether exactly one lockfile package matches the pinned identity.
fn has_exact_locked_package(
    cargo_lock: &CargoLock,
    name: &str,
    version: &str,
    checksum_sha256: &str,
) -> bool {
    let mut matching = cargo_lock.package.iter().filter(|row| row.name == name);
    let Some(package) = matching.next() else {
        return false;
    };
    let identity_matches =
        package.version == version && package.checksum.as_deref() == Some(checksum_sha256);
    identity_matches && matching.next().is_none()
}

/// Validate the common no-selection claim state.
fn validate_common_state(
    report: &CandidateReadinessReport,
    manifest: &EvaluationManifest,
) -> Result<(), Box<dyn Error>> {
    require(
        report.schema_version == 1
            && report.artifact_kind == "projectatlas.optional-pack-candidate-readiness"
            && report.claim_status == ClaimStatus::PreregisteredNotReady
            && report.selection_rule
                == "No parser host, ANN index, or local model is selected until every required input and measurement is retained and eligible."
            && manifest.measurements.is_empty(),
        "candidate report became selection-eligible or its closed identity drifted",
    )
}

/// Validate the complete parser-host readiness contract.
fn validate_parser_readiness(
    report: &CandidateReadinessReport,
    manifest: &EvaluationManifest,
    cargo_lock: &CargoLock,
    symbols_manifest: &SymbolsCargoManifest,
    baselines: &Value,
) -> Result<(), Box<dyn Error>> {
    let parser = &report.parser_host;
    require(
        parser.owner_task == "ARRI-2.14"
            && parser.selection_state == SelectionState::Blocked
            && parser.selected_candidate.is_none(),
        "parser host was selected before evidence was eligible",
    )?;
    let expected_metrics = BTreeMap::from([
        (
            ParserDimension::Correctness,
            BTreeSet::from(["error_bytes", "modes_attempted", "modes_passed"]),
        ),
        (
            ParserDimension::Throughput,
            BTreeSet::from(["throughput_bytes_per_second"]),
        ),
        (
            ParserDimension::ProcessTreeRss,
            BTreeSet::from(["process_tree_peak_rss_bytes"]),
        ),
        (ParserDimension::Startup, BTreeSet::from(["startup_ns"])),
        (
            ParserDimension::PackageSize,
            BTreeSet::from(["package_bytes"]),
        ),
        (
            ParserDimension::AbiCompatibility,
            BTreeSet::from(["abi_ok"]),
        ),
        (
            ParserDimension::Containment,
            BTreeSet::from(["containment_ok"]),
        ),
        (
            ParserDimension::ReleasePortability,
            BTreeSet::from(["platform_ok"]),
        ),
    ]);
    validate_dimension_metrics(
        &parser.required_dimensions,
        &parser.dimension_metrics,
        &expected_metrics,
        &manifest.result_schema.parser_host_result_metrics,
        "parser-host",
    )?;

    let mut native_candidates = 0_usize;
    let mut wasm_candidates = 0_usize;
    for candidate in &parser.candidates {
        match candidate {
            ParserCandidate::TrustedNativeGrammarCrates {
                candidate_id,
                candidate_kind,
                readiness,
                runner_operation_id,
                result_schema,
                profile,
                fixture_root,
                packages,
                missing_evidence,
            } => {
                native_candidates += 1;
                require(
                    candidate_id == "native-tree-sitter-compiled-grammars"
                        && *candidate_kind == CandidateKind::ParserHost
                        && *readiness == CandidateState::InputReadyMeasurementPending
                        && *result_schema == CandidateResultSchema::ParserHostResult
                        && fixture_root == "fixtures/languages",
                    "native parser candidate identity drifted",
                )?;
                require_operation_binding(manifest, runner_operation_id, profile, *result_schema)?;
                validate_native_grammar_packages(
                    packages,
                    cargo_lock,
                    symbols_manifest,
                    baselines,
                )?;
                require(
                    exact_set(missing_evidence)?
                        == BTreeSet::from([
                            "complete-process-tree-rss",
                            "packaged-platform-runs",
                            "paired-measurements",
                        ]),
                    "native parser blockers are incomplete",
                )?;
            }
            ParserCandidate::VersionedWasmGrammarPack {
                candidate_id,
                candidate_kind,
                readiness,
                runner_operation_id,
                result_schema,
                profile,
                runtime_package,
                grammar_pack_manifest,
                missing_evidence,
            } => {
                wasm_candidates += 1;
                require(
                    candidate_id == "tree-sitter-wasm-grammar-pack"
                        && *candidate_kind == CandidateKind::ParserHost
                        && *readiness == CandidateState::BlockedMissingPackArtifact
                        && *result_schema == CandidateResultSchema::ParserHostResult
                        && grammar_pack_manifest.is_none(),
                    "WebAssembly parser candidate became ready without a pack manifest",
                )?;
                require_operation_binding(manifest, runner_operation_id, profile, *result_schema)?;
                require(
                    runtime_package.name == "tree-sitter"
                        && runtime_package.feature == "wasm"
                        && is_sha256(&runtime_package.checksum_sha256)
                        && has_exact_locked_package(
                            cargo_lock,
                            &runtime_package.name,
                            &runtime_package.version,
                            &runtime_package.checksum_sha256,
                        ),
                    "WebAssembly runtime candidate is not exactly lock-bound",
                )?;
                require(
                    exact_set(missing_evidence)?
                        == BTreeSet::from([
                            "artifact-digests",
                            "complete-process-tree-rss",
                            "packaged-platform-runs",
                            "paired-measurements",
                            "versioned-pack-manifest",
                        ]),
                    "WebAssembly parser blockers are incomplete",
                )?;
            }
        }
    }
    require(
        parser.candidates.len() == 2 && native_candidates == 1 && wasm_candidates == 1,
        "parser readiness must contain exactly one native and one WebAssembly candidate",
    )?;
    validate_blockers(
        &parser.blockers,
        &BTreeMap::from([
            (
                ReadinessBlockerCode::WasmPackManifestMissing,
                BTreeSet::from(["ARRI-5.1", "ARRI-5.7", "ARRI-5.16"]),
            ),
            (
                ReadinessBlockerCode::ParserCandidateRunnerNotExecutable,
                BTreeSet::from(["ARRI-5.8", "ARRI-11.5"]),
            ),
            (
                ReadinessBlockerCode::HostedPortabilityEvidenceMissing,
                BTreeSet::from(["ARRI-5.14", "ARRI-11.5", "ARRI-11.23"]),
            ),
        ]),
    )
}

/// Validate the complete optional-semantic readiness contract.
fn validate_semantic_readiness(
    report: &CandidateReadinessReport,
    manifest: &EvaluationManifest,
) -> Result<(), Box<dyn Error>> {
    let semantic = &report.semantic_pack;
    require(
        semantic.owner_task == "ARRI-2.15"
            && semantic.selection_state == SelectionState::BlockedPendingEvaluation
            && semantic.selected_ann_candidate.is_none()
            && semantic.selected_model_candidate.is_none(),
        "semantic backend was selected before labeled evidence",
    )?;
    let expected_metrics = BTreeMap::from([
        (SemanticDimension::Mrr, BTreeSet::from(["mrr"])),
        (SemanticDimension::NdcgAt10, BTreeSet::from(["ndcg_at_10"])),
        (
            SemanticDimension::RecallAt10,
            BTreeSet::from(["recall_at_10"]),
        ),
        (SemanticDimension::Precision, BTreeSet::from(["precision"])),
        (
            SemanticDimension::DeterministicModelInputs,
            BTreeSet::from(["deterministic_inputs"]),
        ),
        (
            SemanticDimension::VectorTolerance,
            BTreeSet::from(["vector_tolerance_ok"]),
        ),
        (SemanticDimension::Licensing, BTreeSet::from(["license_ok"])),
        (
            SemanticDimension::PackageSize,
            BTreeSet::from(["package_bytes"]),
        ),
        (
            SemanticDimension::PlatformSupport,
            BTreeSet::from(["platform_ok"]),
        ),
        (SemanticDimension::UpdateCost, BTreeSet::from(["update_ns"])),
        (
            SemanticDimension::ProcessTreeRss,
            BTreeSet::from(["process_tree_peak_rss_bytes"]),
        ),
        (SemanticDimension::P50Latency, BTreeSet::from(["p50_ns"])),
        (SemanticDimension::P95Latency, BTreeSet::from(["p95_ns"])),
    ]);
    validate_dimension_metrics(
        &semantic.required_dimensions,
        &semantic.dimension_metrics,
        &expected_metrics,
        &manifest.result_schema.semantic_result_metrics,
        "optional-semantic",
    )?;
    let observed_candidates = semantic
        .candidate_shortlist
        .iter()
        .map(|candidate| {
            (
                candidate.candidate_id.as_str(),
                candidate.candidate_kind,
                candidate.readiness,
                candidate.crate_name.as_str(),
                candidate.version.as_str(),
                candidate.license.as_str(),
                candidate.crate_sha256.as_str(),
                candidate.crate_bytes,
            )
        })
        .collect::<BTreeSet<_>>();
    let expected_candidates = BTreeSet::from([
        (
            "hnsw-rs-0.3.4",
            CandidateKind::AnnIndex,
            CandidateState::NotMaterialized,
            "hnsw_rs",
            "0.3.4",
            "MIT/Apache-2.0",
            "43a5258f079b97bf2e8311ff9579e903c899dcbac0d9a138d62e9a066778bd07",
            72_119,
        ),
        (
            "instant-distance-0.6.1",
            CandidateKind::AnnIndex,
            CandidateState::NotMaterialized,
            "instant-distance",
            "0.6.1",
            "MIT OR Apache-2.0",
            "8c619cdaa30bb84088963968bee12a45ea5fbbf355f2c021bcd15589f5ca494a",
            15_734,
        ),
        (
            "candle-transformers-0.11.0",
            CandidateKind::LocalModelRuntime,
            CandidateState::NotMaterialized,
            "candle-transformers",
            "0.11.0",
            "MIT OR Apache-2.0",
            "3bcbbf7ff00ff6fe2af22b93600195917fe90e90ff48424a140d1a926c44b1c1",
            515_399,
        ),
        (
            "fastembed-5.17.2",
            CandidateKind::LocalModelRuntime,
            CandidateState::NotMaterialized,
            "fastembed",
            "5.17.2",
            "Apache-2.0",
            "545e4fb17fc48768ff36c2a3854aa5b0b809d0ed595ab5530fa8ac94f31bd0ea",
            448_141,
        ),
    ]);
    require(
        observed_candidates.len() == semantic.candidate_shortlist.len()
            && observed_candidates == expected_candidates
            && observed_candidates
                .iter()
                .all(|candidate| is_sha256(candidate.6) && candidate.7 > 0),
        "semantic candidate shortlist or pinned source evidence drifted",
    )?;
    require(
        semantic.labeled_retrieval_corpus.is_none()
            && semantic.model_input_contract.is_none()
            && semantic.vector_tolerance_contract.is_none(),
        "semantic input contracts changed without readiness reconciliation",
    )?;
    require(
        semantic.result_schema == CandidateResultSchema::SemanticResult
            && semantic.default_core_fallback == "structural-and-lexical-remain-authoritative",
        "optional semantic readiness weakened the default core",
    )?;
    require_operation_binding(
        manifest,
        &semantic.runner_operation_id,
        &semantic.profile,
        semantic.result_schema,
    )?;
    validate_blockers(
        &semantic.blockers,
        &BTreeMap::from([
            (
                ReadinessBlockerCode::LabeledRetrievalCorpusMissing,
                BTreeSet::from(["ARRI-10.7", "ARRI-10.9"]),
            ),
            (
                ReadinessBlockerCode::ModelInputAndVectorContractsMissing,
                BTreeSet::from(["ARRI-10.5", "ARRI-10.6"]),
            ),
            (
                ReadinessBlockerCode::SemanticCandidateRunnerNotExecutable,
                BTreeSet::from(["ARRI-10.4", "ARRI-10.9", "ARRI-11.5"]),
            ),
            (
                ReadinessBlockerCode::HostedResourceEvidenceMissing,
                BTreeSet::from(["ARRI-10.9", "ARRI-11.5", "ARRI-11.18", "ARRI-11.23"]),
            ),
        ]),
    )
}

/// Return one mutable JSON object at a pointer for adversarial mutation.
fn object_at_mut<'a>(
    value: &'a mut Value,
    pointer: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, Box<dyn Error>> {
    let selected = if pointer.is_empty() {
        value
    } else {
        value
            .pointer_mut(pointer)
            .ok_or_else(|| io::Error::other(format!("missing mutation pointer {pointer}")))?
    };
    selected.as_object_mut().ok_or_else(|| {
        io::Error::other(format!("mutation pointer {pointer} is not an object")).into()
    })
}

/// Fail a focused contract test with a readable diagnostic.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// ARRI-2.14: parser-host selection remains blocked until real candidate evidence exists.
#[test]
fn arri_2_14_parser_host_candidates_fail_closed_until_ready() -> Result<(), Box<dyn Error>> {
    let report = readiness_report()?;
    let manifest = evaluation_manifest()?;
    validate_common_state(&report, &manifest)?;
    validate_parser_readiness(
        &report,
        &manifest,
        &cargo_lock()?,
        &symbols_manifest()?,
        &fixture_baselines()?,
    )
}

/// ARRI-2.15: semantic selection stays blocked until labeled evidence exists.
#[test]
fn arri_2_15_semantic_candidates_fail_closed_until_labeled() -> Result<(), Box<dyn Error>> {
    let report = readiness_report()?;
    let manifest = evaluation_manifest()?;
    validate_common_state(&report, &manifest)?;
    validate_semantic_readiness(&report, &manifest)
}

/// Readiness decoding rejects unknown fields at every owned schema boundary.
#[test]
fn readiness_schema_rejects_unknown_fields() -> Result<(), Box<dyn Error>> {
    for pointer in [
        "",
        "/parser_host",
        "/semantic_pack",
        "/parser_host/candidates/0",
        "/parser_host/candidates/0/packages/0",
        "/parser_host/candidates/1",
        "/parser_host/candidates/1/runtime_package",
        "/parser_host/blockers/0",
        "/semantic_pack/candidate_shortlist/0",
        "/semantic_pack/blockers/0",
    ] {
        let mut value: Value = serde_json::from_slice(READINESS_BYTES)?;
        object_at_mut(&mut value, pointer)?.insert("unexpected".into(), Value::Bool(true));
        require(
            serde_json::from_value::<CandidateReadinessReport>(value).is_err(),
            format!("unknown field was accepted at {pointer}"),
        )?;
    }
    Ok(())
}

/// Exact inventories, metric mappings, and blocker owners reject omissions or substitution.
#[test]
fn readiness_contract_rejects_adversarial_omissions() -> Result<(), Box<dyn Error>> {
    let cargo_lock = cargo_lock()?;
    let symbols_manifest = symbols_manifest()?;
    let baselines = fixture_baselines()?;

    let mut missing_python: Value = serde_json::from_slice(READINESS_BYTES)?;
    let packages = missing_python
        .pointer_mut("/parser_host/candidates/0/packages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("native package array is missing"))?;
    packages.retain(|package| {
        package.get("name").and_then(Value::as_str) != Some("tree-sitter-python")
    });
    let report: CandidateReadinessReport = serde_json::from_value(missing_python)?;
    require(
        validate_parser_readiness(
            &report,
            &evaluation_manifest()?,
            &cargo_lock,
            &symbols_manifest,
            &baselines,
        )
        .is_err(),
        "missing compiled Python grammar was accepted",
    )?;

    let mut parser_schema: Value = serde_json::from_slice(EVALUATION_MANIFEST_BYTES)?;
    parser_schema
        .pointer_mut("/result_schema/parser_host_result_metrics")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("parser result schema is missing"))?
        .retain(|metric| metric.as_str() != Some("containment_ok"));
    let manifest: EvaluationManifest = serde_json::from_value(parser_schema)?;
    require(
        validate_parser_readiness(
            &readiness_report()?,
            &manifest,
            &cargo_lock,
            &symbols_manifest,
            &baselines,
        )
        .is_err(),
        "incomplete parser result schema was accepted",
    )?;

    let mut semantic_candidate: Value = serde_json::from_slice(READINESS_BYTES)?;
    let removed_candidate = semantic_candidate
        .pointer_mut("/semantic_pack/candidate_shortlist")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("semantic candidate shortlist is missing"))?
        .pop();
    require(
        removed_candidate.is_some(),
        "semantic candidate shortlist cannot be empty",
    )?;
    let report: CandidateReadinessReport = serde_json::from_value(semantic_candidate)?;
    require(
        validate_semantic_readiness(&report, &evaluation_manifest()?).is_err(),
        "incomplete semantic candidate shortlist was accepted",
    )?;

    let mut semantic_schema: Value = serde_json::from_slice(EVALUATION_MANIFEST_BYTES)?;
    semantic_schema
        .pointer_mut("/result_schema/semantic_result_metrics")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("semantic result schema is missing"))?
        .retain(|metric| metric.as_str() != Some("vector_tolerance_ok"));
    let manifest: EvaluationManifest = serde_json::from_value(semantic_schema)?;
    require(
        validate_semantic_readiness(&readiness_report()?, &manifest).is_err(),
        "incomplete semantic result schema was accepted",
    )?;

    let mut blocker_owner: Value = serde_json::from_slice(READINESS_BYTES)?;
    blocker_owner["parser_host"]["blockers"][0]["owner_tasks"] = serde_json::json!(["ARRI-11.17"]);
    let report: CandidateReadinessReport = serde_json::from_value(blocker_owner)?;
    require(
        validate_parser_readiness(
            &report,
            &evaluation_manifest()?,
            &cargo_lock,
            &symbols_manifest,
            &baselines,
        )
        .is_err(),
        "incorrect blocker ownership was accepted",
    )
}
