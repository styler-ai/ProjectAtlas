//! Purpose: Own deterministic language capability and detection truth.

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

/// Version of the generated language registry contract.
pub const LANGUAGE_CAPABILITY_REGISTRY_VERSION: u32 = 4;

/// Version of the direct and embedded semantic-provider projection.
pub const SEMANTIC_PROVIDER_CONTRACT_VERSION: u32 = 1;

/// Version of the accepted language capability floor.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION: u32 = 12;

/// Version of exact detector precedence and content-matching semantics.
pub const LANGUAGE_DETECTION_POLICY_VERSION: u32 = 1;

/// Historical acceptance seal for capability-set version 1.
///
/// Any membership, detector, owner, minimum, fixture, provenance, or platform
/// change must bump the accepted-set version instead of rewriting this seal.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V1_DIGEST: &str =
    "58f2e1e6755464d573df998c1c1cecb2d076c829c0dbe04a463ce14a5a239861";

/// Historical acceptance seal for capability-set version 2.
///
/// Version 2 preserves version 1 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.0 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V2_DIGEST: &str =
    "8397f73a7593b849d0e83b3892e4721874ac4a4fd93f62b42dac9ffe166a2a7c";

/// Historical acceptance seal for capability-set version 3.
///
/// Version 3 adds the independently owned Rust, ECMAScript, Python, and Cargo
/// semantic provider floor plus typed HTML-like/component/template embedding.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V3_DIGEST: &str =
    "a4b69ce4aed2ebf8d28f7b237ead76a53e5363e34c8c97ea5980776ea4217ef4";

/// Historical acceptance seal for capability-set version 4.
///
/// Version 4 preserves version 3 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.1 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V4_DIGEST: &str =
    "e9a952d0b3bc2d2c5db832130d85b7cdfd656aaa07ebbafab1505da6b87d9084";

/// Historical acceptance seal for capability-set version 5.
///
/// Version 5 preserves version 4 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.2 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V5_DIGEST: &str =
    "07a3d2c45a4736bc764e44016a6ba9b7f9ea1b769b0100604702160528679bc7";

/// Historical acceptance seal for capability-set version 6.
///
/// Version 6 preserves version 5 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.3 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V6_DIGEST: &str =
    "e9342f2b06b083a72ecc58af0afe4ba12f0ec33321225199bb0c9be4f4375c7a";

/// Historical acceptance seal for capability-set version 7.
///
/// Version 7 preserves version 6 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.4 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V7_DIGEST: &str =
    "50fcac887dffecc27f1b7d365ff5da991f2a86dc15e0474ef5bcc339c58bfd60";

/// Historical acceptance seal for capability-set version 8.
///
/// Version 8 adds registry-owned content roles and bounded Markdown heading
/// symbols without weakening any version 7 capability.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V8_DIGEST: &str =
    "2b26ae43b74475ea0dcb78d5b182329d500d76c33b2d87470d303400886ead1b";

/// Historical acceptance seal for capability-set version 9.
///
/// Version 9 preserves version 8 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.5-rc1 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V9_DIGEST: &str =
    "5fa0073094df29fba7160cecf85afa3ba5a9bcfa7ee4b5a53cde9e371b0077d2";

/// Historical acceptance seal for capability-set version 10.
///
/// Version 10 preserves version 9 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.5-rc2 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V10_DIGEST: &str =
    "cbede576a7b2ab4309798075210b59dbece0cf99cc7874a9659fd17f3c2d961f";

/// Historical acceptance seal for capability-set version 11.
///
/// Version 11 preserves version 10 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.5-rc3 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V11_DIGEST: &str =
    "3776f19c62b3debfcae13715e3bdc3ec3029978a4f7ba1428b7a06d433524915";

/// Historical acceptance seal for capability-set version 12.
///
/// Version 12 preserves version 11 membership and capability strength while
/// binding ProjectAtlas-owned parser provenance to the 0.4.5 runtime.
pub const ACCEPTED_LANGUAGE_CAPABILITY_SET_V12_DIGEST: &str =
    "bae01db588d8e6c8666bb1afd66ffcbffb3022c23c68df52f9822c291f9d895c";

/// Maximum content prefix inspected by the bounded content/dialect detector.
pub const LANGUAGE_CONTENT_DETECTION_MAX_BYTES: usize = 512;

/// Pinned metadata catalog used to admit broad detection and fallback rows.
pub const OPTIONAL_GRAMMAR_CATALOG: &str = "tree-sitter-language-pack";

/// Exact catalog version used by the accepted broad-language registry.
pub const OPTIONAL_GRAMMAR_CATALOG_VERSION: &str = "1.13.2";

/// Exact upstream release-tag revision used by the accepted broad-language registry.
pub const OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION: &str =
    "6258abac30304283763a0d2dc8a48cb87fbcf438";

/// Minimum additional non-built-in grammars required for the broad-pack claim.
pub const OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS: usize = 150;

/// Stable owner identity for the one logical optional broad parser pack.
pub const BROAD_PARSER_PACK_ID: &str = "broad-parser";

/// Maximum JSON bytes for the compact language settings projection.
pub const LANGUAGE_REGISTRY_REPORT_MAX_BYTES: usize = 32_000;

/// Parser coverage level retained for compatibility with the 0.3.26 API.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageParserSupport {
    /// A native Tree-sitter adapter backs symbol extraction.
    Native,
    /// A manifest-specific parser backs package/dependency extraction.
    Manifest,
    /// A deterministic structural summarizer backs agent-facing summaries.
    Structural,
    /// A conservative fallback parser is the current coverage boundary.
    Fallback,
}

/// Independently achieved support for one language capability axis.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilitySupportLevel {
    /// The capability has not been accepted or proved.
    Unavailable,
    /// Conservative heuristic behavior exists and is reported as fallback.
    Fallback,
    /// The capability is supported by its declared owner and fixtures.
    Supported,
}

impl CapabilitySupportLevel {
    /// Return the stable settings and documentation label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Fallback => "fallback",
            Self::Supported => "supported",
        }
    }
}

/// Independent language capability axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageCapabilitySupport {
    /// The language can be selected deterministically.
    pub detected: CapabilitySupportLevel,
    /// Source can be parsed at the declared quality level.
    pub parsed: CapabilitySupportLevel,
    /// Definitions and relationships can be extracted at the declared quality level.
    pub symbols: CapabilitySupportLevel,
    /// Project-wide semantic resolution has been independently validated.
    pub semantic: CapabilitySupportLevel,
    /// Representative benchmark coverage exists.
    pub benchmarked: CapabilitySupportLevel,
}

impl LanguageCapabilitySupport {
    /// Build one independent capability profile.
    #[must_use]
    pub const fn new(
        detected: CapabilitySupportLevel,
        parsed: CapabilitySupportLevel,
        symbols: CapabilitySupportLevel,
        semantic: CapabilitySupportLevel,
        benchmarked: CapabilitySupportLevel,
    ) -> Self {
        Self {
            detected,
            parsed,
            symbols,
            semantic,
            benchmarked,
        }
    }

    /// Return whether every achieved axis meets the accepted minimum.
    #[must_use]
    pub const fn meets(self, minimum: Self) -> bool {
        self.detected as u8 >= minimum.detected as u8
            && self.parsed as u8 >= minimum.parsed as u8
            && self.symbols as u8 >= minimum.symbols as u8
            && self.semantic as u8 >= minimum.semantic as u8
            && self.benchmarked as u8 >= minimum.benchmarked as u8
    }
}

/// Grammar-backed or purpose-built parsing and symbol support.
const SUPPORTED_NATIVE: LanguageCapabilitySupport = LanguageCapabilitySupport::new(
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Unavailable,
    CapabilitySupportLevel::Unavailable,
);

/// Grammar-backed or purpose-built support with project-wide semantic resolution.
const SUPPORTED_SEMANTIC: LanguageCapabilitySupport = LanguageCapabilitySupport::new(
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Unavailable,
);

/// Deterministic structural parsing without a symbol extractor.
const SUPPORTED_STRUCTURAL: LanguageCapabilitySupport = LanguageCapabilitySupport::new(
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Unavailable,
    CapabilitySupportLevel::Unavailable,
    CapabilitySupportLevel::Unavailable,
);

/// Deterministic detection with conservative parsing and symbol fallback.
const SUPPORTED_FALLBACK: LanguageCapabilitySupport = LanguageCapabilitySupport::new(
    CapabilitySupportLevel::Supported,
    CapabilitySupportLevel::Fallback,
    CapabilitySupportLevel::Fallback,
    CapabilitySupportLevel::Unavailable,
    CapabilitySupportLevel::Unavailable,
);

/// Closed built-in Tree-sitter grammar identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TreeSitterGrammar {
    /// Rust grammar.
    Rust,
    /// Python grammar.
    Python,
    /// JavaScript grammar.
    JavaScript,
    /// TypeScript grammar.
    TypeScript,
    /// TSX grammar.
    Tsx,
    /// Java grammar.
    Java,
    /// Kotlin grammar.
    Kotlin,
    /// C# grammar.
    CSharp,
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

impl TreeSitterGrammar {
    /// Every closed built-in grammar identity.
    pub const ALL: &'static [Self] = &[
        Self::Rust,
        Self::Python,
        Self::JavaScript,
        Self::TypeScript,
        Self::Tsx,
        Self::Java,
        Self::Kotlin,
        Self::CSharp,
        Self::Go,
        Self::ObjectiveC,
        Self::Zig,
        Self::C,
        Self::Cpp,
    ];

    /// Return the exact Cargo package that provides this built-in grammar.
    #[must_use]
    pub const fn package(self) -> &'static str {
        match self {
            Self::Rust => "tree-sitter-rust",
            Self::Python => "tree-sitter-python",
            Self::JavaScript => "tree-sitter-javascript",
            Self::TypeScript | Self::Tsx => "tree-sitter-typescript",
            Self::Java => "tree-sitter-java",
            Self::Kotlin => "tree-sitter-kotlin-ng",
            Self::CSharp => "tree-sitter-c-sharp",
            Self::Go => "tree-sitter-go",
            Self::ObjectiveC => "tree-sitter-objc",
            Self::Zig => "tree-sitter-zig",
            Self::C => "tree-sitter-c",
            Self::Cpp => "tree-sitter-cpp",
        }
    }

    /// Return the exact workspace-pinned Cargo version of this grammar.
    #[must_use]
    pub const fn version(self) -> &'static str {
        match self {
            Self::Rust | Self::C => "0.24.2",
            Self::Python | Self::JavaScript | Self::Go => "0.25.0",
            Self::TypeScript | Self::Tsx => "0.23.2",
            Self::Java | Self::CSharp => "0.23.5",
            Self::Kotlin => "1.1.0",
            Self::ObjectiveC => "3.0.2",
            Self::Zig => "1.1.2",
            Self::Cpp => "0.23.4",
        }
    }
}

/// Closed owner of built-in symbol extraction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolParserOwner {
    /// A declared Tree-sitter grammar.
    TreeSitter(TreeSitterGrammar),
    /// `ProjectAtlas` Cargo manifest/lock extraction.
    CargoManifest,
    /// `ProjectAtlas` Vue component extraction.
    Vue,
    /// `ProjectAtlas` `PowerShell` extraction.
    PowerShell,
    /// `ProjectAtlas` Markdown heading and explicit-reference extraction.
    Markdown,
    /// Conservative `ProjectAtlas` fallback extraction.
    Fallback,
    /// No definition or relationship extractor is available.
    Unavailable,
}

/// Closed owner of project-wide semantic resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticProviderOwner {
    /// Rust modules, imports, exports, and references.
    Rust,
    /// JavaScript, TypeScript, and TSX modules, imports, exports, and references.
    EcmaScript,
    /// Python modules, imports, exports, and references.
    Python,
    /// Cargo package and dependency identities.
    Cargo,
    /// No independently validated project-wide semantic provider is available.
    Unavailable,
}

impl SemanticProviderOwner {
    /// Return the stable settings and documentation label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::EcmaScript => "ecma-script",
            Self::Python => "python",
            Self::Cargo => "cargo",
            Self::Unavailable => "unavailable",
        }
    }

    /// Return the canonical cross-language resolution family owned by this provider.
    ///
    /// The provider label identifies the implementation owner while this family
    /// keeps compatible source dialects, such as JavaScript, TypeScript, and
    /// TSX, in one resolution namespace.
    #[must_use]
    pub const fn resolution_family(self) -> Option<&'static str> {
        match self {
            Self::Rust => Some("rust"),
            Self::EcmaScript => Some("ecmascript"),
            Self::Python => Some("python"),
            Self::Cargo => Some("cargo"),
            Self::Unavailable => None,
        }
    }
}

/// Closed host class for bounded embedded-source projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddedHostKind {
    /// HTML-like host source.
    HtmlLike,
    /// Component host source.
    Component,
    /// Template host source.
    Template,
}

impl EmbeddedHostKind {
    /// Return the stable settings and documentation label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HtmlLike => "html-like",
            Self::Component => "component",
            Self::Template => "template",
        }
    }
}

/// One accepted host-to-embedded semantic provider pairing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EmbeddedLanguageCapability {
    /// Host extraction class used for bounded source reconciliation.
    pub host_kind: EmbeddedHostKind,
    /// Provider that interprets accepted embedded source regions.
    pub semantic_provider: SemanticProviderOwner,
}

/// Closed owner of deterministic structural summaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructuralSummaryOwner {
    /// Markdown/CommonMark summaries.
    Markdown,
    /// JSON and JSONC summaries.
    Json,
    /// YAML summaries.
    Yaml,
    /// TOML and Cargo manifest summaries.
    Toml,
    /// XML summaries.
    Xml,
    /// CSS-family summaries.
    Css,
    /// HTML summaries.
    Html,
    /// TOON summaries.
    Toon,
    /// `PowerShell` summaries.
    PowerShell,
    /// Configuration and plain-text summaries.
    ConfigText,
}

/// Provenance class for an accepted language row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityProvenance {
    /// ProjectAtlas-owned deterministic behavior under the workspace license.
    ProjectAtlas,
    /// A pinned Cargo Tree-sitter grammar dependency.
    TreeSitter(TreeSitterGrammar),
    /// ProjectAtlas-accepted detection metadata from the pinned optional catalog.
    ///
    /// This does not claim that the corresponding grammar binary, its subtree
    /// license, or its required-platform behavior has passed optional-pack gates.
    PinnedOptionalCatalog,
}

