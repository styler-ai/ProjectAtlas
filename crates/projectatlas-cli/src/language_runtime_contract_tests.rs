//! Independent historical language-runtime migration baseline.

use crate::language_capability_settings::{
    CURRENT_LANGUAGE_POLICY, language_policy_for_public_mode,
};
use crate::runtime::is_symbol_candidate;
use crate::structural::{is_structural_summary_candidate, structural_summary_for_path};
use projectatlas_core::language::{
    BROAD_SOURCE_EXTENSIONS, LANGUAGE_SPECS, LanguageParserSupport, detect_language,
    detect_language_for_path, language_spec,
};
use projectatlas_core::normalized_extension;
use projectatlas_core::symbols::{ParserKind, SymbolKind};
use projectatlas_symbols::{extract_symbol_graph, has_specialized_parser, specialized_languages};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Debug;
use std::io;
use std::path::Path;

const LANGUAGE_RUNTIME_CONTRACT: &str =
    include_str!("../../../fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon");
const BASELINE_RELEASE: &str = "v0.3.26";
const BASELINE_COMMIT: &str = "d3b3e157f954c7d360d821ed0385762e8b044385";
const BASELINE_RAW_SHA256: &str =
    "fb9c35414a3fd478de9bf0d1653aac9cca8c1558d3a76947a94d62cbca5038e0";
const CARGO_ROUTING_CORRECTION_IDS: &[&str] = &["near-miss-cargo-toml", "near-miss-cargo-lock"];
const AUGMENTER_ROUTE_IDENTITIES: &[(&str, SymbolAdapterCase, AugmenterCase, usize)] = &[
    (
        "kotlin",
        SymbolAdapterCase::TreeSitter,
        AugmenterCase::Kotlin,
        0,
    ),
    (
        "kotlin",
        SymbolAdapterCase::TreeSitter,
        AugmenterCase::GradleKotlin,
        1,
    ),
    (
        "objective-c",
        SymbolAdapterCase::TreeSitter,
        AugmenterCase::ObjectiveC,
        0,
    ),
    ("zig", SymbolAdapterCase::TreeSitter, AugmenterCase::Zig, 0),
    (
        "groovy",
        SymbolAdapterCase::Fallback,
        AugmenterCase::GradleGroovy,
        0,
    ),
];

/// Typed historical contract that remains independent from registry generation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageRuntimeContract {
    /// Fixture schema version.
    schema_version: u32,
    /// `ProjectAtlas` release whose behavior is frozen.
    baseline_release: String,
    /// Exact `ProjectAtlas` commit whose behavior is frozen.
    baseline_commit: String,
    /// Ordered public scanner extensions and detected modes.
    broad_detection: Vec<DetectionCase>,
    /// Detection API extensions intentionally absent from the scanner list.
    api_only_detection: Vec<DetectionCase>,
    /// Case-sensitive exact-filename rules and precedence witnesses.
    exact_filenames: Vec<ExactFilenameCase>,
    /// Unsupported and case-sensitive fallback witnesses.
    negative_detection: Vec<PathDetectionCase>,
    /// Compound and ordinary extension normalization witnesses.
    extension_normalization: Vec<ExtensionNormalizationCase>,
    /// Reviewed corrections to historical Cargo suffix-routing behavior.
    cargo_routing_corrections: Vec<CargoRoutingCorrection>,
    /// Ordered public modes and their achieved summary/symbol pipeline.
    language_pipelines: Vec<LanguagePipelineCase>,
    /// Ordered post-parser and fallback augmentation routes.
    augmenter_routes: Vec<AugmenterRouteCase>,
    /// Ordered built-in parser witnesses.
    specialized_parsers: Vec<SpecializedParserCase>,
    /// Complete path-class by supplied-mode adapter-precedence cross-product.
    adapter_precedence: [AdapterPrecedenceCase; 7],
}

/// One extension-to-language expectation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectionCase {
    /// Extension passed to the public detection API.
    extension: String,
    /// Expected public mode.
    language: String,
}

/// One exact-filename precedence expectation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactFilenameCase {
    /// Case-sensitive basename.
    file_name: String,
    /// Deliberately conflicting extension used to prove filename precedence.
    conflicting_extension: String,
    /// Expected public mode.
    language: String,
}