impl CapabilityProvenance {
    /// Return the declared license input for settings and generated documentation.
    #[must_use]
    pub const fn license(self) -> &'static str {
        match self {
            Self::ProjectAtlas | Self::TreeSitter(_) | Self::PinnedOptionalCatalog => "MIT",
        }
    }

    /// Return the exact owning package or catalog identity.
    #[must_use]
    pub const fn source(self) -> &'static str {
        match self {
            Self::ProjectAtlas => "projectatlas",
            Self::TreeSitter(grammar) => grammar.package(),
            Self::PinnedOptionalCatalog => OPTIONAL_GRAMMAR_CATALOG,
        }
    }

    /// Return the exact owning package or catalog version.
    #[must_use]
    pub const fn version(self) -> &'static str {
        match self {
            Self::ProjectAtlas => env!("CARGO_PKG_VERSION"),
            Self::TreeSitter(grammar) => grammar.version(),
            Self::PinnedOptionalCatalog => OPTIONAL_GRAMMAR_CATALOG_VERSION,
        }
    }
}

/// Required platform applicability for one accepted capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequiredPlatformSet {
    /// Every platform supported by the `ProjectAtlas` release matrix.
    AllSupported,
}

/// Natural positive and negative detector fixtures for an accepted row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageCapabilityFixtures {
    /// Natural path that must select this language.
    pub positive_path: &'static str,
    /// Similar path that must not select this language.
    pub negative_path: &'static str,
}

/// Derived role of one admitted repository file.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentClassification {
    /// Program, template, query, build, or style source.
    Source,
    /// Human-facing documentation or specification content.
    Documentation,
    /// Structured configuration or data content.
    ConfigurationData,
    /// Eligible UTF-8 text without a more specific registry role.
    OtherText,
    /// Admitted metadata whose content is binary or invalid UTF-8.
    Opaque,
}

impl ContentClassification {
    /// Complete stable persisted classification set.
    pub const ALL: [Self; 5] = [
        Self::Source,
        Self::Documentation,
        Self::ConfigurationData,
        Self::OtherText,
        Self::Opaque,
    ];

    /// Return the stable database and payload spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Documentation => "documentation",
            Self::ConfigurationData => "configuration_data",
            Self::OtherText => "other_text",
            Self::Opaque => "opaque",
        }
    }

    /// Parse one stable persisted classification.
    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "source" => Some(Self::Source),
            "documentation" => Some(Self::Documentation),
            "configuration_data" => Some(Self::ConfigurationData),
            "other_text" => Some(Self::OtherText),
            "opaque" => Some(Self::Opaque),
            _ => None,
        }
    }
}

impl fmt::Display for ContentClassification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Caller selection applied to classified repository content.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentSelection {
    /// The request omitted selection and must retain the legacy candidate universe.
    #[default]
    #[serde(skip)]
    UnspecifiedLegacy,
    /// Select only source files for ordinary candidates and graph frontiers.
    Source,
    /// Select only documentation for ordinary candidates and graph frontiers.
    Documentation,
    /// Select source and documentation while excluding other classifications.
    Both,
}

impl ContentSelection {
    /// Return the explicit wire spelling, or `None` for an omitted legacy request.
    #[must_use]
    pub const fn explicit_value(self) -> Option<&'static str> {
        match self {
            Self::UnspecifiedLegacy => None,
            Self::Source => Some("source"),
            Self::Documentation => Some("documentation"),
            Self::Both => Some("both"),
        }
    }

    /// Return whether the classification belongs to this selection.
    #[must_use]
    pub const fn includes(self, classification: ContentClassification) -> bool {
        match self {
            Self::UnspecifiedLegacy => true,
            Self::Source => matches!(classification, ContentClassification::Source),
            Self::Documentation => {
                matches!(classification, ContentClassification::Documentation)
            }
            Self::Both => matches!(
                classification,
                ContentClassification::Source | ContentClassification::Documentation
            ),
        }
    }
}

/// Invalid explicit content selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSelectionParseError {
    /// Rejected caller value retained for adapter diagnostics.
    requested: String,
}

impl ContentSelectionParseError {
    /// Borrow the rejected caller input.
    #[must_use]
    pub fn requested(&self) -> &str {
        &self.requested
    }
}

impl fmt::Display for ContentSelectionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported content selection {:?}; expected source, documentation, or both",
            self.requested
        )
    }
}

impl Error for ContentSelectionParseError {}

impl FromStr for ContentSelection {
    type Err = ContentSelectionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "source" => Ok(Self::Source),
            "documentation" => Ok(Self::Documentation),
            "both" => Ok(Self::Both),
            _ => Err(ContentSelectionParseError {
                requested: value.to_owned(),
            }),
        }
    }
}

/// One accepted canonical language capability row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageCapability {
    /// Stable canonical language or file-family identifier.
    pub id: &'static str,
    /// Compatibility aliases accepted only for explicit selection.
    pub aliases: &'static [&'static str],
    /// Registry-owned content role.
    pub classification: ContentClassification,
    /// Compatibility parser tier.
    pub parser_support: LanguageParserSupport,
    /// Closed symbol parser owner.
    pub symbol_parser: SymbolParserOwner,
    /// Closed project-wide semantic provider owner.
    pub semantic_provider: SemanticProviderOwner,
    /// Optional bounded host-to-embedded provider pairing.
    pub embedded_language: Option<EmbeddedLanguageCapability>,
    /// Optional structural summary owner.
    pub structural_summary: Option<StructuralSummaryOwner>,
    /// Optional broad-parser pack owner; absent for the 0.3.26 core floor.
    pub optional_pack: Option<&'static str>,
    /// Currently achieved independent support.
    pub support: LanguageCapabilitySupport,
    /// Accepted minimum that cannot shrink silently in this set version.
    pub accepted_minimum: LanguageCapabilitySupport,
    /// Natural detector fixtures.
    pub fixtures: LanguageCapabilityFixtures,
    /// Parser or `ProjectAtlas` provenance input.
    pub provenance: CapabilityProvenance,
    /// Required release platforms.
    pub required_platforms: RequiredPlatformSet,
}

impl LanguageCapability {
    /// Return the accepted semantic provider for direct or embedded source.
    ///
    /// Direct providers take precedence. Embedded hosts expose their provider
    /// only for admitted embedded facts; callers remain responsible for not
    /// treating the host itself as an ordinary module without such facts.
    #[must_use]
    pub const fn effective_semantic_provider(self) -> Option<SemanticProviderOwner> {
        match self.semantic_provider {
            SemanticProviderOwner::Unavailable => match self.embedded_language {
                Some(embedded)
                    if !matches!(
                        embedded.semantic_provider,
                        SemanticProviderOwner::Unavailable
                    ) =>
                {
                    Some(embedded.semantic_provider)
                }
                _ => None,
            },
            provider => Some(provider),
        }
    }
}

/// Static compatibility parser metadata for one detected language family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageSpec {
    /// Detected language or file-family identifier.
    pub language: &'static str,
    /// Parser coverage level.
    pub parser_support: LanguageParserSupport,
}

/// Detection rule class and precedence reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageDetectionReason {
    /// A validated explicit override selected the language.
    ExplicitOverride,
    /// A case-sensitive exact filename selected the language.
    ExactFilename,
    /// The longest declared compound extension selected the language.
    CompoundExtension,
    /// A case-insensitive ordinary extension selected the language.
    Extension,
    /// A bounded declared content/dialect rule selected the language.
    ContentDialect,
}

/// Typed language detection result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageDetection {
    /// Canonical registry identifier.
    pub language: &'static str,
    /// Rule class that selected the identifier.
    pub reason: LanguageDetectionReason,
}

/// Typed language detection request.
#[derive(Clone, Copy, Debug, Default)]
pub struct LanguageDetectionRequest<'a> {
    /// Repository or native path.
    pub path: &'a str,
    /// Pre-normalized extension supplied by a scanner, when available.
    pub extension: Option<&'a str>,
    /// Explicit canonical ID or alias, when configured.
    pub explicit_override: Option<&'a str>,
    /// Bounded content prefix captured by the owning source read.
    pub content_prefix: Option<&'a [u8]>,
}

impl<'a> LanguageDetectionRequest<'a> {
    /// Build a request compatible with the 0.3.26 path/extension API.
    #[must_use]
    pub const fn new(path: &'a str, extension: Option<&'a str>) -> Self {
        Self {
            path,
            extension,
            explicit_override: None,
            content_prefix: None,
        }
    }
}

/// Invalid explicit language selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageDetectionError {
    /// Invalid canonical ID or alias supplied by the caller.
    requested: String,
}

impl fmt::Display for LanguageDetectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown explicit language override {:?}",
            self.requested
        )
    }
}

impl Error for LanguageDetectionError {}

/// Registry validation failure with both conflicting owners where applicable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageRegistryError {
    /// Deterministic validation diagnostic.
    message: String,
}

impl LanguageRegistryError {
    /// Build one deterministic registry diagnostic.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for LanguageRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for LanguageRegistryError {}

/// One static exact, compound, or extension detector rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageDetectionRule {
    /// Match value.
    pub value: &'static str,
    /// Canonical owner language.
    pub language: &'static str,
}

/// One registry-owned shebang interpreter rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageContentRule {
    /// Lowercase interpreter basename matched after optional `env` or `busybox`.
    pub interpreter: &'static str,
    /// Whether an ASCII numeric version suffix such as `3.12` is accepted.
    pub allow_version_suffix: bool,
    /// Canonical owner language.
    pub language: &'static str,
}

/// Default an omitted semantic provider to an honest unavailable claim.
macro_rules! semantic_provider_or_unavailable {
    () => {
        SemanticProviderOwner::Unavailable
    };
    ($provider:expr) => {
        $provider
    };
}

/// Default an omitted embedded-language contract to no host projection.
macro_rules! embedded_language_or_none {
    () => {
        None
    };
    ($capability:expr) => {
        Some($capability)
    };
}

/// Default a registry row to source unless it declares a non-source role.
macro_rules! content_classification_or_source {
    () => {
        ContentClassification::Source
    };
    ($classification:expr) => {
        $classification
    };
}

/// Generate typed capability and detection projections from one local manifest.
macro_rules! define_language_registry {
    (
        capabilities {
            $(
                $id:literal => {
                    aliases: [$($alias:literal),* $(,)?],
                    $(classification: $classification:expr,)?
                    parser_support: $parser_support:ident,
                    symbol_parser: $symbol_parser:expr,
                    structural_summary: $structural_summary:expr,
                    support: $support:expr,
                    $(semantic_provider: $semantic_provider:expr,)?
                    $(embedded_language: $embedded_language:expr,)?
                    positive: $positive:literal,
                    negative: $negative:literal,
                    provenance: $provenance:expr
                }
            ),* $(,)?
        }
        optional_capabilities {
            $(
                $optional_id:literal => {
                    aliases: [$($optional_alias:literal),* $(,)?],
                    $(classification: $optional_classification:expr,)?
                    extension: $optional_extension:literal
                }
            ),* $(,)?
        }
        exact_filenames { $($exact:literal => $exact_language:literal),* $(,)? }
        compound_extensions { $($compound:literal => $compound_language:literal),* $(,)? }
        broad_extensions { $($extension:literal => $extension_language:literal),* $(,)? }
        additional_extensions { $($additional:literal => $additional_language:literal),* $(,)? }
        content_interpreters {
            $(
                $interpreter:literal => {
                    language: $content_language:literal,
                    version_suffix: $version_suffix:literal
                }
            ),* $(,)?
        }
    ) => {
        /// Accepted canonical language capabilities.
        pub const LANGUAGE_CAPABILITIES: &[LanguageCapability] = &[
            $(LanguageCapability {
                id: $id,
                aliases: &[$($alias),*],
                classification: content_classification_or_source!($($classification)?),
                parser_support: LanguageParserSupport::$parser_support,
                symbol_parser: $symbol_parser,
                semantic_provider: semantic_provider_or_unavailable!($($semantic_provider)?),
                embedded_language: embedded_language_or_none!($($embedded_language)?),
                structural_summary: $structural_summary,
                optional_pack: None,
                support: $support,
                accepted_minimum: $support,
                fixtures: LanguageCapabilityFixtures {
                    positive_path: $positive,
                    negative_path: $negative,
                },
                provenance: $provenance,
                required_platforms: RequiredPlatformSet::AllSupported,
            }),*,
            $(LanguageCapability {
                id: $optional_id,
                aliases: &[$($optional_alias),*],
                classification: content_classification_or_source!($($optional_classification)?),
                parser_support: LanguageParserSupport::Fallback,
                symbol_parser: SymbolParserOwner::Fallback,
                semantic_provider: SemanticProviderOwner::Unavailable,
                embedded_language: None,
                structural_summary: None,
                optional_pack: Some(BROAD_PARSER_PACK_ID),
                support: SUPPORTED_FALLBACK,
                accepted_minimum: SUPPORTED_FALLBACK,
                fixtures: LanguageCapabilityFixtures {
                    positive_path: concat!("fixture", $optional_extension),
                    negative_path: concat!("fixture", $optional_extension, ".bak"),
                },
                provenance: CapabilityProvenance::PinnedOptionalCatalog,
                required_platforms: RequiredPlatformSet::AllSupported,
            }),*
        ];

        /// Compatibility parser metadata generated from the accepted manifest.
        pub const LANGUAGE_SPECS: &[LanguageSpec] = &[
            $(LanguageSpec {
                language: $id,
                parser_support: LanguageParserSupport::$parser_support,
            }),*,
            $(LanguageSpec {
                language: $optional_id,
                parser_support: LanguageParserSupport::Fallback,
            }),*
        ];

        /// Broad source extensions supported by the 0.3.26 scanner contract.
        pub const BROAD_SOURCE_EXTENSIONS: &[&str] = &[$($extension),*];

        /// Complete registry-known extension projection, including recognition-only pack rows.
        pub const DETECTED_SOURCE_EXTENSIONS: &[&str] = &[
            $($extension),*,
            $($optional_extension),*
        ];

        /// Case-sensitive exact filename rules.
        pub const EXACT_FILENAME_RULES: &[LanguageDetectionRule] = &[
            $(LanguageDetectionRule { value: $exact, language: $exact_language }),*
        ];

        /// Longest-first compound extension rules.
        pub const COMPOUND_EXTENSION_RULES: &[LanguageDetectionRule] = &[
            $(LanguageDetectionRule { value: $compound, language: $compound_language }),*
        ];

        /// Ordinary extension rules, including compatibility-only hidden extensions.
        pub const EXTENSION_RULES: &[LanguageDetectionRule] = &[
            $(LanguageDetectionRule { value: $extension, language: $extension_language }),*,
            $(LanguageDetectionRule { value: $optional_extension, language: $optional_id }),*,
            $(LanguageDetectionRule { value: $additional, language: $additional_language }),*
        ];

        /// Bounded shebang interpreter rules owned by the accepted registry.
        pub const CONTENT_DIALECT_RULES: &[LanguageContentRule] = &[
            $(LanguageContentRule {
                interpreter: $interpreter,
                allow_version_suffix: $version_suffix,
                language: $content_language,
            }),*
        ];

        fn exact_filename_language(file_name: &str) -> Option<&'static str> {
            match file_name {
                $($exact => Some($exact_language),)*
                _ => None,
            }
        }

        fn extension_language(extension: &str) -> Option<&'static str> {
            match extension {
                $($extension => Some($extension_language),)*
                $($optional_extension => Some($optional_id),)*
                $($additional => Some($additional_language),)*
                _ => None,
            }
        }

        /// Resolve a canonical language ID or compatibility alias.
        #[must_use]
        pub fn canonical_language_id(value: &str) -> Option<&'static str> {
            let trimmed = value.trim();
            if let Some(canonical) = canonical_language_id_exact(trimmed) {
                return Some(canonical);
            }
            let normalized = trimmed.to_ascii_lowercase();
            canonical_language_id_exact(&normalized)
        }

        /// Resolve an already-normalized canonical language ID or alias.
        fn canonical_language_id_exact(value: &str) -> Option<&'static str> {
            match value {
                $($id $(| $alias)* => Some($id),)*
                $($optional_id $(| $optional_alias)* => Some($optional_id),)*
                _ => None,
            }
        }
    };
}

define_language_registry! {
    capabilities {
        "rust" => { aliases: ["rs"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Rust), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::Rust, positive: "fixture.rs", negative: "fixture.rs.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Rust) },
        "rust-build-script" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Rust), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::Rust, positive: "build.rs", negative: "build.rs.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Rust) },
        "python" => { aliases: ["py"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Python), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::Python, positive: "fixture.py", negative: "fixture.py.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Python) },
        "javascript" => { aliases: ["js"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::JavaScript), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::EcmaScript, positive: "fixture.js", negative: "fixture.js.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::JavaScript) },
        "typescript" => { aliases: ["ts"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::TypeScript), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::EcmaScript, positive: "fixture.ts", negative: "fixture.ts.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::TypeScript) },
        "tsx" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Tsx), structural_summary: None, support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::EcmaScript, positive: "fixture.tsx", negative: "fixture.tsx.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Tsx) },
        "java" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Java), structural_summary: None, support: SUPPORTED_NATIVE, positive: "Fixture.java", negative: "Fixture.java.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Java) },
        "kotlin" => { aliases: ["kt"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Kotlin), structural_summary: None, support: SUPPORTED_NATIVE, positive: "Fixture.kt", negative: "Fixture.kt.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Kotlin) },
        "csharp" => { aliases: ["c#", "cs"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::CSharp), structural_summary: None, support: SUPPORTED_NATIVE, positive: "Fixture.cs", negative: "Fixture.cs.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::CSharp) },
        "go" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Go), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.go", negative: "fixture.go.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Go) },
        "objective-c" => { aliases: ["objc"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::ObjectiveC), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.m", negative: "fixture.m.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::ObjectiveC) },
        "zig" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Zig), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.zig", negative: "fixture.zig.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Zig) },
        "c" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::C), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.c", negative: "fixture.c.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::C) },
        "cpp" => { aliases: ["c++"], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Cpp), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.cpp", negative: "fixture.cpp.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Cpp) },
        "h" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::C), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.h", negative: "fixture.h.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::C) },
        "hpp" => { aliases: [], parser_support: Native, symbol_parser: SymbolParserOwner::TreeSitter(TreeSitterGrammar::Cpp), structural_summary: None, support: SUPPORTED_NATIVE, positive: "fixture.hpp", negative: "fixture.hpp.bak", provenance: CapabilityProvenance::TreeSitter(TreeSitterGrammar::Cpp) },
        "cargo-manifest" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Manifest, symbol_parser: SymbolParserOwner::CargoManifest, structural_summary: Some(StructuralSummaryOwner::Toml), support: SUPPORTED_SEMANTIC, semantic_provider: SemanticProviderOwner::Cargo, positive: "Cargo.toml", negative: "Cargo.toml.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "cargo-lock" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Manifest, symbol_parser: SymbolParserOwner::CargoManifest, structural_summary: None, support: SUPPORTED_NATIVE, positive: "Cargo.lock", negative: "Cargo.lock.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "vue" => { aliases: [], parser_support: Structural, symbol_parser: SymbolParserOwner::Vue, structural_summary: None, support: SUPPORTED_NATIVE, embedded_language: EmbeddedLanguageCapability { host_kind: EmbeddedHostKind::Component, semantic_provider: SemanticProviderOwner::EcmaScript }, positive: "Fixture.vue", negative: "Fixture.vue.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "markdown" => { aliases: ["md"], classification: ContentClassification::Documentation, parser_support: Structural, symbol_parser: SymbolParserOwner::Markdown, structural_summary: Some(StructuralSummaryOwner::Markdown), support: SUPPORTED_NATIVE, positive: "fixture.md", negative: "fixture.md.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "json" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Json), support: SUPPORTED_STRUCTURAL, positive: "fixture.json", negative: "fixture.json.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "yaml" => { aliases: ["yml"], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Yaml), support: SUPPORTED_STRUCTURAL, positive: "fixture.yml", negative: "fixture.yml.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "css" => { aliases: [], parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Css), support: SUPPORTED_STRUCTURAL, positive: "fixture.css", negative: "fixture.css.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "html" => { aliases: [], parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Html), support: SUPPORTED_STRUCTURAL, embedded_language: EmbeddedLanguageCapability { host_kind: EmbeddedHostKind::HtmlLike, semantic_provider: SemanticProviderOwner::EcmaScript }, positive: "fixture.html", negative: "fixture.html.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "toon" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Toon), support: SUPPORTED_STRUCTURAL, positive: "fixture.toon", negative: "fixture.toon.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "dockerfile" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "Dockerfile", negative: "Dockerfile.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "makefile" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "Makefile", negative: "Makefile.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "text" => { aliases: ["txt"], classification: ContentClassification::OtherText, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::ConfigText), support: SUPPORTED_STRUCTURAL, positive: "fixture.txt", negative: "fixture.txt.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "toml" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Toml), support: SUPPORTED_STRUCTURAL, positive: "fixture.toml", negative: "fixture.toml.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "xml" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::Xml), support: SUPPORTED_STRUCTURAL, positive: "fixture.xml", negative: "fixture.xml.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "svelte" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, embedded_language: EmbeddedLanguageCapability { host_kind: EmbeddedHostKind::Template, semantic_provider: SemanticProviderOwner::EcmaScript }, positive: "Fixture.svelte", negative: "Fixture.svelte.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "astro" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "Fixture.astro", negative: "Fixture.astro.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "jsp" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.jsp", negative: "fixture.jsp.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "jsp-tag" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.tag", negative: "fixture.tag.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "gsp" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.gsp", negative: "fixture.gsp.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "groovy" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.groovy", negative: "fixture.groovy.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "protobuf" => { aliases: ["proto"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.proto", negative: "fixture.proto.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "handlebars" => { aliases: ["hbs"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.hbs", negative: "fixture.hbs.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "ejs" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.ejs", negative: "fixture.ejs.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "pug" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.pug", negative: "fixture.pug.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "freemarker" => { aliases: ["ftl"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.ftl", negative: "fixture.ftl.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "mustache" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.mustache", negative: "fixture.mustache.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "liquid" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.liquid", negative: "fixture.liquid.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "erb" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.erb", negative: "fixture.erb.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "sql" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.sql", negative: "fixture.sql.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "graphql" => { aliases: ["gql"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.gql", negative: "fixture.gql.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "config" => { aliases: [], classification: ContentClassification::ConfigurationData, parser_support: Structural, symbol_parser: SymbolParserOwner::Unavailable, structural_summary: Some(StructuralSummaryOwner::ConfigText), support: SUPPORTED_STRUCTURAL, positive: "fixture.ini", negative: "fixture.ini.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "ruby" => { aliases: ["rb"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.rb", negative: "fixture.rb.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "php" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.php", negative: "fixture.php.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "swift" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.swift", negative: "fixture.swift.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "scala" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.scala", negative: "fixture.scala.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "shell" => { aliases: ["sh"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.sh", negative: "fixture.sh.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "powershell" => { aliases: ["pwsh"], parser_support: Fallback, symbol_parser: SymbolParserOwner::PowerShell, structural_summary: Some(StructuralSummaryOwner::PowerShell), support: SUPPORTED_NATIVE, positive: "fixture.ps1", negative: "fixture.ps1.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "batch" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.bat", negative: "fixture.bat.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "r" => { aliases: ["rscript"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.r", negative: "fixture.r.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "perl" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.pl", negative: "fixture.pl.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "lua" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.lua", negative: "fixture.lua.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "dart" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.dart", negative: "fixture.dart.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "haskell" => { aliases: ["hs"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.hs", negative: "fixture.hs.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "ocaml" => { aliases: [], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.ml", negative: "fixture.ml.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "fsharp" => { aliases: ["f#"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.fs", negative: "fixture.fs.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "clojure" => { aliases: ["clj"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.clj", negative: "fixture.clj.bak", provenance: CapabilityProvenance::ProjectAtlas },
        "vim" => { aliases: ["vimscript"], parser_support: Fallback, symbol_parser: SymbolParserOwner::Fallback, structural_summary: None, support: SUPPORTED_FALLBACK, positive: "fixture.vim", negative: "fixture.vim.bak", provenance: CapabilityProvenance::ProjectAtlas }
    }
    optional_capabilities {
        "abl" => { aliases: [], extension: ".p" },
        "actionscript" => { aliases: [], extension: ".as" },
        "ada" => { aliases: [], extension: ".ada" },
        "agda" => { aliases: [], extension: ".agda" },
        "al" => { aliases: [], extension: ".al" },
        "arduino" => { aliases: [], extension: ".ino" },
        "asciidoc" => { aliases: [], classification: ContentClassification::Documentation, extension: ".adoc" },
        "asm" => { aliases: [], extension: ".s" },
        "awk" => { aliases: [], extension: ".awk" },
        "beancount" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".beancount" },
        "bibtex" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".bib" },
        "bicep" => { aliases: [], extension: ".bicep" },
        "bitbake" => { aliases: [], extension: ".bb" },
        "blade" => { aliases: [], extension: ".blade" },
        "brightscript" => { aliases: [], extension: ".brs" },
        "bsl" => { aliases: [], extension: ".bsl" },
        "c3" => { aliases: [], extension: ".c3" },
        "caddy" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".caddyfile" },
        "cairo" => { aliases: [], extension: ".cairo" },
        "capnp" => { aliases: [], extension: ".capnp" },
        "cedar" => { aliases: [], extension: ".cedar" },
        "cedarschema" => { aliases: [], extension: ".cedarschema" },
        "cel" => { aliases: [], extension: ".cel" },
        "cfml" => { aliases: [], extension: ".cfc" },
        "chatito" => { aliases: [], extension: ".chatito" },
        "chuck" => { aliases: [], extension: ".ck" },
        "circom" => { aliases: [], extension: ".circom" },
        "clarity" => { aliases: [], extension: ".clar" },
        "cmake" => { aliases: [], extension: ".cmake" },
        "cobol" => { aliases: [], extension: ".cobol" },
        "commonlisp" => { aliases: [], extension: ".lisp" },
        "cooklang" => { aliases: [], extension: ".cook" },
        "corn" => { aliases: [], extension: ".corn" },
        "cpon" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".cpon" },
        "crystal" => { aliases: [], extension: ".cr" },
        "cst" => { aliases: [], extension: ".cst" },
        "csv" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".csv" },
        "cuda" => { aliases: [], extension: ".cu" },
        "cue" => { aliases: [], extension: ".cue" },
        "cylc" => { aliases: [], extension: ".cylc" },
        "d" => { aliases: [], extension: ".d" },
        "desktop" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".desktop" },
        "devicetree" => { aliases: [], extension: ".dts" },
        "dhall" => { aliases: [], extension: ".dhall" },
        "diff" => { aliases: [], classification: ContentClassification::OtherText, extension: ".diff" },
        "djot" => { aliases: [], classification: ContentClassification::Documentation, extension: ".dj" },
        "dot" => { aliases: [], extension: ".dot" },
        "dtd" => { aliases: [], extension: ".dtd" },
        "ebnf" => { aliases: [], extension: ".ebnf" },
        "eds" => { aliases: [], extension: ".eds" },
        "eex" => { aliases: [], extension: ".eex" },
        "elisp" => { aliases: [], extension: ".el" },
        "elixir" => { aliases: [], extension: ".ex" },
        "elm" => { aliases: [], extension: ".elm" },
        "elsa" => { aliases: [], extension: ".lc" },
        "elvish" => { aliases: [], extension: ".elv" },
        "enforce" => { aliases: [], extension: ".enforce" },
        "erlang" => { aliases: [], extension: ".erl" },
        "facility" => { aliases: [], extension: ".fsd" },
        "faust" => { aliases: [], extension: ".dsp" },
        "fennel" => { aliases: [], extension: ".fnl" },
        "fidl" => { aliases: [], extension: ".fidl" },
        "firrtl" => { aliases: [], extension: ".fir" },
        "fish" => { aliases: [], extension: ".fish" },
        "forth" => { aliases: [], extension: ".fth" },
        "fortran" => { aliases: [], extension: ".f90" },
        "fsharp_signature" => { aliases: [], extension: ".fsi" },
        "func" => { aliases: [], extension: ".fc" },
        "gap" => { aliases: [], extension: ".g" },
        "gdscript" => { aliases: [], extension: ".gd" },
        "gdshader" => { aliases: [], extension: ".gdshader" },
        "gherkin" => { aliases: [], extension: ".feature" },
        "gitattributes" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".gitattributes" },
        "gleam" => { aliases: [], extension: ".gleam" },
        "glsl" => { aliases: [], extension: ".glsl" },
        "gn" => { aliases: [], extension: ".gn" },
        "gnuplot" => { aliases: [], extension: ".gp" },
        "godot_resource" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".tres" },
        "gomod" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".mod" },
        "gotmpl" => { aliases: [], extension: ".gotmpl" },
        "gren" => { aliases: [], extension: ".gren" },
        "hack" => { aliases: [], extension: ".hack" },
        "hare" => { aliases: [], extension: ".hare" },
        "haxe" => { aliases: [], extension: ".hx" },
        "hcl" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".hcl" },
        "heex" => { aliases: [], extension: ".heex" },
        "hjson" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".hjson" },
        "hlsl" => { aliases: [], extension: ".hlsl" },
        "hocon" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".hocon" },
        "hoon" => { aliases: [], extension: ".hoon" },
        "http" => { aliases: [], extension: ".http" },
        "hurl" => { aliases: [], extension: ".hurl" },
        "idris" => { aliases: [], extension: ".idr" },
        "ispc" => { aliases: [], extension: ".ispc" },
        "jai" => { aliases: [], extension: ".jai" },
        "janet" => { aliases: [], extension: ".janet" },
        "jinja2" => { aliases: [], extension: ".j2" },
        "jq" => { aliases: [], extension: ".jq" },
        "json5" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".json5" },
        "jsonnet" => { aliases: [], extension: ".jsonnet" },
        "julia" => { aliases: [], extension: ".jl" },
        "just" => { aliases: [], extension: ".just" },
        "kcl" => { aliases: [], extension: ".k" },
        "kdl" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".kdl" },
        "latex" => { aliases: [], classification: ContentClassification::Documentation, extension: ".tex" },
        "lean" => { aliases: [], extension: ".lean" },
        "ledger" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".ldg" },
        "linkerscript" => { aliases: [], extension: ".lds" },
        "llvm" => { aliases: [], extension: ".ll" },
        "luau" => { aliases: [], extension: ".luau" },
        "magik" => { aliases: [], extension: ".magik" },
        "make" => { aliases: [], extension: ".mk" },
        "matlab" => { aliases: [], extension: ".matlab" },
        "mermaid" => { aliases: [], extension: ".mmd" },
        "meson" => { aliases: [], extension: ".meson" },
        "mlir" => { aliases: [], extension: ".mlir" },
        "mojo" => { aliases: [], extension: ".mojo" },
        "move" => { aliases: [], extension: ".move" },
        "nasm" => { aliases: [], extension: ".nasm" },
        "netlinx" => { aliases: [], extension: ".axs" },
        "nginx" => { aliases: [], extension: ".nginx" },
        "nickel" => { aliases: [], extension: ".ncl" },
        "nim" => { aliases: [], extension: ".nim" },
        "ninja" => { aliases: [], extension: ".ninja" },
        "nix" => { aliases: [], extension: ".nix" },
        "norg" => { aliases: [], classification: ContentClassification::Documentation, extension: ".norg" },
        "nqc" => { aliases: [], extension: ".nqc" },
        "nushell" => { aliases: [], extension: ".nu" },
        "ocamllex" => { aliases: [], extension: ".mll" },
        "odin" => { aliases: [], extension: ".odin" },
        "openscad" => { aliases: [], extension: ".scad" },
        "org" => { aliases: [], classification: ContentClassification::Documentation, extension: ".org" },
        "pascal" => { aliases: [], extension: ".pas" },
        "pem" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".pem" },
        "pgn" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".pgn" },
        "pkl" => { aliases: [], extension: ".pkl" },
        "po" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".po" },
        "poe_filter" => { aliases: [], extension: ".filter" },
        "pony" => { aliases: [], extension: ".pony" },
        "postscript" => { aliases: [], extension: ".ps" },
        "prisma" => { aliases: [], extension: ".prisma" },
        "prolog" => { aliases: [], extension: ".pro" },
        "promql" => { aliases: [], extension: ".promql" },
        "prql" => { aliases: [], extension: ".prql" },
        "psv" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".psv" },
        "puppet" => { aliases: [], extension: ".pp" },
        "purescript" => { aliases: [], extension: ".purs" },
        "ql" => { aliases: [], extension: ".ql" },
        "qmljs" => { aliases: [], extension: ".qml" },
        "racket" => { aliases: [], extension: ".rkt" },
        "rasi" => { aliases: [], extension: ".rasi" },
        "razor" => { aliases: [], extension: ".razor" },
        "rbs" => { aliases: [], extension: ".rbs" },
        "re2c" => { aliases: [], extension: ".re" },
        "rego" => { aliases: [], extension: ".rego" },
        "rescript" => { aliases: [], extension: ".res" },
        "robot" => { aliases: [], extension: ".robot" },
        "roc" => { aliases: [], extension: ".roc" },
        "ron" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".ron" },
        "rst" => { aliases: [], classification: ContentClassification::Documentation, extension: ".rst" },
        "rtf" => { aliases: [], classification: ContentClassification::Documentation, extension: ".rtf" },
        "scheme" => { aliases: [], extension: ".scm" },
        "slang" => { aliases: [], extension: ".slang" },
        "smali" => { aliases: [], extension: ".smali" },
        "smalltalk" => { aliases: [], extension: ".st" },
        "smithy" => { aliases: [], extension: ".smithy" },
        "sml" => { aliases: [], extension: ".sml" },
        "snakemake" => { aliases: [], extension: ".smk" },
        "solidity" => { aliases: [], extension: ".sol" },
        "souffle" => { aliases: [], extension: ".dl" },
        "sourcepawn" => { aliases: [], extension: ".sp" },
        "sql_bigquery" => { aliases: [], extension: ".bq" },
        "squirrel" => { aliases: [], extension: ".squirrel" },
        "stan" => { aliases: [], extension: ".stan" },
        "starlark" => { aliases: [], extension: ".star" },
        "superhtml" => { aliases: [], extension: ".shtml" },
        "sway" => { aliases: [], extension: ".sw" },
        "systemverilog" => { aliases: [], extension: ".sv" },
        "tablegen" => { aliases: [], extension: ".td" },
        "tact" => { aliases: [], extension: ".tact" },
        "tcl" => { aliases: [], extension: ".tcl" },
        "teal" => { aliases: [], extension: ".tl" },
        "templ" => { aliases: [], extension: ".templ" },
        "tera" => { aliases: [], extension: ".tera" },
        "terraform" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".tf" },
        "textproto" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".textproto" },
        "thrift" => { aliases: [], extension: ".thrift" },
        "tlaplus" => { aliases: [], extension: ".tla" },
        "todotxt" => { aliases: [], classification: ContentClassification::OtherText, extension: ".todotxt" },
        "tsv" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".tsv" },
        "turtle" => { aliases: [], classification: ContentClassification::ConfigurationData, extension: ".ttl" },
        "twig" => { aliases: [], extension: ".twig" },
        "typespec" => { aliases: [], extension: ".tsp" },
        "typoscript" => { aliases: [], extension: ".typoscript" },
        "typst" => { aliases: [], classification: ContentClassification::Documentation, extension: ".typst" },
        "uxntal" => { aliases: [], extension: ".tal" },
        "v" => { aliases: [], extension: ".v" },
        "vb" => { aliases: [], extension: ".vb" },
        "verilog" => { aliases: [], extension: ".verilog" },
        "vhdl" => { aliases: [], extension: ".vhdl" },
        "vhs" => { aliases: [], extension: ".tape" },
        "vrl" => { aliases: [], extension: ".vrl" },
        "wast" => { aliases: [], extension: ".wast" },
        "wat" => { aliases: [], extension: ".wat" },
        "wgsl" => { aliases: [], extension: ".wgsl" },
        "wit" => { aliases: [], extension: ".wit" },
        "yuck" => { aliases: [], extension: ".yuck" },
        "ziggy" => { aliases: [], extension: ".ziggy" },
    }
    exact_filenames {
        "Cargo.toml" => "cargo-manifest",
        "Cargo.lock" => "cargo-lock",
        "build.rs" => "rust-build-script",
        "Dockerfile" => "dockerfile",
        "Makefile" => "makefile"
    }
    compound_extensions { ".d.ts" => "typescript" }
    broad_extensions {
        ".py" => "python", ".pyw" => "python", ".js" => "javascript", ".jsx" => "javascript", ".ts" => "typescript", ".tsx" => "tsx", ".mjs" => "javascript", ".cjs" => "javascript", ".d.ts" => "typescript", ".java" => "java", ".c" => "c", ".cpp" => "cpp", ".h" => "h", ".hpp" => "hpp", ".cxx" => "cpp", ".cc" => "cpp", ".hxx" => "hpp", ".hh" => "hpp", ".cs" => "csharp", ".go" => "go", ".m" => "objective-c", ".mm" => "objective-c", ".rb" => "ruby", ".php" => "php", ".swift" => "swift", ".kt" => "kotlin", ".kts" => "kotlin", ".rs" => "rust", ".scala" => "scala", ".sh" => "shell", ".bash" => "shell", ".zsh" => "shell", ".ps1" => "powershell", ".psm1" => "powershell", ".psd1" => "powershell", ".bat" => "batch", ".cmd" => "batch", ".r" => "r", ".R" => "r", ".pl" => "perl", ".pm" => "perl", ".lua" => "lua", ".dart" => "dart", ".hs" => "haskell", ".ml" => "ocaml", ".mli" => "ocaml", ".fs" => "fsharp", ".fsx" => "fsharp", ".clj" => "clojure", ".cljs" => "clojure", ".vim" => "vim", ".zig" => "zig", ".zon" => "zig", ".html" => "html", ".htm" => "html", ".css" => "css", ".scss" => "css", ".sass" => "css", ".less" => "css", ".stylus" => "css", ".styl" => "css", ".md" => "markdown", ".mdx" => "markdown", ".json" => "json", ".jsonc" => "json", ".xml" => "xml", ".yml" => "yaml", ".yaml" => "yaml", ".toml" => "toml", ".toon" => "toon", ".txt" => "text", ".ini" => "config", ".cfg" => "config", ".conf" => "config", ".vue" => "vue", ".svelte" => "svelte", ".astro" => "astro", ".jsp" => "jsp", ".jspx" => "jsp", ".jspf" => "jsp", ".tag" => "jsp-tag", ".tagx" => "jsp-tag", ".gsp" => "gsp", ".properties" => "config", ".gradle" => "groovy", ".groovy" => "groovy", ".proto" => "protobuf", ".hbs" => "handlebars", ".handlebars" => "handlebars", ".ejs" => "ejs", ".pug" => "pug", ".ftl" => "freemarker", ".mustache" => "mustache", ".liquid" => "liquid", ".erb" => "erb", ".sql" => "sql", ".ddl" => "sql", ".dml" => "sql", ".mysql" => "sql", ".postgresql" => "sql", ".psql" => "sql", ".sqlite" => "sql", ".mssql" => "sql", ".oracle" => "sql", ".ora" => "sql", ".db2" => "sql", ".proc" => "sql", ".procedure" => "sql", ".func" => "sql", ".function" => "sql", ".view" => "sql", ".trigger" => "sql", ".index" => "sql", ".migration" => "sql", ".seed" => "sql", ".fixture" => "sql", ".schema" => "sql", ".cql" => "sql", ".cypher" => "sql", ".sparql" => "sql", ".gql" => "graphql", ".liquibase" => "sql", ".flyway" => "sql"
    }
    additional_extensions {
        ".env" => "config", ".gitignore" => "config", ".dockerignore" => "config", ".editorconfig" => "config"
    }
    content_interpreters {
        "python" => { language: "python", version_suffix: true },
        "pythonw" => { language: "python", version_suffix: true },
        "node" => { language: "javascript", version_suffix: false },
        "deno" => { language: "javascript", version_suffix: false },
        "powershell" => { language: "powershell", version_suffix: false },
        "pwsh" => { language: "powershell", version_suffix: false },
        "ruby" => { language: "ruby", version_suffix: true },
        "perl" => { language: "perl", version_suffix: true },
        "lua" => { language: "lua", version_suffix: true },
        "rscript" => { language: "r", version_suffix: false },
        "sh" => { language: "shell", version_suffix: false },
        "bash" => { language: "shell", version_suffix: false },
        "dash" => { language: "shell", version_suffix: false },
        "ash" => { language: "shell", version_suffix: false },
        "zsh" => { language: "shell", version_suffix: false },
        "ksh" => { language: "shell", version_suffix: false },
        "mksh" => { language: "shell", version_suffix: false },
        "fish" => { language: "shell", version_suffix: false }
    }
}