/// One path/extension detection expectation; empty strings represent absence.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathDetectionCase {
    /// Repository path supplied to detection.
    path: String,
    /// Extension supplied to detection, or empty for no extension.
    extension: String,
    /// Expected public mode, or empty for no detection.
    language: String,
}

/// One normalized-extension expectation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionNormalizationCase {
    /// Native path supplied to the extension helper.
    path: String,
    /// Expected normalized extension, or empty for none.
    extension: String,
}

/// Review disposition for a historical behavior difference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum CompatibilityDisposition {
    /// Historical behavior is deliberately corrected rather than preserved.
    IntentionalCorrection,
}

/// Achieved public parser-support category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SupportCase {
    /// Built-in tree-sitter extraction.
    Native,
    /// Cargo manifest extraction.
    Manifest,
    /// Structural summary or symbol extraction.
    Structural,
    /// Conservative fallback extraction.
    Fallback,
}

/// Achieved summary adapter selected for a public mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SummaryAdapterCase {
    /// No structural summary adapter.
    None,
    /// Markdown summary adapter.
    Markdown,
    /// JSON/JSONC summary adapter.
    Json,
    /// YAML summary adapter.
    Yaml,
    /// TOML summary adapter.
    Toml,
    /// XML summary adapter.
    Xml,
    /// CSS summary adapter.
    Css,
    /// HTML summary adapter.
    Html,
    /// `PowerShell` summary adapter.
    Powershell,
    /// Configuration/plain-text summary adapter.
    ConfigText,
    /// TOON summary adapter.
    Toon,
}

/// Achieved symbol adapter selected for a public mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum SymbolAdapterCase {
    /// Symbol extraction is intentionally skipped.
    None,
    /// Built-in tree-sitter adapter.
    TreeSitter,
    /// Cargo manifest adapter.
    Manifest,
    /// Vue structural symbol adapter.
    VueStructural,
    /// `PowerShell` structural symbol adapter.
    PowershellStructural,
    /// Conservative fallback adapter.
    Fallback,
}

/// Closed historical language-specific augmentation step.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum AugmenterCase {
    /// Kotlin package, type, and method enrichment.
    Kotlin,
    /// Gradle Kotlin DSL task enrichment.
    GradleKotlin,
    /// Objective-C interface/implementation normalization.
    ObjectiveC,
    /// Zig binding and method enrichment.
    Zig,
    /// Gradle Groovy DSL task enrichment.
    GradleGroovy,
}

/// One ordered historical post-parser or fallback augmentation route.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AugmenterRouteCase {
    /// Public language mode that owns the route.
    language: String,
    /// Base symbol adapter that runs before augmentation.
    base_adapter: SymbolAdapterCase,
    /// Closed language-specific augmentation step.
    augmenter: AugmenterCase,
    /// Zero-based execution position within the language/base-adapter route.
    ordinal: usize,
}

/// One public mode and its achieved runtime pipeline.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguagePipelineCase {
    /// Public `ProjectAtlas` mode.
    language: String,
    /// Public parser-support category.
    support: SupportCase,
    /// Exact structural summary adapter reserved for generated-registry comparison.
    summary_adapter: SummaryAdapterCase,
    /// Exact symbol adapter reserved for generated-registry comparison.
    symbol_adapter: SymbolAdapterCase,
}

/// Expected declaration kind from one built-in parser witness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SymbolKindCase {
    /// Function declaration.
    Function,
    /// Class declaration.
    Class,
    /// Package declaration.
    Package,
    /// Dependency declaration.
    Dependency,
    /// Value declaration.
    Value,
}

/// One built-in parser selection and extraction witness.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializedParserCase {
    /// Public `ProjectAtlas` mode.
    language: String,
    /// Historical trusted parser-component identity.
    parser_component: String,
    /// Minimal valid source input.
    source: String,
    /// Expected declaration kind.
    symbol_kind: SymbolKindCase,
    /// Expected declaration name.
    symbol_name: String,
}