/// Return parser coverage metadata for a detected language family.
#[must_use]
pub fn language_spec(language: &str) -> Option<&'static LanguageSpec> {
    let canonical = canonical_language_id(language)?;
    LANGUAGE_SPECS
        .iter()
        .find(|spec| spec.language == canonical)
}

/// Return the accepted capability row for a canonical ID or alias.
#[must_use]
pub fn language_capability(language: &str) -> Option<&'static LanguageCapability> {
    static BY_ID: OnceLock<BTreeMap<&'static str, &'static LanguageCapability>> = OnceLock::new();
    let canonical = canonical_language_id(language)?;
    BY_ID
        .get_or_init(|| {
            LANGUAGE_CAPABILITIES
                .iter()
                .map(|capability| (capability.id, capability))
                .collect()
        })
        .get(canonical)
        .copied()
}

/// Classify one already-admitted file from registry truth and UTF-8 validity.
///
/// Admission and ignore policy remain owned by the scanner. Registry truth wins
/// for known rows; only otherwise unknown content falls back to UTF-8 evidence.
#[must_use]
pub fn content_classification(language: Option<&str>, valid_utf8: bool) -> ContentClassification {
    if let Some(classification) = language
        .and_then(language_capability)
        .map(|capability| capability.classification)
    {
        return classification;
    }
    if valid_utf8 {
        ContentClassification::OtherText
    } else {
        ContentClassification::Opaque
    }
}

/// Return all accepted language rows as generated documentation inputs.
#[must_use]
pub const fn language_documentation_rows() -> &'static [LanguageCapability] {
    LANGUAGE_CAPABILITIES
}

/// Return the built-in Tree-sitter grammar owner for a language.
#[must_use]
pub fn tree_sitter_grammar(language: &str) -> Option<TreeSitterGrammar> {
    match language_capability(language)?.symbol_parser {
        SymbolParserOwner::TreeSitter(grammar) => Some(grammar),
        SymbolParserOwner::CargoManifest
        | SymbolParserOwner::Vue
        | SymbolParserOwner::PowerShell
        | SymbolParserOwner::Markdown
        | SymbolParserOwner::Fallback
        | SymbolParserOwner::Unavailable => None,
    }
}