/// Expected graph parser family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ParserKindCase {
    /// Built-in tree-sitter graph.
    TreeSitter,
    /// Manifest graph.
    Manifest,
    /// Structural graph.
    Structural,
    /// Fallback graph.
    Fallback,
}

/// One reviewed correction to v0.3.26 Cargo suffix routing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoRoutingCorrection {
    /// Stable review identity.
    case_id: String,
    /// Near-miss path that v0.3.26 treated as a Cargo document.
    path: String,
    /// Supplied language mode, or empty for no mode.
    supplied_language: String,
    /// Whether v0.3.26 selected the path for symbol extraction.
    baseline_symbol_candidate: bool,
    /// Whether the accepted behavior selects the path for symbol extraction.
    accepted_symbol_candidate: bool,
    /// Parser family selected by v0.3.26.
    baseline_parser_kind: ParserKindCase,
    /// Parser family selected by the accepted behavior.
    accepted_parser_kind: ParserKindCase,
    /// Explicit review disposition for this compatibility difference.
    disposition: CompatibilityDisposition,
    /// Human-readable root-cause explanation retained with the evidence.
    rationale: String,
}

/// Frozen path class used by the adapter-precedence cross-product.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum AdapterPathClass {
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

/// Ordered path classes required by the adapter-precedence compatibility matrix.
const ADAPTER_PRECEDENCE_PATH_CLASSES: &[AdapterPathClass] = &[
    AdapterPathClass::CargoManifest,
    AdapterPathClass::CargoLock,
    AdapterPathClass::Vue,
    AdapterPathClass::Powershell,
    AdapterPathClass::Ordinary,
    AdapterPathClass::CargoManifestNearMiss,
    AdapterPathClass::CargoLockNearMiss,
];

/// Effective adapter selected by one frozen precedence cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum AdapterExpectation {
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

/// Supplied public mode axis for the frozen precedence matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuppliedMode {
    /// No explicit language.
    Absent,
    /// Cargo manifest mode.
    CargoManifest,
    /// Cargo lock mode.
    CargoLock,
    /// Vue mode.
    Vue,
    /// `PowerShell` mode.
    Powershell,
    /// Rust built-in mode.
    BuiltIn,
    /// Ruby fallback mode.
    Fallback,
    /// Unrecognized public mode.
    Unknown,
}

impl SuppliedMode {
    /// Complete supplied-mode axis in fixture-column order.
    const ALL: [Self; 8] = [
        Self::Absent,
        Self::CargoManifest,
        Self::CargoLock,
        Self::Vue,
        Self::Powershell,
        Self::BuiltIn,
        Self::Fallback,
        Self::Unknown,
    ];

    /// Public language value passed to extraction.
    const fn language(self) -> Option<&'static str> {
        match self {
            Self::Absent => None,
            Self::CargoManifest => Some("cargo-manifest"),
            Self::CargoLock => Some("cargo-lock"),
            Self::Vue => Some("vue"),
            Self::Powershell => Some("powershell"),
            Self::BuiltIn => Some("rust"),
            Self::Fallback => Some("ruby"),
            Self::Unknown => Some("unknown"),
        }
    }
}

/// One complete supplied-mode row in the adapter-precedence cross-product.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterPrecedenceCase {
    /// Closed path class.
    path_class: AdapterPathClass,
    /// Basename expanded across accepted repository path styles.
    path: String,
    /// Effective adapter without a supplied language.
    absent: AdapterExpectation,
    /// Effective adapter for supplied cargo-manifest.
    cargo_manifest: AdapterExpectation,
    /// Effective adapter for supplied cargo-lock.
    cargo_lock: AdapterExpectation,
    /// Effective adapter for supplied vue.
    vue: AdapterExpectation,
    /// Effective adapter for supplied powershell.
    powershell: AdapterExpectation,
    /// Effective adapter for supplied rust.
    built_in: AdapterExpectation,
    /// Effective adapter for supplied ruby.
    fallback: AdapterExpectation,
    /// Effective adapter for an unrecognized supplied mode.
    unknown: AdapterExpectation,
}

impl AdapterPrecedenceCase {
    /// Expected adapter for one supplied-mode column.
    const fn expectation(&self, mode: SuppliedMode) -> AdapterExpectation {
        match mode {
            SuppliedMode::Absent => self.absent,
            SuppliedMode::CargoManifest => self.cargo_manifest,
            SuppliedMode::CargoLock => self.cargo_lock,
            SuppliedMode::Vue => self.vue,
            SuppliedMode::Powershell => self.powershell,
            SuppliedMode::BuiltIn => self.built_in,
            SuppliedMode::Fallback => self.fallback,
            SuppliedMode::Unknown => self.unknown,
        }
    }
}

/// Decode and validate the immutable historical fixture.
fn language_runtime_contract() -> Result<LanguageRuntimeContract, Box<dyn Error>> {
    let raw_digest = format!("{:x}", Sha256::digest(LANGUAGE_RUNTIME_CONTRACT.as_bytes()));
    require_equal(
        &raw_digest.as_str(),
        &BASELINE_RAW_SHA256,
        "contract raw SHA-256",
    )?;
    let value = toon_format::decode_default(LANGUAGE_RUNTIME_CONTRACT).map_err(|error| {
        io::Error::other(format!("language contract TOON decode failed: {error}"))
    })?;
    let contract: LanguageRuntimeContract = serde_json::from_value(value)?;
    require_equal(&contract.schema_version, &3, "contract schema version")?;
    require_equal(
        &contract.baseline_release.as_str(),
        &BASELINE_RELEASE,
        "contract baseline release",
    )?;
    require_equal(
        &contract.baseline_commit.as_str(),
        &BASELINE_COMMIT,
        "contract baseline commit",
    )?;

    require_unique(
        contract
            .broad_detection
            .iter()
            .map(|row| row.extension.as_str()),
        "broad extension",
    )?;
    require_unique(
        contract
            .api_only_detection
            .iter()
            .map(|row| row.extension.as_str()),
        "API-only extension",
    )?;
    require_unique(
        contract
            .exact_filenames
            .iter()
            .map(|row| row.file_name.as_str()),
        "exact filename",
    )?;
    require_unique(
        contract
            .cargo_routing_corrections
            .iter()
            .map(|row| row.case_id.as_str()),
        "Cargo routing correction",
    )?;
    let correction_ids = contract
        .cargo_routing_corrections
        .iter()
        .map(|row| row.case_id.as_str())
        .collect::<Vec<_>>();
    require_equal(
        &correction_ids.as_slice(),
        &CARGO_ROUTING_CORRECTION_IDS,
        "reviewed Cargo routing correction identities",
    )?;
    require_unique(
        contract
            .language_pipelines
            .iter()
            .map(|row| row.language.as_str()),
        "language pipeline",
    )?;
    validate_augmenter_routes(&contract)?;
    require_unique(
        contract
            .specialized_parsers
            .iter()
            .map(|row| row.language.as_str()),
        "specialized parser",
    )?;
    for row in &contract.specialized_parsers {
        require(
            !row.parser_component.is_empty(),
            format!("empty parser component for {}", row.language),
        )?;
    }
    let path_classes = contract
        .adapter_precedence
        .iter()
        .map(|row| row.path_class)
        .collect::<Vec<_>>();
    require_equal(
        &path_classes.as_slice(),
        &ADAPTER_PRECEDENCE_PATH_CLASSES,
        "adapter-precedence path-class order",
    )?;
    require(
        contract
            .adapter_precedence
            .iter()
            .all(|row| !row.path.is_empty() && Path::new(&row.path).file_name().is_some()),
        "adapter-precedence path inventory contains an empty path",
    )?;
    Ok(contract)
}