/// Return all canonical language IDs backed by built-in Tree-sitter grammars.
#[must_use]
pub fn builtin_tree_sitter_language_ids() -> &'static [&'static str] {
    static IDS: OnceLock<Box<[&'static str]>> = OnceLock::new();
    IDS.get_or_init(|| {
        LANGUAGE_CAPABILITIES
            .iter()
            .filter_map(|capability| {
                matches!(capability.symbol_parser, SymbolParserOwner::TreeSitter(_))
                    .then_some(capability.id)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    })
}

/// Return whether a path ends in a declared compound language extension.
#[must_use]
pub fn compound_language_extension(path: &str) -> Option<&'static str> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = file_name.to_ascii_lowercase();
    COMPOUND_EXTENSION_RULES
        .iter()
        .find(|rule| lower.ends_with(rule.value))
        .map(|rule| rule.value)
}

/// Return the normalized extension used by the scanner and detector.
#[must_use]
pub fn normalized_language_extension(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    if let Some(compound) = compound_language_extension(&file_name) {
        return Some(compound.to_string());
    }
    path.extension()
        .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
}

/// Detect one language through the typed precedence contract.
///
/// # Errors
///
/// Returns an error when an explicit override is not a known canonical ID or alias.
pub fn detect_language_request(
    request: LanguageDetectionRequest<'_>,
) -> Result<Option<LanguageDetection>, LanguageDetectionError> {
    if let Some(explicit) = request.explicit_override {
        let Some(language) = canonical_language_id(explicit) else {
            return Err(LanguageDetectionError {
                requested: explicit.to_string(),
            });
        };
        return Ok(Some(LanguageDetection {
            language,
            reason: LanguageDetectionReason::ExplicitOverride,
        }));
    }

    let file_name = request
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(request.path);
    if let Some(language) = exact_filename_language(file_name) {
        return Ok(Some(LanguageDetection {
            language,
            reason: LanguageDetectionReason::ExactFilename,
        }));
    }

    let lower_file_name = file_name.to_ascii_lowercase();
    if let Some(rule) = COMPOUND_EXTENSION_RULES
        .iter()
        .find(|rule| lower_file_name.ends_with(rule.value))
    {
        return Ok(Some(LanguageDetection {
            language: rule.language,
            reason: LanguageDetectionReason::CompoundExtension,
        }));
    }

    let normalized_extension = request.extension.map(str::to_ascii_lowercase).or_else(|| {
        file_name
            .rsplit_once('.')
            .map(|(_, extension)| format!(".{}", extension.to_ascii_lowercase()))
    });
    if let Some(extension) = normalized_extension
        && let Some(language) = extension_language(&extension)
    {
        return Ok(Some(LanguageDetection {
            language,
            reason: LanguageDetectionReason::Extension,
        }));
    }

    Ok(
        content_dialect_language(request.content_prefix).map(|language| LanguageDetection {
            language,
            reason: LanguageDetectionReason::ContentDialect,
        }),
    )
}

/// Detect a language or file family from an extension.
#[must_use]
pub fn detect_language(extension: Option<&str>) -> Option<String> {
    detect_language_request(LanguageDetectionRequest::new("", extension))
        .ok()
        .flatten()
        .map(|detected| detected.language.to_string())
}

/// Detect a language or file family from a path plus extension.
#[must_use]
pub fn detect_language_for_path(path: &str, extension: Option<&str>) -> Option<String> {
    detect_language_request(LanguageDetectionRequest::new(path, extension))
        .ok()
        .flatten()
        .map(|detected| detected.language.to_string())
}

/// Apply bounded registry-owned shebang rules after filename/extension rules miss.
fn content_dialect_language(content_prefix: Option<&[u8]>) -> Option<&'static str> {
    let prefix = content_prefix?;
    let bounded = &prefix[..prefix.len().min(LANGUAGE_CONTENT_DETECTION_MAX_BYTES)];
    let line_end = bounded
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(bounded.len());
    let first_line = std::str::from_utf8(&bounded[..line_end])
        .ok()?
        .trim_end_matches('\r');
    let shebang = first_line.strip_prefix("#!")?.trim();
    let interpreter = shebang_interpreter(shebang)?;
    CONTENT_DIALECT_RULES
        .iter()
        .find(|rule| content_interpreter_matches(&interpreter, rule))
        .map(|rule| rule.language)
}

/// Return the lowercase interpreter basename from a bounded shebang command.
fn shebang_interpreter(shebang: &str) -> Option<String> {
    let mut tokens = shebang.split_ascii_whitespace();
    let first = tokens.next()?;
    let first_basename = command_basename(first);
    let selected = if first_basename.eq_ignore_ascii_case("env") {
        tokens.find(|token| !token.starts_with('-') && !token.contains('='))?
    } else if first_basename.eq_ignore_ascii_case("busybox") {
        tokens.next()?
    } else {
        first
    };
    let normalized = command_basename(selected).to_ascii_lowercase();
    Some(
        normalized
            .strip_suffix(".exe")
            .unwrap_or(&normalized)
            .to_string(),
    )
}

/// Return a command basename for Unix and Windows-style shebang paths.
fn command_basename(command: &str) -> &str {
    command.rsplit(['/', '\\']).next().unwrap_or(command)
}

/// Match one normalized interpreter against its declared exact/versioned rule.
fn content_interpreter_matches(interpreter: &str, rule: &LanguageContentRule) -> bool {
    if interpreter == rule.interpreter {
        return true;
    }
    rule.allow_version_suffix
        && interpreter
            .strip_prefix(rule.interpreter)
            .is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix.split('.').all(|segment| {
                        !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
}

/// Count levels for one capability axis.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CapabilityLevelCounts {
    /// Rows without this capability.
    pub unavailable: usize,
    /// Rows with conservative fallback behavior.
    pub fallback: usize,
    /// Rows with supported behavior.
    pub supported: usize,
}

/// Derived accepted language capability counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LanguageCapabilityCounts {
    /// Canonical accepted rows; aliases and extensions do not add to this count.
    pub accepted: usize,
    /// Rows owned entirely by the default-core runtime.
    pub built_in: usize,
    /// Broad rows eligible for the separately verified optional pack.
    pub optional_candidates: usize,
    /// Detection tier counts.
    pub detected: CapabilityLevelCounts,
    /// Parsing tier counts.
    pub parsed: CapabilityLevelCounts,
    /// Symbol tier counts.
    pub symbols: CapabilityLevelCounts,
    /// Semantic tier counts.
    pub semantic: CapabilityLevelCounts,
    /// Benchmark tier counts.
    pub benchmarked: CapabilityLevelCounts,
}

/// One content-free settings and generated-documentation capability row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageCapabilityReportRow {
    /// Stable canonical language or file-family identifier.
    pub id: &'static str,
    /// Compatibility aliases; these never add to capability counts.
    pub aliases: &'static [&'static str],
    /// Registry-owned content role.
    pub classification: ContentClassification,
    /// Exact filename detector rules owned by this row.
    pub exact_filenames: Vec<&'static str>,
    /// Compound extension detector rules owned by this row.
    pub compound_extensions: Vec<&'static str>,
    /// Ordinary extension detector rules owned by this row.
    pub extensions: Vec<&'static str>,
    /// Shebang interpreter basenames owned by this row.
    pub content_interpreters: Vec<&'static str>,
    /// Compatibility parser tier.
    pub parser_support: LanguageParserSupport,
    /// Closed symbol parser owner.
    pub symbol_parser: SymbolParserOwner,
    /// Closed project-wide semantic provider owner.
    pub semantic_provider: SemanticProviderOwner,
    /// Optional bounded host-to-embedded provider pairing.
    pub embedded_language: Option<EmbeddedLanguageCapability>,
    /// Optional structural summary owner.
    pub structural_summary: Option<StructuralSummaryOwner>,
    /// Optional broad-parser pack owner.
    pub optional_pack: Option<&'static str>,
    /// Currently achieved independent support.
    pub support: LanguageCapabilitySupport,
    /// Accepted minimum support.
    pub accepted_minimum: LanguageCapabilitySupport,
    /// Natural positive and negative detector fixtures.
    pub fixtures: LanguageCapabilityFixtures,
    /// Exact provenance package or catalog identity.
    pub provenance_source: &'static str,
    /// Exact provenance package or catalog version.
    pub provenance_version: &'static str,
    /// License applying to the declared implementation or catalog metadata.
    pub provenance_license: &'static str,
    /// Required release platforms.
    pub required_platforms: RequiredPlatformSet,
}

/// Pinned broad-language catalog identity used for detection candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageCatalogReport {
    /// Catalog package identity.
    pub name: &'static str,
    /// Exact catalog release.
    pub version: &'static str,
    /// Exact catalog source revision.
    pub revision: &'static str,
    /// License applying to the catalog metadata, not every grammar subtree.
    pub metadata_license: &'static str,
    /// Minimum additional accepted grammars required for the broad-pack claim.
    pub minimum_additional_grammars: usize,
}

/// Content-free language registry settings projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LanguageRegistryReport {
    /// Registry schema version.
    pub registry_version: u32,
    /// Accepted capability-set version.
    pub accepted_set_version: u32,
    /// Detector precedence and content-matching policy version.
    pub detection_policy_version: u32,
    /// Digest of runtime rows and detection rules.
    pub registry_digest: String,
    /// Digest of accepted minimums.
    pub accepted_set_digest: String,
    /// Digest of direct/embedded provider ownership and resolution-family truth.
    pub semantic_provider_digest: String,
    /// Derived independent support counts.
    pub counts: LanguageCapabilityCounts,
    /// Pinned catalog identity for optional capability candidates.
    pub optional_catalog: LanguageCatalogReport,
}

/// Build a content-free settings/documentation report from the registry authority.
#[must_use]
pub fn language_registry_report() -> LanguageRegistryReport {
    LanguageRegistryReport {
        registry_version: LANGUAGE_CAPABILITY_REGISTRY_VERSION,
        accepted_set_version: ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION,
        detection_policy_version: LANGUAGE_DETECTION_POLICY_VERSION,
        registry_digest: language_registry_digest(),
        accepted_set_digest: accepted_language_capability_digest(),
        semantic_provider_digest: semantic_provider_digest(),
        counts: language_capability_counts(),
        optional_catalog: LanguageCatalogReport {
            name: OPTIONAL_GRAMMAR_CATALOG,
            version: OPTIONAL_GRAMMAR_CATALOG_VERSION,
            revision: OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
            metadata_license: "MIT",
            minimum_additional_grammars: OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS,
        },
    }
}

/// Build the complete content-free capability matrix from registry authority.
#[must_use]
pub fn language_capability_report_rows() -> Vec<LanguageCapabilityReportRow> {
    LANGUAGE_CAPABILITIES
        .iter()
        .map(|capability| LanguageCapabilityReportRow {
            id: capability.id,
            aliases: capability.aliases,
            classification: capability.classification,
            exact_filenames: rules_for_language(EXACT_FILENAME_RULES, capability.id),
            compound_extensions: rules_for_language(COMPOUND_EXTENSION_RULES, capability.id),
            extensions: rules_for_language(EXTENSION_RULES, capability.id),
            content_interpreters: CONTENT_DIALECT_RULES
                .iter()
                .filter_map(|rule| (rule.language == capability.id).then_some(rule.interpreter))
                .collect(),
            parser_support: capability.parser_support,
            symbol_parser: capability.symbol_parser,
            semantic_provider: capability.semantic_provider,
            embedded_language: capability.embedded_language,
            structural_summary: capability.structural_summary,
            optional_pack: capability.optional_pack,
            support: capability.support,
            accepted_minimum: capability.accepted_minimum,
            fixtures: capability.fixtures,
            provenance_source: capability.provenance.source(),
            provenance_version: capability.provenance.version(),
            provenance_license: capability.provenance.license(),
            required_platforms: capability.required_platforms,
        })
        .collect()
}

/// Render the checked-in public language support matrix from registry authority.
///
/// # Errors
///
/// Returns a formatting error if writing to the owned `String` fails.
pub fn render_language_support_markdown() -> Result<String, fmt::Error> {
    let report = language_registry_report();
    let capability_rows = language_capability_report_rows();
    let mut output = String::new();
    output.push_str("# ProjectAtlas Language Support\n\n");
    output.push_str(
        "This document is generated from the versioned Rust language capability registry. \
Do not edit the capability table or totals by hand. Canonical rows count once; aliases and \
extensions never increase a capability total.\n\n",
    );
    write!(
        &mut output,
        "Registry version: `{}`. Accepted capability-set version: `{}`. Detection policy \
version: `{}`. \
Registry digest: `{}`. Accepted-set digest: `{}`. Semantic-provider digest: `{}`.\n\n",
        report.registry_version,
        report.accepted_set_version,
        report.detection_policy_version,
        report.registry_digest,
        report.accepted_set_digest,
        report.semantic_provider_digest
    )?;
    write!(
        &mut output,
        "Optional catalog input: `{}@{}` revision `{}` under `{}` metadata license. \
This catalog identity is not a grammar-license or runtime-support claim.\n\n",
        report.optional_catalog.name,
        report.optional_catalog.version,
        report.optional_catalog.revision,
        report.optional_catalog.metadata_license
    )?;
    write!(
        &mut output,
        "The registry contains **{}** canonical rows: **{}** default-core rows and **{}** \
optional-pack candidates. Detection is supported for {} rows. Parsing is supported for {}, \
fallback for {}, and unavailable for {}. Symbols are supported for {}, fallback for {}, and \
unavailable for {}. Semantic resolution and benchmark coverage are reported independently.\n\n",
        report.counts.accepted,
        report.counts.built_in,
        report.counts.optional_candidates,
        report.counts.detected.supported,
        report.counts.parsed.supported,
        report.counts.parsed.fallback,
        report.counts.parsed.unavailable,
        report.counts.symbols.supported,
        report.counts.symbols.fallback,
        report.counts.symbols.unavailable
    )?;
    output.push_str(
        "Rows marked `broad-parser` are detected and, when explicitly admitted to the scan \
policy, remain usable through the conservative default-core fallback while the optional pack is \
absent. Catalog recognition alone does not add these extensions to the default scan surface. The \
pinned catalog is provenance for detection metadata only. A row becomes grammar-backed parsed \
support only after its exact \
grammar binary, subtree license, ABI/export, fixtures, and every accepted optional-pack target \
pass the separate acceptance gates. The v0.4 optional-pack targets are Linux x86-64 and Windows \
x86-64; macOS keeps the full built-in surface and reports `unsupported_containment` for optional-\
pack activation. Built-in owners always retain precedence.\n\n",
    );
    output.push_str(
        "Broad candidate rows are admitted only when the pinned catalog supplies a stable \
canonical grammar identity and at least one ordinary extension that does not conflict with an \
already accepted detector owner. Extensionless, ambiguous, duplicate, pseudo, or conflicting \
catalog entries remain unadvertised until a separate deterministic rule and evidence exist.\n\n",
    );
    output.push_str(
        "| Language | Classification | Aliases | Detection rules | Parser owner | Parsed | Symbols | Semantic | Embedded source | Benchmarked | Optional pack | Provenance | License |\n",
    );
    output.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for row in &capability_rows {
        let rules = detector_rule_summary(row);
        let aliases = list_or_dash(row.aliases.iter().copied());
        let optional_pack = row.optional_pack.unwrap_or("—");
        let provenance = format!("{}@{}", row.provenance_source, row.provenance_version);
        writeln!(
            &mut output,
            "| `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | `{}` | `{}` |",
            row.id,
            row.classification,
            aliases,
            rules,
            symbol_parser_label(row.symbol_parser),
            row.support.parsed.as_str(),
            row.support.symbols.as_str(),
            semantic_support_label(row.support.semantic, row.semantic_provider),
            embedded_language_label(row.embedded_language),
            row.support.benchmarked.as_str(),
            optional_pack,
            provenance,
            row.provenance_license
        )?;
    }
    crate::support_catalog::append_support_catalog_markdown(&mut output)?;
    Ok(output)
}