/// Validate exact augmentation identities, base adapters, and contiguous order.
fn validate_augmenter_routes(contract: &LanguageRuntimeContract) -> Result<(), Box<dyn Error>> {
    let actual = contract
        .augmenter_routes
        .iter()
        .map(|row| {
            (
                row.language.as_str(),
                row.base_adapter,
                row.augmenter,
                row.ordinal,
            )
        })
        .collect::<Vec<_>>();
    require_equal(
        &actual.as_slice(),
        &AUGMENTER_ROUTE_IDENTITIES,
        "ordered historical augmenter routes",
    )?;

    let pipeline_adapters = contract
        .language_pipelines
        .iter()
        .map(|row| (row.language.as_str(), row.symbol_adapter))
        .collect::<BTreeMap<_, _>>();
    let mut rows = BTreeSet::new();
    let mut slots = BTreeSet::new();
    let mut ordinals = BTreeMap::<(&str, SymbolAdapterCase), Vec<usize>>::new();
    for row in &contract.augmenter_routes {
        require(
            pipeline_adapters.get(row.language.as_str()) == Some(&row.base_adapter),
            format!(
                "augmenter route {} does not match its language pipeline",
                row.language
            ),
        )?;
        require(
            rows.insert((
                row.language.as_str(),
                row.base_adapter,
                row.augmenter,
                row.ordinal,
            )),
            format!("duplicate augmenter route for {}", row.language),
        )?;
        require(
            slots.insert((row.language.as_str(), row.base_adapter, row.ordinal)),
            format!("duplicate augmenter ordinal for {}", row.language),
        )?;
        ordinals
            .entry((row.language.as_str(), row.base_adapter))
            .or_default()
            .push(row.ordinal);
    }
    for ((language, _), actual_ordinals) in ordinals {
        let expected = (0..actual_ordinals.len()).collect::<Vec<_>>();
        require_equal(
            &actual_ordinals,
            &expected,
            format!("contiguous augmenter ordinals for {language}"),
        )?;
    }
    Ok(())
}

/// Require unique non-empty values in one fixture identity column.
fn require_unique<'a>(
    values: impl IntoIterator<Item = &'a str>,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let mut unique = BTreeSet::new();
    for value in values {
        require(!value.is_empty(), format!("empty {label}"))?;
        require(unique.insert(value), format!("duplicate {label} {value}"))?;
    }
    Ok(())
}

/// Return a typed test error when a required condition is false.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Return a typed test error when two contract values differ.
fn require_equal<T: Debug + PartialEq>(
    actual: &T,
    expected: &T,
    context: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{}: expected {expected:?}, got {actual:?}",
            context.into()
        ))
        .into())
    }
}