/// Render all detector rules for one documentation row.
fn detector_rule_summary(row: &LanguageCapabilityReportRow) -> String {
    let mut rules = Vec::new();
    rules.extend(
        row.exact_filenames
            .iter()
            .map(|value| format!("exact `{value}`")),
    );
    rules.extend(
        row.compound_extensions
            .iter()
            .map(|value| format!("compound `{value}`")),
    );
    rules.extend(row.extensions.iter().map(|value| format!("`{value}`")));
    rules.extend(
        row.content_interpreters
            .iter()
            .map(|value| format!("shebang `{value}`")),
    );
    if rules.is_empty() {
        "—".to_string()
    } else {
        rules.join(", ")
    }
}

/// Render one closed symbol-parser owner without duplicating selection policy.
fn symbol_parser_label(owner: SymbolParserOwner) -> String {
    match owner {
        SymbolParserOwner::TreeSitter(grammar) => {
            format!("{}@{}", grammar.package(), grammar.version())
        }
        SymbolParserOwner::CargoManifest => "projectatlas:cargo-manifest".to_string(),
        SymbolParserOwner::Vue => "projectatlas:vue".to_string(),
        SymbolParserOwner::PowerShell => "projectatlas:powershell".to_string(),
        SymbolParserOwner::Markdown => "projectatlas:markdown".to_string(),
        SymbolParserOwner::Fallback => "projectatlas:fallback".to_string(),
        SymbolParserOwner::Unavailable => "unavailable".to_string(),
    }
}

/// Render semantic support with its closed owner when available.
fn semantic_support_label(support: CapabilitySupportLevel, owner: SemanticProviderOwner) -> String {
    match owner {
        SemanticProviderOwner::Unavailable => support.as_str().to_string(),
        _ => format!("{} ({})", support.as_str(), owner.as_str()),
    }
}

/// Render one accepted host-to-embedded provider pairing.
fn embedded_language_label(capability: Option<EmbeddedLanguageCapability>) -> String {
    capability.map_or_else(
        || "—".to_string(),
        |capability| {
            format!(
                "{} → {}",
                capability.host_kind.as_str(),
                capability.semantic_provider.as_str()
            )
        },
    )
}

/// Join documentation values or render an explicit empty marker.
fn list_or_dash<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        "—".to_string()
    } else {
        values
            .into_iter()
            .map(|value| format!("`{value}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Collect rule values owned by one canonical language row.
fn rules_for_language(rules: &[LanguageDetectionRule], language: &str) -> Vec<&'static str> {
    rules
        .iter()
        .filter_map(|rule| (rule.language == language).then_some(rule.value))
        .collect()
}

/// Derive independent support counts without mutable literals.
#[must_use]
pub fn language_capability_counts() -> LanguageCapabilityCounts {
    let mut counts = LanguageCapabilityCounts {
        accepted: LANGUAGE_CAPABILITIES.len(),
        ..LanguageCapabilityCounts::default()
    };
    for capability in LANGUAGE_CAPABILITIES {
        if capability.optional_pack.is_some() {
            counts.optional_candidates += 1;
        } else {
            counts.built_in += 1;
        }
        increment_level(&mut counts.detected, capability.support.detected);
        increment_level(&mut counts.parsed, capability.support.parsed);
        increment_level(&mut counts.symbols, capability.support.symbols);
        increment_level(&mut counts.semantic, capability.support.semantic);
        increment_level(&mut counts.benchmarked, capability.support.benchmarked);
    }
    counts
}

/// Increment one derived support-level bucket.
fn increment_level(counts: &mut CapabilityLevelCounts, level: CapabilitySupportLevel) {
    match level {
        CapabilitySupportLevel::Unavailable => counts.unavailable += 1,
        CapabilitySupportLevel::Fallback => counts.fallback += 1,
        CapabilitySupportLevel::Supported => counts.supported += 1,
    }
}

/// Return the deterministic digest of achieved registry truth and detection rules.
#[must_use]
pub fn language_registry_digest() -> String {
    hash_language_registry(false)
}

/// Return the deterministic digest of accepted minimum capability truth.
#[must_use]
pub fn accepted_language_capability_digest() -> String {
    hash_language_registry(true)
}

/// Return the deterministic digest of semantic-provider ownership.
///
/// The projection is deliberately narrower than the complete language registry:
/// it binds only provider ownership, canonical resolution families, embedded-host
/// admission, and achieved semantic strength. Relation behavior remains owned by
/// `projectatlas-symbols` until the accepted relation-family inventory lands.
#[must_use]
pub fn semantic_provider_digest() -> String {
    let mut hasher = Hasher::new();
    hasher.update(&SEMANTIC_PROVIDER_CONTRACT_VERSION.to_le_bytes());
    for capability in LANGUAGE_CAPABILITIES {
        let effective_provider = capability.effective_semantic_provider();
        if effective_provider.is_none() && capability.embedded_language.is_none() {
            continue;
        }
        hash_value(&mut hasher, capability.id);
        hash_value(&mut hasher, capability.semantic_provider.as_str());
        hash_value(
            &mut hasher,
            capability
                .semantic_provider
                .resolution_family()
                .unwrap_or("unavailable"),
        );
        hash_value(
            &mut hasher,
            effective_provider
                .and_then(SemanticProviderOwner::resolution_family)
                .unwrap_or("unavailable"),
        );
        if let Some(embedded) = capability.embedded_language {
            hash_value(&mut hasher, embedded.host_kind.as_str());
            hash_value(&mut hasher, embedded.semantic_provider.as_str());
            hash_value(
                &mut hasher,
                embedded
                    .semantic_provider
                    .resolution_family()
                    .unwrap_or("unavailable"),
            );
        } else {
            hash_value(&mut hasher, "no-embedded-provider");
        }
        hash_value(&mut hasher, capability.support.semantic.as_str());
    }
    hasher.finalize().to_hex().to_string()
}

/// Hash either achieved runtime truth or accepted minimums deterministically.
fn hash_language_registry(accepted_only: bool) -> String {
    hash_language_registry_with_content_rules(accepted_only, CONTENT_DIALECT_RULES)
}

/// Hash registry truth with an explicit content-rule projection for validation tests.
fn hash_language_registry_with_content_rules(
    accepted_only: bool,
    content_rules: &[LanguageContentRule],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(&LANGUAGE_CAPABILITY_REGISTRY_VERSION.to_le_bytes());
    hasher.update(&ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION.to_le_bytes());
    hasher.update(&LANGUAGE_DETECTION_POLICY_VERSION.to_le_bytes());
    hasher.update(&(LANGUAGE_CONTENT_DETECTION_MAX_BYTES as u64).to_le_bytes());
    hash_value(&mut hasher, OPTIONAL_GRAMMAR_CATALOG);
    hash_value(&mut hasher, OPTIONAL_GRAMMAR_CATALOG_VERSION);
    hash_value(&mut hasher, OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION);
    hasher.update(&(OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS as u64).to_le_bytes());
    for capability in LANGUAGE_CAPABILITIES {
        hash_value(&mut hasher, capability.id);
        for alias in capability.aliases {
            hash_value(&mut hasher, alias);
        }
        hash_value(&mut hasher, capability.classification.as_str());
        let support = if accepted_only {
            capability.accepted_minimum
        } else {
            capability.support
        };
        hash_support(&mut hasher, support);
        hash_value(&mut hasher, &format!("{:?}", capability.parser_support));
        hash_value(&mut hasher, &format!("{:?}", capability.symbol_parser));
        hash_value(&mut hasher, &format!("{:?}", capability.semantic_provider));
        hash_value(&mut hasher, &format!("{:?}", capability.embedded_language));
        hash_value(&mut hasher, &format!("{:?}", capability.structural_summary));
        hash_value(
            &mut hasher,
            capability.optional_pack.unwrap_or("default-core"),
        );
        hash_value(&mut hasher, capability.fixtures.positive_path);
        hash_value(&mut hasher, capability.fixtures.negative_path);
        hash_value(&mut hasher, capability.provenance.source());
        hash_value(&mut hasher, capability.provenance.version());
        hash_value(&mut hasher, capability.provenance.license());
        hash_value(&mut hasher, &format!("{:?}", capability.required_platforms));
    }
    for (kind, rules) in [
        ("exact", EXACT_FILENAME_RULES),
        ("compound", COMPOUND_EXTENSION_RULES),
        ("extension", EXTENSION_RULES),
    ] {
        for rule in rules {
            hash_value(&mut hasher, kind);
            hash_value(&mut hasher, rule.value);
            hash_value(&mut hasher, rule.language);
        }
    }
    for rule in content_rules {
        hash_value(&mut hasher, "content-interpreter");
        hash_value(&mut hasher, rule.interpreter);
        hasher.update(&[u8::from(rule.allow_version_suffix)]);
        hash_value(&mut hasher, rule.language);
    }
    hasher.finalize().to_hex().to_string()
}

/// Hash the five independent support axes.
fn hash_support(hasher: &mut Hasher, support: LanguageCapabilitySupport) {
    hasher.update(&[
        support.detected as u8,
        support.parsed as u8,
        support.symbols as u8,
        support.semantic as u8,
        support.benchmarked as u8,
    ]);
}

/// Hash one length-delimited registry string.
fn hash_value(hasher: &mut Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

/// Validate the accepted registry and all generated projections.
///
/// # Errors
///
/// Returns the first deterministic conflict or incomplete accepted row.
pub fn validate_language_registry() -> Result<(), LanguageRegistryError> {
    let mut canonical = BTreeSet::new();
    let mut names = BTreeMap::new();
    for capability in LANGUAGE_CAPABILITIES {
        if capability.id.is_empty() {
            return Err(LanguageRegistryError::new("empty canonical language ID"));
        }
        if !canonical.insert(capability.id) {
            return Err(LanguageRegistryError::new(format!(
                "duplicate canonical language ID {:?}",
                capability.id
            )));
        }
        register_owner(&mut names, capability.id, capability.id, "language name")?;
        for alias in capability.aliases {
            register_owner(&mut names, alias, capability.id, "language alias")?;
        }
        if !capability.support.meets(capability.accepted_minimum) {
            return Err(LanguageRegistryError::new(format!(
                "language {:?} is weaker than accepted capability set version {}",
                capability.id, ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION
            )));
        }
        if capability.fixtures.positive_path.is_empty()
            || capability.fixtures.negative_path.is_empty()
            || capability.fixtures.positive_path == capability.fixtures.negative_path
        {
            return Err(LanguageRegistryError::new(format!(
                "language {:?} lacks distinct natural positive and negative fixtures",
                capability.id
            )));
        }
        if capability.provenance.license().is_empty() {
            return Err(LanguageRegistryError::new(format!(
                "language {:?} lacks a provenance license input",
                capability.id
            )));
        }
        if capability
            .optional_pack
            .is_some_and(|owner| owner != BROAD_PARSER_PACK_ID)
        {
            return Err(LanguageRegistryError::new(format!(
                "optional language {:?} is assigned to unknown pack owner {:?}",
                capability.id, capability.optional_pack
            )));
        }
        if capability.optional_pack.is_some()
            && capability.provenance != CapabilityProvenance::PinnedOptionalCatalog
        {
            return Err(LanguageRegistryError::new(format!(
                "optional language {:?} lacks pinned catalog provenance",
                capability.id
            )));
        }
        if capability.optional_pack.is_none()
            && capability.provenance == CapabilityProvenance::PinnedOptionalCatalog
        {
            return Err(LanguageRegistryError::new(format!(
                "default-core language {:?} incorrectly claims optional catalog provenance",
                capability.id
            )));
        }
        match (capability.support.symbols, capability.symbol_parser) {
            (CapabilitySupportLevel::Unavailable, SymbolParserOwner::Unavailable)
            | (CapabilitySupportLevel::Fallback, SymbolParserOwner::Fallback)
            | (
                CapabilitySupportLevel::Supported,
                SymbolParserOwner::TreeSitter(_)
                | SymbolParserOwner::CargoManifest
                | SymbolParserOwner::Vue
                | SymbolParserOwner::PowerShell
                | SymbolParserOwner::Markdown,
            ) => {}
            (support, owner) => {
                return Err(LanguageRegistryError::new(format!(
                    "language {:?} advertises symbol support {support:?} with incompatible owner {owner:?}",
                    capability.id
                )));
            }
        }
        match (capability.support.semantic, capability.semantic_provider) {
            (CapabilitySupportLevel::Unavailable, SemanticProviderOwner::Unavailable)
            | (
                CapabilitySupportLevel::Supported,
                SemanticProviderOwner::Rust
                | SemanticProviderOwner::EcmaScript
                | SemanticProviderOwner::Python
                | SemanticProviderOwner::Cargo,
            ) => {}
            (support, owner) => {
                return Err(LanguageRegistryError::new(format!(
                    "language {:?} advertises semantic support {support:?} with incompatible owner {owner:?}",
                    capability.id
                )));
            }
        }
        if let Some(embedded) = capability.embedded_language {
            if embedded.semantic_provider == SemanticProviderOwner::Unavailable {
                return Err(LanguageRegistryError::new(format!(
                    "embedded language host {:?} lacks a semantic provider owner",
                    capability.id
                )));
            }
            if capability.support.semantic != CapabilitySupportLevel::Unavailable
                || capability.semantic_provider != SemanticProviderOwner::Unavailable
            {
                return Err(LanguageRegistryError::new(format!(
                    "embedded language host {:?} conflates host and embedded semantic support",
                    capability.id
                )));
            }
        }
        if capability.optional_pack.is_some()
            && (capability.semantic_provider != SemanticProviderOwner::Unavailable
                || capability.embedded_language.is_some())
        {
            return Err(LanguageRegistryError::new(format!(
                "optional language {:?} advertises unvalidated semantic capability",
                capability.id
            )));
        }
    }

    validate_rules("exact filename", EXACT_FILENAME_RULES, &canonical, false)?;
    validate_rules(
        "compound extension",
        COMPOUND_EXTENSION_RULES,
        &canonical,
        true,
    )?;
    validate_rules("extension", EXTENSION_RULES, &canonical, true)?;
    validate_content_rules(&canonical)?;

    let mut detected: BTreeSet<_> = EXACT_FILENAME_RULES
        .iter()
        .chain(COMPOUND_EXTENSION_RULES)
        .chain(EXTENSION_RULES)
        .map(|rule| rule.language)
        .collect();
    detected.extend(CONTENT_DIALECT_RULES.iter().map(|rule| rule.language));
    for capability in LANGUAGE_CAPABILITIES {
        if capability.support.detected == CapabilitySupportLevel::Supported
            && !detected.contains(capability.id)
        {
            return Err(LanguageRegistryError::new(format!(
                "accepted language {:?} is a ghost row with no detector rule",
                capability.id
            )));
        }
        let detected_fixture = detect_language_request(LanguageDetectionRequest::new(
            capability.fixtures.positive_path,
            None,
        ))
        .map_err(|source| LanguageRegistryError::new(source.to_string()))?;
        if detected_fixture.map(|result| result.language) != Some(capability.id) {
            return Err(LanguageRegistryError::new(format!(
                "positive fixture {:?} does not select owning language {:?}",
                capability.fixtures.positive_path, capability.id
            )));
        }
        let negative_fixture = detect_language_request(LanguageDetectionRequest::new(
            capability.fixtures.negative_path,
            None,
        ))
        .map_err(|source| LanguageRegistryError::new(source.to_string()))?;
        if negative_fixture.map(|result| result.language) == Some(capability.id) {
            return Err(LanguageRegistryError::new(format!(
                "negative fixture {:?} still selects owning language {:?}",
                capability.fixtures.negative_path, capability.id
            )));
        }
    }
    let optional_candidates = LANGUAGE_CAPABILITIES
        .iter()
        .filter(|capability| capability.optional_pack.is_some())
        .count();
    if optional_candidates < OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS {
        return Err(LanguageRegistryError::new(format!(
            "optional catalog exposes only {optional_candidates} distinct candidate rows; at least {OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS} are required"
        )));
    }
    let accepted_digest = accepted_language_capability_digest();
    let expected_accepted_digest = match ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION {
        1 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V1_DIGEST,
        2 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V2_DIGEST,
        3 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V3_DIGEST,
        4 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V4_DIGEST,
        5 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V5_DIGEST,
        6 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V6_DIGEST,
        7 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V7_DIGEST,
        8 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V8_DIGEST,
        9 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V9_DIGEST,
        10 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V10_DIGEST,
        11 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V11_DIGEST,
        12 => ACCEPTED_LANGUAGE_CAPABILITY_SET_V12_DIGEST,
        version => {
            return Err(LanguageRegistryError::new(format!(
                "accepted language capability-set version {version} lacks a historical digest seal"
            )));
        }
    };
    if accepted_digest != expected_accepted_digest {
        return Err(LanguageRegistryError::new(format!(
            "accepted language capability-set version {ACCEPTED_LANGUAGE_CAPABILITY_SET_VERSION} changed from {expected_accepted_digest} to {accepted_digest}; bump the set version for an explicit compatibility decision"
        )));
    }
    Ok(())
}

/// Register one normalized ID or alias and report both conflicting owners.
fn register_owner<'a>(
    owners: &mut BTreeMap<String, &'a str>,
    value: &str,
    owner: &'a str,
    kind: &str,
) -> Result<(), LanguageRegistryError> {
    let normalized = value.to_ascii_lowercase();
    if let Some(previous) = owners.insert(normalized.clone(), owner) {
        return Err(LanguageRegistryError::new(format!(
            "conflicting {kind} {normalized:?} is owned by both {previous:?} and {owner:?}"
        )));
    }
    Ok(())
}

/// Validate one precedence-class rule table and its canonical targets.
fn validate_rules(
    kind: &str,
    rules: &[LanguageDetectionRule],
    canonical: &BTreeSet<&str>,
    normalize_case: bool,
) -> Result<(), LanguageRegistryError> {
    let mut owners = BTreeMap::new();
    let mut previous_compound_length = usize::MAX;
    for rule in rules {
        if !canonical.contains(rule.language) {
            return Err(LanguageRegistryError::new(format!(
                "{kind} rule {:?} targets missing language {:?}",
                rule.value, rule.language
            )));
        }
        let key = if normalize_case {
            rule.value.to_ascii_lowercase()
        } else {
            rule.value.to_string()
        };
        if let Some(previous) = owners.insert(key.clone(), rule.language)
            && previous != rule.language
        {
            return Err(LanguageRegistryError::new(format!(
                "conflicting {kind} rule {key:?} is owned by both {previous:?} and {:?}",
                rule.language
            )));
        }
        if kind == "compound extension" {
            if rule.value.len() > previous_compound_length {
                return Err(LanguageRegistryError::new(format!(
                    "compound extension rules are not longest-first at {:?}",
                    rule.value
                )));
            }
            previous_compound_length = rule.value.len();
        }
    }
    Ok(())
}

/// Validate registry-owned content rules and their canonical targets.
fn validate_content_rules(canonical: &BTreeSet<&str>) -> Result<(), LanguageRegistryError> {
    let mut owners = BTreeMap::new();
    for rule in CONTENT_DIALECT_RULES {
        if !canonical.contains(rule.language) {
            return Err(LanguageRegistryError::new(format!(
                "content interpreter {:?} targets missing language {:?}",
                rule.interpreter, rule.language
            )));
        }
        if rule.interpreter.is_empty()
            || !rule
                .interpreter
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(LanguageRegistryError::new(format!(
                "content interpreter {:?} is not a normalized basename",
                rule.interpreter
            )));
        }
        if let Some(previous) = owners.insert(rule.interpreter, rule.language) {
            return Err(LanguageRegistryError::new(format!(
                "conflicting content interpreter {:?} is owned by both {previous:?} and {:?}",
                rule.interpreter, rule.language
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io;

    /// Independent frozen detector corpus from the 0.3.26 release.
    #[derive(Debug, Deserialize)]
    struct DetectionCompatibilityFixture {
        /// Ordered broad extensions and their canonical results.
        extensions: Vec<String>,
        /// Exact filename rules and their canonical results.
        exact_filenames: Vec<String>,
    }

    /// Independently accepted projection of the pinned optional catalog subset.
    #[derive(Debug, Deserialize)]
    struct OptionalDetectionCatalogFixture {
        /// Catalog package identity.
        catalog: String,
        /// Exact catalog release.
        version: String,
        /// Exact source revision.
        revision: String,
        /// Ordered canonical ID to accepted extension mappings.
        rows: Vec<String>,
    }

    fn require_test(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.into()).into())
        }
    }

    #[test]
    fn content_classification_is_registry_owned_and_utf8_safe() -> Result<(), Box<dyn Error>> {
        require_test(
            content_classification(Some("rust"), true) == ContentClassification::Source,
            "Rust did not retain its source classification",
        )?;
        require_test(
            content_classification(Some("markdown"), true) == ContentClassification::Documentation,
            "Markdown did not retain its documentation classification",
        )?;
        require_test(
            content_classification(Some("json"), true) == ContentClassification::ConfigurationData,
            "JSON did not retain its configuration/data classification",
        )?;
        require_test(
            content_classification(None, true) == ContentClassification::OtherText,
            "unknown UTF-8 content was not classified as other text",
        )?;
        require_test(
            content_classification(Some("rust"), false) == ContentClassification::Source,
            "known language classification did not take precedence over UTF-8 fallback",
        )?;
        require_test(
            content_classification(None, false) == ContentClassification::Opaque,
            "unknown invalid UTF-8 content was not opaque",
        )?;
        require_test(
            language_capability("rst").map(|row| row.classification)
                == Some(ContentClassification::Documentation),
            "optional documentation rows lost registry-owned classification",
        )?;
        require_test(
            language_capability("csv").map(|row| row.classification)
                == Some(ContentClassification::ConfigurationData),
            "optional structured-data rows lost registry-owned classification",
        )?;

        let rendered = render_language_support_markdown()?;
        require_test(
            rendered.contains("| Language | Classification | Aliases |"),
            "generated capability matrix omitted the classification column",
        )?;
        require_test(
            rendered.contains("| `markdown` | `documentation` |"),
            "generated capability matrix omitted Markdown's documentation role",
        )
    }

    #[test]
    fn content_classification_storage_and_selection_contracts_are_closed()
    -> Result<(), Box<dyn Error>> {
        for classification in ContentClassification::ALL {
            require_test(
                ContentClassification::from_db(classification.as_str()) == Some(classification),
                format!("classification {classification} did not round-trip"),
            )?;
            require_test(
                serde_json::from_str::<ContentClassification>(&serde_json::to_string(
                    &classification,
                )?)? == classification,
                format!("classification {classification} wire spelling did not round-trip"),
            )?;
        }
        require_test(
            ContentClassification::from_db("generated").is_none(),
            "unsupported generated classification was accepted",
        )?;

        require_test(
            ContentSelection::default() == ContentSelection::UnspecifiedLegacy
                && ContentSelection::default().explicit_value().is_none(),
            "omitted selection no longer preserves the distinct legacy state",
        )?;
        require_test(
            ContentSelection::UnspecifiedLegacy.includes(ContentClassification::ConfigurationData)
                && ContentSelection::Source.includes(ContentClassification::Source)
                && !ContentSelection::Source.includes(ContentClassification::Documentation)
                && ContentSelection::Documentation.includes(ContentClassification::Documentation)
                && ContentSelection::Both.includes(ContentClassification::Source)
                && ContentSelection::Both.includes(ContentClassification::Documentation)
                && !ContentSelection::Both.includes(ContentClassification::OtherText),
            "content selection admitted the wrong classification",
        )?;
        require_test(
            "source".parse::<ContentSelection>()? == ContentSelection::Source
                && "documentation".parse::<ContentSelection>()? == ContentSelection::Documentation
                && "both".parse::<ContentSelection>()? == ContentSelection::Both,
            "an explicit selection failed to parse",
        )?;
        let invalid = "".parse::<ContentSelection>().err().ok_or_else(|| {
            std::io::Error::other("empty explicit content selection was accepted")
        })?;
        require_test(
            invalid.requested().is_empty()
                && invalid
                    .to_string()
                    .contains("source, documentation, or both"),
            "invalid selection did not retain the typed allowed-value diagnostic",
        )?;
        require_test(
            serde_json::to_string(&ContentSelection::Source)? == "\"source\""
                && serde_json::to_string(&ContentSelection::UnspecifiedLegacy).is_err()
                && serde_json::from_str::<ContentSelection>("\"unspecified_legacy\"").is_err(),
            "the internal legacy state leaked into the caller-visible wire contract",
        )
    }

    #[test]
    fn accepted_registry_and_generated_projections_validate() -> Result<(), Box<dyn Error>> {
        validate_language_registry()?;
        require_test(
            LANGUAGE_CAPABILITIES.len() == LANGUAGE_SPECS.len(),
            "language capability and parser metadata projections differ",
        )?;
        let report = language_registry_report();
        require_test(
            report.counts.accepted == LANGUAGE_CAPABILITIES.len(),
            "accepted language count is not registry-derived",
        )?;
        let canonical_ids = LANGUAGE_CAPABILITIES
            .iter()
            .map(|capability| capability.id)
            .collect::<BTreeSet<_>>();
        require_test(
            report.counts.accepted == canonical_ids.len(),
            "aliases or extensions inflated the canonical capability count",
        )?;
        require_test(
            LANGUAGE_CAPABILITIES
                .iter()
                .any(|capability| !capability.aliases.is_empty())
                && !EXTENSION_RULES.is_empty(),
            "alias/extension non-inflation check became vacuous",
        )?;
        require_test(
            report.counts.built_in + report.counts.optional_candidates == report.counts.accepted,
            "built-in and optional language counts do not cover the accepted set",
        )?;
        require_test(
            language_capability_report_rows().len() == report.counts.accepted,
            "documentation projection does not cover the accepted set",
        )?;
        require_test(
            report.counts.optional_candidates >= OPTIONAL_PACK_MINIMUM_ADDITIONAL_GRAMMARS,
            "optional catalog projection is below the accepted breadth floor",
        )?;
        require_test(
            !report.registry_digest.is_empty(),
            "language registry digest is empty",
        )?;
        require_test(
            !report.accepted_set_digest.is_empty(),
            "accepted language-set digest is empty",
        )?;
        require_test(
            report.semantic_provider_digest.len() == 64
                && report
                    .semantic_provider_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "semantic provider digest is not a bounded lowercase hexadecimal identity",
        )?;
        let encoded = serde_json::to_vec(&report)?;
        require_test(
            encoded.len() <= LANGUAGE_REGISTRY_REPORT_MAX_BYTES,
            "language registry settings projection exceeded its content-free output budget",
        )?;
        Ok(())
    }

    #[test]
    fn semantic_provider_and_embedded_host_claims_are_honest() -> Result<(), Box<dyn Error>> {
        let report = language_registry_report();
        let semantic_rows = LANGUAGE_CAPABILITIES
            .iter()
            .filter(|capability| capability.support.semantic == CapabilitySupportLevel::Supported)
            .count();
        require_test(
            report.counts.semantic.supported == semantic_rows,
            "semantic provider rows did not determine the semantic capability count",
        )?;

        for capability in LANGUAGE_CAPABILITIES
            .iter()
            .filter(|capability| capability.optional_pack.is_some())
        {
            require_test(
                capability.semantic_provider == SemanticProviderOwner::Unavailable
                    && capability.embedded_language.is_none()
                    && capability.support.semantic == CapabilitySupportLevel::Unavailable,
                format!(
                    "optional candidate {:?} advertised semantic capability",
                    capability.id
                ),
            )?;
        }

        for (language, expected) in [
            (
                "html",
                EmbeddedLanguageCapability {
                    host_kind: EmbeddedHostKind::HtmlLike,
                    semantic_provider: SemanticProviderOwner::EcmaScript,
                },
            ),
            (
                "vue",
                EmbeddedLanguageCapability {
                    host_kind: EmbeddedHostKind::Component,
                    semantic_provider: SemanticProviderOwner::EcmaScript,
                },
            ),
            (
                "svelte",
                EmbeddedLanguageCapability {
                    host_kind: EmbeddedHostKind::Template,
                    semantic_provider: SemanticProviderOwner::EcmaScript,
                },
            ),
        ] {
            let capability = language_capability(language)
                .ok_or_else(|| io::Error::other(format!("missing {language} capability")))?;
            require_test(
                capability.embedded_language == Some(expected)
                    && capability.semantic_provider == SemanticProviderOwner::Unavailable
                    && capability.effective_semantic_provider()
                        == Some(SemanticProviderOwner::EcmaScript)
                    && capability.support.semantic == CapabilitySupportLevel::Unavailable,
                format!("{language} host/embedded capability drifted"),
            )?;
        }

        for language in ["cargo-lock", "java", "c", "go"] {
            let capability = language_capability(language)
                .ok_or_else(|| io::Error::other(format!("missing {language} capability")))?;
            require_test(
                capability.semantic_provider == SemanticProviderOwner::Unavailable
                    && capability.effective_semantic_provider().is_none()
                    && capability.support.semantic == CapabilitySupportLevel::Unavailable,
                format!("unsupported semantic language {language:?} was promoted"),
            )?;
        }
        require_test(
            SemanticProviderOwner::EcmaScript.resolution_family() == Some("ecmascript")
                && SemanticProviderOwner::Unavailable
                    .resolution_family()
                    .is_none(),
            "semantic provider resolution-family identity drifted",
        )?;
        Ok(())
    }

    #[test]
    fn detects_every_broad_source_extension() {
        for extension in BROAD_SOURCE_EXTENSIONS {
            assert!(
                detect_language(Some(extension)).is_some(),
                "missing broad source extension support for {extension}"
            );
        }
    }

    #[test]
    fn detects_every_accepted_registry_source_extension() {
        for extension in DETECTED_SOURCE_EXTENSIONS {
            assert!(
                detect_language(Some(extension)).is_some(),
                "missing accepted source extension support for {extension}"
            );
        }
    }

    #[test]
    fn generated_language_support_document_is_current() -> Result<(), Box<dyn Error>> {
        let rendered = render_language_support_markdown()?;
        require_test(
            include_str!("../../../docs/language-support.md") == rendered,
            "checked-in language support document is stale",
        )
    }

    #[test]
    fn built_in_grammar_provenance_matches_workspace_pins() {
        let workspace = include_str!("../../../Cargo.toml");
        let mut checked = BTreeSet::new();
        for grammar in TreeSitterGrammar::ALL {
            let package = grammar.package();
            if checked.insert(package) {
                let pin = format!("{package} = \"={}\"", grammar.version());
                assert!(
                    workspace.lines().any(|line| line.trim() == pin),
                    "workspace dependency pin drifted from registry provenance: {pin}"
                );
            }
        }
    }

    #[test]
    fn frozen_v0326_detection_corpus_remains_exact() -> Result<(), Box<dyn Error>> {
        let fixture: DetectionCompatibilityFixture = serde_json::from_str(include_str!(
            "../../../fixtures/languages/v0.3.26-detection.json"
        ))?;
        let ordered_extensions = fixture
            .extensions
            .iter()
            .map(|row| row.split_once('=').map(|(extension, _)| extension))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| io::Error::other("invalid extension compatibility row"))?;
        if ordered_extensions != BROAD_SOURCE_EXTENSIONS {
            return Err(io::Error::other(
                "ordered 0.3.26 broad extension compatibility corpus changed",
            )
            .into());
        }
        for row in fixture.extensions {
            let (extension, expected) = row
                .split_once('=')
                .ok_or_else(|| io::Error::other("invalid extension compatibility row"))?;
            if detect_language(Some(extension)).as_deref() != Some(expected) {
                return Err(io::Error::other(format!(
                    "0.3.26 extension {extension:?} no longer selects {expected:?}"
                ))
                .into());
            }
        }
        for row in fixture.exact_filenames {
            let (path, expected) = row
                .split_once('=')
                .ok_or_else(|| io::Error::other("invalid exact filename compatibility row"))?;
            if detect_language_for_path(path, None).as_deref() != Some(expected)
                || detect_language_for_path(&format!("folder\\{path}"), None).as_deref()
                    != Some(expected)
            {
                return Err(io::Error::other(format!(
                    "0.3.26 exact filename {path:?} no longer selects {expected:?}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn accepted_optional_detection_subset_matches_independent_catalog_projection()
    -> Result<(), Box<dyn Error>> {
        let fixture: OptionalDetectionCatalogFixture = serde_json::from_str(include_str!(
            "../../../fixtures/languages/accepted-optional-detection-catalog.json"
        ))?;
        require_test(
            fixture.catalog == OPTIONAL_GRAMMAR_CATALOG,
            "optional catalog identity drifted",
        )?;
        require_test(
            fixture.version == OPTIONAL_GRAMMAR_CATALOG_VERSION,
            "optional catalog version drifted",
        )?;
        require_test(
            fixture.revision == OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
            "optional catalog revision drifted",
        )?;
        let projected = LANGUAGE_CAPABILITIES
            .iter()
            .filter(|capability| capability.optional_pack.is_some())
            .map(|capability| {
                let extensions = rules_for_language(EXTENSION_RULES, capability.id);
                match extensions.as_slice() {
                    [extension] => Ok(format!("{}={extension}", capability.id)),
                    _ => Err(io::Error::other(format!(
                        "optional detection row {:?} must own exactly one accepted extension",
                        capability.id
                    ))),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        require_test(
            fixture.rows == projected,
            "accepted optional catalog projection drifted",
        )
    }

    #[test]
    fn preserves_representative_broad_source_extensions() {
        assert_eq!(
            detect_language(Some(".d.ts")).as_deref(),
            Some("typescript")
        );
        assert_eq!(detect_language(Some(".pyw")).as_deref(), Some("python"));
        assert_eq!(detect_language(Some(".kts")).as_deref(), Some("kotlin"));
        assert_eq!(
            detect_language(Some(".psm1")).as_deref(),
            Some("powershell")
        );
        assert_eq!(detect_language(Some(".zon")).as_deref(), Some("zig"));
        assert_eq!(detect_language(Some(".proto")).as_deref(), Some("protobuf"));
        assert_eq!(detect_language(Some(".R")).as_deref(), Some("r"));
        assert_eq!(detect_language(Some(".ini")).as_deref(), Some("config"));
        assert_eq!(detect_language(Some(".liquibase")).as_deref(), Some("sql"));
        assert_eq!(detect_language(Some(".toon")).as_deref(), Some("toon"));
    }

    #[test]
    fn preserves_exact_filename_case_and_compound_precedence() {
        let exact =
            detect_language_request(LanguageDetectionRequest::new("Cargo.toml", Some(".toml")));
        assert!(matches!(
            exact,
            Ok(Some(LanguageDetection {
                language: "cargo-manifest",
                reason: LanguageDetectionReason::ExactFilename,
            }))
        ));
        assert_eq!(
            detect_language_for_path("cargo.toml", Some(".toml")).as_deref(),
            Some("toml")
        );
        let compound = detect_language_request(LanguageDetectionRequest::new(
            "types/index.d.ts",
            Some(".d.ts"),
        ));
        assert!(matches!(
            compound,
            Ok(Some(LanguageDetection {
                language: "typescript",
                reason: LanguageDetectionReason::CompoundExtension,
            }))
        ));
    }

    #[test]
    fn typed_precedence_prefers_override_and_bounds_content() -> Result<(), Box<dyn Error>> {
        let override_result = detect_language_request(LanguageDetectionRequest {
            path: "Cargo.toml",
            extension: Some(".toml"),
            explicit_override: Some("py"),
            content_prefix: Some(b"#!/usr/bin/env node\n"),
        })?
        .ok_or_else(|| io::Error::other("override detection missing"))?;
        require_test(
            override_result.language == "python",
            "explicit alias override did not select Python",
        )?;
        require_test(
            override_result.reason == LanguageDetectionReason::ExplicitOverride,
            "explicit override did not retain its typed reason",
        )?;

        let content_result = detect_language_request(LanguageDetectionRequest {
            path: "tool",
            extension: None,
            explicit_override: None,
            content_prefix: Some(b"#!/usr/bin/env python\nprint('ok')\n"),
        })?
        .ok_or_else(|| io::Error::other("content detection missing"))?;
        require_test(
            content_result.language == "python",
            "bounded shebang did not select Python",
        )?;
        require_test(
            content_result.reason == LanguageDetectionReason::ContentDialect,
            "bounded shebang did not retain its typed reason",
        )?;
        let extension_result =
            detect_language_request(LanguageDetectionRequest::new("module.py", Some(".py")))?
                .ok_or_else(|| io::Error::other("extension detection missing"))?;
        require_test(
            extension_result.language == "python",
            "extension did not select Python",
        )?;
        require_test(
            extension_result.reason == LanguageDetectionReason::Extension,
            "extension did not retain its typed reason",
        )?;
        require_test(
            detect_language_request(LanguageDetectionRequest {
                path: "tool",
                extension: None,
                explicit_override: Some("missing-language"),
                content_prefix: None,
            })
            .is_err(),
            "unknown explicit override did not fail closed",
        )?;
        Ok(())
    }

    #[test]
    fn every_detected_language_has_compatible_parser_metadata() -> Result<(), Box<dyn Error>> {
        for rule in EXACT_FILENAME_RULES
            .iter()
            .chain(COMPOUND_EXTENSION_RULES)
            .chain(EXTENSION_RULES)
        {
            let spec = language_spec(rule.language).ok_or_else(|| {
                io::Error::other(format!("missing parser coverage for {}", rule.language))
            })?;
            require_test(
                spec.language == rule.language,
                format!("parser coverage ownership drifted for {}", rule.language),
            )?;
        }
        for rule in CONTENT_DIALECT_RULES {
            let spec = language_spec(rule.language).ok_or_else(|| {
                io::Error::other(format!("missing parser coverage for {}", rule.language))
            })?;
            require_test(
                spec.language == rule.language,
                format!("content detector ownership drifted for {}", rule.language),
            )?;
        }
        Ok(())
    }

    #[test]
    fn structural_summary_rows_do_not_advertise_symbols() {
        for capability in LANGUAGE_CAPABILITIES
            .iter()
            .filter(|capability| capability.parser_support == LanguageParserSupport::Structural)
            .filter(|capability| capability.symbol_parser == SymbolParserOwner::Unavailable)
        {
            assert_eq!(
                capability.support.symbols,
                CapabilitySupportLevel::Unavailable,
                "structural summary row {:?} advertised symbols without an owner",
                capability.id
            );
        }
    }

    #[test]
    fn shebang_rules_are_exact_bounded_and_utf8_boundary_safe() -> Result<(), Box<dyn Error>> {
        for rule in CONTENT_DIALECT_RULES {
            let source = format!("#!/usr/bin/env {}\n", rule.interpreter);
            let detected = detect_language_request(LanguageDetectionRequest {
                path: "tool",
                extension: None,
                explicit_override: None,
                content_prefix: Some(source.as_bytes()),
            })?
            .ok_or_else(|| io::Error::other("declared shebang was not detected"))?;
            require_test(
                detected.language == rule.language,
                format!("shebang owner drifted for {}", rule.interpreter),
            )?;
            require_test(
                detected.reason == LanguageDetectionReason::ContentDialect,
                format!("shebang reason drifted for {}", rule.interpreter),
            )?;
        }

        for near_miss in ["wish", "mesh-agent", "python-helper", "nodejs"] {
            let source = format!("#!/usr/bin/env {near_miss}\n");
            let detected = detect_language_request(LanguageDetectionRequest {
                path: "tool",
                extension: None,
                explicit_override: None,
                content_prefix: Some(source.as_bytes()),
            })?;
            require_test(
                detected.is_none(),
                format!("near-miss interpreter {near_miss:?} was classified"),
            )?;
        }

        for (interpreter, expected) in [
            ("python3", "python"),
            ("python3.12", "python"),
            ("ruby3.3", "ruby"),
            ("lua5.4", "lua"),
        ] {
            let source = format!("#!/usr/bin/env {interpreter}\n");
            let detected = detect_language_request(LanguageDetectionRequest {
                path: "tool",
                extension: None,
                explicit_override: None,
                content_prefix: Some(source.as_bytes()),
            })?;
            require_test(
                detected.map(|result| result.language) == Some(expected),
                format!("valid versioned interpreter {interpreter:?} was not classified"),
            )?;
        }

        for near_miss in ["python.", "python..3", "ruby...", "lua."] {
            let source = format!("#!/usr/bin/env {near_miss}\n");
            let detected = detect_language_request(LanguageDetectionRequest {
                path: "tool",
                extension: None,
                explicit_override: None,
                content_prefix: Some(source.as_bytes()),
            })?;
            require_test(
                detected.is_none(),
                format!("invalid versioned interpreter {near_miss:?} was classified"),
            )?;
        }

        let mut split_utf8_after_shebang = b"#!/usr/bin/env python\n".to_vec();
        split_utf8_after_shebang.resize(LANGUAGE_CONTENT_DETECTION_MAX_BYTES, b'x');
        split_utf8_after_shebang[LANGUAGE_CONTENT_DETECTION_MAX_BYTES - 1] = 0xc3;
        let detected = detect_language_request(LanguageDetectionRequest {
            path: "tool",
            extension: None,
            explicit_override: None,
            content_prefix: Some(&split_utf8_after_shebang),
        })?;
        require_test(
            detected.map(|result| result.language) == Some("python"),
            "valid shebang was lost at a later UTF-8 split boundary",
        )?;

        let mut after_bound = vec![b' '; LANGUAGE_CONTENT_DETECTION_MAX_BYTES];
        after_bound.extend_from_slice(b"#!/usr/bin/env python\n");
        let detected = detect_language_request(LanguageDetectionRequest {
            path: "tool",
            extension: None,
            explicit_override: None,
            content_prefix: Some(&after_bound),
        })?;
        require_test(
            detected.is_none(),
            "content detection inspected bytes beyond its accepted bound",
        )
    }

    #[test]
    fn content_rule_changes_advance_registry_digest() {
        let baseline = hash_language_registry_with_content_rules(false, CONTENT_DIALECT_RULES);
        let mut changed = CONTENT_DIALECT_RULES.to_vec();
        changed[0].language = "ruby";
        assert_ne!(
            baseline,
            hash_language_registry_with_content_rules(false, &changed)
        );
    }

    #[test]
    fn built_in_tree_sitter_projection_is_registry_derived() {
        let projected = builtin_tree_sitter_language_ids();
        let expected = LANGUAGE_CAPABILITIES
            .iter()
            .filter(|capability| {
                matches!(capability.symbol_parser, SymbolParserOwner::TreeSitter(_))
            })
            .count();
        assert_eq!(projected.len(), expected);
        assert!(projected.contains(&"rust"));
        assert!(!projected.contains(&"markdown"));
    }

    #[test]
    fn conflicting_detection_rules_report_both_owners() -> Result<(), Box<dyn Error>> {
        let canonical = BTreeSet::from(["python", "rust"]);
        let rules = [
            LanguageDetectionRule {
                value: ".mixed",
                language: "rust",
            },
            LanguageDetectionRule {
                value: ".MIXED",
                language: "python",
            },
        ];
        let error = validate_rules("extension", &rules, &canonical, true)
            .err()
            .ok_or_else(|| io::Error::other("case-normalized conflicting owners must fail"))?;
        let message = error.to_string();
        require_test(
            message.contains("rust") && message.contains("python"),
            "conflict diagnostic did not name both owners",
        )
    }
}