/// Convert an empty fixture cell into an absent string.
fn optional_text(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

/// Convert public parser-support metadata to the historical test enum.
fn support_case(value: LanguageParserSupport) -> SupportCase {
    match value {
        LanguageParserSupport::Native => SupportCase::Native,
        LanguageParserSupport::Manifest => SupportCase::Manifest,
        LanguageParserSupport::Structural => SupportCase::Structural,
        LanguageParserSupport::Fallback => SupportCase::Fallback,
    }
}

/// Convert a fixture declaration kind to the public graph enum.
fn symbol_kind(value: SymbolKindCase) -> SymbolKind {
    match value {
        SymbolKindCase::Function => SymbolKind::Function,
        SymbolKindCase::Class => SymbolKind::Class,
        SymbolKindCase::Package => SymbolKind::Package,
        SymbolKindCase::Dependency => SymbolKind::Dependency,
        SymbolKindCase::Value => SymbolKind::Value,
    }
}

/// One adapter-specific source and symbol witness for the precedence matrix.
struct AdapterWitness {
    /// Source parsed by the selected adapter.
    source: &'static str,
    /// Expected public parser family.
    parser: ParserKind,
    /// Expected symbol kind.
    symbol_kind: SymbolKind,
    /// Expected symbol name.
    symbol_name: &'static str,
    /// Adapter-specific symbol detail when the parser family alone is ambiguous.
    symbol_detail: Option<&'static str>,
}

/// Return the frozen semantic witness for one effective adapter.
const fn adapter_witness(expectation: AdapterExpectation) -> AdapterWitness {
    match expectation {
        AdapterExpectation::CargoManifest => AdapterWitness {
            source: "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            parser: ParserKind::Manifest,
            symbol_kind: SymbolKind::Package,
            symbol_name: "fixture",
            symbol_detail: Some("cargo-package"),
        },
        AdapterExpectation::CargoLock => AdapterWitness {
            source: "[[package]]\nname = \"fixture-lock\"\nversion = \"1.0.0\"\n",
            parser: ParserKind::Manifest,
            symbol_kind: SymbolKind::Dependency,
            symbol_name: "fixture-lock",
            symbol_detail: Some("cargo-lock-package"),
        },
        AdapterExpectation::Vue => AdapterWitness {
            source: "<script setup>\nconst props = defineProps({ value: String })\n</script>\n",
            parser: ParserKind::Structural,
            symbol_kind: SymbolKind::Value,
            symbol_name: "props",
            symbol_detail: Some("vue-composition-binding"),
        },
        AdapterExpectation::Powershell => AdapterWitness {
            source: "function Invoke-Fixture { 'ok' }\n",
            parser: ParserKind::Structural,
            symbol_kind: SymbolKind::Function,
            symbol_name: "Invoke-Fixture",
            symbol_detail: Some("powershell-function"),
        },
        AdapterExpectation::BuiltIn => AdapterWitness {
            source: "fn run() {}\n",
            parser: ParserKind::TreeSitter,
            symbol_kind: SymbolKind::Function,
            symbol_name: "run",
            symbol_detail: None,
        },
        AdapterExpectation::Fallback => AdapterWitness {
            source: "def fallback_fixture\n  :ok\nend\n",
            parser: ParserKind::Fallback,
            symbol_kind: SymbolKind::Function,
            symbol_name: "fallback_fixture",
            symbol_detail: Some("fallback-python-function"),
        },
    }
}

/// Expand one portable basename across path shapes accepted by the frozen contract.
fn adapter_path_variants(basename: &str) -> [String; 5] {
    [
        basename.to_string(),
        format!("nested/{basename}"),
        format!("nested\\{basename}"),
        format!("dir.with.dot/{basename}"),
        format!("dir.with.dot\\{basename}"),
    ]
}

/// Convert a fixture parser family to the public graph enum.
fn parser_kind(value: ParserKindCase) -> ParserKind {
    match value {
        ParserKindCase::TreeSitter => ParserKind::TreeSitter,
        ParserKindCase::Manifest => ParserKind::Manifest,
        ParserKindCase::Structural => ParserKind::Structural,
        ParserKindCase::Fallback => ParserKind::Fallback,
    }
}

fn assert_v0326_language_migration_baseline_is_stable() -> Result<(), Box<dyn Error>> {
    let contract = language_runtime_contract()?;

    let expected_extensions = contract
        .broad_detection
        .iter()
        .map(|row| row.extension.as_str())
        .collect::<Vec<_>>();
    require_equal(
        &BROAD_SOURCE_EXTENSIONS,
        &expected_extensions.as_slice(),
        "ordered public scanner extensions",
    )?;
    for row in contract
        .broad_detection
        .iter()
        .chain(&contract.api_only_detection)
    {
        let detected = detect_language(Some(&row.extension));
        require_equal(
            &detected.as_deref(),
            &Some(row.language.as_str()),
            format!("literal extension {}", row.extension),
        )?;
        let uppercase = row.extension.to_ascii_uppercase();
        let uppercase_detected = detect_language(Some(&uppercase));
        require_equal(
            &uppercase_detected.as_deref(),
            &Some(row.language.as_str()),
            format!("uppercase extension {uppercase}"),
        )?;
    }

    let broad = expected_extensions.into_iter().collect::<BTreeSet<_>>();
    for row in &contract.api_only_detection {
        require(
            !broad.contains(row.extension.as_str()),
            format!("API-only extension entered broad list: {}", row.extension),
        )?;
    }

    for row in &contract.exact_filenames {
        for path in [
            row.file_name.clone(),
            format!("nested/{}", row.file_name),
            format!("nested\\{}", row.file_name),
        ] {
            let detected = detect_language_for_path(&path, Some(&row.conflicting_extension));
            require_equal(
                &detected.as_deref(),
                &Some(row.language.as_str()),
                format!("exact filename path {path}"),
            )?;
        }
    }

    for row in &contract.negative_detection {
        let detected = detect_language_for_path(&row.path, optional_text(&row.extension));
        require_equal(
            &detected.as_deref(),
            &optional_text(&row.language),
            format!("negative/fallback path {}", row.path),
        )?;
    }

    for row in &contract.extension_normalization {
        let extension = normalized_extension(Path::new(&row.path));
        require_equal(
            &extension.as_deref(),
            &optional_text(&row.extension),
            format!("normalized extension path {}", row.path),
        )?;
    }

    let actual_specs = LANGUAGE_SPECS
        .iter()
        .map(|spec| (spec.language, support_case(spec.parser_support)))
        .collect::<Vec<_>>();
    let expected_specs = contract
        .language_pipelines
        .iter()
        .map(|row| (row.language.as_str(), row.support))
        .collect::<Vec<_>>();
    require_equal(&actual_specs, &expected_specs, "ordered language specs")?;

    for row in &contract.language_pipelines {
        let support = language_spec(&row.language).map(|spec| support_case(spec.parser_support));
        require_equal(
            &support,
            &Some(row.support),
            format!("language spec {}", row.language),
        )?;
        let path = if row.language == "toon" {
            "fixture.toon"
        } else {
            "fixture"
        };
        require_equal(
            &is_structural_summary_candidate(path, Some(&row.language)),
            &(row.summary_adapter != SummaryAdapterCase::None),
            format!("summary adapter availability {}", row.language),
        )?;
        require_equal(
            &is_symbol_candidate(path, Some(&row.language)),
            &(row.symbol_adapter != SymbolAdapterCase::None),
            format!("symbol adapter availability {}", row.language),
        )?;
    }
    require(
        language_spec("unknown-language").is_none(),
        "unknown language unexpectedly has a public spec",
    )?;

    let expected_specialized = contract
        .specialized_parsers
        .iter()
        .map(|row| row.language.as_str())
        .collect::<Vec<_>>();
    require_equal(
        &specialized_languages(),
        &expected_specialized.as_slice(),
        "ordered specialized parser modes",
    )?;
    for row in &contract.specialized_parsers {
        require(
            has_specialized_parser(&row.language),
            format!("missing specialized parser for {}", row.language),
        )?;
        let graph = extract_symbol_graph("fixture", Some(&row.language), &row.source);
        require_equal(
            &graph.parser,
            &ParserKind::TreeSitter,
            format!("parser kind {}", row.language),
        )?;
        require(
            graph.symbols.iter().any(|symbol| {
                symbol.kind == symbol_kind(row.symbol_kind) && symbol.name == row.symbol_name
            }),
            format!(
                "missing {:?} {} for {}: {:?}",
                row.symbol_kind, row.symbol_name, row.language, graph.symbols
            ),
        )?;
    }

    let specialized = expected_specialized.into_iter().collect::<BTreeSet<_>>();
    for row in &contract.language_pipelines {
        if !specialized.contains(row.language.as_str()) {
            require(
                !has_specialized_parser(&row.language),
                format!("unexpected specialized parser for {}", row.language),
            )?;
        }
    }
    require(
        !has_specialized_parser("unknown-language"),
        "unknown language unexpectedly has a specialized parser",
    )?;

    for row in &contract.adapter_precedence {
        for mode in SuppliedMode::ALL {
            let expectation = row.expectation(mode);
            let witness = adapter_witness(expectation);
            for path in adapter_path_variants(&row.path) {
                let graph = extract_symbol_graph(&path, mode.language(), witness.source);
                let label = format!(
                    "adapter precedence {:?} path {path:?} mode {mode:?}",
                    row.path_class
                );
                require_equal(&graph.parser, &witness.parser, format!("{label} parser"))?;
                require(
                    graph.symbols.iter().any(|symbol| {
                        symbol.kind == witness.symbol_kind
                            && symbol.name == witness.symbol_name
                            && symbol.parser == witness.parser
                            && witness
                                .symbol_detail
                                .is_none_or(|detail| symbol.detail.as_deref() == Some(detail))
                    }),
                    format!(
                        "{label} missing {:?} {} detail {:?}: {:?}",
                        witness.symbol_kind,
                        witness.symbol_name,
                        witness.symbol_detail,
                        graph.symbols
                    ),
                )?;
            }
        }
    }

    Ok(())
}

fn assert_cargo_routing_corrections() -> Result<(), Box<dyn Error>> {
    let contract = language_runtime_contract()?;
    let source = "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n";
    for row in &contract.cargo_routing_corrections {
        require_equal(
            &row.disposition,
            &CompatibilityDisposition::IntentionalCorrection,
            format!("compatibility disposition {}", row.case_id),
        )?;
        require_equal(
            &row.baseline_symbol_candidate,
            &true,
            format!("v0.3.26 symbol candidate {}", row.case_id),
        )?;
        require_equal(
            &row.baseline_parser_kind,
            &ParserKindCase::Manifest,
            format!("v0.3.26 parser kind {}", row.case_id),
        )?;
        require(
            row.rationale.contains("suffix") && row.rationale.contains("exact filename"),
            format!("correction {} lacks the suffix root cause", row.case_id),
        )?;

        let language = optional_text(&row.supplied_language);
        let graph = extract_symbol_graph(&row.path, language, source);
        require_equal(
            &graph.parser,
            &parser_kind(row.accepted_parser_kind),
            format!("accepted parser kind {}", row.case_id),
        )?;
        require_equal(
            &is_symbol_candidate(&row.path, language),
            &row.accepted_symbol_candidate,
            format!("accepted symbol candidate {}", row.case_id),
        )?;
    }

    Ok(())
}

fn assert_v0326_summary_and_candidate_precedence() -> Result<(), Box<dyn Error>> {
    require_equal(
        &structural_summary_for_path("Cargo.toml", Some("markdown"), "# Fixture\n").as_deref(),
        &Some("markdown document titled Fixture."),
        "supplied Markdown summary precedes Cargo path",
    )?;
    require_equal(
        &structural_summary_for_path(
            "component.vue",
            Some("cargo-manifest"),
            "[package]\nname = \"fixture\"\n",
        )
        .as_deref(),
        &Some("toml document with tables package."),
        "supplied Cargo summary precedes Vue path",
    )?;
    require_equal(
        &structural_summary_for_path(
            "script.ps1",
            Some("rust"),
            "function Invoke-Fixture { 'ok' }\n",
        ),
        &None,
        "supplied Rust mode suppresses PowerShell path summary",
    )?;
    require_equal(
        &structural_summary_for_path("fixture.toon", Some("rust"), "fixture:\n  ok: true\n")
            .as_deref(),
        &Some("TOON document with sections fixture."),
        "historical TOON path fallback",
    )?;

    for (path, language, expected) in [
        ("Cargo.toml", Some("markdown"), true),
        ("component.vue", Some("markdown"), false),
        ("script.ps1", Some("markdown"), false),
        ("component.vue", Some("vue"), true),
        ("script.ps1", Some("powershell"), true),
        ("component.vue", None, false),
        ("script.ps1", None, false),
        ("fixture", Some("unknown"), true),
    ] {
        require_equal(
            &is_symbol_candidate(path, language),
            &expected,
            format!("historical symbol candidacy for {path:?} with {language:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn v0326_language_migration_baseline_is_stable() -> Result<(), Box<dyn Error>> {
    assert_v0326_language_migration_baseline_is_stable()
}

#[test]
fn cargo_path_routing_applies_reviewed_exact_filename_corrections() -> Result<(), Box<dyn Error>> {
    assert_cargo_routing_corrections()
}

#[test]
fn v0326_summary_and_candidate_precedence_is_stable() -> Result<(), Box<dyn Error>> {
    assert_v0326_summary_and_candidate_precedence()
}

#[test]
fn task_arri_ut_arri_5_4() -> Result<(), Box<dyn Error>> {
    for policy in CURRENT_LANGUAGE_POLICY {
        require(
            language_policy_for_public_mode(policy.public_mode)
                .is_some_and(|resolved| std::ptr::eq(resolved, policy)),
            format!(
                "generated CLI policy lookup did not return the declared row for {}",
                policy.public_mode
            ),
        )?;
    }
    require(
        language_policy_for_public_mode("unknown-language-mode").is_none(),
        "generated CLI policy lookup unexpectedly resolved an unknown mode",
    )?;
    assert_v0326_language_migration_baseline_is_stable()?;
    assert_cargo_routing_corrections()?;
    assert_v0326_summary_and_candidate_precedence()
}
